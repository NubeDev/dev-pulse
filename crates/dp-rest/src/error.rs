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

    /// Row already exists or a unique constraint fired in a way the
    /// caller can recover from (e.g. re-pinning an item that is
    /// already pinned — SCOPE-PROJECTS §6.4). Mapped to `409`.
    #[error("{message}")]
    Conflict {
        /// Stable machine-readable code (e.g. `"pin_exists"`).
        code: &'static str,
        /// Human-readable message; safe to render verbatim.
        message: String,
    },

    /// The targeted row does not exist (e.g. removing a pin that
    /// was never set). Mapped to `404`.
    #[error("{message}")]
    NotFound {
        /// Stable machine-readable code (e.g. `"pin_not_found"`).
        code: &'static str,
        /// Human-readable message; safe to render verbatim.
        message: String,
    },

    /// Caller is authenticated but lacks the capability for the
    /// requested operation — e.g. trying to mutate a tag whose
    /// scope they can see but are not a member of
    /// (SCOPE-PROJECTS §7.4 mutation rule). Mapped to `403`.
    #[error("{message}")]
    Forbidden {
        /// Stable machine-readable code (e.g. `"tag_scope_member_required"`).
        code: &'static str,
        /// Human-readable message; safe to render verbatim.
        message: String,
    },

    /// The caller asked for a write against an org whose GitHub App
    /// install was granted **read-only** (`issues: write` not in
    /// the install's permission set) — or no install record exists
    /// for the org yet (fail-closed). SCOPE-PROJECTS §8.4 / §13.6:
    /// the API mirrors the UI affordance with
    /// `403 writes_not_available_for_org` so callers that bypass
    /// the UI get a deterministic, machine-readable refusal — not
    /// a 500.
    ///
    /// The body carries the offending org's login so the frontend
    /// can render the banner without a second lookup, and a
    /// `manage_url` deep-link the admin-copyable text in §13.6
    /// points at.
    #[error("{message}")]
    WritesNotAvailable {
        /// Stable machine-readable code; always
        /// `"writes_not_available_for_org"`.
        code: &'static str,
        /// Human-readable message; safe to render verbatim.
        message: String,
        /// GitHub login of the org whose install lacks
        /// `issues: write`. Used by the frontend to highlight the
        /// matching row in the §13.6 banner.
        org_login: String,
        /// GitHub-side deep-link to the install's permissions
        /// page — the same URL the §13.6 banner offers as a
        /// copy-able admin link. `None` when dev-pulse has no
        /// install record for the org (fail-closed branch).
        manage_url: Option<String>,
    },

    /// SCOPE §18.3 / §8.3 — the optimistic CAS in the issue write
    /// path missed because the caller's `expected_version` is
    /// behind the local row. The body carries the *current*
    /// `dp_issues.version` so the UI can re-GET the issue and
    /// re-prompt the user with the merged state. Mapped to `409`
    /// with the stable code `stale_local_version`.
    #[error("stale_local_version (current_version = {current_version})")]
    StaleLocalVersion {
        /// Internal issue id the CAS targeted; the UI re-GETs by id.
        issue_id: uuid::Uuid,
        /// The local `dp_issues.version` observed *after* the CAS
        /// miss — what the UI should treat as the new expected
        /// version on its retry.
        current_version: i64,
    },

    /// Per-item validation failure inside a batch request. Used by
    /// the `POST /tags/{id}/links` / `DELETE /tags/{id}/links`
    /// transactional batch path (SCOPE-PROJECTS §7.5): the whole
    /// batch was rejected, and the caller gets one error object per
    /// offending item so the UI can highlight exactly which rows
    /// failed. Mapped to `422`.
    ///
    /// The body shape is `{ error, code, items: [{ index, code,
    /// message }, ...] }` — `code` at the top level is the
    /// envelope-level reason (typically `"batch_rejected"`), each
    /// per-item code is the granular reason (`"target_not_visible"`,
    /// `"wrong_kind"`, `"duplicate"`, …). All-or-nothing semantics:
    /// nothing was committed.
    #[error("{message}")]
    Batch {
        /// Envelope-level code (usually `"batch_rejected"`).
        code: &'static str,
        /// Envelope-level human message.
        message: String,
        /// Per-item failures. Indices reference positions in the
        /// caller's submitted batch.
        items: Vec<BatchItemError>,
    },
}

/// One per-item failure in an [`ApiError::Batch`] response. Always
/// serialises as `{ index, code, message }` — wire-stable so the
/// frontend / MCP client can switch on `code` per item.
#[derive(Debug, Clone, Serialize)]
pub struct BatchItemError {
    /// Zero-based position in the caller's submitted batch.
    pub index: usize,
    /// Stable machine-readable code for this row's failure
    /// (`"target_not_visible"`, `"wrong_kind"`, `"duplicate"`, …).
    pub code: &'static str,
    /// Human-readable message.
    pub message: String,
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

#[derive(Serialize)]
struct BatchErrorBody<'a> {
    error: &'a str,
    code: &'a str,
    items: &'a [BatchItemError],
}

/// Body shape for the §8.3 `stale_local_version` 409. Carries the
/// current `dp_issues.version` (so the UI's next CAS uses it as
/// `expected_version`) and the issue id (so the UI can re-GET the
/// row to refresh the form without a second round-trip lookup).
/// Wire-stable.
#[derive(Serialize)]
struct StaleLocalVersionBody<'a> {
    error: &'a str,
    code: &'a str,
    issue_id: uuid::Uuid,
    current_version: i64,
}

/// Body shape for the §8.4 `writes_not_available_for_org` 403.
/// Carries the offending org's login + a `manage_url` deep-link so
/// the frontend can render the §13.6 banner row without a second
/// round-trip. Wire-stable.
#[derive(Serialize)]
struct WritesNotAvailableBody<'a> {
    error: &'a str,
    code: &'a str,
    org_login: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    manage_url: Option<&'a str>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match &self {
            ApiError::BadRequest { code, message } => (
                StatusCode::BAD_REQUEST,
                Json(ErrorBody {
                    error: message,
                    code,
                }),
            )
                .into_response(),
            ApiError::Store(e) => {
                tracing::error!(error = %e, "store error returned to client");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorBody {
                        error: "internal error",
                        code: "store_error",
                    }),
                )
                    .into_response()
            }
            ApiError::Conflict { code, message } => (
                StatusCode::CONFLICT,
                Json(ErrorBody {
                    error: message,
                    code,
                }),
            )
                .into_response(),
            ApiError::NotFound { code, message } => (
                StatusCode::NOT_FOUND,
                Json(ErrorBody {
                    error: message,
                    code,
                }),
            )
                .into_response(),
            ApiError::Forbidden { code, message } => (
                StatusCode::FORBIDDEN,
                Json(ErrorBody {
                    error: message,
                    code,
                }),
            )
                .into_response(),
            ApiError::WritesNotAvailable {
                code,
                message,
                org_login,
                manage_url,
            } => (
                StatusCode::FORBIDDEN,
                Json(WritesNotAvailableBody {
                    error: message,
                    code,
                    org_login,
                    manage_url: manage_url.as_deref(),
                }),
            )
                .into_response(),
            ApiError::StaleLocalVersion {
                issue_id,
                current_version,
            } => (
                StatusCode::CONFLICT,
                Json(StaleLocalVersionBody {
                    error: "stale_local_version",
                    code: "stale_local_version",
                    issue_id: *issue_id,
                    current_version: *current_version,
                }),
            )
                .into_response(),
            ApiError::Batch {
                code,
                message,
                items,
            } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(BatchErrorBody {
                    error: message,
                    code,
                    items,
                }),
            )
                .into_response(),
        }
    }
}
