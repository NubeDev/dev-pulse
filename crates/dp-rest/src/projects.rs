//! Projects REST CRUD (`linear-projects-v2.md` §7.1).
//!
//! Five routes ship here — the slice-A primary surface that a user
//! drives from `#/projects`:
//!
//! | route                              | what it does                                          |
//! |------------------------------------|-------------------------------------------------------|
//! | `GET    /projects`                 | filtered list (`?org_id=&status=&q=&limit=&offset=`)  |
//! | `GET    /projects/{id}`            | one project                                           |
//! | `POST   /projects`                 | create — assigns `version = 1`, defaults `status`     |
//! | `PATCH  /projects/{id}`            | update under §8.2 CAS (`expected_version`)            |
//! | `POST   /projects/{id}/archive`    | elevated archive op (§9.2)                            |
//!
//! Membership (`/projects/{id}/issues`, §7.2) and the board picker
//! (`/orgs/{org_id}/projects-v2`, §7.3) land in their own modules in
//! later stages; this one is the v1 CRUD spine.
//!
//! Authorisation is `(projects, read)` for the two GETs and
//! `(projects, write)` for create / patch / archive. The §9.2
//! elevated checks (lead-or-author for `archive` and for any
//! `lead_user_id` change) are deferred to the stage that wires the
//! membership / lead-resolution helper alongside the §6.5 detail-
//! pane — slice A handlers gate behind the broader `write` pair
//! and the §9.2 work-item tracks the elevation refinement.
//!
//! Audit verbs are pinned in [`crate::audit`]: [`PROJECT_CREATE`],
//! [`PROJECT_UPDATE`], [`PROJECT_ARCHIVE`]. Every accepted mutation
//! lands one row through [`audit::record`] after the write commits;
//! a failed mutation never audits (matches the `pin.*` / `tag.*`
//! convention in [`crate::pins`] / [`crate::tags`]).
//!
//! Counts on the wire: `issue_count` and `closed_issue_count` are
//! the denormalised columns the store maintains (§7.1 footnote).
//! `board_link_count` is also part of the §7.1 DTO; slice A has no
//! `dp_project_board_links` table yet, so the field is always `0`
//! here and starts reflecting reality once slice B lands the link
//! CRUD + count helper.
//!
//! [`PROJECT_CREATE`]: crate::audit::PROJECT_CREATE
//! [`PROJECT_UPDATE`]: crate::audit::PROJECT_UPDATE
//! [`PROJECT_ARCHIVE`]: crate::audit::PROJECT_ARCHIVE

use std::sync::Arc;

use axum::{
    extract::{Extension, Path, Query, State},
    response::Json,
    routing::{get, patch, post},
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::project::{
    Project, ProjectListFilter, ProjectStatus, ProjectUpsert,
};
use dp_domain::store::{StoreError, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT};

use crate::audit::{self, Principal};
use crate::error::ApiError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Wire DTOs
// ---------------------------------------------------------------------------

/// Wire form of [`ProjectStatus`]. Lowercase to mirror the SQL
/// CHECK vocabulary and the §6.1 sidebar labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProjectStatusDto {
    /// Actively planned against. Default for new projects.
    Active,
    /// On the books but not currently in-flight.
    Backlog,
    /// Work is finished.
    Done,
    /// Hidden from default views.
    Archived,
}

impl From<ProjectStatus> for ProjectStatusDto {
    fn from(s: ProjectStatus) -> Self {
        match s {
            ProjectStatus::Active => Self::Active,
            ProjectStatus::Backlog => Self::Backlog,
            ProjectStatus::Done => Self::Done,
            ProjectStatus::Archived => Self::Archived,
        }
    }
}

impl From<ProjectStatusDto> for ProjectStatus {
    fn from(s: ProjectStatusDto) -> Self {
        match s {
            ProjectStatusDto::Active => Self::Active,
            ProjectStatusDto::Backlog => Self::Backlog,
            ProjectStatusDto::Done => Self::Done,
            ProjectStatusDto::Archived => Self::Archived,
        }
    }
}

/// One row of `GET /projects` and the body shape for the
/// single-row endpoints. Mirrors `linear-projects-v2.md` §7.1.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProjectDto {
    /// Primary key.
    pub id: Uuid,
    /// Parent org.
    pub org_id: Uuid,
    /// Project name (case-insensitively unique within org for
    /// non-archived rows, §8 partial index).
    pub name: String,
    /// Optional markdown description.
    pub description: Option<String>,
    /// Lead user. Drives default visibility and `Mentioned` filter
    /// targets. Mutating this field is an elevated op (§9.2).
    pub lead_user_id: Option<Uuid>,
    /// Lifecycle state.
    pub status: ProjectStatusDto,
    /// Planned start instant, UTC. `None` when unset.
    pub start_at: Option<DateTime<Utc>>,
    /// Planned due instant, UTC. `None` when unset.
    pub due_at: Option<DateTime<Utc>>,
    /// Denormalised membership count (§7.1 footnote).
    pub issue_count: i32,
    /// Denormalised count of attached issues whose `dp_issues.state
    /// = 'closed'`. Drives the §6.2 progress bar.
    pub closed_issue_count: i32,
    /// Count of `dp_project_board_links` rows attached to this
    /// project. Slice A returns `0` (the link table lands in
    /// slice B); the DTO carries the field now so clients can
    /// rely on a stable shape.
    pub board_link_count: i32,
    /// §8.2 CAS counter. PATCH / archive callers echo this back
    /// as `expected_version`; a mismatch ⇒ 409 conflict.
    pub version: i64,
    /// Author (`dp_users.id`). Immutable per §9.2. `None` when the
    /// original author was pseudonymised.
    pub created_by: Option<Uuid>,
    /// First-write timestamp.
    pub created_at: DateTime<Utc>,
    /// Last accepted mutation.
    pub updated_at: DateTime<Utc>,
    /// Adopted primary milestone (PROJECT-VIEW.md §5.5 / §9.5).
    /// Set / cleared by `POST /projects/{id}/adopt-milestone`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_milestone_id: Option<Uuid>,
}

impl From<Project> for ProjectDto {
    fn from(p: Project) -> Self {
        Self {
            id: p.id,
            org_id: p.org_id,
            name: p.name,
            description: p.description,
            lead_user_id: p.lead_user_id,
            status: p.status.into(),
            start_at: p.start_at,
            due_at: p.due_at,
            issue_count: p.issue_count,
            closed_issue_count: p.closed_issue_count,
            // Slice B replaces this with a live count off
            // `dp_project_board_links`. The DTO field exists today
            // so the frontend / MCP wire shape doesn't shift when
            // the table lands.
            board_link_count: 0,
            version: p.version,
            created_by: p.created_by,
            created_at: p.created_at,
            updated_at: p.updated_at,
            primary_milestone_id: p.primary_milestone_id,
        }
    }
}

/// Paginated envelope mirroring [`crate::issues_read::IssueListResponse`].
/// `rows` is omitted when the caller passes `?count_only=1` and the
/// envelope collapses to `{ total, limit, offset, rows: [] }` — the
/// sidebar (§6.1) uses this to render counts without dragging the
/// full row payload over the wire.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProjectListResponse {
    /// Projects on this page, ordered per §6.2 (status → due → name).
    pub rows: Vec<ProjectDto>,
    /// Total matching the filter, ignoring pagination.
    pub total: i64,
    /// Echoed limit (always `0` when `count_only = 1`).
    pub limit: i64,
    /// Echoed offset.
    pub offset: i64,
}

/// Query params for `GET /projects` (§7.1).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListProjectsQuery {
    /// Restrict to one org. The §15 org-gate already narrows the
    /// caller's visible orgs; this filter further narrows to one
    /// (the sidebar passes a single org id from the user's
    /// memberships).
    #[serde(default)]
    pub org_id: Option<Uuid>,
    /// Restrict to one status. `None` ⇒ every status (the §6.2
    /// list page passes `None` and groups in the UI).
    #[serde(default)]
    pub status: Option<ProjectStatusDto>,
    /// Case-insensitive substring on `name` (§6.2 search bar).
    #[serde(default)]
    pub q: Option<String>,
    /// Page size; clamped server-side to
    /// [`MAX_LIST_LIMIT`]. Defaults to [`DEFAULT_LIST_LIMIT`].
    #[serde(default)]
    pub limit: Option<i64>,
    /// Page offset; defaults to 0.
    #[serde(default)]
    pub offset: Option<i64>,
    /// When `1` (or `true`), return only the total count — `rows`
    /// is an empty array and `limit` is `0`. Powers the §6.1
    /// sidebar `Active (3)` / `Backlog (12)` badges without
    /// pulling full rows.
    #[serde(default)]
    pub count_only: Option<u8>,
}

/// Body for `POST /projects`. `status` is optional — when omitted
/// the server defaults to [`ProjectStatusDto::Active`] (§6.2 "Status
/// defaults to `active`"). `created_by` is filled from the
/// [`Principal`], not the body.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct CreateProjectRequest {
    /// Parent org.
    pub org_id: Uuid,
    /// Project name.
    pub name: String,
    /// Optional markdown description.
    #[serde(default)]
    pub description: Option<String>,
    /// Optional initial lead user.
    #[serde(default)]
    pub lead_user_id: Option<Uuid>,
    /// Optional initial status; defaults to `active`.
    #[serde(default)]
    pub status: Option<ProjectStatusDto>,
    /// Optional planned start.
    #[serde(default)]
    pub start_at: Option<DateTime<Utc>>,
    /// Optional planned due.
    #[serde(default)]
    pub due_at: Option<DateTime<Utc>>,
}

/// Body for `PATCH /projects/{id}`. Carries the §8.2
/// `expected_version` plus the merged-in field set. Fields that
/// are omitted are left untouched — the store layer takes a full
/// upsert payload, so the handler reads the current row and
/// overlays the partial body. This keeps the wire shape narrow
/// without leaking the upsert's "all fields required" shape.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct PatchProjectRequest {
    /// The `version` the caller observed; the server CASes against
    /// this and surfaces a `stale_project_version` 409 on mismatch.
    pub expected_version: i64,
    /// New name (optional).
    #[serde(default)]
    pub name: Option<String>,
    /// New description; pass `null` explicitly to clear by sending
    /// `{"description": null}` (we cannot distinguish that from
    /// "missing" with `#[serde(default)]`, so today the only way
    /// to clear is via PATCH with `description: null` interpreted
    /// in the handler — see [`patch_project`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    /// New lead. `None` ⇒ leave unchanged. `Some(None)` ⇒ clear.
    /// Mutating this is an elevated op per §9.2; v1 handler does
    /// not yet check the elevation but the lane is wired so the
    /// follow-up that ships lead-or-author resolution can land
    /// the check without a wire change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_user_id: Option<Option<Uuid>>,
    /// New status.
    #[serde(default)]
    pub status: Option<ProjectStatusDto>,
    /// New start instant. `Some(None)` clears.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_at: Option<Option<DateTime<Utc>>>,
    /// New due instant. `Some(None)` clears.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_at: Option<Option<DateTime<Utc>>>,
}

/// Body for `POST /projects/{id}/archive`. CAS-gated on the
/// project's current `version` (§9.2). The handler is idempotent —
/// archiving an already-archived project echoes the row back
/// without bumping `version`.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct ArchiveProjectRequest {
    /// The `version` the caller observed.
    pub expected_version: i64,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn resolve_pagination(limit: Option<i64>, offset: Option<i64>) -> (i64, i64) {
    let limit = limit
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .clamp(1, MAX_LIST_LIMIT);
    let offset = offset.unwrap_or(0).max(0);
    (limit, offset)
}

fn validate_name(name: &str) -> Result<(), ApiError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest {
            code: "project_name_required",
            message: "project name must be non-empty".into(),
        });
    }
    if trimmed.len() > 200 {
        return Err(ApiError::BadRequest {
            code: "project_name_too_long",
            message: "project name must be 200 characters or fewer".into(),
        });
    }
    Ok(())
}

fn validate_dates(
    start_at: Option<DateTime<Utc>>,
    due_at: Option<DateTime<Utc>>,
) -> Result<(), ApiError> {
    if let (Some(s), Some(d)) = (start_at, due_at) {
        if s > d {
            return Err(ApiError::BadRequest {
                code: "project_dates_inverted",
                message: "start_at must be <= due_at".into(),
            });
        }
    }
    Ok(())
}

fn map_store_cas_error(id: Uuid, err: StoreError) -> ApiError {
    match err {
        StoreError::NotFound { .. } => ApiError::NotFound {
            code: "project_not_found",
            message: format!("no project with id {id}"),
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

/// `GET /projects` — filtered, paginated list.
///
/// * Defaults to `limit = 50`, capped at `MAX_LIST_LIMIT = 200`.
/// * Pass `?count_only=1` for an empty-`rows` count-only envelope
///   (§6.1 sidebar).
/// * Order: status (active → backlog → done → archived) then
///   `due_at ASC NULLS LAST` then `name`.
#[utoipa::path(
    get,
    path = "/projects",
    params(
        ("org_id"     = Option<Uuid>,             Query, description = "Restrict to one org."),
        ("status"     = Option<ProjectStatusDto>, Query, description = "Restrict to one status."),
        ("q"          = Option<String>,           Query, description = "Case-insensitive substring on name."),
        ("limit"      = Option<i64>,              Query, description = "Page size (1..=200, default 50)."),
        ("offset"     = Option<i64>,              Query, description = "Page offset (default 0)."),
        ("count_only" = Option<u8>,               Query, description = "If 1, returns count only with empty rows."),
    ),
    responses(
        (status = 200, description = "Paginated project list", body = ProjectListResponse),
    ),
    tag = "projects",
)]
pub async fn list_projects(
    State(state): State<AppState>,
    Query(q): Query<ListProjectsQuery>,
) -> Result<Json<ProjectListResponse>, ApiError> {
    let (limit, offset) = resolve_pagination(q.limit, q.offset);
    let count_only = matches!(q.count_only, Some(n) if n != 0);
    let filter = ProjectListFilter {
        org_id: q.org_id,
        status: q.status.map(Into::into),
        q: q.q.clone(),
        limit,
        offset,
    };
    let total = state.store.count_projects(&filter).await?;
    if count_only {
        return Ok(Json(ProjectListResponse {
            rows: Vec::new(),
            total,
            limit: 0,
            offset,
        }));
    }
    let rows = state.store.list_projects(&filter).await?;
    Ok(Json(ProjectListResponse {
        rows: rows.into_iter().map(ProjectDto::from).collect(),
        total,
        limit,
        offset,
    }))
}

/// `GET /projects/{id}` — single project, or `404` when absent.
#[utoipa::path(
    get,
    path = "/projects/{id}",
    params(("id" = Uuid, Path, description = "Project id")),
    responses(
        (status = 200, description = "Project row",        body = ProjectDto),
        (status = 404, description = "No such project"),
    ),
    tag = "projects",
)]
pub async fn get_project(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ProjectDto>, ApiError> {
    let project = state
        .store
        .get_project(id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "project_not_found",
            message: format!("no project with id {id}"),
        })?;
    Ok(Json(project.into()))
}

/// `POST /projects` — create. Initialises `version = 1`,
/// `issue_count = closed_issue_count = 0`, stamps `created_by`
/// from the [`Principal`]. Audits [`audit::PROJECT_CREATE`].
#[utoipa::path(
    post,
    path = "/projects",
    request_body = CreateProjectRequest,
    responses(
        (status = 200, description = "Project created", body = ProjectDto),
        (status = 400, description = "Validation failure (name / dates)"),
        (status = 409, description = "Duplicate active project name in this org"),
    ),
    tag = "projects",
)]
pub async fn create_project(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<CreateProjectRequest>,
) -> Result<Json<ProjectDto>, ApiError> {
    validate_name(&body.name)?;
    validate_dates(body.start_at, body.due_at)?;
    let upsert = ProjectUpsert {
        org_id: body.org_id,
        name: body.name.trim().to_string(),
        description: body.description.map(|s| s.trim().to_string()),
        lead_user_id: body.lead_user_id,
        status: body.status.map(Into::into).unwrap_or(ProjectStatus::Active),
        start_at: body.start_at,
        due_at: body.due_at,
        created_by: Some(principal.actor_user_id),
    };
    let project = match state.store.create_project(&upsert).await {
        Ok(p) => p,
        Err(StoreError::Conflict(msg)) => {
            return Err(ApiError::Conflict {
                code: "project_name_taken",
                message: format!(
                    "an active project with this name already exists in the org: {msg}"
                ),
            });
        }
        Err(StoreError::Invalid(msg)) => {
            return Err(ApiError::BadRequest {
                code: "project_invalid",
                message: msg,
            });
        }
        Err(e) => return Err(e.into()),
    };
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_CREATE,
        project.id.to_string(),
    )
    .await?;
    Ok(Json(project.into()))
}

/// `PATCH /projects/{id}` — partial update under §8.2 CAS.
///
/// * `expected_version` mismatch ⇒ `409 stale_project_version`.
/// * Unknown id ⇒ `404 project_not_found`.
/// * Per §9.2, `lead_user_id` mutation is an elevated op; v1
///   handler does not yet enforce the elevation (a follow-up
///   stage wires the lead-or-author check). The lane is reserved
///   so adding the gate is a one-line edit.
/// * Audits [`audit::PROJECT_UPDATE`] after the row commits.
#[utoipa::path(
    patch,
    path = "/projects/{id}",
    params(("id" = Uuid, Path, description = "Project id")),
    request_body = PatchProjectRequest,
    responses(
        (status = 200, description = "Project updated",   body = ProjectDto),
        (status = 400, description = "Validation failure"),
        (status = 404, description = "No such project"),
        (status = 409, description = "Stale `expected_version`"),
    ),
    tag = "projects",
)]
pub async fn patch_project(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchProjectRequest>,
) -> Result<Json<ProjectDto>, ApiError> {
    let current = state
        .store
        .get_project(id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "project_not_found",
            message: format!("no project with id {id}"),
        })?;
    // Overlay the partial body onto the current row to build a
    // full upsert. Triple-Option (`Option<Option<T>>`) lets the
    // caller distinguish "leave alone" (`None`) from "clear"
    // (`Some(None)`) for the nullable fields.
    let merged_name = body.name.unwrap_or_else(|| current.name.clone());
    validate_name(&merged_name)?;
    let merged_description = match body.description {
        None => current.description.clone(),
        Some(v) => v.map(|s| s.trim().to_string()),
    };
    let merged_lead = match body.lead_user_id {
        None => current.lead_user_id,
        Some(v) => v,
    };
    let merged_status = body
        .status
        .map(Into::into)
        .unwrap_or(current.status);
    let merged_start = match body.start_at {
        None => current.start_at,
        Some(v) => v,
    };
    let merged_due = match body.due_at {
        None => current.due_at,
        Some(v) => v,
    };
    validate_dates(merged_start, merged_due)?;
    let upsert = ProjectUpsert {
        org_id: current.org_id,
        name: merged_name.trim().to_string(),
        description: merged_description,
        lead_user_id: merged_lead,
        status: merged_status,
        start_at: merged_start,
        due_at: merged_due,
        // `created_by` is immutable per §9.2 — the store ignores
        // this field on update, but echo the current value so a
        // debug-print of the upsert is honest.
        created_by: current.created_by,
    };
    let updated = state
        .store
        .update_project(id, body.expected_version, &upsert)
        .await
        .map_err(|e| map_store_cas_error(id, e))?;
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_UPDATE,
        id.to_string(),
    )
    .await?;
    Ok(Json(updated.into()))
}

/// `POST /projects/{id}/archive` — elevated archive op (§9.2).
/// Idempotent: archiving an already-archived row echoes it back
/// without bumping `version` or writing an audit row.
#[utoipa::path(
    post,
    path = "/projects/{id}/archive",
    params(("id" = Uuid, Path, description = "Project id")),
    request_body = ArchiveProjectRequest,
    responses(
        (status = 200, description = "Archived (or already-archived no-op)", body = ProjectDto),
        (status = 404, description = "No such project"),
        (status = 409, description = "Stale `expected_version`"),
    ),
    tag = "projects",
)]
pub async fn archive_project(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(body): Json<ArchiveProjectRequest>,
) -> Result<Json<ProjectDto>, ApiError> {
    // Decide up-front whether the call is a no-op so we can skip
    // the audit write (idempotent re-archive should not bloat the
    // audit log, matching the §9.2 wording).
    let pre = state
        .store
        .get_project(id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "project_not_found",
            message: format!("no project with id {id}"),
        })?;
    let already_archived = pre.status == ProjectStatus::Archived;
    let archived = state
        .store
        .archive_project(id, body.expected_version)
        .await
        .map_err(|e| map_store_cas_error(id, e))?;
    if !already_archived {
        audit::record(
            state.store.as_ref(),
            principal.actor_user_id,
            audit::PROJECT_ARCHIVE,
            id.to_string(),
        )
        .await?;
    }
    Ok(Json(archived.into()))
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the projects router fragment. `dp-server::build` mounts
/// this via `Router::merge`. The `(projects, read|write)` resource
/// pair is registered in `dp_server::auth::policy::register_dev_pulse_resources`.
pub fn projects_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new()
                .route("/projects", get(list_projects))
                .route("/projects/{id}", get(get_project)),
            "projects",
            "read",
        ))
        .merge(with_permission(
            Router::new()
                .route("/projects", post(create_project))
                .route("/projects/{id}", patch(patch_project))
                .route("/projects/{id}/archive", post(archive_project)),
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
    use axum::http::{Request, StatusCode};
    use std::sync::Mutex;
    use tower::ServiceExt;

    use dp_domain::audit::AuditEntry;
    use dp_domain::store::{EventActorRow, Store, StoreError};
    use dp_domain::{
        ActivityEvent, ActorRole, EventActor, FetchCursor, FetchRun, FetchRunKind, Membership,
        Org, Repo, ResourceKind, Team, User, WebhookDelivery, Window,
    };

    // -----------------------------------------------------------------
    // In-memory store fake — minimal surface to drive the projects routes
    // -----------------------------------------------------------------

    #[derive(Default)]
    struct MemStore {
        projects: Mutex<Vec<Project>>,
        audit: Mutex<Vec<AuditEntry>>,
        names_taken: Mutex<Vec<(Uuid, String)>>,
    }

    impl MemStore {
        fn audit_rows(&self) -> Vec<AuditEntry> {
            self.audit.lock().unwrap().clone()
        }
        fn projects(&self) -> Vec<Project> {
            self.projects.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl Store for MemStore {
        async fn list_projects(
            &self,
            filter: &ProjectListFilter,
        ) -> Result<Vec<Project>, StoreError> {
            let rows: Vec<Project> = self
                .projects
                .lock()
                .unwrap()
                .iter()
                .filter(|p| filter.org_id.map_or(true, |o| p.org_id == o))
                .filter(|p| filter.status.map_or(true, |s| p.status == s))
                .filter(|p| {
                    filter
                        .q
                        .as_deref()
                        .map(|q| p.name.to_lowercase().contains(&q.to_lowercase()))
                        .unwrap_or(true)
                })
                .cloned()
                .collect();
            let start = filter.offset.max(0) as usize;
            let end = (start + filter.limit.max(0) as usize).min(rows.len());
            Ok(rows[start.min(rows.len())..end].to_vec())
        }
        async fn count_projects(
            &self,
            filter: &ProjectListFilter,
        ) -> Result<i64, StoreError> {
            Ok(self
                .projects
                .lock()
                .unwrap()
                .iter()
                .filter(|p| filter.org_id.map_or(true, |o| p.org_id == o))
                .filter(|p| filter.status.map_or(true, |s| p.status == s))
                .filter(|p| {
                    filter
                        .q
                        .as_deref()
                        .map(|q| p.name.to_lowercase().contains(&q.to_lowercase()))
                        .unwrap_or(true)
                })
                .count() as i64)
        }
        async fn get_project(&self, id: Uuid) -> Result<Option<Project>, StoreError> {
            Ok(self
                .projects
                .lock()
                .unwrap()
                .iter()
                .find(|p| p.id == id)
                .cloned())
        }
        async fn create_project(
            &self,
            upsert: &ProjectUpsert,
        ) -> Result<Project, StoreError> {
            let mut taken = self.names_taken.lock().unwrap();
            let key = (upsert.org_id, upsert.name.to_lowercase());
            if taken.iter().any(|x| x == &key) {
                return Err(StoreError::Conflict("project name taken".into()));
            }
            taken.push(key);
            let p = Project {
                id: Uuid::new_v4(),
                org_id: upsert.org_id,
                name: upsert.name.clone(),
                description: upsert.description.clone(),
                lead_user_id: upsert.lead_user_id,
                status: upsert.status,
                start_at: upsert.start_at,
                due_at: upsert.due_at,
                issue_count: 0,
                closed_issue_count: 0,
                created_by: upsert.created_by,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                version: 1,
                primary_milestone_id: None,
            };
            self.projects.lock().unwrap().push(p.clone());
            Ok(p)
        }
        async fn update_project(
            &self,
            id: Uuid,
            expected_version: i64,
            upsert: &ProjectUpsert,
        ) -> Result<Project, StoreError> {
            let mut rows = self.projects.lock().unwrap();
            let row = rows
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or_else(|| StoreError::NotFound {
                    entity: "project",
                    id: id.to_string(),
                })?;
            if row.version != expected_version {
                return Err(StoreError::Conflict(format!(
                    "project version mismatch: expected {expected_version}, found {}",
                    row.version
                )));
            }
            row.name = upsert.name.clone();
            row.description = upsert.description.clone();
            row.lead_user_id = upsert.lead_user_id;
            row.status = upsert.status;
            row.start_at = upsert.start_at;
            row.due_at = upsert.due_at;
            row.version += 1;
            row.updated_at = Utc::now();
            Ok(row.clone())
        }
        async fn archive_project(
            &self,
            id: Uuid,
            expected_version: i64,
        ) -> Result<Project, StoreError> {
            let mut rows = self.projects.lock().unwrap();
            let row = rows
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or_else(|| StoreError::NotFound {
                    entity: "project",
                    id: id.to_string(),
                })?;
            if row.status == ProjectStatus::Archived {
                return Ok(row.clone());
            }
            if row.version != expected_version {
                return Err(StoreError::Conflict(format!(
                    "project version mismatch: expected {expected_version}, found {}",
                    row.version
                )));
            }
            row.status = ProjectStatus::Archived;
            row.version += 1;
            row.updated_at = Utc::now();
            Ok(row.clone())
        }
        async fn record_audit_log(&self, entry: &AuditEntry) -> Result<(), StoreError> {
            self.audit.lock().unwrap().push(entry.clone());
            Ok(())
        }

        // --- minimal stubs for the rest of the Store surface ----------
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
    // Test harness
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
        projects_router(app_state)
            .layer(Extension(Principal { actor_user_id: actor }))
            .layer(Extension(spi_principal))
            .layer(Extension(engine))
    }

    async fn json_of(resp: axum::response::Response) -> serde_json::Value {
        let body = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    fn seed_project(store: &MemStore, name: &str, org: Uuid, status: ProjectStatus) -> Uuid {
        let p = Project {
            id: Uuid::new_v4(),
            org_id: org,
            name: name.into(),
            description: None,
            lead_user_id: None,
            status,
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
        let id = p.id;
        store.projects.lock().unwrap().push(p);
        store
            .names_taken
            .lock()
            .unwrap()
            .push((org, name.to_lowercase()));
        id
    }

    // -----------------------------------------------------------------
    // POST /projects
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn create_project_persists_row_and_audits() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let app = build_app(store.clone(), actor);
        let body = serde_json::json!({
            "org_id": Uuid::new_v4(),
            "name":   "Rubix v2 launch",
            "description": "Q3 rollout",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/projects")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert_eq!(v["name"], "Rubix v2 launch");
        assert_eq!(v["status"], "active", "status defaults to active");
        assert_eq!(v["version"], 1);
        assert_eq!(v["issue_count"], 0);
        assert_eq!(v["board_link_count"], 0);
        assert_eq!(v["created_by"], serde_json::json!(actor));
        let rows = store.audit_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].action, audit::PROJECT_CREATE);
        assert_eq!(rows[0].actor_user_id, actor);
    }

    #[tokio::test]
    async fn create_project_rejects_blank_name_with_400() {
        let store = Arc::new(MemStore::default());
        let app = build_app(store.clone(), Uuid::new_v4());
        let body = serde_json::json!({
            "org_id": Uuid::new_v4(),
            "name":   "   ",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/projects")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "project_name_required");
        assert!(store.audit_rows().is_empty());
    }

    #[tokio::test]
    async fn create_project_rejects_inverted_dates_with_400() {
        let store = Arc::new(MemStore::default());
        let app = build_app(store.clone(), Uuid::new_v4());
        let body = serde_json::json!({
            "org_id":   Uuid::new_v4(),
            "name":     "Bad dates",
            "start_at": "2026-06-15T00:00:00Z",
            "due_at":   "2026-05-01T00:00:00Z",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/projects")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "project_dates_inverted");
    }

    #[tokio::test]
    async fn create_project_rejects_duplicate_with_409() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        seed_project(&store, "dup", org, ProjectStatus::Active);
        let app = build_app(store.clone(), Uuid::new_v4());
        let body = serde_json::json!({ "org_id": org, "name": "dup" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/projects")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "project_name_taken");
    }

    // -----------------------------------------------------------------
    // GET /projects + /projects/{id}
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn list_projects_filters_and_paginates() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        seed_project(&store, "alpha", org, ProjectStatus::Active);
        seed_project(&store, "beta", org, ProjectStatus::Backlog);
        seed_project(&store, "alpha-two", org, ProjectStatus::Active);
        seed_project(&store, "noise", Uuid::new_v4(), ProjectStatus::Active);
        let app = build_app(store.clone(), Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/projects?org_id={org}&status=active&q=alpha"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert_eq!(v["total"], 2);
        let names: Vec<&str> = v["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"alpha-two"));
    }

    #[tokio::test]
    async fn list_projects_count_only_returns_total_and_empty_rows() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        for i in 0..3 {
            seed_project(&store, &format!("p{i}"), org, ProjectStatus::Active);
        }
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/projects?org_id={org}&status=active&count_only=1"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let v = json_of(resp).await;
        assert_eq!(v["total"], 3);
        assert_eq!(v["limit"], 0);
        assert!(v["rows"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn get_project_returns_404_when_absent() {
        let store = Arc::new(MemStore::default());
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}", Uuid::new_v4()))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "project_not_found");
    }

    // -----------------------------------------------------------------
    // PATCH /projects/{id}
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn patch_project_merges_partial_body_and_bumps_version() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let org = Uuid::new_v4();
        let id = seed_project(&store, "name-1", org, ProjectStatus::Active);
        let app = build_app(store.clone(), actor);
        let body = serde_json::json!({
            "expected_version": 1,
            "name": "name-2",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/projects/{id}"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert_eq!(v["name"], "name-2");
        assert_eq!(v["status"], "active", "status preserved");
        assert_eq!(v["version"], 2);
        let rows = store.audit_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].action, audit::PROJECT_UPDATE);
    }

    #[tokio::test]
    async fn patch_project_rejects_stale_version_with_409() {
        let store = Arc::new(MemStore::default());
        let id = seed_project(&store, "n", Uuid::new_v4(), ProjectStatus::Active);
        let app = build_app(store.clone(), Uuid::new_v4());
        let body = serde_json::json!({
            "expected_version": 99,
            "status": "done",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/projects/{id}"))
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
    async fn patch_project_returns_404_when_missing() {
        let store = Arc::new(MemStore::default());
        let app = build_app(store, Uuid::new_v4());
        let body = serde_json::json!({ "expected_version": 1, "name": "x" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/projects/{}", Uuid::new_v4()))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // -----------------------------------------------------------------
    // POST /projects/{id}/archive
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn archive_project_flips_status_and_audits_once() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let id = seed_project(&store, "p", Uuid::new_v4(), ProjectStatus::Active);
        let app = build_app(store.clone(), actor);
        let body = serde_json::json!({ "expected_version": 1 });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/projects/{id}/archive"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert_eq!(v["status"], "archived");
        assert_eq!(v["version"], 2);
        // Second call should be idempotent — no version bump, no
        // new audit row.
        let body2 = serde_json::json!({ "expected_version": 2 });
        let resp2 = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/projects/{id}/archive"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body2.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);
        let v2 = json_of(resp2).await;
        assert_eq!(v2["status"], "archived");
        assert_eq!(v2["version"], 2, "no bump on re-archive");
        let rows = store.audit_rows();
        assert_eq!(rows.len(), 1, "only the first archive audits");
        assert_eq!(rows[0].action, audit::PROJECT_ARCHIVE);
    }

    #[tokio::test]
    async fn archive_project_returns_409_on_stale_version() {
        let store = Arc::new(MemStore::default());
        let id = seed_project(&store, "p", Uuid::new_v4(), ProjectStatus::Active);
        let app = build_app(store.clone(), Uuid::new_v4());
        let body = serde_json::json!({ "expected_version": 42 });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/projects/{id}/archive"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "stale_project_version");
        let rows = store.projects();
        assert_eq!(rows[0].status, ProjectStatus::Active);
    }
}
