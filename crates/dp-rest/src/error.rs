//! [`ApiError`] — the one error type every dp-rest handler returns.
//!
//! Stage 3 only needs the variants the report handlers can hit:
//!
//! * [`ApiError::BadRequest`] — query / path validation, including
//!   `ResolveError` (bad TZ, missing custom range, inverted range).
//! * [`ApiError::Store`] — anything bubbling up from
//!   [`dp_domain::store::StoreError`]. Mapped to `500` because every
//!   variant the report layer can trip is operator-actionable; v1
//!   has no "user-recoverable" store error on the read path.
//!
//! Later Phase 4 stages widen the set (auth-denied, forbidden,
//! conflict on home-org-set, …). Keeping the enum `#[non_exhaustive]`
//! so adding variants is non-breaking.
//!
//! `IntoResponse` writes a small JSON body `{ "error": "...", "code":
//! "..." }` so the frontend can show a deterministic message rather
//! than parsing free-form text.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde::Serialize;

use dp_domain::store::StoreError;
use dp_reports::ResolveError;

/// Every dp-rest handler returns `Result<_, ApiError>`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ApiError {
    /// Client-side validation failure. Carries a stable `code` the
    /// UI can switch on plus a human message.
    #[error("{message}")]
    BadRequest {
        /// Stable machine-readable code (e.g. `"invalid_tz"`).
        code: &'static str,
        /// Human-readable message; safe to render verbatim.
        message: String,
    },

    /// A store failure. Always mapped to 500 in v1 — none of the
    /// read-path variants are user-recoverable.
    #[error("store error: {0}")]
    Store(#[from] StoreError),
}

impl From<ResolveError> for ApiError {
    fn from(err: ResolveError) -> Self {
        let code = match &err {
            ResolveError::InvalidTz(_) => "invalid_tz",
            ResolveError::MissingCustomRange => "missing_custom_range",
            ResolveError::InvertedCustomRange => "inverted_custom_range",
            // `ResolveError` is `#[non_exhaustive]` — fall back to a
            // generic code so adding a variant upstream stays a
            // compile-clean change here.
            _ => "invalid_window",
        };
        ApiError::BadRequest {
            code,
            message: err.to_string(),
        }
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
    code: &'a str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            ApiError::BadRequest { code, message } => {
                (StatusCode::BAD_REQUEST, *code, message.clone())
            }
            ApiError::Store(e) => {
                tracing::error!(error = %e, "store error returned to client");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "store_error",
                    "internal error".to_string(),
                )
            }
        };
        (
            status,
            Json(ErrorBody {
                error: &message,
                code,
            }),
        )
            .into_response()
    }
}
