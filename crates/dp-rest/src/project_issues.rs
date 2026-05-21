//! Projects ↔ issues membership REST surface
//! (`linear-projects-v2.md` §7.2).
//!
//! Four routes ship here:
//!
//! | route                                                  | what it does                                                  |
//! |--------------------------------------------------------|---------------------------------------------------------------|
//! | `GET    /projects/{id}/issues`                         | paginated issue list scoped to a project (`IssueListResponse`) |
//! | `POST   /projects/{id}/issues`                         | bulk add (`{ expected_version, issue_ids: [..] }`) ⇒ `BulkAddResult` |
//! | `DELETE /projects/{id}/issues/{issue_id}?expected_version=` | single detach, CAS-gated; 204 on success                  |
//! | `GET    /issues/{id}/project`                          | resolve the (single, per v1 `UNIQUE (issue_id)`) project for an issue, or `null` |
//!
//! `BulkAddResult` mirrors the per-row outcome shape pinned in
//! `linear-projects-v2.md` §7.2 / `SCOPE-PROJECTS.md` §7 — every
//! input id ends up either in `added` (the store accepted it) or in
//! `skipped` with a closed-vocabulary `reason` (`"already_in_project"`
//! also carries `existing_project_id` so the UI can render the
//! `Move here?` follow-up without a second round-trip).
//!
//! The bulk-add request is CAS-gated on the **project's** `version`
//! (matches `PATCH /projects/{id}` from §7.1). The detach takes the
//! same `expected_version` as a query param so the URL stays a clean
//! REST shape. The list and "what project owns this issue" GETs are
//! pure reads — no CAS.
//!
//! Authorisation: `(projects, read)` for the two GETs and `(projects,
//! write)` for POST / DELETE — same lanes as the §7.1 CRUD spine.
//! Audit verbs are pinned in [`crate::audit`]: one
//! [`PROJECT_ISSUE_ADD`] per accepted row in a bulk add, and one
//! [`PROJECT_ISSUE_REMOVE`] per detach. Skipped rows never audit
//! (they did not mutate state).
//!
//! [`PROJECT_ISSUE_ADD`]: crate::audit::PROJECT_ISSUE_ADD
//! [`PROJECT_ISSUE_REMOVE`]: crate::audit::PROJECT_ISSUE_REMOVE

use std::sync::Arc;

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{delete, get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::project::{ProjectIssueAddOutcome, ProjectIssueAddSkip};
use dp_domain::store::StoreError;

use crate::audit::{self, Principal};
use crate::error::ApiError;
use crate::issues_read::{
    attach_repo_slugs, IssueDto, IssueListResponse,
};
use crate::projects::ProjectDto;
use crate::repos::{clamp_limit, clamp_offset};
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Wire DTOs
// ---------------------------------------------------------------------------

/// Hard cap on `issue_ids` per bulk-add request, pinned in
/// `linear-projects-v2.md` §7.2 / §9.3. Larger selections from the
/// §6.6 triage bulk affordance are chunked client-side.
pub const BULK_ADD_ISSUE_CAP: usize = 100;

/// Body for `POST /projects/{id}/issues`. CAS-gated on the project's
/// current `version` (§7.2); `issue_ids` is capped at
/// [`BULK_ADD_ISSUE_CAP`].
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct BulkAddIssuesRequest {
    /// The `version` the caller observed on the project row. A
    /// mismatch returns `409 stale_project_version` just like the
    /// §7.1 PATCH / archive routes.
    pub expected_version: i64,
    /// Issue ids to attach. Capped at [`BULK_ADD_ISSUE_CAP`]; over
    /// the cap returns `400 bulk_add_too_large`. An empty array is
    /// accepted as a no-op (returns `BulkAddResult { added: [],
    /// skipped: [] }` and does not bump the project version).
    pub issue_ids: Vec<Uuid>,
}

/// One row in [`BulkAddResult::skipped`]. Mirrors
/// [`ProjectIssueAddSkip`] but kept as a separate wire type so the
/// OpenAPI schema is decoupled from the domain crate and so the
/// `reason` vocabulary is documented at the REST boundary.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BulkAddSkipDto {
    /// The issue id that was rejected.
    pub issue_id: Uuid,
    /// Closed-vocabulary reason; one of:
    ///
    /// * `"already_in_project"` — the v1 `UNIQUE (issue_id)`
    ///   constraint fired. `existing_project_id` is set so the UI
    ///   can render a `Move here?` affordance.
    /// * `"unknown_issue"` — the issue id did not resolve in
    ///   `dp_issues`.
    /// * `"cross_org"` — the issue's `org_id` differs from the
    ///   project's `org_id` (v1: one org per project, §4).
    pub reason: String,
    /// Set when `reason == "already_in_project"`. Lets the UI link
    /// directly to the existing project's detail page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_project_id: Option<Uuid>,
}

impl From<ProjectIssueAddSkip> for BulkAddSkipDto {
    fn from(s: ProjectIssueAddSkip) -> Self {
        Self {
            issue_id: s.issue_id,
            reason: s.reason,
            existing_project_id: s.existing_project_id,
        }
    }
}

/// `BulkAddResult` — the per-row outcome shape `linear-projects-v2.md`
/// §7.2 / `SCOPE-PROJECTS.md` §7 wire through the REST layer so the
/// UI can render add-by-add status from one round-trip.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BulkAddResult {
    /// Issue ids the store accepted into the project.
    pub added: Vec<Uuid>,
    /// Issue ids the store refused, each with a closed-vocabulary
    /// `reason`. See [`BulkAddSkipDto`].
    pub skipped: Vec<BulkAddSkipDto>,
}

impl From<ProjectIssueAddOutcome> for BulkAddResult {
    fn from(o: ProjectIssueAddOutcome) -> Self {
        Self {
            added: o.added,
            skipped: o.skipped.into_iter().map(BulkAddSkipDto::from).collect(),
        }
    }
}

/// Query params for `DELETE /projects/{id}/issues/{issue_id}`. The
/// `expected_version` rides as a query param so the URL stays a
/// clean REST shape — matches the §7.1 PATCH convention.
#[derive(Debug, Clone, Deserialize)]
pub struct RemoveIssueQuery {
    /// The `version` the caller observed on the project row.
    pub expected_version: i64,
}

/// Query params for `GET /projects/{id}/issues`. Slice A keeps the
/// filter narrow: pagination + state + a title substring. The full
/// `ListIssuesQuery` lane is reserved for slice B once project-aware
/// SQL filtering lands; v1 pulls the membership list, hydrates each
/// row, and filters in-memory — which is correct for the
/// O(≤100) project sizes the slice-A surfaces target.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListProjectIssuesQuery {
    /// State filter (`open` / `closed` / `all`); defaults to `all`
    /// here (a project detail surface wants to see both open and
    /// closed work by default — different from `GET /issues` which
    /// defaults to `open`). Pass `?state=open` for an active-only
    /// view.
    #[serde(default)]
    pub state: Option<String>,
    /// Case-insensitive substring on issue title.
    #[serde(default)]
    pub q: Option<String>,
    /// Page size; clamped 1..=200, default 50.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Page offset, 0-based.
    #[serde(default)]
    pub offset: Option<i64>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn map_cas_error(project_id: Uuid, err: StoreError) -> ApiError {
    match err {
        StoreError::NotFound { entity: "project", .. } => ApiError::NotFound {
            code: "project_not_found",
            message: format!("no project with id {project_id}"),
        },
        StoreError::NotFound { entity: "project_issue", id } => ApiError::NotFound {
            code: "project_issue_not_found",
            message: format!("issue {id} is not attached to project {project_id}"),
        },
        StoreError::Conflict(msg) => ApiError::Conflict {
            code: "stale_project_version",
            message: msg,
        },
        StoreError::Invalid(msg) => ApiError::BadRequest {
            code: "project_invalid",
            message: msg,
        },
        e => e.into(),
    }
}

/// `GET /projects/{id}/issues` — paginated issue list scoped to a
/// project (§7.2). Same envelope as `GET /issues`.
///
/// Implementation note: v1 resolves membership via
/// [`Store::list_issue_ids_for_project`], fetches each issue row
/// with [`Store::get_issue`], applies optional `state` / `q` filters
/// in-memory, then paginates. Correct for the slice-A target of
/// projects with ≤ 100 issues. The natural follow-up — a SQL-level
/// "filter `dp_issues` by `project_id`" — is deferred until
/// `IssueListFilter` grows a `project_id` field in slice B.
#[utoipa::path(
    get,
    path = "/projects/{id}/issues",
    params(
        ("id"     = Uuid,           Path,  description = "Project id"),
        ("state"  = Option<String>, Query, description = "open|closed|all (default all)"),
        ("q"      = Option<String>, Query, description = "Substring search on title"),
        ("limit"  = Option<i64>,    Query, description = "Page size (1..=200, default 50)"),
        ("offset" = Option<i64>,    Query, description = "Page offset (default 0)"),
    ),
    responses(
        (status = 200, description = "Paginated issue list scoped to the project", body = IssueListResponse),
        (status = 404, description = "No such project"),
    ),
    tag = "projects",
)]
pub async fn list_project_issues(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Query(q): Query<ListProjectIssuesQuery>,
) -> Result<Json<IssueListResponse>, ApiError> {
    // 404 fast when the project itself is missing so the caller does
    // not get an empty-rows list and assume an empty project.
    let _project = state
        .store
        .get_project(project_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "project_not_found",
            message: format!("no project with id {project_id}"),
        })?;

    let state_filter = match q.state.as_deref() {
        None | Some("") | Some("all") => None,
        Some("open") => Some(dp_domain::issue::IssueState::Open),
        Some("closed") => Some(dp_domain::issue::IssueState::Closed),
        Some(other) => {
            return Err(ApiError::BadRequest {
                code: "invalid_state",
                message: format!("invalid state filter: {other}"),
            });
        }
    };
    let q_str = q.q.as_deref().map(|s| s.trim().to_lowercase());

    let ids = state.store.list_issue_ids_for_project(project_id).await?;
    // Resolve each issue row. Missing rows (target FK was hard-deleted
    // out from under us — unlikely given `ON DELETE CASCADE` but
    // belt-and-braces) are silently dropped; the membership row
    // would normally have been cascaded along with it, so this
    // branch should never fire in practice.
    let mut issues: Vec<dp_domain::issue::Issue> = Vec::with_capacity(ids.len());
    for id in &ids {
        if let Some(i) = state.store.get_issue(*id).await? {
            issues.push(i);
        }
    }
    // Apply in-memory filters in the same conjunctive style the SQL
    // layer would.
    if let Some(s) = state_filter {
        issues.retain(|i| i.state == s);
    }
    if let Some(needle) = q_str.as_deref().filter(|s| !s.is_empty()) {
        issues.retain(|i| i.title.to_lowercase().contains(needle));
    }
    let total = issues.len() as i64;
    let limit = clamp_limit(q.limit);
    let offset = clamp_offset(q.offset);
    let start = offset.max(0) as usize;
    let end = (start + limit.max(0) as usize).min(issues.len());
    let page = if start >= issues.len() {
        Vec::new()
    } else {
        issues[start..end].to_vec()
    };
    let mut dtos: Vec<IssueDto> = page.into_iter().map(IssueDto::from).collect();
    attach_repo_slugs(&*state.store, &mut dtos).await?;
    Ok(Json(IssueListResponse {
        rows: dtos,
        total,
        limit,
        offset,
    }))
}

/// `POST /projects/{id}/issues` — bulk add (§7.2). Returns
/// `BulkAddResult` so per-row outcomes flow back in one round-trip.
///
/// * `issue_ids` capped at [`BULK_ADD_ISSUE_CAP`]; over the cap
///   returns `400 bulk_add_too_large`.
/// * CAS-gated on the project's `version`; mismatch returns
///   `409 stale_project_version`.
/// * One audit row per accepted issue
///   ([`audit::PROJECT_ISSUE_ADD`]); skipped rows never audit.
#[utoipa::path(
    post,
    path = "/projects/{id}/issues",
    params(("id" = Uuid, Path, description = "Project id")),
    request_body = BulkAddIssuesRequest,
    responses(
        (status = 200, description = "Per-row outcome of the bulk add", body = BulkAddResult),
        (status = 400, description = "Validation failure (cap, etc.)"),
        (status = 404, description = "No such project"),
        (status = 409, description = "Stale `expected_version`"),
    ),
    tag = "projects",
)]
pub async fn bulk_add_issues(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(project_id): Path<Uuid>,
    Json(body): Json<BulkAddIssuesRequest>,
) -> Result<Json<BulkAddResult>, ApiError> {
    if body.issue_ids.len() > BULK_ADD_ISSUE_CAP {
        return Err(ApiError::BadRequest {
            code: "bulk_add_too_large",
            message: format!(
                "issue_ids is capped at {BULK_ADD_ISSUE_CAP}; got {}",
                body.issue_ids.len()
            ),
        });
    }
    let outcome = state
        .store
        .add_issues_to_project(
            project_id,
            body.expected_version,
            &body.issue_ids,
            Some(principal.actor_user_id),
        )
        .await
        .map_err(|e| map_cas_error(project_id, e))?;
    for issue_id in &outcome.added {
        audit::record(
            state.store.as_ref(),
            principal.actor_user_id,
            audit::PROJECT_ISSUE_ADD,
            format!("{project_id}:{issue_id}"),
        )
        .await?;
    }
    Ok(Json(outcome.into()))
}

/// `DELETE /projects/{id}/issues/{issue_id}?expected_version=` —
/// single detach (§7.2). 204 on success; CAS-gated on the project's
/// `version`. A no-op detach (the issue is not currently in this
/// project) is `404 project_issue_not_found` — same idempotence-at-
/// the-application-boundary contract as the store layer.
#[utoipa::path(
    delete,
    path = "/projects/{id}/issues/{issue_id}",
    params(
        ("id"       = Uuid, Path,  description = "Project id"),
        ("issue_id" = Uuid, Path,  description = "Issue id to detach"),
        ("expected_version" = i64, Query, description = "Caller-observed project version (CAS)"),
    ),
    responses(
        (status = 204, description = "Detached"),
        (status = 404, description = "No such project, or issue is not in this project"),
        (status = 409, description = "Stale `expected_version`"),
    ),
    tag = "projects",
)]
pub async fn remove_project_issue(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((project_id, issue_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<RemoveIssueQuery>,
) -> Result<Response, ApiError> {
    state
        .store
        .remove_issue_from_project(project_id, issue_id, q.expected_version)
        .await
        .map_err(|e| map_cas_error(project_id, e))?;
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_ISSUE_REMOVE,
        format!("{project_id}:{issue_id}"),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `GET /issues/{id}/project` — resolve the (single, per v1 `UNIQUE
/// (issue_id)`) project for an issue, or `null` when the issue is
/// not in any project. Backs the §6.5 detail-pane chip.
#[utoipa::path(
    get,
    path = "/issues/{id}/project",
    params(("id" = Uuid, Path, description = "Issue id")),
    responses(
        (status = 200, description = "ProjectDto or null", body = Option<ProjectDto>),
    ),
    tag = "projects",
)]
pub async fn get_project_for_issue(
    State(state): State<AppState>,
    Path(issue_id): Path<Uuid>,
) -> Result<Json<Option<ProjectDto>>, ApiError> {
    let project = state.store.get_project_for_issue(issue_id).await?;
    Ok(Json(project.map(ProjectDto::from)))
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the project ↔ issue membership router fragment.
/// `dp-server::build` merges this into the protected stack alongside
/// the §7.1 projects spine. Reads are gated on `(projects, read)`;
/// writes on `(projects, write)` — same lanes as the §7.1 routes.
pub fn project_issues_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new()
                .route("/projects/{id}/issues", get(list_project_issues))
                .route("/issues/{id}/project", get(get_project_for_issue)),
            "projects",
            "read",
        ))
        .merge(with_permission(
            Router::new()
                .route("/projects/{id}/issues", post(bulk_add_issues))
                .route(
                    "/projects/{id}/issues/{issue_id}",
                    delete(remove_project_issue),
                ),
            "projects",
            "write",
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
    use axum::http::Request;
    use chrono::Utc;
    use std::sync::Mutex;
    use tower::ServiceExt;

    use dp_domain::audit::AuditEntry;
    use dp_domain::issue::{Issue, IssueState};
    use dp_domain::project::{Project, ProjectStatus};
    use dp_domain::store::{EventActorRow, Store, StoreError};
    use dp_domain::{
        ActivityEvent, ActorRole, EventActor, FetchCursor, FetchRun, FetchRunKind, Membership,
        Org, Repo, ResourceKind, Team, User, WebhookDelivery, Window,
    };

    // -----------------------------------------------------------------
    // Minimal in-memory store: just the surface the membership routes
    // exercise. The §7.1 tests use a similar pattern in `projects.rs`.
    // -----------------------------------------------------------------

    #[derive(Default)]
    struct MemStore {
        projects: Mutex<Vec<Project>>,
        issues: Mutex<Vec<Issue>>,
        memberships: Mutex<Vec<(Uuid, Uuid)>>, // (project, issue)
        audit: Mutex<Vec<AuditEntry>>,
    }

    impl MemStore {
        fn audit_rows(&self) -> Vec<AuditEntry> {
            self.audit.lock().unwrap().clone()
        }
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

        async fn list_issue_ids_for_project(
            &self,
            project_id: Uuid,
        ) -> Result<Vec<Uuid>, StoreError> {
            Ok(self
                .memberships
                .lock()
                .unwrap()
                .iter()
                .filter(|(p, _)| *p == project_id)
                .map(|(_, i)| *i)
                .collect())
        }

        async fn get_issue(&self, id: Uuid) -> Result<Option<Issue>, StoreError> {
            Ok(self
                .issues
                .lock()
                .unwrap()
                .iter()
                .find(|i| i.id == id)
                .cloned())
        }

        async fn add_issues_to_project(
            &self,
            project_id: Uuid,
            expected_version: i64,
            issue_ids: &[Uuid],
            _actor: Option<Uuid>,
        ) -> Result<ProjectIssueAddOutcome, StoreError> {
            let mut projects = self.projects.lock().unwrap();
            let project = projects
                .iter_mut()
                .find(|p| p.id == project_id)
                .ok_or_else(|| StoreError::NotFound {
                    entity: "project",
                    id: project_id.to_string(),
                })?;
            if project.version != expected_version {
                return Err(StoreError::Conflict(format!(
                    "project version mismatch: expected {expected_version}, found {}",
                    project.version
                )));
            }
            let project_org = project.org_id;
            drop(projects);

            let mut added = Vec::new();
            let mut skipped = Vec::new();
            for &issue_id in issue_ids {
                let issue = self
                    .issues
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|i| i.id == issue_id)
                    .cloned();
                let Some(issue) = issue else {
                    skipped.push(ProjectIssueAddSkip {
                        issue_id,
                        reason: "unknown_issue".into(),
                        existing_project_id: None,
                    });
                    continue;
                };
                if issue.org_id != project_org {
                    skipped.push(ProjectIssueAddSkip {
                        issue_id,
                        reason: "cross_org".into(),
                        existing_project_id: None,
                    });
                    continue;
                }
                let existing = self
                    .memberships
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|(_, i)| *i == issue_id)
                    .map(|(p, _)| *p);
                if let Some(existing) = existing {
                    skipped.push(ProjectIssueAddSkip {
                        issue_id,
                        reason: "already_in_project".into(),
                        existing_project_id: Some(existing),
                    });
                    continue;
                }
                self.memberships
                    .lock()
                    .unwrap()
                    .push((project_id, issue_id));
                added.push(issue_id);
            }
            if !added.is_empty() {
                let mut projects = self.projects.lock().unwrap();
                let project = projects.iter_mut().find(|p| p.id == project_id).unwrap();
                project.version += 1;
                project.issue_count = self
                    .memberships
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(p, _)| *p == project_id)
                    .count() as i32;
                project.updated_at = Utc::now();
            }
            Ok(ProjectIssueAddOutcome { added, skipped })
        }

        async fn remove_issue_from_project(
            &self,
            project_id: Uuid,
            issue_id: Uuid,
            expected_version: i64,
        ) -> Result<Project, StoreError> {
            let mut projects = self.projects.lock().unwrap();
            let project = projects
                .iter_mut()
                .find(|p| p.id == project_id)
                .ok_or_else(|| StoreError::NotFound {
                    entity: "project",
                    id: project_id.to_string(),
                })?;
            if project.version != expected_version {
                return Err(StoreError::Conflict(format!(
                    "project version mismatch: expected {expected_version}, found {}",
                    project.version
                )));
            }
            let mut links = self.memberships.lock().unwrap();
            let before = links.len();
            links.retain(|(p, i)| !(*p == project_id && *i == issue_id));
            if links.len() == before {
                return Err(StoreError::NotFound {
                    entity: "project_issue",
                    id: issue_id.to_string(),
                });
            }
            project.version += 1;
            project.issue_count = links.iter().filter(|(p, _)| *p == project_id).count() as i32;
            project.updated_at = Utc::now();
            Ok(project.clone())
        }

        async fn get_project_for_issue(
            &self,
            issue_id: Uuid,
        ) -> Result<Option<Project>, StoreError> {
            let owner = self
                .memberships
                .lock()
                .unwrap()
                .iter()
                .find(|(_, i)| *i == issue_id)
                .map(|(p, _)| *p);
            let Some(pid) = owner else { return Ok(None) };
            self.get_project(pid).await
        }

        async fn record_audit_log(&self, entry: &AuditEntry) -> Result<(), StoreError> {
            self.audit.lock().unwrap().push(entry.clone());
            Ok(())
        }

        // --- minimal stubs for the rest of the Store surface --------
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
            Ok(vec![])
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
            _: Uuid,
        ) -> Result<Vec<Membership>, StoreError> {
            Ok(vec![])
        }
        async fn set_home_org(
            &self,
            _: Uuid,
            _: Uuid,
            _: Option<Uuid>,
        ) -> Result<(), StoreError> {
            Ok(())
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
    }

    // -----------------------------------------------------------------
    // Harness
    // -----------------------------------------------------------------

    fn build_app(store: Arc<MemStore>, actor: Uuid) -> Router {
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
        project_issues_router(app_state)
            .layer(Extension(Principal { actor_user_id: actor }))
            .layer(Extension(spi_principal))
            .layer(Extension(engine))
    }

    async fn json_of(resp: axum::response::Response) -> serde_json::Value {
        let body = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    fn seed_project(store: &MemStore, org: Uuid) -> Project {
        let p = Project {
            id: Uuid::new_v4(),
            org_id: org,
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
        };
        store.projects.lock().unwrap().push(p.clone());
        p
    }

    fn seed_issue(store: &MemStore, org: Uuid, state: IssueState, title: &str) -> Issue {
        let i = Issue {
            id: Uuid::new_v4(),
            org_id: org,
            repo_id: Uuid::new_v4(),
            github_id: 0,
            number: 1,
            title: title.into(),
            body: None,
            state,
            labels: Vec::new(),
            assignees: Vec::new(),
            milestone: None,
            version: 1,
            github_node_id: None,
            updated_at: Utc::now(),
        };
        store.issues.lock().unwrap().push(i.clone());
        i
    }

    // -----------------------------------------------------------------
    // POST /projects/{id}/issues
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn bulk_add_returns_added_and_skipped_with_audit() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let i_ok = seed_issue(&store, org, IssueState::Open, "ok");
        let i_cross = seed_issue(&store, Uuid::new_v4(), IssueState::Open, "wrong org");
        let i_unknown = Uuid::new_v4();
        let app = build_app(store.clone(), actor);
        let body = serde_json::json!({
            "expected_version": 1,
            "issue_ids": [i_ok.id, i_cross.id, i_unknown],
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/projects/{}/issues", project.id))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert_eq!(v["added"], serde_json::json!([i_ok.id]));
        let skipped = v["skipped"].as_array().unwrap();
        assert_eq!(skipped.len(), 2);
        let reasons: Vec<&str> = skipped
            .iter()
            .map(|s| s["reason"].as_str().unwrap())
            .collect();
        assert!(reasons.contains(&"cross_org"));
        assert!(reasons.contains(&"unknown_issue"));
        let audit_rows = store.audit_rows();
        assert_eq!(audit_rows.len(), 1, "only the accepted row audits");
        assert_eq!(audit_rows[0].action, audit::PROJECT_ISSUE_ADD);
    }

    #[tokio::test]
    async fn bulk_add_returns_already_in_project_with_existing_id() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let other_project = seed_project(&store, org);
        let issue = seed_issue(&store, org, IssueState::Open, "shared");
        // Pre-attach to the other project.
        store
            .memberships
            .lock()
            .unwrap()
            .push((other_project.id, issue.id));
        let app = build_app(store.clone(), Uuid::new_v4());
        let body = serde_json::json!({
            "expected_version": 1,
            "issue_ids": [issue.id],
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/projects/{}/issues", project.id))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert!(v["added"].as_array().unwrap().is_empty());
        let skip0 = &v["skipped"][0];
        assert_eq!(skip0["reason"], "already_in_project");
        assert_eq!(skip0["existing_project_id"], serde_json::json!(other_project.id));
    }

    #[tokio::test]
    async fn bulk_add_rejects_stale_version_with_409() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let issue = seed_issue(&store, org, IssueState::Open, "x");
        let app = build_app(store.clone(), Uuid::new_v4());
        let body = serde_json::json!({
            "expected_version": 99,
            "issue_ids": [issue.id],
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/projects/{}/issues", project.id))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "stale_project_version");
        assert!(store.audit_rows().is_empty());
    }

    #[tokio::test]
    async fn bulk_add_rejects_over_cap_with_400() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let app = build_app(store.clone(), Uuid::new_v4());
        let ids: Vec<Uuid> = (0..(BULK_ADD_ISSUE_CAP + 1)).map(|_| Uuid::new_v4()).collect();
        let body = serde_json::json!({
            "expected_version": 1,
            "issue_ids": ids,
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/projects/{}/issues", project.id))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "bulk_add_too_large");
    }

    #[tokio::test]
    async fn bulk_add_empty_list_is_noop() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let app = build_app(store.clone(), Uuid::new_v4());
        let body = serde_json::json!({
            "expected_version": 1,
            "issue_ids": [],
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/projects/{}/issues", project.id))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert!(v["added"].as_array().unwrap().is_empty());
        assert!(v["skipped"].as_array().unwrap().is_empty());
        assert!(store.audit_rows().is_empty());
        // Version unchanged when nothing landed.
        let row = &store.projects.lock().unwrap()[0];
        assert_eq!(row.version, 1);
    }

    // -----------------------------------------------------------------
    // DELETE /projects/{id}/issues/{issue_id}
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn remove_detaches_and_returns_204() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let issue = seed_issue(&store, org, IssueState::Open, "x");
        store.memberships.lock().unwrap().push((project.id, issue.id));
        let app = build_app(store.clone(), actor);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/projects/{}/issues/{}?expected_version=1",
                        project.id, issue.id
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(store.memberships.lock().unwrap().is_empty());
        let rows = store.audit_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].action, audit::PROJECT_ISSUE_REMOVE);
    }

    #[tokio::test]
    async fn remove_returns_404_when_membership_missing() {
        let store = Arc::new(MemStore::default());
        let project = seed_project(&store, Uuid::new_v4());
        let app = build_app(store.clone(), Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/projects/{}/issues/{}?expected_version=1",
                        project.id,
                        Uuid::new_v4(),
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "project_issue_not_found");
        assert!(store.audit_rows().is_empty());
    }

    #[tokio::test]
    async fn remove_rejects_stale_version_with_409() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let issue = seed_issue(&store, org, IssueState::Open, "x");
        store.memberships.lock().unwrap().push((project.id, issue.id));
        let app = build_app(store.clone(), Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/projects/{}/issues/{}?expected_version=42",
                        project.id, issue.id
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    // -----------------------------------------------------------------
    // GET /issues/{id}/project
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn get_project_for_issue_returns_null_when_unattached() {
        let store = Arc::new(MemStore::default());
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/issues/{}/project", Uuid::new_v4()))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert!(v.is_null());
    }

    #[tokio::test]
    async fn get_project_for_issue_returns_owning_project() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let issue = seed_issue(&store, org, IssueState::Open, "x");
        store.memberships.lock().unwrap().push((project.id, issue.id));
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/issues/{}/project", issue.id))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert_eq!(v["id"], serde_json::json!(project.id));
    }

    // -----------------------------------------------------------------
    // GET /projects/{id}/issues
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn list_project_issues_returns_only_attached_rows() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let i_in = seed_issue(&store, org, IssueState::Open, "in-project");
        let _i_out = seed_issue(&store, org, IssueState::Open, "out-of-project");
        store
            .memberships
            .lock()
            .unwrap()
            .push((project.id, i_in.id));
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}/issues", project.id))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert_eq!(v["total"], 1);
        assert_eq!(v["rows"][0]["title"], "in-project");
    }

    #[tokio::test]
    async fn list_project_issues_filters_by_state_and_query() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let open_match = seed_issue(&store, org, IssueState::Open, "Alpha rollout");
        let closed_match = seed_issue(&store, org, IssueState::Closed, "Alpha cleanup");
        let open_other = seed_issue(&store, org, IssueState::Open, "Beta scout");
        for id in [open_match.id, closed_match.id, open_other.id] {
            store.memberships.lock().unwrap().push((project.id, id));
        }
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/projects/{}/issues?state=open&q=alpha",
                        project.id
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let v = json_of(resp).await;
        assert_eq!(v["total"], 1);
        assert_eq!(v["rows"][0]["title"], "Alpha rollout");
    }

    #[tokio::test]
    async fn list_project_issues_404_when_project_missing() {
        let store = Arc::new(MemStore::default());
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}/issues", Uuid::new_v4()))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
