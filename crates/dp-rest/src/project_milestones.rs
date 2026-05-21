//! Project milestones strip (PROJECT-VIEW.md §5.5, Slice 1).
//!
//! One route ships here:
//!
//! | route                                       | what it does                                                |
//! |---------------------------------------------|-------------------------------------------------------------|
//! | `GET /projects/{id}/milestones`             | active milestones across linked repos, due-soonest first    |
//!
//! `?include_closed=true` extends the response with closed
//! milestones for the `▸ Show closed` toggle. The default
//! (`false`) returns only `state = 'open'` rows — the strip's
//! primary case.
//!
//! No write surface ships in Slice 1. `Adopt as primary` (§9.5)
//! arrives in Slice 5 once `dp_projects.primary_milestone_id`
//! lands; until then the strip exposes it as a disabled overflow
//! action.

use std::sync::Arc;

use axum::{
    extract::{Extension, Path, Query, State},
    response::Json,
    routing::{get, post},
    Router,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use dp_domain::milestone::Milestone;
use dp_domain::store::StoreError;

use crate::audit::{self, Principal};
use crate::error::ApiError;
use crate::projects::ProjectDto;
use crate::state::AppState;

/// One milestone row on the wire. The strip needs every
/// progress-bar field — `open_issues`, `closed_issues`,
/// `due_on` — plus the `repo_id` so the chip can disambiguate
/// when two repos share a milestone title.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MilestoneDto {
    /// Stable id.
    pub id: Uuid,
    /// Parent repo. The strip uses this for the
    /// disambiguation suffix when two linked repos share a
    /// milestone title.
    pub repo_id: Uuid,
    /// GitHub-side milestone number (the integer in the
    /// `https://github.com/{owner}/{repo}/milestone/{n}` URL).
    pub github_number: i32,
    /// Milestone title.
    pub title: String,
    /// Long-form description. May contain markdown.
    pub description: Option<String>,
    /// `"open"` | `"closed"`.
    pub state: String,
    /// Due date (`YYYY-MM-DD`) or `null`.
    pub due_on: Option<NaiveDate>,
    /// Open issues on GitHub. Authoritative.
    pub open_issues: i32,
    /// Closed issues on GitHub. Authoritative.
    pub closed_issues: i32,
    /// GitHub creation timestamp.
    pub created_at: DateTime<Utc>,
    /// GitHub last-update timestamp.
    pub updated_at: DateTime<Utc>,
    /// GitHub close timestamp (only on `state = "closed"`).
    pub closed_at: Option<DateTime<Utc>>,
}

impl From<Milestone> for MilestoneDto {
    fn from(m: Milestone) -> Self {
        Self {
            id: m.id,
            repo_id: m.repo_id,
            github_number: m.github_number,
            title: m.title,
            description: m.description,
            state: m.state.as_str().into(),
            due_on: m.due_on,
            open_issues: m.open_issues,
            closed_issues: m.closed_issues,
            created_at: m.created_at,
            updated_at: m.updated_at,
            closed_at: m.closed_at,
        }
    }
}

/// Query string for [`list_project_milestones`].
#[derive(Debug, Default, Deserialize, IntoParams)]
pub struct ListMilestonesQuery {
    /// When `true`, include `state = 'closed'` rows after the open
    /// set (for the `▸ Show closed` toggle). Defaults to `false`.
    #[serde(default)]
    pub include_closed: bool,
}

/// `GET /projects/{id}/milestones` — milestones across every linked
/// repo, sorted by `(state ASC, due_on ASC NULLS LAST, title ASC)`
/// so the soonest-due open milestone is first and the no-due-date
/// tail sinks to the bottom of the open block.
#[utoipa::path(
    get,
    path = "/projects/{id}/milestones",
    params(
        ("id" = Uuid, Path, description = "Project id"),
        ListMilestonesQuery,
    ),
    responses(
        (status = 200, description = "Milestones for the project", body = Vec<MilestoneDto>),
        (status = 404, description = "No such project"),
    ),
    tag = "projects",
)]
pub async fn list_project_milestones(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Query(q): Query<ListMilestonesQuery>,
) -> Result<Json<Vec<MilestoneDto>>, ApiError> {
    // Resolve project first so unknown ids 404 cleanly instead of
    // leaking an empty list (which would imply "no milestones",
    // not "no project").
    state
        .store
        .get_project(project_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "project_not_found",
            message: format!("no project with id {project_id}"),
        })?;
    let rows = state
        .store
        .list_project_milestones(project_id, q.include_closed)
        .await?;
    Ok(Json(rows.into_iter().map(MilestoneDto::from).collect()))
}

/// Mount the milestones strip route under `(projects, read)`.
pub fn project_milestones_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new().route(
                "/projects/{id}/milestones",
                get(list_project_milestones),
            ),
            "projects",
            "read",
        ))
        .merge(with_permission(
            Router::new().route(
                "/projects/{id}/adopt-milestone",
                post(adopt_milestone),
            ),
            "projects",
            "write",
        ))
        .with_state(inner)
}

/// Body for [`adopt_milestone`]. `milestone_id = None` clears the
/// project's primary pointer; `Some(id)` adopts that milestone.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct AdoptMilestoneBody {
    /// Milestone to adopt, or `null` to clear the pointer.
    #[serde(default)]
    pub milestone_id: Option<Uuid>,
}

/// `POST /projects/{id}/adopt-milestone` — set or clear the
/// project's [`primary_milestone_id`](ProjectDto::primary_milestone_id).
/// Returns the updated [`ProjectDto`] so the caller can refresh
/// the `★ primary` chip without a follow-up GET.
///
/// 400 (`milestone_not_linked`) when the milestone exists but
/// doesn't belong to a repo currently linked to this project — the
/// strip never surfaces these but a stale UI must not be able to
/// adopt one via a direct API call.
#[utoipa::path(
    post,
    path = "/projects/{id}/adopt-milestone",
    params(("id" = Uuid, Path, description = "Project id")),
    request_body = AdoptMilestoneBody,
    responses(
        (status = 200, description = "Updated project", body = ProjectDto),
        (status = 400, description = "Milestone not linked to this project's repos"),
        (status = 404, description = "No such project"),
    ),
    tag = "projects",
)]
pub async fn adopt_milestone(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(project_id): Path<Uuid>,
    Json(body): Json<AdoptMilestoneBody>,
) -> Result<Json<ProjectDto>, ApiError> {
    let result = state
        .store
        .set_project_primary_milestone(project_id, body.milestone_id)
        .await;
    let project = match result {
        Ok(p) => p,
        Err(StoreError::NotFound { .. }) => {
            return Err(ApiError::NotFound {
                code: "project_not_found",
                message: format!("no project with id {project_id}"),
            });
        }
        Err(StoreError::Invalid(msg)) => {
            return Err(ApiError::BadRequest {
                code: "milestone_not_linked",
                message: msg,
            });
        }
        Err(e) => return Err(e.into()),
    };
    let target = match body.milestone_id {
        Some(mid) => format!("{project_id}:{mid}"),
        None => format!("{project_id}:"),
    };
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_MILESTONE_ADOPT,
        target,
    )
    .await
    .ok();
    Ok(Json(project.into()))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use async_trait::async_trait;
    use axum::body::to_bytes;
    use axum::http::Request;
    use axum::response::Response;
    use dp_domain::milestone::MilestoneState;
    use dp_domain::project::{Project, ProjectStatus};
    use dp_domain::store::{EventActorRow, Store, StoreError};
    use dp_domain::{
        ActivityEvent, ActorRole, EventActor, FetchCursor, FetchRun, FetchRunKind, Membership,
        Org, Repo, ResourceKind, Team, User, WebhookDelivery, Window,
    };
    use tower::ServiceExt;

    use crate::audit::Principal;

    #[derive(Default)]
    struct MemStore {
        projects: Mutex<Vec<Project>>,
        milestones: Mutex<Vec<Milestone>>,
    }

    #[async_trait]
    impl Store for MemStore {
        async fn get_project(&self, id: Uuid) -> Result<Option<Project>, StoreError> {
            Ok(self
                .projects
                .lock()
                .unwrap()
                .iter()
                .find(|p| p.id == id)
                .cloned())
        }

        async fn list_project_milestones(
            &self,
            project_id: Uuid,
            include_closed: bool,
        ) -> Result<Vec<Milestone>, StoreError> {
            // Test scaffold returns every milestone in the store
            // regardless of repo wiring; tests scope by populating
            // only the rows they care about. The `project_id`
            // parameter is preserved here for parity with the real
            // Postgres impl but unused — the per-project test
            // boundary is enforced by `seed_project` matching the
            // milestone's `repo_id` to the project.
            let _ = project_id;
            let mut rows = self.milestones.lock().unwrap().clone();
            if !include_closed {
                rows.retain(|m| matches!(m.state, MilestoneState::Open));
            }
            // Match the PG ordering: state then due_on NULLS LAST
            // then title — tests can assert on the order.
            rows.sort_by(|a, b| {
                let sa = matches!(a.state, MilestoneState::Open) as u8;
                let sb = matches!(b.state, MilestoneState::Open) as u8;
                sb.cmp(&sa)
                    .then_with(|| match (a.due_on, b.due_on) {
                        (Some(x), Some(y)) => x.cmp(&y),
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        (None, None) => std::cmp::Ordering::Equal,
                    })
                    .then_with(|| a.title.cmp(&b.title))
            });
            Ok(rows)
        }

        async fn set_project_primary_milestone(
            &self,
            project_id: Uuid,
            milestone_id: Option<Uuid>,
        ) -> Result<Project, StoreError> {
            // Validate eligibility — milestone must be in this
            // store (the same scope `list_project_milestones`
            // returns above).
            if let Some(mid) = milestone_id {
                let known = self
                    .milestones
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|m| m.id == mid);
                if !known {
                    return Err(StoreError::Invalid(
                        "milestone not linked".into(),
                    ));
                }
            }
            let mut projects = self.projects.lock().unwrap();
            let p = projects
                .iter_mut()
                .find(|p| p.id == project_id)
                .ok_or_else(|| StoreError::NotFound {
                    entity: "project",
                    id: project_id.to_string(),
                })?;
            p.primary_milestone_id = milestone_id;
            p.version += 1;
            p.updated_at = Utc::now();
            Ok(p.clone())
        }

        // --- minimal stubs for the rest of the Store surface --------
        async fn record_audit_log(
            &self,
            _entry: &dp_domain::audit::AuditEntry,
        ) -> Result<(), StoreError> {
            Ok(())
        }
        async fn upsert_user(&self, u: &User) -> Result<User, StoreError> { Ok(u.clone()) }
        async fn get_user(&self, _: Uuid) -> Result<User, StoreError> { unimplemented!() }
        async fn get_user_by_github_id(&self, _: i64) -> Result<User, StoreError> { unimplemented!() }
        async fn list_users(&self) -> Result<Vec<User>, StoreError> { Ok(vec![]) }
        async fn pseudonymise_user(&self, _: Uuid) -> Result<(), StoreError> { Ok(()) }
        async fn upsert_org(&self, o: &Org) -> Result<Org, StoreError> { Ok(o.clone()) }
        async fn upsert_team(&self, t: &Team) -> Result<Team, StoreError> { Ok(t.clone()) }
        async fn upsert_repo(&self, r: &Repo) -> Result<Repo, StoreError> { Ok(r.clone()) }
        async fn upsert_membership(&self, m: &Membership) -> Result<Membership, StoreError> { Ok(m.clone()) }
        async fn list_memberships_for_user(&self, _: Uuid) -> Result<Vec<Membership>, StoreError> { Ok(vec![]) }
        async fn set_home_org(&self, _: Uuid, _: Uuid, _: Option<Uuid>) -> Result<(), StoreError> { Ok(()) }
        async fn record_event(&self, e: &ActivityEvent) -> Result<ActivityEvent, StoreError> { Ok(e.clone()) }
        async fn add_event_actors(&self, _: &[EventActor]) -> Result<(), StoreError> { Ok(()) }
        async fn list_event_actor_rows_in_window(
            &self,
            _: &Window,
            _: &[Uuid],
            _: &[Uuid],
            _: &[Uuid],
            _: &[ActorRole],
        ) -> Result<Vec<EventActorRow>, StoreError> { Ok(vec![]) }
        async fn get_cursor(&self, _: Uuid, _: Option<Uuid>, _: ResourceKind) -> Result<FetchCursor, StoreError> {
            Err(StoreError::NotFound { entity: "fetch_cursor", id: String::new() })
        }
        async fn put_cursor(&self, _: &FetchCursor) -> Result<(), StoreError> { Ok(()) }
        async fn start_fetch_run(&self, _: FetchRunKind) -> Result<Uuid, StoreError> { Ok(Uuid::new_v4()) }
        async fn finish_fetch_run(&self, _: Uuid, _: i64, _: i64, _: bool) -> Result<(), StoreError> { Ok(()) }
        async fn list_recent_fetch_runs(&self, _: i64) -> Result<Vec<FetchRun>, StoreError> { Ok(vec![]) }
        async fn data_as_of(&self) -> Result<dp_domain::freshness::DataAsOf, StoreError> {
            Ok(dp_domain::freshness::DataAsOf::default())
        }
        async fn enqueue_webhook(&self, _: &WebhookDelivery) -> Result<(), StoreError> { Ok(()) }
        async fn claim_webhooks(&self, _: i64) -> Result<Vec<WebhookDelivery>, StoreError> { Ok(vec![]) }
        async fn mark_webhook_processed(&self, _: Uuid) -> Result<(), StoreError> { Ok(()) }
        async fn mark_webhook_failed(&self, _: Uuid, _: &str) -> Result<(), StoreError> { Ok(()) }
    }

    fn build_app(store: Arc<MemStore>, actor: Uuid) -> Router {
        use axum::extract::Extension;
        use starter_spi::auth::{Principal as SpiPrincipal, Role};
        use starter_spi::authz::{NoopPolicyEngine, PolicyEngine};
        use std::sync::Arc as StdArc;
        let app_state = Arc::new(AppState::new(store));
        let engine: StdArc<dyn PolicyEngine> = StdArc::new(NoopPolicyEngine);
        let spi_principal = SpiPrincipal {
            subject: actor.to_string(),
            role: Role::Admin,
            scopes: Vec::new(),
            extra: serde_json::Value::Null,
        };
        project_milestones_router(app_state)
            .layer(Extension(Principal { actor_user_id: actor }))
            .layer(Extension(spi_principal))
            .layer(Extension(engine))
    }

    fn seed_project(store: &MemStore) -> Project {
        let p = Project {
            id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            name: "p".into(),
            description: None,
            lead_user_id: None,
            status: ProjectStatus::Active,
            start_at: None,
            due_at: None,
            issue_count: 0,
            closed_issue_count: 0,
            created_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            version: 1,
            primary_milestone_id: None,
        };
        store.projects.lock().unwrap().push(p.clone());
        p
    }

    fn seed_milestone(
        store: &MemStore,
        title: &str,
        due_on: Option<NaiveDate>,
        state: MilestoneState,
    ) -> Milestone {
        let m = Milestone {
            id: Uuid::new_v4(),
            repo_id: Uuid::new_v4(),
            github_number: 1,
            github_node_id: "MI_x".into(),
            title: title.into(),
            description: None,
            state,
            due_on,
            open_issues: 3,
            closed_issues: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: None,
            fetched_at: Utc::now(),
            remote_missing_streak: 0,
        };
        store.milestones.lock().unwrap().push(m.clone());
        m
    }

    async fn body_json(resp: Response) -> serde_json::Value {
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn list_returns_open_milestones_due_soonest_first() {
        let store = Arc::new(MemStore::default());
        let project = seed_project(&store);
        seed_milestone(
            &store,
            "Later",
            Some(NaiveDate::from_ymd_opt(2099, 6, 1).unwrap()),
            MilestoneState::Open,
        );
        seed_milestone(
            &store,
            "Sooner",
            Some(NaiveDate::from_ymd_opt(2099, 1, 1).unwrap()),
            MilestoneState::Open,
        );
        seed_milestone(&store, "NoDate", None, MilestoneState::Open);
        let app = build_app(store.clone(), Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}/milestones", project.id))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let v = body_json(resp).await;
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["title"], "Sooner");
        assert_eq!(arr[1]["title"], "Later");
        assert_eq!(arr[2]["title"], "NoDate");
    }

    #[tokio::test]
    async fn closed_milestones_excluded_by_default() {
        let store = Arc::new(MemStore::default());
        let project = seed_project(&store);
        seed_milestone(&store, "OpenOne", None, MilestoneState::Open);
        seed_milestone(&store, "ClosedOne", None, MilestoneState::Closed);
        let app = build_app(store.clone(), Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}/milestones", project.id))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let arr = body_json(resp).await;
        let titles: Vec<String> = arr
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["title"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(titles, vec!["OpenOne"]);
    }

    #[tokio::test]
    async fn include_closed_appends_closed_after_open() {
        let store = Arc::new(MemStore::default());
        let project = seed_project(&store);
        seed_milestone(&store, "OpenOne", None, MilestoneState::Open);
        seed_milestone(&store, "ClosedOne", None, MilestoneState::Closed);
        let app = build_app(store.clone(), Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/projects/{}/milestones?include_closed=true",
                        project.id
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let arr = body_json(resp).await;
        let titles: Vec<String> = arr
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["title"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(titles, vec!["OpenOne", "ClosedOne"]);
    }

    #[tokio::test]
    async fn unknown_project_returns_404() {
        let store = Arc::new(MemStore::default());
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}/milestones", Uuid::new_v4()))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
    }

    async fn post_adopt(
        app: &Router,
        project_id: Uuid,
        body: serde_json::Value,
    ) -> Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/projects/{}/adopt-milestone", project_id))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn adopt_sets_primary_milestone_id() {
        let store = Arc::new(MemStore::default());
        let project = seed_project(&store);
        let milestone = seed_milestone(&store, "v1", None, MilestoneState::Open);
        let app = build_app(store.clone(), Uuid::new_v4());
        let resp = post_adopt(
            &app,
            project.id,
            serde_json::json!({ "milestone_id": milestone.id }),
        )
        .await;
        assert_eq!(resp.status(), 200);
        let v = body_json(resp).await;
        assert_eq!(
            v["primary_milestone_id"].as_str().unwrap(),
            milestone.id.to_string()
        );
        // Underlying row updated too.
        let stored = store
            .projects
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.id == project.id)
            .cloned()
            .unwrap();
        assert_eq!(stored.primary_milestone_id, Some(milestone.id));
    }

    #[tokio::test]
    async fn adopt_with_null_clears_primary() {
        let store = Arc::new(MemStore::default());
        let project = seed_project(&store);
        let milestone = seed_milestone(&store, "v1", None, MilestoneState::Open);
        // Seed an existing primary first.
        store
            .projects
            .lock()
            .unwrap()
            .iter_mut()
            .find(|p| p.id == project.id)
            .unwrap()
            .primary_milestone_id = Some(milestone.id);
        let app = build_app(store.clone(), Uuid::new_v4());
        let resp = post_adopt(
            &app,
            project.id,
            serde_json::json!({ "milestone_id": null }),
        )
        .await;
        assert_eq!(resp.status(), 200);
        let v = body_json(resp).await;
        assert!(v.get("primary_milestone_id").is_none_or(|x| x.is_null()));
    }

    #[tokio::test]
    async fn adopt_unknown_milestone_returns_400() {
        let store = Arc::new(MemStore::default());
        let project = seed_project(&store);
        let app = build_app(store.clone(), Uuid::new_v4());
        let resp = post_adopt(
            &app,
            project.id,
            serde_json::json!({ "milestone_id": Uuid::new_v4() }),
        )
        .await;
        assert_eq!(resp.status(), 400);
    }

    #[tokio::test]
    async fn adopt_on_unknown_project_returns_404() {
        let store = Arc::new(MemStore::default());
        let app = build_app(store.clone(), Uuid::new_v4());
        let resp = post_adopt(
            &app,
            Uuid::new_v4(),
            serde_json::json!({ "milestone_id": null }),
        )
        .await;
        assert_eq!(resp.status(), 404);
    }
}
