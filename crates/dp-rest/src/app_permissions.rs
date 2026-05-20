//! GitHub App permission surface — SCOPE-PROJECTS §8.4 + §13.6.
//!
//! Stage 8 of the projects-issues job introduces the App permission
//! bump (`issues: write` joins the default permission set) behind
//! the `dp-config` flag `github.app.request_issues_write`
//! ([`GitHubAppConfig::request_issues_write`]). Three deliverables
//! land here:
//!
//! 1. **Configuration shape** — [`GitHubAppConfig`] is the
//!    deployment-shaped flag bundle. The bin layer reads it out of
//!    `[github.app]` in `dp-config` and hands it through
//!    [`crate::AppState::github_app`].
//! 2. **Write-gate** — [`require_issues_write`] is the single
//!    function every future §8 write handler routes through. It
//!    checks (a) that the deployment-level flag is on and (b) that
//!    the per-org install record allows `issues: write`. On miss it
//!    returns the §8.4
//!    [`ApiError::WritesNotAvailable`][crate::ApiError::WritesNotAvailable]
//!    so the caller (UI or MCP / curl) gets a deterministic
//!    `403 writes_not_available_for_org` with the org's login and
//!    the GitHub-side manage-permissions deep-link.
//! 3. **Migration banner** — `GET /me/app-install-banner` returns
//!    one row per org the viewer is in, marking each as
//!    `writes_available` or not and (when not) carrying a
//!    copy-able admin text snippet the viewer can paste into Slack
//!    / email to ask the org admin to re-consent. §13.6 says this
//!    is the **one-shot** prompt; the §8.4 affordance is the
//!    steady-state. The handler is read-only and not audited.
//!
//! Nothing here calls GitHub directly. The per-org install record
//! is whatever the fetcher / install-callback has written into the
//! store (`Store::get_org_app_install`); stage 8 reads it through
//! the trait method and treats `None` as fail-closed.
//!
//! The App manifest itself — the JSON GitHub renders when an admin
//! installs the App — is built from [`GitHubAppConfig`] via
//! [`app_manifest_permissions`]. Deployment tooling (and a
//! future `/admin/app-manifest` endpoint) renders this so the
//! `issues: write` request is *declared*, not silently expected.

use std::sync::Arc;

use axum::{
    extract::{Extension, State},
    response::Json,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::store::Store;
use dp_domain::Org;

use crate::audit::Principal;
use crate::error::ApiError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// The `[github.app]` block from `dp-config`.
///
/// SCOPE-PROJECTS §13.6 step 1: the manifest change ships behind
/// [`request_issues_write`] (default `true` in new deployments,
/// `false` for the documented "hard-disable §8" escape hatch). When
/// `false`, the §8 issue mutation surface is disabled wholesale —
/// the write-gate returns `WritesNotAvailable` for every org
/// without consulting the install record, the App manifest stops
/// requesting `issues: write` at install time, and tag links of
/// kind `issue` continue to work because the surrounding tag
/// surface does not need the write scope.
///
/// [`request_issues_write`]: GitHubAppConfig::request_issues_write
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubAppConfig {
    /// Deployment-level switch for §8 writes. `true` (the default)
    /// declares `issues: write` in the App's default permission
    /// set; the write-gate then defers to the per-org install
    /// record. `false` hard-disables §8 — useful for deployments
    /// whose security policy forbids any App with write scope
    /// (§13.6 "Revisit if").
    #[serde(default = "default_request_issues_write")]
    pub request_issues_write: bool,
    /// GitHub App slug used to build the `manage_url` deep-link
    /// the §13.6 banner offers as a copy-able admin link. Format:
    /// `https://github.com/organizations/<org>/settings/installations/<install>/permissions`
    /// — but stage 8 only owns the construction helper, not the
    /// slug; the bin layer fills this in from `dp-config`.
    ///
    /// `None` (or empty) means we omit `manage_url` from
    /// responses and the §13.6 banner shows the copy-able text
    /// without a deep-link button.
    #[serde(default)]
    pub slug: Option<String>,
}

fn default_request_issues_write() -> bool {
    true
}

impl Default for GitHubAppConfig {
    /// Mirrors the §13.6 step-1 default: `request_issues_write =
    /// true`, no slug. Tests that need the disabled branch
    /// construct the struct explicitly.
    fn default() -> Self {
        Self {
            request_issues_write: true,
            slug: None,
        }
    }
}

impl GitHubAppConfig {
    /// Construct the `manage_url` deep-link for a given org login
    /// + installation id. Returns `None` when no `slug` is set;
    /// the §13.6 banner row then renders the copy-able text only.
    ///
    /// Format: GitHub-side per-install permissions page,
    /// `https://github.com/organizations/<org>/settings/installations/<install>`.
    /// The `slug` field is the *App* slug (e.g. `"dev-pulse"`),
    /// not the org's. We surface it back in [`AppManifest::slug`]
    /// so the banner copy text can mention the App by name.
    pub fn manage_url(&self, org_login: &str, installation_id: i64) -> Option<String> {
        // The deep-link only needs the org login + installation id;
        // the App slug is informational. Skip the URL entirely if
        // no slug is configured so the caller doesn't render a
        // half-built link.
        self.slug.as_ref().map(|_| {
            format!(
                "https://github.com/organizations/{org_login}/settings/installations/{installation_id}"
            )
        })
    }

    /// Render the §13.6 copy-able admin text for one org. The
    /// banner shows this in a `<pre>`-style block with a
    /// copy-to-clipboard button; the viewer pastes it into Slack
    /// or email to nudge their org admin to re-consent.
    ///
    /// Wire-stable: the exact string is part of the §13.6 UI
    /// contract, so changes ripple to the frontend. Keep edits
    /// surgical.
    pub fn admin_copy_text(&self, org_login: &str) -> String {
        let app_name = self.slug.as_deref().unwrap_or("dev-pulse");
        format!(
            "Hi — could you re-consent the `{app_name}` GitHub App on the `{org_login}` \
             org so it has `issues: write`? dev-pulse needs it to file / edit / close issues \
             on your team's behalf from the dashboard. The install lives at \
             github.com/organizations/{org_login}/settings/installations. \
             Until then, dev-pulse will silently fall back to read-only on `{org_login}` and \
             show a 'writes not available' affordance on the affected issues."
        )
    }
}

/// The App's default permission set as declared in its manifest.
///
/// Each key is a GitHub permission resource (e.g. `"issues"`,
/// `"metadata"`); each value is the requested level (`"read"` or
/// `"write"`). [`app_manifest_permissions`] renders this from
/// [`GitHubAppConfig`] — `issues` is `"write"` when
/// [`GitHubAppConfig::request_issues_write`] is `true`, omitted
/// otherwise. The other resources are the read-only set the v1
/// reporting surface needs and do not change with the flag.
///
/// Used by deployment tooling and the (future) `/admin/app-manifest`
/// surface. Pure value; no side-effects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AppManifest {
    /// App slug — informational, copied verbatim from
    /// [`GitHubAppConfig::slug`]. Empty when unset.
    pub slug: String,
    /// `resource -> level` map. Serialised in BTreeMap order so
    /// the JSON is deterministic across runs (the OpenAPI
    /// snapshot test cares).
    pub default_permissions: BTreeMap<String, String>,
}

/// Render the App manifest's default permission set from config.
///
/// Always includes the v1 read-only set
/// (`metadata`, `contents`, `pull_requests`, `members`) plus
/// `issues: write` when [`GitHubAppConfig::request_issues_write`]
/// is `true`. The §13.6 "hard-disable §8" escape hatch
/// (`request_issues_write = false`) omits the `issues` key
/// entirely so GitHub's consent screen does not even show the
/// write scope.
pub fn app_manifest_permissions(cfg: &GitHubAppConfig) -> AppManifest {
    let mut perms = BTreeMap::new();
    perms.insert("metadata".to_string(), "read".to_string());
    perms.insert("contents".to_string(), "read".to_string());
    perms.insert("pull_requests".to_string(), "read".to_string());
    perms.insert("members".to_string(), "read".to_string());
    if cfg.request_issues_write {
        perms.insert("issues".to_string(), "write".to_string());
    }
    AppManifest {
        slug: cfg.slug.clone().unwrap_or_default(),
        default_permissions: perms,
    }
}

// ---------------------------------------------------------------------------
// Write-gate (§8.4)
// ---------------------------------------------------------------------------

/// Single point through which every §8 issue-write handler asks
/// "may this caller mutate issues in `org`?".
///
/// Returns `Ok(())` iff (a) the deployment-level
/// `request_issues_write` flag is on AND (b) the per-org install
/// record (`Store::get_org_app_install`) carries
/// `issues_write = true`. Otherwise returns
/// [`ApiError::WritesNotAvailable`] wrapping the org's login and
/// the GitHub-side manage-permissions deep-link (when the App slug
/// is configured).
///
/// `org` is the dev-pulse-local [`Org`] — we want the *login* in
/// the error body, not the UUID, because the banner / 403 surface
/// it directly to a human.
///
/// Fail-closed branches:
///
/// * `request_issues_write = false` — the entire §8 surface is
///   disabled, return `WritesNotAvailable` with `manage_url =
///   None`. Distinct from the per-org branch but the wire shape
///   matches so the UI renders the banner the same way.
/// * No install record for `org` — treated the same as
///   `issues_write = false`; the §8.4 affordance applies.
pub async fn require_issues_write(
    store: &dyn Store,
    cfg: &GitHubAppConfig,
    org: &Org,
) -> Result<(), ApiError> {
    if !cfg.request_issues_write {
        return Err(ApiError::WritesNotAvailable {
            code: "writes_not_available_for_org",
            message: format!(
                "issue writes are disabled in this deployment \
                 (github.app.request_issues_write = false)"
            ),
            org_login: org.login.clone(),
            manage_url: None,
        });
    }
    let install = store.get_org_app_install(org.id).await?;
    match install {
        Some(i) if i.allows_issues_write() => Ok(()),
        Some(i) => Err(ApiError::WritesNotAvailable {
            code: "writes_not_available_for_org",
            message: format!(
                "the dev-pulse GitHub App install on `{}` is read-only; ask an org \
                 admin to re-consent with `issues: write`",
                org.login
            ),
            org_login: org.login.clone(),
            manage_url: cfg.manage_url(&org.login, i.installation_id),
        }),
        None => Err(ApiError::WritesNotAvailable {
            code: "writes_not_available_for_org",
            message: format!(
                "no GitHub App install observed for `{}`; cannot mutate issues",
                org.login
            ),
            org_login: org.login.clone(),
            manage_url: None,
        }),
    }
}

// ---------------------------------------------------------------------------
// Migration banner (§13.6)
// ---------------------------------------------------------------------------

/// One row in `GET /me/app-install-banner`.
///
/// `writes_available` mirrors the write-gate verdict for the
/// `(viewer, org)` pair so the frontend can render the §8.4
/// affordance without re-querying. `admin_copy_text` is the
/// §13.6 copy-able snippet; `manage_url` is the GitHub deep-link
/// (omitted when no App slug is configured).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AppInstallBannerOrgDto {
    /// dev-pulse-local org id.
    pub org_id: Uuid,
    /// GitHub org login (e.g. `"NubeIO"`).
    pub login: String,
    /// Display name, if known.
    pub name: Option<String>,
    /// `true` iff a §8 issue write against this org would succeed
    /// (deployment flag on AND install record allows
    /// `issues: write`).
    pub writes_available: bool,
    /// GitHub-side deep-link to the install's permissions page.
    /// `None` when no slug is configured or when no install
    /// record exists for the org.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manage_url: Option<String>,
    /// Copy-able admin text the viewer pastes into Slack / email
    /// to ask their org admin to re-consent. Always present —
    /// even for orgs where writes are already available, the
    /// frontend hides the row but the text is harmless to carry.
    pub admin_copy_text: String,
}

/// Full §13.6 banner response.
///
/// `request_issues_write` mirrors the deployment-level flag so the
/// UI can render the "writes disabled in this deployment" mode
/// without inspecting each row's `writes_available`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AppInstallBannerResponse {
    /// `false` when [`GitHubAppConfig::request_issues_write`] is
    /// off; the §8 surface is hard-disabled deployment-wide.
    pub request_issues_write: bool,
    /// One row per org the viewer is a member of (read through
    /// `Store::list_memberships_for_user`). Orgs the viewer
    /// cannot see at all are *not* included — the §15.11 access
    /// gate is the single visibility check (§9 cross-ref).
    pub orgs: Vec<AppInstallBannerOrgDto>,
}

/// `GET /me/app-install-banner` — the §13.6 one-shot migration
/// banner data for the caller.
///
/// * Reads memberships through `Store::list_memberships_for_user`
///   so the §15.11 access gate is the only visibility check
///   (out-of-org users get an empty `orgs` list — same as the
///   `/orgs` directory surface).
/// * Per-org `writes_available` is the *same* verdict
///   [`require_issues_write`] would return — no separate code
///   path, no risk of the banner saying "you can write" while the
///   write-gate disagrees.
/// * Not audited (read-only, no mutation).
#[utoipa::path(
    get,
    path = "/me/app-install-banner",
    responses(
        (status = 200, description = "Per-org App install status + §13.6 admin copy text", body = AppInstallBannerResponse),
    ),
    tag = "github_app",
)]
pub async fn list_app_install_banner(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Result<Json<AppInstallBannerResponse>, ApiError> {
    let memberships = state
        .store
        .list_memberships_for_user(principal.actor_user_id)
        .await?;
    let mut rows = Vec::with_capacity(memberships.len());
    for m in memberships {
        // Best-effort: skip orgs that have been deleted out from
        // under the membership row rather than 500. `list_orgs`
        // is the simpler surface; per-row fetch keeps the cost
        // bounded by the viewer's org count (typically <10).
        let org = match state.store.list_orgs().await {
            Ok(orgs) => orgs.into_iter().find(|o| o.id == m.org_id),
            Err(e) => return Err(e.into()),
        };
        let Some(org) = org else {
            continue;
        };
        let install = state.store.get_org_app_install(org.id).await?;
        let writes_available = state.github_app.request_issues_write
            && install.as_ref().is_some_and(|i| i.allows_issues_write());
        let manage_url = install
            .as_ref()
            .and_then(|i| state.github_app.manage_url(&org.login, i.installation_id));
        let admin_copy_text = state.github_app.admin_copy_text(&org.login);
        rows.push(AppInstallBannerOrgDto {
            org_id: org.id,
            login: org.login,
            name: org.name,
            writes_available,
            manage_url,
            admin_copy_text,
        });
    }
    Ok(Json(AppInstallBannerResponse {
        request_issues_write: state.github_app.request_issues_write,
        orgs: rows,
    }))
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the GitHub App permission router fragment. Mount via
/// `Router::merge` from `dp-server::build`; the composition layer
/// wires the principal extension and the `with_permission`
/// middleware sees the `(github_app, read)` pair this fragment
/// registers.
pub fn app_permissions_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new().route("/me/app-install-banner", get(list_app_install_banner)),
            "github_app",
            "read",
        ))
        .with_state(inner)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use dp_domain::app_install::{AppInstallPermissions, OrgAppInstall};
    use dp_domain::store::StoreError;

    /// Minimal Store stub that returns a hand-seeded install
    /// record for a known org_id. Other methods unreachable for
    /// the gate-level tests.
    struct StubStore {
        install: Option<OrgAppInstall>,
    }

    #[async_trait]
    impl Store for StubStore {
        async fn upsert_user(
            &self,
            _user: &dp_domain::User,
        ) -> Result<dp_domain::User, StoreError> {
            unreachable!()
        }
        async fn get_user(&self, _id: Uuid) -> Result<dp_domain::User, StoreError> {
            unreachable!()
        }
        async fn get_user_by_github_id(
            &self,
            _github_id: i64,
        ) -> Result<dp_domain::User, StoreError> {
            unreachable!()
        }
        async fn list_users(&self) -> Result<Vec<dp_domain::User>, StoreError> {
            unreachable!()
        }
        async fn pseudonymise_user(&self, _id: Uuid) -> Result<(), StoreError> {
            unreachable!()
        }
        async fn upsert_org(&self, _org: &Org) -> Result<Org, StoreError> {
            unreachable!()
        }
        async fn upsert_team(
            &self,
            _team: &dp_domain::Team,
        ) -> Result<dp_domain::Team, StoreError> {
            unreachable!()
        }
        async fn upsert_repo(
            &self,
            _repo: &dp_domain::Repo,
        ) -> Result<dp_domain::Repo, StoreError> {
            unreachable!()
        }
        async fn upsert_membership(
            &self,
            _membership: &dp_domain::Membership,
        ) -> Result<dp_domain::Membership, StoreError> {
            unreachable!()
        }
        async fn list_memberships_for_user(
            &self,
            _user_id: Uuid,
        ) -> Result<Vec<dp_domain::Membership>, StoreError> {
            Ok(vec![])
        }
        async fn set_home_org(
            &self,
            _user_id: Uuid,
            _org_id: Uuid,
            _home_org: Option<Uuid>,
        ) -> Result<(), StoreError> {
            unreachable!()
        }
        async fn record_event(
            &self,
            _event: &dp_domain::ActivityEvent,
        ) -> Result<dp_domain::ActivityEvent, StoreError> {
            unreachable!()
        }
        async fn add_event_actors(
            &self,
            _actors: &[dp_domain::EventActor],
        ) -> Result<(), StoreError> {
            unreachable!()
        }
        async fn list_event_actor_rows_in_window(
            &self,
            _window: &dp_domain::Window,
            _orgs: &[Uuid],
            _repos: &[Uuid],
            _users: &[Uuid],
            _roles: &[dp_domain::ActorRole],
        ) -> Result<Vec<dp_domain::store::EventActorRow>, StoreError> {
            unreachable!()
        }
        async fn get_cursor(
            &self,
            _org_id: Uuid,
            _repo_id: Option<Uuid>,
            _resource_kind: dp_domain::ResourceKind,
        ) -> Result<dp_domain::FetchCursor, StoreError> {
            unreachable!()
        }
        async fn put_cursor(
            &self,
            _cursor: &dp_domain::FetchCursor,
        ) -> Result<(), StoreError> {
            unreachable!()
        }
        async fn start_fetch_run(
            &self,
            _kind: dp_domain::FetchRunKind,
        ) -> Result<Uuid, StoreError> {
            unreachable!()
        }
        async fn finish_fetch_run(
            &self,
            _id: Uuid,
            _items: i64,
            _errors: i64,
            _partial: bool,
        ) -> Result<(), StoreError> {
            unreachable!()
        }
        async fn list_recent_fetch_runs(
            &self,
            _limit: i64,
        ) -> Result<Vec<dp_domain::FetchRun>, StoreError> {
            unreachable!()
        }
        async fn list_event_actor_rows_for_user_page(
            &self,
            _user_id: Uuid,
            _offset: i64,
            _limit: i64,
        ) -> Result<Vec<dp_domain::store::EventActorRow>, StoreError> {
            unreachable!()
        }
        async fn data_as_of(&self) -> Result<dp_domain::DataAsOf, StoreError> {
            unreachable!()
        }
        async fn enqueue_webhook(
            &self,
            _delivery: &dp_domain::WebhookDelivery,
        ) -> Result<(), StoreError> {
            unreachable!()
        }
        async fn claim_webhooks(
            &self,
            _max: i64,
        ) -> Result<Vec<dp_domain::WebhookDelivery>, StoreError> {
            unreachable!()
        }
        async fn mark_webhook_processed(&self, _id: Uuid) -> Result<(), StoreError> {
            unreachable!()
        }
        async fn mark_webhook_failed(
            &self,
            _id: Uuid,
            _error: &str,
        ) -> Result<(), StoreError> {
            unreachable!()
        }
        async fn get_org_app_install(
            &self,
            _org_id: Uuid,
        ) -> Result<Option<OrgAppInstall>, StoreError> {
            Ok(self.install.clone())
        }
    }

    fn org() -> Org {
        Org {
            id: Uuid::nil(),
            github_id: 1,
            login: "acme".into(),
            name: Some("ACME".into()),
        }
    }

    #[tokio::test]
    async fn write_gate_passes_when_flag_on_and_install_grants_write() {
        let store = StubStore {
            install: Some(OrgAppInstall {
                org_id: Uuid::nil(),
                installation_id: 42,
                permissions: AppInstallPermissions { issues_write: true },
                observed_at: Utc::now(),
            }),
        };
        let cfg = GitHubAppConfig {
            slug: Some("dev-pulse".into()),
            ..Default::default()
        };
        require_issues_write(&store, &cfg, &org()).await.unwrap();
    }

    #[tokio::test]
    async fn write_gate_blocks_when_deployment_flag_off() {
        let store = StubStore {
            install: Some(OrgAppInstall {
                org_id: Uuid::nil(),
                installation_id: 42,
                permissions: AppInstallPermissions { issues_write: true },
                observed_at: Utc::now(),
            }),
        };
        let cfg = GitHubAppConfig {
            request_issues_write: false,
            slug: Some("dev-pulse".into()),
        };
        let err = require_issues_write(&store, &cfg, &org())
            .await
            .unwrap_err();
        match err {
            ApiError::WritesNotAvailable {
                code,
                org_login,
                manage_url,
                ..
            } => {
                assert_eq!(code, "writes_not_available_for_org");
                assert_eq!(org_login, "acme");
                assert!(manage_url.is_none(), "deployment-off branch omits manage_url");
            }
            other => panic!("expected WritesNotAvailable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn write_gate_blocks_when_install_read_only() {
        let store = StubStore {
            install: Some(OrgAppInstall {
                org_id: Uuid::nil(),
                installation_id: 77,
                permissions: AppInstallPermissions::READ_ONLY,
                observed_at: Utc::now(),
            }),
        };
        let cfg = GitHubAppConfig {
            slug: Some("dev-pulse".into()),
            ..Default::default()
        };
        let err = require_issues_write(&store, &cfg, &org())
            .await
            .unwrap_err();
        match err {
            ApiError::WritesNotAvailable {
                code,
                org_login,
                manage_url,
                ..
            } => {
                assert_eq!(code, "writes_not_available_for_org");
                assert_eq!(org_login, "acme");
                assert_eq!(
                    manage_url.as_deref(),
                    Some("https://github.com/organizations/acme/settings/installations/77"),
                );
            }
            other => panic!("expected WritesNotAvailable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn write_gate_blocks_when_install_record_missing() {
        let store = StubStore { install: None };
        let cfg = GitHubAppConfig::default();
        let err = require_issues_write(&store, &cfg, &org())
            .await
            .unwrap_err();
        match err {
            ApiError::WritesNotAvailable {
                code,
                org_login,
                manage_url,
                ..
            } => {
                assert_eq!(code, "writes_not_available_for_org");
                assert_eq!(org_login, "acme");
                assert!(
                    manage_url.is_none(),
                    "missing-install branch omits manage_url"
                );
            }
            other => panic!("expected WritesNotAvailable, got {other:?}"),
        }
    }

    #[test]
    fn manifest_includes_issues_write_only_when_flag_on() {
        let on = app_manifest_permissions(&GitHubAppConfig::default());
        assert_eq!(on.default_permissions.get("issues"), Some(&"write".to_string()));
        assert_eq!(on.default_permissions.get("metadata"), Some(&"read".to_string()));

        let off = app_manifest_permissions(&GitHubAppConfig {
            request_issues_write: false,
            slug: None,
        });
        assert!(off.default_permissions.get("issues").is_none());
        // The read-only set is unaffected by the flag.
        assert_eq!(off.default_permissions.get("metadata"), Some(&"read".to_string()));
        assert_eq!(off.default_permissions.get("contents"), Some(&"read".to_string()));
        assert_eq!(off.default_permissions.get("pull_requests"), Some(&"read".to_string()));
        assert_eq!(off.default_permissions.get("members"), Some(&"read".to_string()));
    }

    #[test]
    fn admin_copy_text_mentions_org_and_app() {
        let cfg = GitHubAppConfig {
            slug: Some("dev-pulse".into()),
            ..Default::default()
        };
        let text = cfg.admin_copy_text("acme");
        assert!(text.contains("`dev-pulse`"));
        assert!(text.contains("`acme`"));
        assert!(text.contains("issues: write"));
    }

    #[test]
    fn admin_copy_text_falls_back_to_default_app_name_when_slug_unset() {
        let cfg = GitHubAppConfig::default();
        let text = cfg.admin_copy_text("acme");
        assert!(text.contains("`dev-pulse`"));
        assert!(text.contains("`acme`"));
    }

    #[test]
    fn manage_url_is_none_without_slug() {
        let cfg = GitHubAppConfig::default();
        assert!(cfg.manage_url("acme", 42).is_none());
    }

    #[test]
    fn manage_url_renders_when_slug_set() {
        let cfg = GitHubAppConfig {
            slug: Some("dev-pulse".into()),
            ..Default::default()
        };
        assert_eq!(
            cfg.manage_url("acme", 42).as_deref(),
            Some("https://github.com/organizations/acme/settings/installations/42")
        );
    }
}
