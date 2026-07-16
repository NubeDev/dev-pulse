//! `POST /me/password` — self-serve password change (issue #14).
//!
//! The counterpart to `PUT /admin/users/{id}/password`. Two
//! differences, both deliberate:
//!
//! 1. **No path parameter.** The target is always
//!    `principal.actor_user_id`. There is no way to spell "someone
//!    else's password" against this route, so no ownership check can
//!    be forgotten.
//! 2. **The current password is verified first.** A stolen session
//!    cookie alone cannot rotate the credential — the attacker would
//!    also need the password they are trying to replace.
//!
//! The permission lane is `(identities, read)`: every authenticated
//! user passes it, which is correct here. The security boundary for
//! this route is the current-password check in
//! `starter_auth_users::admin::change_password`, not the RBAC gate —
//! gating it on anything narrower would lock Readers out of changing
//! their own password, which is the whole point of the route.

use std::sync::Arc;

use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::post,
    Router,
};
use serde::Deserialize;
use starter_auth_users::admin::ChangePasswordError;
use utoipa::ToSchema;

use crate::audit::{self, Principal};
use crate::error::ApiError;
use crate::state::AppState;

/// Body for `POST /me/password`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ChangeMyPasswordRequest {
    /// The caller's current password. Verified before anything is
    /// written.
    pub current_password: String,
    /// The replacement. Subject to the same strength policy as
    /// signup.
    pub new_password: String,
}

/// `POST /me/password` — change your own password.
///
/// Audit: writes [`audit::USER_PASSWORD_CHANGED`] with target
/// `user:<id>` (actor and target are the same user). The passwords
/// themselves are never logged.
#[utoipa::path(
    post,
    path = "/me/password",
    request_body = ChangeMyPasswordRequest,
    responses(
        (status = 204, description = "Password changed"),
        (status = 400, description = "New password failed strength validation"),
        (status = 403, description = "Current password incorrect, or no local password set"),
        (status = 503, description = "Local password store not wired in this deployment"),
    ),
    tag = "me",
)]
pub async fn change_my_password(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(req): Json<ChangeMyPasswordRequest>,
) -> Result<StatusCode, Response> {
    let Some(users) = state.user_store.clone() else {
        // Same rationale as `/me/identities`: a deployment that never
        // wired the store should fail loudly, not report success for a
        // password change that went nowhere.
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "local password store not configured",
        )
            .into_response());
    };
    let user_id = principal.actor_user_id.to_string();

    starter_auth_users::admin::change_password(
        users.as_ref(),
        &user_id,
        &req.current_password,
        &req.new_password,
    )
    .await
    .map_err(|e| match e {
        // A wrong current password and a nonexistent local account are
        // both 403 with the same shape on purpose — distinguishing
        // them would tell an attacker with a session cookie whether
        // the account has a local password at all.
        ChangePasswordError::WrongPassword | ChangePasswordError::NotFound => ApiError::Forbidden {
            code: "wrong_password",
            message: "current password is incorrect".into(),
        }
        .into_response(),
        ChangePasswordError::PasswordNotSet => ApiError::Forbidden {
            code: "password_not_set",
            message: "this account signs in with GitHub and has no local password; \
                      ask an operator to set one"
                .into(),
        }
        .into_response(),
        ChangePasswordError::Validation(msg) => ApiError::BadRequest {
            code: "weak_password",
            message: msg,
        }
        .into_response(),
        other => {
            tracing::warn!(
                target: "dp_rest::me_password",
                user_id = %user_id,
                error = %other,
                "password change failed",
            );
            (StatusCode::INTERNAL_SERVER_ERROR, "password change failed").into_response()
        }
    })?;

    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::USER_PASSWORD_CHANGED,
        format!("user:{user_id}"),
    )
    .await
    .map_err(|e| ApiError::Store(e).into_response())?;
    Ok(StatusCode::NO_CONTENT)
}

/// Build the `/me/password` router fragment. See the module docs for
/// why this rides the `(identities, read)` lane.
pub fn me_password_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new().route("/me/password", post(change_my_password)),
            "identities",
            "read",
        ))
        .with_state(inner)
}
