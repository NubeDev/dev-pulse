//! Directory handlers (Phase 4 stage 4).
//!
//! The "who's who" surface every operator UI needs to drive the
//! report / admin pages:
//!
//! | route                                  | shape                              |
//! |----------------------------------------|------------------------------------|
//! | `GET /users?org_id=<uuid>` (optional)  | `Vec<UserDto>`                     |
//! | `GET /orgs`                            | `Vec<OrgDto>`                      |
//! | `GET /teams?org_id=<uuid>`             | `Vec<TeamDto>`                     |
//! | `POST /home-org` body `{user_id, …}`   | `{ ok: true }`                     |
//!
//! Every handler is wrapped in [`with_principal`] at composition
//! time (stage 8) and writes one `audit_log` row through
//! [`crate::audit::record`]. The home-org mutation is the only one
//! that mutates and is therefore the only one that has an
//! atomicity contract: exactly one `memberships.home_org =
//! Some(org_id)` per user after the call returns, enforced by the
//! Store impl via [`Store::set_home_org_for_user`].
//!
//! [`Store::set_home_org_for_user`]: dp_domain::store::Store::set_home_org_for_user
//! [`with_principal`]: ../../../starter/crates/starter-server/src/auth/principal_layer.rs

use std::sync::Arc;

use axum::{
    extract::{Extension, Query, State},
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::{Org, Team, User};

use crate::audit::{self, Principal};
use crate::error::ApiError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Wire DTOs — flat clones of the dp-domain types so we can carry
// utoipa `ToSchema` derives without leaking them upstream.
// ---------------------------------------------------------------------------

/// `users.json` row.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UserDto {
    /// Internal user id.
    pub id: Uuid,
    /// GitHub numeric user id (stable across renames).
    pub github_id: i64,
    /// GitHub login.
    pub login: String,
    /// Display name, if known.
    pub name: Option<String>,
    /// Email, if known.
    pub email: Option<String>,
    /// Operator-controlled role tier
    /// (DOCS/SCOPE-AUTHZ-USERS.md §3). Lowercase wire form
    /// (`"reader"` / `"writer"` / `"admin"`).
    pub role: String,
}

impl From<User> for UserDto {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            github_id: u.github_id,
            login: u.login,
            name: u.name,
            email: u.email,
            role: u.role.as_str().to_string(),
        }
    }
}

/// `orgs.json` row.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OrgDto {
    /// Internal org id.
    pub id: Uuid,
    /// GitHub numeric org id.
    pub github_id: i64,
    /// GitHub login (slug).
    pub login: String,
    /// Display name, if set.
    pub name: Option<String>,
}

impl From<Org> for OrgDto {
    fn from(o: Org) -> Self {
        Self {
            id: o.id,
            github_id: o.github_id,
            login: o.login,
            name: o.name,
        }
    }
}

/// `teams.json` row.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TeamDto {
    /// Internal team id.
    pub id: Uuid,
    /// Parent org id.
    pub org_id: Uuid,
    /// GitHub numeric team id.
    pub github_id: i64,
    /// URL slug.
    pub slug: String,
    /// Display name.
    pub name: String,
}

impl From<Team> for TeamDto {
    fn from(t: Team) -> Self {
        Self {
            id: t.id,
            org_id: t.org_id,
            github_id: t.github_id,
            slug: t.slug,
            name: t.name,
        }
    }
}

// ---------------------------------------------------------------------------
// Query / body shapes
// ---------------------------------------------------------------------------

/// Optional `?org_id=…` filter for the listing handlers.
#[derive(Debug, Clone, Deserialize)]
pub struct OrgFilter {
    /// Restrict to one org. Absent → list everything.
    #[serde(default)]
    pub org_id: Option<Uuid>,
}

/// Required `?org_id=…` for `GET /teams`. Teams are always scoped
/// to one org; listing every team across every org isn't a v1
/// shape the UI asks for.
#[derive(Debug, Clone, Deserialize)]
pub struct OrgRequired {
    /// Org to list teams within.
    pub org_id: Uuid,
}

/// Body for `POST /home-org`.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct SetHomeOrgRequest {
    /// User to flip home org for.
    pub user_id: Uuid,
    /// Org that becomes the new home for that user.
    pub org_id: Uuid,
}

/// Trivial response shape — handlers that don't return data still
/// return a body so the UI doesn't switch on `204`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Ack {
    /// Always `true` on success.
    pub ok: bool,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /users` — list users, optionally filtered to one org.
///
/// Audit: writes one `report.read`-style row… actually `directory`
/// reads aren't in the v1 audit vocabulary (D4.4 only pins the eight
/// listed verbs). Listing users is a low-sensitivity read so we
/// intentionally do not audit it. The stage-4 mutation that does
/// audit is `POST /home-org`.
#[utoipa::path(
    get,
    path = "/users",
    params(
        ("org_id" = Option<Uuid>, Query, description = "Optional org filter")
    ),
    responses(
        (status = 200, description = "User directory", body = Vec<UserDto>),
    ),
    tag = "directory",
)]
pub async fn list_users(
    State(state): State<AppState>,
    Extension(_principal): Extension<Principal>,
    Query(filter): Query<OrgFilter>,
) -> Result<Json<Vec<UserDto>>, ApiError> {
    let users = match filter.org_id {
        Some(o) => state.store.list_users_for_org(o).await?,
        None => state.store.list_users().await?,
    };
    Ok(Json(users.into_iter().map(UserDto::from).collect()))
}

/// `GET /orgs` — every org dev-pulse has observed. (Stage 9 will
/// narrow to "orgs the principal can see"; stage 4 returns all.)
#[utoipa::path(
    get,
    path = "/orgs",
    responses(
        (status = 200, description = "Org directory", body = Vec<OrgDto>),
    ),
    tag = "directory",
)]
pub async fn list_orgs(
    State(state): State<AppState>,
    Extension(_principal): Extension<Principal>,
) -> Result<Json<Vec<OrgDto>>, ApiError> {
    let orgs = state.store.list_orgs().await?;
    Ok(Json(orgs.into_iter().map(OrgDto::from).collect()))
}

/// `GET /me/orgs` — orgs the caller has a membership in.
///
/// Narrower than `GET /orgs` (which returns every org dev-pulse
/// has observed). Use this surface when the UI needs to scope a
/// write to an org the caller is allowed to write to — e.g. the
/// Account → Tags page picking a scope for `POST /tags`, whose
/// backend gate (`tags::ViewerVisibility::visible_org_ids`) only
/// admits orgs the caller is a direct member of. Returning the
/// full directory there causes a confusing 403 when the operator
/// picks an org they can see but isn't in.
#[utoipa::path(
    get,
    path = "/me/orgs",
    responses(
        (status = 200, description = "Orgs the caller is a member of", body = Vec<OrgDto>),
    ),
    tag = "directory",
)]
pub async fn list_my_orgs(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Result<Json<Vec<OrgDto>>, ApiError> {
    let memberships = state
        .store
        .list_memberships_for_user(principal.actor_user_id)
        .await?;
    let mut out: Vec<OrgDto> = Vec::with_capacity(memberships.len());
    for m in memberships {
        if let Some(o) = state.store.get_org(m.org_id).await? {
            out.push(OrgDto::from(o));
        }
    }
    Ok(Json(out))
}

/// `GET /teams?org_id=…` — teams inside one org.
#[utoipa::path(
    get,
    path = "/teams",
    params(
        ("org_id" = Uuid, Query, description = "Org to list teams for")
    ),
    responses(
        (status = 200, description = "Team directory for the org", body = Vec<TeamDto>),
        (status = 400, description = "Missing org_id"),
    ),
    tag = "directory",
)]
pub async fn list_teams(
    State(state): State<AppState>,
    Extension(_principal): Extension<Principal>,
    Query(q): Query<OrgRequired>,
) -> Result<Json<Vec<TeamDto>>, ApiError> {
    let teams = state.store.list_teams_for_org(q.org_id).await?;
    Ok(Json(teams.into_iter().map(TeamDto::from).collect()))
}

/// `POST /home-org` — atomically flip the user's home org.
///
/// Postcondition (enforced by [`Store::set_home_org_for_user`]):
/// exactly one membership row per `user_id` has `home_org =
/// Some(org_id)` after this call returns.
///
/// Audit: writes one [`audit::HOME_ORG_SET`] row, `target =
/// "user:<user_id>"`. The row is written **after** the store flip
/// succeeds so a failed mutation doesn't leave a misleading audit
/// trail.
///
/// [`Store::set_home_org_for_user`]: dp_domain::store::Store::set_home_org_for_user
#[utoipa::path(
    post,
    path = "/home-org",
    request_body = SetHomeOrgRequest,
    responses(
        (status = 200, description = "Home-org flipped", body = Ack),
        (status = 404, description = "No such membership"),
    ),
    tag = "directory",
)]
pub async fn set_home_org(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<SetHomeOrgRequest>,
) -> Result<Json<Ack>, ApiError> {
    state
        .store
        .set_home_org_for_user(body.user_id, body.org_id)
        .await?;
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::HOME_ORG_SET,
        format!("user:{}", body.user_id),
    )
    .await?;
    Ok(Json(Ack { ok: true }))
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the directory router fragment. Mount with `Router::merge`
/// from the composition root (`dp-server::build()`); the
/// `with_principal` and `require_permission` wrappers are added by
/// the composition layer per Phase 4 stage 8.
pub fn directory_router(state: Arc<AppState>) -> Router {
    // See the note in `reports::reports_router` about the
    // `with_permission` wrapping pattern. Each route group is a
    // tiny Router wrapped in its own permission layer so the
    // kind/action pair is per-route, mirroring the audit
    // vocabulary in `crate::audit`.
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new().route("/users", get(list_users)),
            "users",
            "read",
        ))
        .merge(with_permission(
            Router::new().route("/orgs", get(list_orgs)),
            "orgs",
            "read",
        ))
        .merge(with_permission(
            Router::new().route("/me/orgs", get(list_my_orgs)),
            "orgs",
            "read",
        ))
        .merge(with_permission(
            Router::new().route("/teams", get(list_teams)),
            "teams",
            "read",
        ))
        .merge(with_permission(
            Router::new().route("/home-org", post(set_home_org)),
            "home_org",
            "set",
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
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use chrono::Utc;
    use std::sync::Mutex;
    use tower::ServiceExt;

    use dp_domain::audit::AuditEntry;
    use dp_domain::store::{EventActorRow, Store, StoreError};
    use dp_domain::{
        ActivityEvent, ActorRole, EventActor, FetchCursor, FetchRun, FetchRunKind, Membership,
        MembershipRole, Org, Repo, ResourceKind, Team, User, WebhookDelivery, Window,
    };

    // -----------------------------------------------------------------
    // In-memory store fake
    // -----------------------------------------------------------------

    /// Minimal in-memory store that supports the directory surface
    /// plus the audit-log writer. Every other Store method falls back
    /// to the trait default (Ok-empty or no-op) so this fake stays
    /// small — directory tests don't touch events / cursors / webhooks.
    #[derive(Default)]
    struct MemStore {
        users: Mutex<Vec<User>>,
        orgs: Mutex<Vec<Org>>,
        teams: Mutex<Vec<Team>>,
        memberships: Mutex<Vec<Membership>>,
        audit: Mutex<Vec<AuditEntry>>,
    }

    impl MemStore {
        fn seed_user(&self, id: Uuid, login: &str) {
            self.users.lock().unwrap().push(User {
                id,
                github_id: 1,
                login: login.into(),
                name: None,
                email: None,
                role: dp_domain::Role::default(),
                deleted_at: None,
            });
        }
        fn seed_org(&self, id: Uuid, login: &str) {
            self.orgs.lock().unwrap().push(Org {
                id,
                github_id: 1,
                login: login.into(),
                name: None,
            });
        }
        fn seed_team(&self, id: Uuid, org_id: Uuid, slug: &str) {
            self.teams.lock().unwrap().push(Team {
                id,
                org_id,
                github_id: 1,
                slug: slug.into(),
                name: slug.into(),
            });
        }
        fn seed_membership(&self, user_id: Uuid, org_id: Uuid, home: Option<Uuid>) {
            self.memberships.lock().unwrap().push(Membership {
                user_id,
                org_id,
                role: MembershipRole::Member,
                home_org: home,
                joined_at: Utc::now(),
            });
        }
        fn audit_rows(&self) -> Vec<AuditEntry> {
            self.audit.lock().unwrap().clone()
        }
        fn memberships_for(&self, user_id: Uuid) -> Vec<Membership> {
            self.memberships
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.user_id == user_id)
                .cloned()
                .collect()
        }
    }

    #[async_trait]
    impl Store for MemStore {
        async fn upsert_user(&self, u: &User) -> Result<User, StoreError> {
            Ok(u.clone())
        }
        async fn get_user(&self, _: Uuid) -> Result<User, StoreError> {
            unimplemented!()
        }
        async fn get_user_by_github_id(&self, _: i64) -> Result<User, StoreError> {
            unimplemented!()
        }
        async fn list_users(&self) -> Result<Vec<User>, StoreError> {
            Ok(self.users.lock().unwrap().clone())
        }
        async fn pseudonymise_user(&self, _: Uuid) -> Result<(), StoreError> {
            Ok(())
        }
        async fn upsert_org(&self, o: &Org) -> Result<Org, StoreError> {
            Ok(o.clone())
        }
        async fn upsert_team(&self, t: &Team) -> Result<Team, StoreError> {
            Ok(t.clone())
        }
        async fn upsert_repo(&self, r: &Repo) -> Result<Repo, StoreError> {
            Ok(r.clone())
        }
        async fn upsert_membership(&self, m: &Membership) -> Result<Membership, StoreError> {
            Ok(m.clone())
        }
        async fn list_memberships_for_user(
            &self,
            user_id: Uuid,
        ) -> Result<Vec<Membership>, StoreError> {
            Ok(self.memberships_for(user_id))
        }
        async fn set_home_org(
            &self,
            user_id: Uuid,
            org_id: Uuid,
            home: Option<Uuid>,
        ) -> Result<(), StoreError> {
            let mut ms = self.memberships.lock().unwrap();
            let found = ms
                .iter_mut()
                .find(|m| m.user_id == user_id && m.org_id == org_id);
            match found {
                Some(m) => {
                    m.home_org = home;
                    Ok(())
                }
                None => Err(StoreError::NotFound {
                    entity: "membership",
                    id: format!("({user_id}, {org_id})"),
                }),
            }
        }
        async fn set_home_org_for_user(
            &self,
            user_id: Uuid,
            org_id: Uuid,
        ) -> Result<(), StoreError> {
            // Atomic flip — done under one lock so a concurrent reader
            // can never observe two home_org=Some rows for this user.
            let mut ms = self.memberships.lock().unwrap();
            // Verify the target row exists before mutating anything.
            if !ms
                .iter()
                .any(|m| m.user_id == user_id && m.org_id == org_id)
            {
                return Err(StoreError::NotFound {
                    entity: "membership",
                    id: format!("({user_id}, {org_id})"),
                });
            }
            for m in ms.iter_mut().filter(|m| m.user_id == user_id) {
                m.home_org = if m.org_id == org_id {
                    Some(org_id)
                } else {
                    None
                };
            }
            Ok(())
        }
        async fn list_orgs(&self) -> Result<Vec<Org>, StoreError> {
            Ok(self.orgs.lock().unwrap().clone())
        }
        async fn list_teams_for_org(&self, org_id: Uuid) -> Result<Vec<Team>, StoreError> {
            Ok(self
                .teams
                .lock()
                .unwrap()
                .iter()
                .filter(|t| t.org_id == org_id)
                .cloned()
                .collect())
        }
        async fn list_users_for_org(&self, org_id: Uuid) -> Result<Vec<User>, StoreError> {
            let members: Vec<Uuid> = self
                .memberships
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.org_id == org_id)
                .map(|m| m.user_id)
                .collect();
            Ok(self
                .users
                .lock()
                .unwrap()
                .iter()
                .filter(|u| members.contains(&u.id))
                .cloned()
                .collect())
        }
        async fn record_event(&self, e: &ActivityEvent) -> Result<ActivityEvent, StoreError> {
            Ok(e.clone())
        }
        async fn add_event_actors(&self, _: &[EventActor]) -> Result<(), StoreError> {
            Ok(())
        }
        async fn list_event_actor_rows_in_window(
            &self,
            _: &Window,
            _: &[Uuid],
            _: &[Uuid],
            _: &[Uuid],
            _: &[ActorRole],
        ) -> Result<Vec<EventActorRow>, StoreError> {
            Ok(vec![])
        }
        async fn get_cursor(
            &self,
            _: Uuid,
            _: Option<Uuid>,
            _: ResourceKind,
        ) -> Result<FetchCursor, StoreError> {
            Err(StoreError::NotFound {
                entity: "fetch_cursor",
                id: String::new(),
            })
        }
        async fn put_cursor(&self, _: &FetchCursor) -> Result<(), StoreError> {
            Ok(())
        }
        async fn start_fetch_run(&self, _: FetchRunKind) -> Result<Uuid, StoreError> {
            Ok(Uuid::new_v4())
        }
        async fn finish_fetch_run(
            &self,
            _: Uuid,
            _: i64,
            _: i64,
            _: bool,
        ) -> Result<(), StoreError> {
            Ok(())
        }
        async fn list_recent_fetch_runs(&self, _: i64) -> Result<Vec<FetchRun>, StoreError> {
            Ok(vec![])
        }
        async fn data_as_of(&self) -> Result<dp_domain::freshness::DataAsOf, StoreError> {
            Ok(dp_domain::freshness::DataAsOf::default())
        }
        async fn enqueue_webhook(&self, _: &WebhookDelivery) -> Result<(), StoreError> {
            Ok(())
        }
        async fn claim_webhooks(&self, _: i64) -> Result<Vec<WebhookDelivery>, StoreError> {
            Ok(vec![])
        }
        async fn mark_webhook_processed(&self, _: Uuid) -> Result<(), StoreError> {
            Ok(())
        }
        async fn mark_webhook_failed(&self, _: Uuid, _: &str) -> Result<(), StoreError> {
            Ok(())
        }
        async fn record_audit_log(&self, entry: &AuditEntry) -> Result<(), StoreError> {
            self.audit.lock().unwrap().push(entry.clone());
            Ok(())
        }
    }

    // -----------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------

    fn build_app(store: Arc<MemStore>, principal: Principal) -> Router {
        // Layer in (a) a Principal (the `require_permission`
        // middleware reads `Extension<Principal>` to know who the
        // caller is) and (b) a `NoopPolicyEngine` as the
        // `Arc<dyn PolicyEngine>` extension so the gate always
        // allows in unit-test contexts. Production wiring (in
        // `dp_server::build`) replaces the no-op with the
        // dev-pulse `StaticRbacEngine`.
        use starter_spi::auth::{Principal as SpiPrincipal, Role};
        use starter_spi::authz::{NoopPolicyEngine, PolicyEngine};
        use std::sync::Arc as StdArc;
        let app_state = Arc::new(AppState::new(store));
        let engine: StdArc<dyn PolicyEngine> = StdArc::new(NoopPolicyEngine);
        let spi_principal = SpiPrincipal {
            subject: principal.actor_user_id.to_string(),
            role: Role::Admin,
            scopes: Vec::new(),
            tenant_id: None,
            teams: Vec::new(),
            extra: serde_json::Value::Null,
        };
        directory_router(app_state)
            .layer(Extension(principal))
            .layer(Extension(spi_principal))
            .layer(Extension(engine))
    }

    async fn json_of(resp: axum::response::Response) -> serde_json::Value {
        let body = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    // -----------------------------------------------------------------
    // GET /users / /orgs / /teams
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn list_users_returns_seeded_rows_and_respects_org_filter() {
        let store = Arc::new(MemStore::default());
        let u1 = Uuid::new_v4();
        let u2 = Uuid::new_v4();
        let o = Uuid::new_v4();
        store.seed_user(u1, "alice");
        store.seed_user(u2, "bob");
        store.seed_org(o, "acme");
        store.seed_membership(u1, o, None); // only alice is in org `o`
        let app = build_app(
            store.clone(),
            Principal {
                actor_user_id: Uuid::new_v4(),
            },
        );

        // No filter — both users.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/users")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let v = json_of(resp).await;
        assert_eq!(v.as_array().unwrap().len(), 2);

        // Filtered to org `o` — only alice.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/users?org_id={o}"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let v = json_of(resp).await;
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["login"], "alice");
    }

    #[tokio::test]
    async fn list_orgs_returns_seeded_rows() {
        let store = Arc::new(MemStore::default());
        store.seed_org(Uuid::new_v4(), "acme");
        store.seed_org(Uuid::new_v4(), "globex");
        let app = build_app(
            store,
            Principal {
                actor_user_id: Uuid::new_v4(),
            },
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/orgs")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let v = json_of(resp).await;
        let mut logins: Vec<String> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["login"].as_str().unwrap().to_string())
            .collect();
        logins.sort();
        assert_eq!(logins, vec!["acme".to_string(), "globex".into()]);
    }

    #[tokio::test]
    async fn list_teams_filters_by_org_id() {
        let store = Arc::new(MemStore::default());
        let o1 = Uuid::new_v4();
        let o2 = Uuid::new_v4();
        store.seed_team(Uuid::new_v4(), o1, "platform");
        store.seed_team(Uuid::new_v4(), o1, "data");
        store.seed_team(Uuid::new_v4(), o2, "frontend");
        let app = build_app(
            store,
            Principal {
                actor_user_id: Uuid::new_v4(),
            },
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/teams?org_id={o1}"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let v = json_of(resp).await;
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        for t in arr {
            assert_eq!(t["org_id"], serde_json::json!(o1));
        }
    }

    #[tokio::test]
    async fn list_teams_without_org_id_returns_400() {
        let store = Arc::new(MemStore::default());
        let app = build_app(
            store,
            Principal {
                actor_user_id: Uuid::new_v4(),
            },
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/teams")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // axum's Query rejection — missing required field surfaces
        // as 400 via the extractor's default rejection.
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // -----------------------------------------------------------------
    // POST /home-org — audit + atomicity
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn post_home_org_writes_audit_row_with_pinned_action() {
        let store = Arc::new(MemStore::default());
        let user = Uuid::new_v4();
        let org = Uuid::new_v4();
        let actor = Uuid::new_v4();
        store.seed_user(user, "alice");
        store.seed_org(org, "acme");
        store.seed_membership(user, org, None);
        let app = build_app(
            store.clone(),
            Principal {
                actor_user_id: actor,
            },
        );

        let body = serde_json::json!({ "user_id": user, "org_id": org });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/home-org")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let v = json_of(resp).await;
        assert_eq!(v["ok"], true);

        let rows = store.audit_rows();
        assert_eq!(rows.len(), 1, "exactly one audit row per call");
        let row = &rows[0];
        assert_eq!(row.actor_user_id, actor);
        assert_eq!(row.action, crate::audit::HOME_ORG_SET);
        assert_eq!(row.target, format!("user:{user}"));
    }

    #[tokio::test]
    async fn post_home_org_flips_atomically_one_home_org_per_user() {
        let store = Arc::new(MemStore::default());
        let user = Uuid::new_v4();
        let org_a = Uuid::new_v4();
        let org_b = Uuid::new_v4();
        let org_c = Uuid::new_v4();
        store.seed_user(user, "alice");
        store.seed_org(org_a, "a");
        store.seed_org(org_b, "b");
        store.seed_org(org_c, "c");
        // User is in three orgs; home starts on `a`.
        store.seed_membership(user, org_a, Some(org_a));
        store.seed_membership(user, org_b, None);
        store.seed_membership(user, org_c, None);

        let app = build_app(
            store.clone(),
            Principal {
                actor_user_id: Uuid::new_v4(),
            },
        );
        let body = serde_json::json!({ "user_id": user, "org_id": org_b });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/home-org")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let ms = store.memberships_for(user);
        let homes: Vec<Option<Uuid>> = ms.iter().map(|m| m.home_org).collect();
        let set_count = homes.iter().filter(|h| h.is_some()).count();
        assert_eq!(
            set_count, 1,
            "exactly one membership row carries home_org after flip"
        );
        let b_row = ms.iter().find(|m| m.org_id == org_b).unwrap();
        assert_eq!(b_row.home_org, Some(org_b));
        // `a` was previously home; now cleared.
        let a_row = ms.iter().find(|m| m.org_id == org_a).unwrap();
        assert_eq!(a_row.home_org, None);
    }

    #[tokio::test]
    async fn post_home_org_returns_500_when_membership_missing_and_writes_no_audit() {
        // The membership doesn't exist; the store returns NotFound,
        // which the ApiError::Store mapping turns into 500 (the v1
        // error model — see error.rs). The audit row must NOT land
        // because the mutation failed.
        let store = Arc::new(MemStore::default());
        let app = build_app(
            store.clone(),
            Principal {
                actor_user_id: Uuid::new_v4(),
            },
        );
        let body = serde_json::json!({ "user_id": Uuid::new_v4(), "org_id": Uuid::new_v4() });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/home-org")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            store.audit_rows().is_empty(),
            "no audit row for a failed mutation"
        );
    }
}
