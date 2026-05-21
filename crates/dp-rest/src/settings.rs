//! Per-user settings — the REST surface behind the frontend
//! "Account → Settings" page.
//!
//! Backed by `dp_user_settings` (migration 0029). Designed as a
//! pinned-catalogue K/V so new settings ship as one entry in
//! [`KEYS`] + a frontend form field — never a schema migration.
//!
//! ## Routes
//!
//! | route                            | purpose                                                  |
//! |----------------------------------|----------------------------------------------------------|
//! | `GET    /me/settings`            | list all of the caller's settings (secrets redacted)     |
//! | `GET    /me/settings/{key}`      | fetch one setting (secret redacted to `{ has_value }`)   |
//! | `PUT    /me/settings/{key}`      | upsert one setting (`{ value }`)                         |
//! | `DELETE /me/settings/{key}`      | remove one setting                                       |
//!
//! ## Pinned key catalogue ([`KEYS`])
//!
//! Every (key, is_secret) pair the server will accept is
//! enumerated here. Unknown keys return `400 unknown_setting`
//! so a typo can't silently grow the schema. Adding a new
//! setting is a one-line edit to [`KEYS`] plus the frontend
//! form field — no migration, no Rust handler change.
//!
//! v1 catalogue:
//!
//! | key              | secret? | purpose                                              |
//! |------------------|---------|------------------------------------------------------|
//! | `github.pat`     | ✅      | per-user GitHub PAT for issue writes / mirror calls. |
//!
//! ## Secrets
//!
//! When [`SettingSpec::is_secret`] is `true`, the GET handlers
//! redact `value` to `null` and set [`SettingDto::has_value`] so
//! the UI can render "•••• (set)" without ever receiving the
//! plaintext. The write path obviously needs the plaintext and
//! accepts it via the JSON body — only over the
//! cookie-authenticated session, which is the same trust
//! envelope as every other `/me/*` mutation.
//!
//! v1 stores values as plain TEXT in Postgres (see migration
//! `0029_user_settings.sql` TODO). A follow-up should wrap
//! `is_secret == true` values with the same age key
//! `starter-secrets-file` already loads for the webhook secret.
//!
//! ## Audit
//!
//! Every mutating route writes one audit row via
//! [`crate::audit::record`]. The target is `"<key>"` — never
//! the value, even for non-secret keys (audit rows are not the
//! place to surface user input).

use std::sync::Arc;

use axum::{
    extract::{Extension, Path, State},
    response::Json,
    routing::{get, post},
    Router,
};
use chrono::{DateTime, Utc};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use dp_domain::setting::UserSetting;
use dp_domain::store::StoreError;
use dp_fetcher::client::{Client as GhClient, ClientError as GhClientError, Fetched};

use crate::audit::{self, Principal};
use crate::directory::Ack;
use crate::error::ApiError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Pinned key catalogue
// ---------------------------------------------------------------------------

/// One row in the pinned [`KEYS`] catalogue.
#[derive(Debug, Clone, Copy)]
pub struct SettingSpec {
    /// Dotted-namespace key (e.g. `"github.pat"`).
    pub key: &'static str,
    /// When `true`, GET handlers redact `value` to `null` and
    /// surface only `has_value`. The bit is also stored on the
    /// row (defence-in-depth for future direct-DB consumers).
    pub is_secret: bool,
    /// Human-friendly label the UI can render next to the
    /// input. Kept here so the frontend doesn't have to embed
    /// a parallel constant.
    pub label: &'static str,
    /// One-line help text the UI can render under the input.
    pub help: &'static str,
}

/// Every setting key the server accepts. Unknown keys hit
/// `400 unknown_setting`. Add a new setting by appending a row
/// here + a frontend form field — no migration, no other Rust
/// change.
pub const KEYS: &[SettingSpec] = &[
    SettingSpec {
        key: "github.pat",
        is_secret: true,
        label: "GitHub Personal Access Token",
        help: "Used for issue writes and Projects v2 mirror calls performed by you. \
               Minimum scopes: repo, read:org. Leave blank to fall back to the \
               server-wide PAT.",
    },
];

/// Look up a key in [`KEYS`]; returns `None` for unknown keys
/// so the REST layer can map to `400 unknown_setting`.
pub fn spec_for(key: &str) -> Option<&'static SettingSpec> {
    KEYS.iter().find(|s| s.key == key)
}

// ---------------------------------------------------------------------------
// Wire DTOs
// ---------------------------------------------------------------------------

/// One setting on the wire. `value` is `null` for unset keys
/// and for `is_secret` keys that have been set — the UI reads
/// `has_value` to render the "set" affordance.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SettingDto {
    /// Dotted-namespace key.
    pub key: String,
    /// Human-friendly label from [`SettingSpec::label`].
    pub label: String,
    /// One-line help text from [`SettingSpec::help`].
    pub help: String,
    /// Whether the catalogue marks this key as a secret. The UI
    /// uses this to pick `<input type="password">` vs plain text
    /// and to render the redaction affordance.
    pub is_secret: bool,
    /// Whether a row exists for `(user, key)`. `true` for both
    /// secret and non-secret keys when the user has set a value.
    pub has_value: bool,
    /// The actual value. `null` when the key is `is_secret` or
    /// when no row exists. Non-secret keys with a row return the
    /// plaintext here.
    pub value: Option<String>,
    /// When the row was last written. `None` for unset keys.
    pub updated_at: Option<DateTime<Utc>>,
}

impl SettingDto {
    /// Build a DTO from a catalogue spec + the optional store row.
    /// Handles redaction so handlers don't have to.
    fn from_parts(spec: &SettingSpec, row: Option<&UserSetting>) -> Self {
        let has_value = row.is_some();
        let value = match (spec.is_secret, row) {
            (true, _) => None,
            (false, Some(r)) => Some(r.value.clone()),
            (false, None) => None,
        };
        Self {
            key: spec.key.to_string(),
            label: spec.label.to_string(),
            help: spec.help.to_string(),
            is_secret: spec.is_secret,
            has_value,
            value,
            updated_at: row.map(|r| r.updated_at),
        }
    }
}

/// Body for `PUT /me/settings/{key}`. `value` is the plaintext
/// the user typed (for secret keys, this is the only ingress
/// path — the GET handlers never echo it back).
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct PutSettingRequest {
    /// New value for the setting. Empty string is accepted and
    /// distinguishable from "no row" — pass `DELETE` to remove
    /// the row entirely.
    pub value: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /me/settings` — list every catalogue entry, joined with
/// the caller's row when one exists.
///
/// Returns one [`SettingDto`] per entry in [`KEYS`] (not per
/// row) so the frontend can render the full settings form even
/// for a brand-new user with zero saved settings. Secret values
/// are always redacted (see [`SettingDto::value`]).
#[utoipa::path(
    get,
    path = "/me/settings",
    responses(
        (status = 200, description = "All settings (secrets redacted)", body = Vec<SettingDto>),
    ),
    tag = "settings",
)]
pub async fn list_settings(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Result<Json<Vec<SettingDto>>, ApiError> {
    let rows = state
        .store
        .list_user_settings(principal.actor_user_id)
        .await?;
    let dtos: Vec<SettingDto> = KEYS
        .iter()
        .map(|spec| {
            let row = rows.iter().find(|r| r.key == spec.key);
            SettingDto::from_parts(spec, row)
        })
        .collect();
    Ok(Json(dtos))
}

/// `GET /me/settings/{key}` — fetch a single setting.
///
/// Returns `400 unknown_setting` for keys not in [`KEYS`] and
/// a [`SettingDto`] with `has_value: false` for known-but-unset
/// keys (i.e. *not* a 404 — the catalogue entry always exists).
#[utoipa::path(
    get,
    path = "/me/settings/{key}",
    params(("key" = String, Path, description = "Pinned key from the catalogue")),
    responses(
        (status = 200, description = "Setting (secret redacted)", body = SettingDto),
        (status = 400, description = "Key not in the pinned catalogue"),
    ),
    tag = "settings",
)]
pub async fn get_setting(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(key): Path<String>,
) -> Result<Json<SettingDto>, ApiError> {
    let spec = spec_for(&key).ok_or_else(|| unknown_setting(&key))?;
    let row = state
        .store
        .get_user_setting(principal.actor_user_id, spec.key)
        .await?;
    Ok(Json(SettingDto::from_parts(spec, row.as_ref())))
}

/// `PUT /me/settings/{key}` — upsert one setting.
///
/// Returns `400 unknown_setting` for keys not in [`KEYS`]. The
/// response is the post-write [`SettingDto`] (with redaction
/// applied for secret keys, so the UI can confirm "set" without
/// ever receiving the value back).
///
/// Audit: writes [`audit::SETTING_SET`] with target = the key
/// (never the value).
#[utoipa::path(
    put,
    path = "/me/settings/{key}",
    params(("key" = String, Path, description = "Pinned key from the catalogue")),
    request_body = PutSettingRequest,
    responses(
        (status = 200, description = "Setting upserted (secret redacted in response)", body = SettingDto),
        (status = 400, description = "Key not in the pinned catalogue"),
    ),
    tag = "settings",
)]
pub async fn put_setting(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(key): Path<String>,
    Json(body): Json<PutSettingRequest>,
) -> Result<Json<SettingDto>, ApiError> {
    let spec = spec_for(&key).ok_or_else(|| unknown_setting(&key))?;
    let user_id = principal.actor_user_id;
    let setting = UserSetting {
        user_id,
        key: spec.key.to_string(),
        value: body.value,
        is_secret: spec.is_secret,
        // Bumped server-side in the upsert; this is just a placeholder.
        updated_at: Utc::now(),
    };
    let saved = state.store.upsert_user_setting(&setting).await?;
    audit::record(
        state.store.as_ref(),
        user_id,
        audit::SETTING_SET,
        spec.key.to_string(),
    )
    .await?;
    Ok(Json(SettingDto::from_parts(spec, Some(&saved))))
}

/// `DELETE /me/settings/{key}` — remove one setting.
///
/// Returns `400 unknown_setting` for keys not in [`KEYS`] and
/// `404 setting_unset` when the key is known but no row exists.
///
/// Audit: writes [`audit::SETTING_DELETE`] with target = the key.
#[utoipa::path(
    delete,
    path = "/me/settings/{key}",
    params(("key" = String, Path, description = "Pinned key from the catalogue")),
    responses(
        (status = 200, description = "Setting removed", body = Ack),
        (status = 400, description = "Key not in the pinned catalogue"),
        (status = 404, description = "Key is known but no row exists for the caller"),
    ),
    tag = "settings",
)]
pub async fn delete_setting(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(key): Path<String>,
) -> Result<Json<Ack>, ApiError> {
    let spec = spec_for(&key).ok_or_else(|| unknown_setting(&key))?;
    let user_id = principal.actor_user_id;
    match state.store.delete_user_setting(user_id, spec.key).await {
        Ok(()) => {}
        Err(StoreError::NotFound { .. }) => {
            return Err(ApiError::NotFound {
                code: "setting_unset",
                message: format!("setting {} is not set for caller", spec.key),
            });
        }
        Err(e) => return Err(e.into()),
    }
    audit::record(
        state.store.as_ref(),
        user_id,
        audit::SETTING_DELETE,
        spec.key.to_string(),
    )
    .await?;
    Ok(Json(Ack { ok: true }))
}

fn unknown_setting(key: &str) -> ApiError {
    ApiError::BadRequest {
        code: "unknown_setting",
        message: format!(
            "setting key `{key}` is not in the pinned catalogue; \
             see GET /me/settings for the supported list"
        ),
    }
}

// ---------------------------------------------------------------------------
// `POST /me/settings/github.pat/test` — diagnostic
// ---------------------------------------------------------------------------

/// Response from the `github.pat` connectivity probe. Carries
/// either an `ok: true` payload with the GitHub identity the
/// token resolves to, or `ok: false` with a stable `code` the
/// UI can switch on.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(tag = "ok")]
pub enum TestGithubPatResponse {
    /// Token authenticated successfully against `GET /user`.
    #[serde(rename = "true")]
    Ok {
        /// GitHub login the token belongs to.
        login: String,
        /// Display name from the GitHub profile (may be `null`).
        name: Option<String>,
        /// Account type — usually `"User"`.
        account_type: Option<String>,
    },
    /// Token did not authenticate. `code` is one of
    /// `unauthorized`, `rate_limited`, `network`, `unset`.
    #[serde(rename = "false")]
    Err {
        /// Stable machine code.
        code: &'static str,
        /// Human-readable message safe to render.
        message: String,
    },
}

/// `POST /me/settings/github.pat/test` — call `GET /user` on
/// github.com with the caller's stored PAT and report the
/// outcome. Diagnostic only; does not store or surface the
/// token value back to the caller. Returns `200` for both
/// success and failure outcomes so the UI can switch on the
/// `ok` discriminator instead of catching errors.
#[utoipa::path(
    post,
    path = "/me/settings/github.pat/test",
    responses(
        (status = 200, description = "Probe result (ok | err)", body = TestGithubPatResponse),
    ),
    tag = "settings",
)]
pub async fn test_github_pat(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Result<Json<TestGithubPatResponse>, ApiError> {
    let row = state
        .store
        .get_user_setting(principal.actor_user_id, "github.pat")
        .await?;
    let token = match row {
        Some(r) if !r.value.is_empty() => r.value,
        _ => {
            return Ok(Json(TestGithubPatResponse::Err {
                code: "unset",
                message: "No GitHub PAT is set for your account.".into(),
            }));
        }
    };

    // v1: target github.com. GHE base-url support is a future
    // setting (e.g. `github.base_url`); not in the catalogue yet.
    let base_url = "https://api.github.com";
    let client = match GhClient::with_personal_token(SecretString::from(token), base_url) {
        Ok(c) => c,
        Err(e) => {
            return Ok(Json(TestGithubPatResponse::Err {
                code: "network",
                message: format!("GitHub client init failed: {e}"),
            }));
        }
    };

    match client.get_authenticated_user().await {
        Ok(Fetched::Ok { body, .. }) => {
            let login = body
                .get("login")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = body.get("name").and_then(|v| v.as_str()).map(String::from);
            let account_type = body.get("type").and_then(|v| v.as_str()).map(String::from);
            Ok(Json(TestGithubPatResponse::Ok {
                login,
                name,
                account_type,
            }))
        }
        // 304 with no body is impossible without an If-None-Match;
        // treat as ok-but-empty for robustness.
        Ok(Fetched::NotModified { .. }) => Ok(Json(TestGithubPatResponse::Err {
            code: "network",
            message: "GitHub returned 304 unexpectedly.".into(),
        })),
        Err(GhClientError::Unauthorized) => Ok(Json(TestGithubPatResponse::Err {
            code: "unauthorized",
            message: "GitHub rejected the token (401). Check that the PAT is correct, \
                      not expired, and has the required scopes (repo, read:org)."
                .into(),
        })),
        Err(GhClientError::PrimaryRateLimit { reset_at }) => {
            Ok(Json(TestGithubPatResponse::Err {
                code: "rate_limited",
                message: format!("Primary rate limit hit; resets at {reset_at}."),
            }))
        }
        Err(GhClientError::SecondaryRateLimit { retry_at }) => {
            Ok(Json(TestGithubPatResponse::Err {
                code: "rate_limited",
                message: format!("Secondary rate limit; retry at {retry_at}."),
            }))
        }
        Err(e) => Ok(Json(TestGithubPatResponse::Err {
            code: "network",
            message: format!("GitHub request failed: {e}"),
        })),
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the `/me/settings` router fragment. Gated by
/// `(settings, read)` for the GET routes and `(settings, write)`
/// for the PUT / DELETE routes — same split as the pins surface
/// (§6.4).
pub fn settings_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    let reads = with_permission(
        Router::new()
            .route("/me/settings", get(list_settings))
            .route("/me/settings/{key}", get(get_setting)),
        "settings",
        "read",
    );
    let writes = with_permission(
        Router::new()
            .route(
                "/me/settings/{key}",
                axum::routing::put(put_setting).delete(delete_setting),
            )
            .route("/me/settings/github.pat/test", post(test_github_pat)),
        "settings",
        "write",
    );
    Router::new().merge(reads).merge(writes).with_state(inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn redacts_secret_values() {
        let spec = SettingSpec {
            key: "github.pat",
            is_secret: true,
            label: "",
            help: "",
        };
        let row = UserSetting {
            user_id: Uuid::nil(),
            key: "github.pat".into(),
            value: "ghp_secret".into(),
            is_secret: true,
            updated_at: Utc::now(),
        };
        let dto = SettingDto::from_parts(&spec, Some(&row));
        assert!(dto.has_value);
        assert!(dto.value.is_none(), "secret value must not surface");
    }

    #[test]
    fn surfaces_plain_values() {
        let spec = SettingSpec {
            key: "ui.theme",
            is_secret: false,
            label: "",
            help: "",
        };
        let row = UserSetting {
            user_id: Uuid::nil(),
            key: "ui.theme".into(),
            value: "dark".into(),
            is_secret: false,
            updated_at: Utc::now(),
        };
        let dto = SettingDto::from_parts(&spec, Some(&row));
        assert_eq!(dto.value.as_deref(), Some("dark"));
    }

    #[test]
    fn unset_keys_have_no_value() {
        let spec = &KEYS[0];
        let dto = SettingDto::from_parts(spec, None);
        assert!(!dto.has_value);
        assert!(dto.value.is_none());
        assert!(dto.updated_at.is_none());
    }

    #[test]
    fn spec_for_known_and_unknown() {
        assert!(spec_for("github.pat").is_some());
        assert!(spec_for("does.not.exist").is_none());
    }
}
