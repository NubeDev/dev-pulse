//! `GET /me/identities` — the dev-pulse-shaped projection of the
//! viewer's linked OAuth identities, per
//! `linear-projects-idea.md` §3.0 / §10.
//!
//! The starter ecosystem already ships
//! `GET /auth/oauth/identities` (in
//! `starter_auth_oauth::routes::list`). That endpoint is fine for
//! a generic "linked accounts" list, but the §10 multi-identity
//! page wants a few dev-pulse-specific affordances on top:
//!
//! * a stable `id` per row the SPA can key React renders on without
//!   leaking the raw provider subject (`provider_sub`),
//! * an `is_primary` flag — for v1 the primary is "most recent
//!   `linked_at`", matching the
//!   [`starter_auth_oauth::OAuthPrincipalExtras`] convention so the
//!   primary identity here agrees with whatever the authz `oauth.*`
//!   bus block reflects,
//! * a tag on the response carrying `primary_id` separately so the
//!   frontend can render "Primary" badges without rescanning the
//!   list.
//!
//! Write surfaces (link / unlink / set-primary / transfer) are
//! deliberately *not* in this module. `link` is just the existing
//! `GET /auth/oauth/github` start URL; `unlink` is the existing
//! `DELETE /auth/oauth/{provider}` in starter; `set-primary` and
//! `transfer` need schema work (`is_primary` column + the
//! `dp_membership_identities` provenance table) that §3.0 calls
//! out as a separate slice. Until they land the frontend keeps
//! its client-side stubs and the user-visible "deferred" toasts.
//!
//! Routing: gated by the `(identities, read)` resource the
//! dp-server policy registry registers at boot. The boundary is
//! intentionally narrower than `(users, read)` because identity
//! provenance is more sensitive than the user row — a future
//! deployment may want to expose `users.read` to teammates
//! (directory-style) without leaking the linked-account list.

use std::sync::Arc;

use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::get,
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use starter_auth_oauth::OAuthIdentity;
use utoipa::ToSchema;

use crate::audit::Principal;
use crate::state::AppState;

/// One linked identity in the response. `id` is a synthetic
/// stable handle the SPA can key on — it is `"{provider}:{sub}"`
/// rather than the bare `provider_sub` so two rows from different
/// providers can never collide and so the SPA never has to assume
/// a specific provider's sub format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MeIdentityDto {
    /// Synthetic stable id: `"{provider}:{provider_sub}"`. Safe to
    /// expose because the SPA already sees `provider`, and
    /// `provider_sub` for the *currently signed-in* identity is
    /// already in the OAuth callback URL flow — wrapping it in a
    /// composite `id` keys React renders without inviting the SPA
    /// to parse the sub out by itself.
    pub id: String,
    /// Provider id (`"github"`, `"google"`).
    pub provider: String,
    /// Email the provider returned at link / last sign-in.
    pub email: Option<String>,
    /// Display name the provider returned at link / last sign-in.
    /// Often the GitHub login or the Google given-name — the wire
    /// shape is provider-defined.
    pub display_name: Option<String>,
    /// Wall-clock time the identity was first linked. v1 doubles
    /// as "last login" because `IdentityStore` does not yet touch
    /// the row on sign-in; the contract here is the same as
    /// `starter_auth_oauth::IdentitySummary`'s `last_login_at`.
    pub linked_at: DateTime<Utc>,
    /// `true` for the row currently treated as the primary
    /// identity (the most-recent `linked_at`, matching
    /// [`starter_auth_oauth::OAuthPrincipalExtras`]).
    pub is_primary: bool,
}

/// Top-level response. JSON object (not bare list) so additive
/// fields like `principal_dirty` / pagination metadata can land
/// without a breaking change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MeIdentitiesResponse {
    /// The viewer's linked identities, ordered by `linked_at`
    /// **descending** (newest first — the §10 page renders the
    /// primary at the top of the stack).
    pub identities: Vec<MeIdentityDto>,
    /// The `id` of the row marked `is_primary == true`, or `None`
    /// if the viewer has no linked identities (i.e. local-password
    /// only). The SPA reads this instead of re-scanning the list.
    pub primary_id: Option<String>,
}

/// Render the `(provider, provider_sub)` composite as the wire
/// `id`. Kept in one place so a future change to the synthetic
/// shape only edits one site.
fn synthetic_id(provider: &str, provider_sub: &str) -> String {
    format!("{provider}:{provider_sub}")
}

/// Sort rows newest-first and stamp `is_primary` on the head row.
/// Pulled out of the handler so a unit test can exercise the
/// ordering rule without spinning up axum.
fn project(rows: Vec<OAuthIdentity>) -> MeIdentitiesResponse {
    let mut sorted = rows;
    // Newest `linked_at` wins as primary; the secondary sort on
    // `(provider, provider_sub)` keeps the order deterministic
    // when two rows share a timestamp (CI fixture / clock skew).
    sorted.sort_by(|a, b| {
        b.linked_at
            .cmp(&a.linked_at)
            .then_with(|| a.provider.cmp(&b.provider))
            .then_with(|| a.provider_sub.cmp(&b.provider_sub))
    });
    let identities: Vec<MeIdentityDto> = sorted
        .iter()
        .enumerate()
        .map(|(i, row)| MeIdentityDto {
            id: synthetic_id(&row.provider, &row.provider_sub),
            provider: row.provider.clone(),
            email: row.email.clone(),
            display_name: row.display_name.clone(),
            linked_at: row.linked_at,
            is_primary: i == 0,
        })
        .collect();
    let primary_id = identities.first().map(|r| r.id.clone());
    MeIdentitiesResponse {
        identities,
        primary_id,
    }
}

/// `GET /me/identities`.
#[utoipa::path(
    get,
    path = "/me/identities",
    responses(
        (status = 200, description = "Linked OAuth identities for the caller", body = MeIdentitiesResponse),
        (status = 503, description = "Identity store not wired in this deployment"),
    ),
    tag = "identities",
)]
pub async fn list_me_identities(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Result<Json<MeIdentitiesResponse>, Response> {
    let Some(store) = state.identity_store.clone() else {
        // The bin layer wires this in production; tests can
        // leave it unset. Fail loud rather than returning an
        // empty list — an operator looking at the SPA would not
        // be able to tell "no linked identities" from "store not
        // wired" otherwise.
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "identity store not configured",
        )
            .into_response());
    };
    let user_id = principal.actor_user_id.to_string();
    let rows = store.list_for_user(&user_id).await.map_err(|e| {
        tracing::warn!(
            target: "dp_rest::me_identities",
            user_id = %user_id,
            error = %e,
            "identity_store.list_for_user failed",
        );
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "identity store read failed",
        )
            .into_response()
    })?;
    Ok(Json(project(rows)))
}

/// Build the `/me/identities` router fragment. Gated by
/// `(identities, read)` — the resource is registered in
/// `dp_server::auth::policy::register_dev_pulse_resources`.
pub fn me_identities_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new().route("/me/identities", get(list_me_identities)),
            "identities",
            "read",
        ))
        .with_state(inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(provider: &str, sub: &str, offset_secs: i64) -> OAuthIdentity {
        OAuthIdentity {
            provider: provider.into(),
            provider_sub: sub.into(),
            user_id: "user-1".into(),
            email: Some(format!("{sub}@example.test")),
            display_name: Some(sub.into()),
            linked_at: DateTime::<Utc>::from_timestamp(offset_secs, 0).unwrap(),
        }
    }

    #[test]
    fn empty_input_yields_no_primary() {
        let r = project(vec![]);
        assert!(r.identities.is_empty());
        assert!(r.primary_id.is_none());
    }

    #[test]
    fn most_recent_is_primary_and_response_orders_newest_first() {
        // Oldest first in input; the projector must sort newest-first.
        let rows = vec![
            row("github", "g-old", 100),
            row("google", "go-mid", 200),
            row("github", "g-new", 300),
        ];
        let r = project(rows);
        let ids: Vec<&str> = r.identities.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["github:g-new", "google:go-mid", "github:g-old"]
        );
        assert!(r.identities[0].is_primary);
        assert!(!r.identities[1].is_primary);
        assert!(!r.identities[2].is_primary);
        assert_eq!(r.primary_id.as_deref(), Some("github:g-new"));
    }

    #[test]
    fn tie_break_is_deterministic_on_provider_then_sub() {
        let rows = vec![
            row("google", "z", 100),
            row("github", "a", 100),
            row("github", "b", 100),
        ];
        let r = project(rows);
        // All share linked_at, so the secondary keys decide. We
        // walk *forward* by provider, then by sub. Newest-first
        // ordering does not flip secondary keys.
        let ids: Vec<&str> = r.identities.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["github:a", "github:b", "google:z"]);
        // Whichever row landed on top is `is_primary` — deterministically
        // the github/a row.
        assert!(r.identities[0].is_primary);
        assert_eq!(r.primary_id.as_deref(), Some("github:a"));
    }
}
