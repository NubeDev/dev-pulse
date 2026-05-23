//! Project saved views CRUD + reorder (PROJECT-VIEW.md §7.1,
//! Slice 4). Backs the `<ViewsTabStrip>` above the workbench
//! toolbar.
//!
//! Routes (all gated on `(projects, read|write)` — same lanes as
//! the §7.1 project CRUD spine; v1 ships private-only views and
//! the store layer scopes every read by `owner_user_id =
//! principal`, so cross-user access is invisible at the SQL level):
//!
//! | route                                            | what it does                              |
//! |--------------------------------------------------|-------------------------------------------|
//! | `GET    /projects/{id}/views`                    | `Vec<ProjectViewDto>` for the caller       |
//! | `POST   /projects/{id}/views`                    | create a view, append at end of strip      |
//! | `GET    /projects/{id}/views/{view_id}`          | fetch one (owner-scoped)                   |
//! | `PATCH  /projects/{id}/views/{view_id}`          | rename, change `(group, filter, sort)`     |
//! | `DELETE /projects/{id}/views/{view_id}`          | 204; gaps in `position` are tolerated      |
//! | `POST   /projects/{id}/views/reorder`            | atomic rewrite of the caller's positions   |
//!
//! The `filter_clauses` wire shape mirrors
//! `dp_domain::project_view::ProjectViewFilterClause` — the
//! validator rejects clauses with unknown `dim`, missing required
//! keys, or invalid `tag` key grammar before the store ever sees
//! them. JSONB never carries shapes the trait can't decode.

use std::sync::Arc;

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, patch, post},
    Router,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::project_view::{
    ProjectView, ProjectViewFilterClause, ProjectViewUpsert, ProjectViewVisibility,
};
use dp_domain::store::StoreError;

use crate::audit::{self, Principal};
use crate::error::ApiError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Wire DTOs
// ---------------------------------------------------------------------------

/// One saved view on the wire. The serialised shape of
/// `filter_clauses` is the discriminated `#[serde(tag = "dim")]`
/// enum mirrored in `dp_domain::project_view`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectViewDto {
    /// Stable id.
    pub id: Uuid,
    /// Parent project.
    pub project_id: Uuid,
    /// Owner — v1 always the caller.
    pub owner_user_id: Uuid,
    /// Tab label.
    pub name: String,
    /// Group-by dimension or `null` for a flat view.
    pub group_by: Option<String>,
    /// Canonical filter clauses (`{"dim": …, …}` objects).
    pub filter_clauses: Vec<serde_json::Value>,
    /// Sort order. `updated_desc` | `updated_asc` | `title_asc`.
    pub sort: String,
    /// Per-owner position within the project's tab strip.
    pub position: i32,
    /// `"private"` (v1) or `"project"` (reserved).
    pub visibility: String,
    /// Optional start date for the view's timeline.
    /// Serialised as `YYYY-MM-DD` (or omitted/null when unset).
    pub start_date: Option<NaiveDate>,
    /// Optional due date for the view's timeline. Same shape as
    /// [`Self::start_date`].
    pub due_date: Option<NaiveDate>,
    /// Ordered category slugs rendered as collapsible sections
    /// inside this view. Lowercase, `[a-z0-9_-]{1,50}`, deduped,
    /// max 32. Empty array — flat view (whatever `group_by` says).
    #[serde(default)]
    pub categories: Vec<String>,
    /// First write timestamp.
    pub created_at: DateTime<Utc>,
    /// Most recent mutation.
    pub updated_at: DateTime<Utc>,
    /// Open issues currently visible inside this view (post-filter,
    /// post-membership). `None` on write responses where the count
    /// would be a wasted round-trip — only `GET /projects/{id}/views`
    /// populates it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_issue_count: Option<i32>,
    /// Total issues currently visible inside this view (open +
    /// closed). Same population rule as [`Self::open_issue_count`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_issue_count: Option<i32>,
}

impl From<ProjectView> for ProjectViewDto {
    fn from(v: ProjectView) -> Self {
        let filter_clauses = v
            .filter_clauses
            .into_iter()
            .map(|c| serde_json::to_value(c).expect("clause is always serialisable"))
            .collect();
        Self {
            id: v.id,
            project_id: v.project_id,
            owner_user_id: v.owner_user_id,
            name: v.name,
            group_by: v.group_by,
            filter_clauses,
            sort: v.sort,
            position: v.position,
            visibility: v.visibility.as_str().to_string(),
            start_date: v.start_date,
            due_date: v.due_date,
            categories: v.categories,
            created_at: v.created_at,
            updated_at: v.updated_at,
            open_issue_count: None,
            total_issue_count: None,
        }
    }
}

/// `POST /projects/{id}/views` body. Identical to PATCH apart from
/// being required (POST has no partial-update semantics).
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ProjectViewCreateBody {
    /// Tab label. 1..=60 chars after trim.
    pub name: String,
    /// Group-by dimension, or `null` for a flat view. `"status"`
    /// or `"tag:<key>"`.
    #[serde(default)]
    pub group_by: Option<String>,
    /// Canonical filter clauses; validated against
    /// [`ProjectViewFilterClause`] before insert.
    #[serde(default)]
    pub filter_clauses: Vec<serde_json::Value>,
    /// Sort order. Empty string is rejected — the client sends
    /// `"updated_desc"` to mean "default".
    #[serde(default = "default_sort")]
    pub sort: String,
    /// Optional start date for the view's timeline (AU
    /// `dd/mm/yyyy` in the picker, `YYYY-MM-DD` on the wire).
    #[serde(default)]
    pub start_date: Option<NaiveDate>,
    /// Optional due date for the view's timeline. Same shape as
    /// [`Self::start_date`].
    #[serde(default)]
    pub due_date: Option<NaiveDate>,
    /// Ordered category slugs (sections inside the view). Lowercase,
    /// `[a-z0-9_-]{1,50}`. Validated by `validate_categories`.
    #[serde(default)]
    pub categories: Vec<String>,
}

fn default_sort() -> String {
    "updated_desc".to_string()
}

/// `PATCH /projects/{id}/views/{view_id}` body. v1 mutates all
/// fields atomically — the workbench's `[Save changes]` button
/// sends the full set every time, so a real "partial" PATCH would
/// just be cargo-culting. Reorder lives on its own route so a
/// rename and a drag-reorder don't race on `position`.
pub type ProjectViewUpdateBody = ProjectViewCreateBody;

/// `POST /projects/{id}/views/reorder` body — the full ordered
/// id list for the caller's views on this project.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ProjectViewReorderBody {
    /// New order. Must equal the caller's existing view-id set.
    pub ordered_ids: Vec<Uuid>,
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Allowed sort tokens. Mirrors
/// `project_issues::parse_sort` — keep these in sync if a new
/// sort lands.
const ALLOWED_SORTS: &[&str] = &["updated_desc", "updated_asc", "title_asc"];

/// Parse a `tag:<key>` group_by spec, returning the key on success.
/// Reuses the grammar from `project_issues::parse_group_by` — kept
/// inline here so the modules don't bleed parsing helpers across
/// surface boundaries.
fn tag_key_from_group_by(spec: &str) -> Option<&str> {
    let key = spec.strip_prefix("tag:")?;
    if key.is_empty() || key.len() > 50 {
        return None;
    }
    let mut chars = key.chars();
    let first = chars.next()?;
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return None;
    }
    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return None;
    }
    Some(key)
}

fn validate_group_by(spec: Option<&str>) -> Result<(), ApiError> {
    let Some(s) = spec else {
        return Ok(());
    };
    if s == "status" {
        return Ok(());
    }
    if tag_key_from_group_by(s).is_some() {
        return Ok(());
    }
    Err(ApiError::BadRequest {
        code: "invalid_group_by",
        message: format!("unknown group_by `{s}`"),
    })
}

fn validate_sort(sort: &str) -> Result<(), ApiError> {
    if ALLOWED_SORTS.contains(&sort) {
        return Ok(());
    }
    Err(ApiError::BadRequest {
        code: "invalid_sort",
        message: format!("unknown sort `{sort}`"),
    })
}

fn validate_name(name: &str) -> Result<String, ApiError> {
    let trimmed = name.trim();
    let len = trimmed.chars().count();
    if !(1..=60).contains(&len) {
        return Err(ApiError::BadRequest {
            code: "invalid_view_name",
            message: "view name must be 1..=60 chars after trim".into(),
        });
    }
    Ok(trimmed.to_string())
}

/// Decode + validate the raw JSON clauses into the canonical enum.
/// On any unknown dim, missing required key, or invalid tag-key
/// grammar, returns a 400 with a stable `invalid_filter` code.
fn validate_filter_clauses(
    raw: &[serde_json::Value],
) -> Result<Vec<ProjectViewFilterClause>, ApiError> {
    let mut out = Vec::with_capacity(raw.len());
    for (idx, v) in raw.iter().enumerate() {
        // serde does the heavy lifting via the discriminator. We
        // additionally enforce the value-shape invariants per dim
        // because serde alone accepts e.g. `status:"banana"`.
        let parsed: ProjectViewFilterClause = serde_json::from_value(v.clone())
            .map_err(|e| ApiError::BadRequest {
                code: "invalid_filter",
                message: format!("clause #{idx}: {e}"),
            })?;
        match &parsed {
            ProjectViewFilterClause::Status { value } => {
                if value != "open" && value != "closed" {
                    return Err(ApiError::BadRequest {
                        code: "invalid_filter",
                        message: format!(
                            "clause #{idx}: status value must be `open` or `closed`"
                        ),
                    });
                }
            }
            ProjectViewFilterClause::Assignee { value }
            | ProjectViewFilterClause::Label { value } => {
                if value.trim().is_empty() {
                    return Err(ApiError::BadRequest {
                        code: "invalid_filter",
                        message: format!("clause #{idx}: value must be non-empty"),
                    });
                }
            }
            ProjectViewFilterClause::Tag { key, value } => {
                if tag_key_from_group_by(&format!("tag:{key}")).is_none() {
                    return Err(ApiError::BadRequest {
                        code: "invalid_filter",
                        message: format!("clause #{idx}: bad tag key `{key}`"),
                    });
                }
                if value.trim().is_empty() {
                    return Err(ApiError::BadRequest {
                        code: "invalid_filter",
                        message: format!("clause #{idx}: tag value must be non-empty"),
                    });
                }
            }
            ProjectViewFilterClause::Milestone { value } => {
                // Re-parse to canonical hyphenated lowercase so
                // round-trips through the store are stable; reject
                // anything that isn't a UUID.
                let parsed_uuid = Uuid::parse_str(value).map_err(|_| {
                    ApiError::BadRequest {
                        code: "invalid_filter",
                        message: format!(
                            "clause #{idx}: milestone value must be a UUID"
                        ),
                    }
                })?;
                out.push(ProjectViewFilterClause::Milestone {
                    value: parsed_uuid.to_string(),
                });
                continue;
            }
        }
        out.push(parsed);
    }
    Ok(out)
}

fn body_to_upsert(body: ProjectViewCreateBody) -> Result<ProjectViewUpsert, ApiError> {
    let name = validate_name(&body.name)?;
    validate_group_by(body.group_by.as_deref())?;
    validate_sort(&body.sort)?;
    let filter_clauses = validate_filter_clauses(&body.filter_clauses)?;
    let categories = validate_categories(&body.categories)?;
    Ok(ProjectViewUpsert {
        name,
        group_by: body.group_by,
        filter_clauses,
        sort: body.sort,
        // v1 — private only. Reserved enum slot makes the future
        // shared-view slice a body field change, not a migration.
        visibility: ProjectViewVisibility::Private,
        start_date: body.start_date,
        due_date: body.due_date,
        categories,
    })
}

/// Validate the `categories` array. Returns the normalised list
/// (trimmed, deduped, lower-cased). Slug grammar mirrors
/// `tagging.md` §3: `[a-z0-9_-]{1,50}`. Rejects > 32 entries
/// because the workbench can't usefully render more sections.
fn validate_categories(input: &[String]) -> Result<Vec<String>, ApiError> {
    const MAX_CATEGORIES: usize = 32;
    if input.len() > MAX_CATEGORIES {
        return Err(ApiError::BadRequest {
            code: "invalid_categories",
            message: format!(
                "too many categories: {} (max {MAX_CATEGORIES})",
                input.len()
            ),
        });
    }
    let mut out: Vec<String> = Vec::with_capacity(input.len());
    for (idx, raw) in input.iter().enumerate() {
        let slug = raw.trim();
        if slug.is_empty() {
            return Err(ApiError::BadRequest {
                code: "invalid_categories",
                message: format!("category #{idx} is empty"),
            });
        }
        if slug.len() > 50 {
            return Err(ApiError::BadRequest {
                code: "invalid_categories",
                message: format!(
                    "category #{idx} `{slug}` exceeds 50 chars"
                ),
            });
        }
        if !slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
        {
            return Err(ApiError::BadRequest {
                code: "invalid_categories",
                message: format!(
                    "category #{idx} `{slug}` must match [a-z0-9_-]"
                ),
            });
        }
        if out.iter().any(|s| s == slug) {
            return Err(ApiError::BadRequest {
                code: "invalid_categories",
                message: format!("category #{idx} `{slug}` is duplicated"),
            });
        }
        out.push(slug.to_string());
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn ensure_project(state: &AppState, project_id: Uuid) -> Result<(), ApiError> {
    if state.store.get_project(project_id).await?.is_none() {
        return Err(ApiError::NotFound {
            code: "project_not_found",
            message: format!("no project with id {project_id}"),
        });
    }
    Ok(())
}

/// `GET /projects/{id}/views` — caller's saved views in
/// `position ASC` order.
#[utoipa::path(
    get,
    path = "/projects/{id}/views",
    params(("id" = Uuid, Path, description = "Project id")),
    responses(
        (status = 200, description = "Caller's views, position ASC", body = [ProjectViewDto]),
        (status = 404, description = "No such project"),
    ),
    tag = "projects",
)]
pub async fn list_project_views(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Extension(principal): Extension<Principal>,
) -> Result<Json<Vec<ProjectViewDto>>, ApiError> {
    ensure_project(&state, project_id).await?;
    let rows = state
        .store
        .list_project_views(project_id, principal.actor_user_id)
        .await?;

    // PROJECT-VIEW.md §5.4 amendment — tab counts must match what
    // `GET /projects/{id}/issues?view=<id>` renders: membership comes
    // from `dp_project_view_issues`, then the view's stored filter
    // clauses are applied in-memory, then we split open/closed. This
    // mirrors the [`crate::project_issues::list_project_issues`] flow
    // so the badge can't disagree with the list it labels.
    let mut out: Vec<ProjectViewDto> = Vec::with_capacity(rows.len());
    for view in rows {
        let ids = state.store.list_issue_ids_for_view(view.id).await?;
        let mut issues: Vec<dp_domain::issue::Issue> = Vec::with_capacity(ids.len());
        for id in &ids {
            if let Some(i) = state.store.get_issue(*id).await? {
                issues.push(i);
            }
        }
        let clauses: Vec<crate::project_issues::FilterClause> = view
            .filter_clauses
            .iter()
            .filter_map(crate::project_issues::view_clause_to_filter)
            .collect();
        crate::project_issues::apply_filter_clauses(
            &*state.store,
            project_id,
            &clauses,
            &mut issues,
        )
        .await?;
        let total = issues.len() as i32;
        let open = issues
            .iter()
            .filter(|i| i.state == dp_domain::issue::IssueState::Open)
            .count() as i32;
        let mut dto = ProjectViewDto::from(view);
        dto.open_issue_count = Some(open);
        dto.total_issue_count = Some(total);
        out.push(dto);
    }
    Ok(Json(out))
}

/// `POST /projects/{id}/views` — create + append.
#[utoipa::path(
    post,
    path = "/projects/{id}/views",
    params(("id" = Uuid, Path, description = "Project id")),
    request_body = ProjectViewCreateBody,
    responses(
        (status = 201, description = "The created view", body = ProjectViewDto),
        (status = 400, description = "Validation failure (name, group_by, filter, sort)"),
        (status = 404, description = "No such project"),
        (status = 409, description = "Duplicate name"),
    ),
    tag = "projects",
)]
pub async fn create_project_view(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<ProjectViewCreateBody>,
) -> Result<(StatusCode, Json<ProjectViewDto>), ApiError> {
    ensure_project(&state, project_id).await?;
    let upsert = body_to_upsert(body)?;
    let view = state
        .store
        .create_project_view(project_id, principal.actor_user_id, &upsert)
        .await
        .map_err(|e| match e {
            StoreError::Conflict(_) => ApiError::Conflict {
                code: "view_name_taken",
                message: "a view with that name already exists".into(),
            },
            other => other.into(),
        })?;
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_VIEW_CREATE,
        view.id.to_string(),
    )
    .await
    .ok();
    Ok((StatusCode::CREATED, Json(view.into())))
}

/// `GET /projects/{id}/views/{view_id}`.
#[utoipa::path(
    get,
    path = "/projects/{id}/views/{view_id}",
    params(
        ("id" = Uuid, Path, description = "Project id"),
        ("view_id" = Uuid, Path, description = "View id"),
    ),
    responses(
        (status = 200, description = "The view", body = ProjectViewDto),
        (status = 404, description = "No such project or view"),
    ),
    tag = "projects",
)]
pub async fn get_project_view(
    State(state): State<AppState>,
    Path((project_id, view_id)): Path<(Uuid, Uuid)>,
    Extension(principal): Extension<Principal>,
) -> Result<Json<ProjectViewDto>, ApiError> {
    ensure_project(&state, project_id).await?;
    let view = state
        .store
        .get_project_view(view_id, principal.actor_user_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "view_not_found",
            message: format!("no view with id {view_id}"),
        })?;
    if view.project_id != project_id {
        // Mismatch between the path id and the view's parent —
        // surface as 404 rather than leak the parent's identity.
        return Err(ApiError::NotFound {
            code: "view_not_found",
            message: format!("no view with id {view_id}"),
        });
    }
    Ok(Json(view.into()))
}

/// `PATCH /projects/{id}/views/{view_id}`.
#[utoipa::path(
    patch,
    path = "/projects/{id}/views/{view_id}",
    params(
        ("id" = Uuid, Path, description = "Project id"),
        ("view_id" = Uuid, Path, description = "View id"),
    ),
    request_body = ProjectViewCreateBody,
    responses(
        (status = 200, description = "The updated view", body = ProjectViewDto),
        (status = 400, description = "Validation failure"),
        (status = 404, description = "No such project or view"),
        (status = 409, description = "Duplicate name"),
    ),
    tag = "projects",
)]
pub async fn update_project_view(
    State(state): State<AppState>,
    Path((project_id, view_id)): Path<(Uuid, Uuid)>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<ProjectViewUpdateBody>,
) -> Result<Json<ProjectViewDto>, ApiError> {
    ensure_project(&state, project_id).await?;
    // Force the view to actually belong to this project before
    // we let the update through — the store's by-id+owner key
    // doesn't enforce the path-project association.
    let existing = state
        .store
        .get_project_view(view_id, principal.actor_user_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "view_not_found",
            message: format!("no view with id {view_id}"),
        })?;
    if existing.project_id != project_id {
        return Err(ApiError::NotFound {
            code: "view_not_found",
            message: format!("no view with id {view_id}"),
        });
    }
    let upsert = body_to_upsert(body)?;
    let view = state
        .store
        .update_project_view(view_id, principal.actor_user_id, &upsert)
        .await
        .map_err(|e| match e {
            StoreError::Conflict(_) => ApiError::Conflict {
                code: "view_name_taken",
                message: "a view with that name already exists".into(),
            },
            other => other.into(),
        })?;
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_VIEW_UPDATE,
        view.id.to_string(),
    )
    .await
    .ok();
    Ok(Json(view.into()))
}

/// `DELETE /projects/{id}/views/{view_id}` — 204 on success.
#[utoipa::path(
    delete,
    path = "/projects/{id}/views/{view_id}",
    params(
        ("id" = Uuid, Path, description = "Project id"),
        ("view_id" = Uuid, Path, description = "View id"),
    ),
    responses(
        (status = 204, description = "View removed"),
        (status = 404, description = "No such project or view"),
    ),
    tag = "projects",
)]
pub async fn delete_project_view(
    State(state): State<AppState>,
    Path((project_id, view_id)): Path<(Uuid, Uuid)>,
    Extension(principal): Extension<Principal>,
) -> Result<Response, ApiError> {
    ensure_project(&state, project_id).await?;
    let existing = state
        .store
        .get_project_view(view_id, principal.actor_user_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "view_not_found",
            message: format!("no view with id {view_id}"),
        })?;
    if existing.project_id != project_id {
        return Err(ApiError::NotFound {
            code: "view_not_found",
            message: format!("no view with id {view_id}"),
        });
    }
    state
        .store
        .delete_project_view(view_id, principal.actor_user_id)
        .await?;
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_VIEW_DELETE,
        view_id.to_string(),
    )
    .await
    .ok();
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `POST /projects/{id}/views/reorder` — atomic position rewrite.
#[utoipa::path(
    post,
    path = "/projects/{id}/views/reorder",
    params(("id" = Uuid, Path, description = "Project id")),
    request_body = ProjectViewReorderBody,
    responses(
        (status = 200, description = "Views in the new order", body = [ProjectViewDto]),
        (status = 400, description = "`ordered_ids` did not match the caller's view set"),
        (status = 404, description = "No such project"),
    ),
    tag = "projects",
)]
pub async fn reorder_project_views(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<ProjectViewReorderBody>,
) -> Result<Json<Vec<ProjectViewDto>>, ApiError> {
    ensure_project(&state, project_id).await?;
    let rows = state
        .store
        .reorder_project_views(project_id, principal.actor_user_id, &body.ordered_ids)
        .await
        .map_err(|e| match e {
            StoreError::Invalid(msg) => ApiError::BadRequest {
                code: "invalid_reorder",
                message: msg,
            },
            other => other.into(),
        })?;
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_VIEW_REORDER,
        project_id.to_string(),
    )
    .await
    .ok();
    Ok(Json(rows.into_iter().map(ProjectViewDto::from).collect()))
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the project saved views router fragment. Reads gated on
/// `(projects, read)`; writes on `(projects, write)`.
pub fn project_views_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new()
                .route("/projects/{id}/views", get(list_project_views))
                .route(
                    "/projects/{id}/views/{view_id}",
                    get(get_project_view),
                ),
            "projects",
            "read",
        ))
        .merge(with_permission(
            Router::new()
                .route("/projects/{id}/views", post(create_project_view))
                .route(
                    "/projects/{id}/views/{view_id}",
                    patch(update_project_view).delete(delete_project_view),
                )
                .route(
                    "/projects/{id}/views/reorder",
                    post(reorder_project_views),
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
    use std::sync::Mutex;

    use async_trait::async_trait;
    use axum::body::to_bytes;
    use axum::http::Request;
    use dp_domain::project::{Project, ProjectStatus};
    use dp_domain::store::{EventActorRow, Store};
    use dp_domain::{
        ActivityEvent, ActorRole, EventActor, FetchCursor, FetchRun, FetchRunKind, Membership,
        Org, Repo, ResourceKind, Team, User, WebhookDelivery, Window,
    };
    use serde_json::json;
    use tower::ServiceExt;

    // Minimal in-memory store — just the surface this module
    // exercises. Other trait methods fall through to the default
    // impls in `dp_domain::store::Store`.
    #[derive(Default)]
    struct MemStore {
        projects: Mutex<Vec<Project>>,
        views: Mutex<Vec<ProjectView>>,
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

        async fn record_audit_log(
            &self,
            _entry: &dp_domain::audit::AuditEntry,
        ) -> Result<(), StoreError> {
            Ok(())
        }

        async fn list_project_views(
            &self,
            project_id: Uuid,
            owner_user_id: Uuid,
        ) -> Result<Vec<ProjectView>, StoreError> {
            let mut rows: Vec<ProjectView> = self
                .views
                .lock()
                .unwrap()
                .iter()
                .filter(|v| v.project_id == project_id && v.owner_user_id == owner_user_id)
                .cloned()
                .collect();
            rows.sort_by_key(|v| (v.position, v.created_at));
            Ok(rows)
        }

        async fn get_project_view(
            &self,
            id: Uuid,
            owner_user_id: Uuid,
        ) -> Result<Option<ProjectView>, StoreError> {
            Ok(self
                .views
                .lock()
                .unwrap()
                .iter()
                .find(|v| v.id == id && v.owner_user_id == owner_user_id)
                .cloned())
        }

        async fn create_project_view(
            &self,
            project_id: Uuid,
            owner_user_id: Uuid,
            upsert: &ProjectViewUpsert,
        ) -> Result<ProjectView, StoreError> {
            let mut views = self.views.lock().unwrap();
            if views.iter().any(|v| {
                v.project_id == project_id
                    && v.owner_user_id == owner_user_id
                    && v.name == upsert.name
            }) {
                return Err(StoreError::Conflict("name taken".into()));
            }
            let position = views
                .iter()
                .filter(|v| v.project_id == project_id && v.owner_user_id == owner_user_id)
                .count() as i32;
            let now = Utc::now();
            let v = ProjectView {
                id: Uuid::new_v4(),
                project_id,
                owner_user_id,
                name: upsert.name.clone(),
                group_by: upsert.group_by.clone(),
                filter_clauses: upsert.filter_clauses.clone(),
                sort: if upsert.sort.is_empty() {
                    "updated_desc".into()
                } else {
                    upsert.sort.clone()
                },
                position,
                visibility: upsert.visibility,
                start_date: upsert.start_date,
                due_date: upsert.due_date,
                categories: upsert.categories.clone(),
                created_at: now,
                updated_at: now,
            };
            views.push(v.clone());
            Ok(v)
        }

        async fn update_project_view(
            &self,
            id: Uuid,
            owner_user_id: Uuid,
            upsert: &ProjectViewUpsert,
        ) -> Result<ProjectView, StoreError> {
            let mut views = self.views.lock().unwrap();
            if views.iter().any(|v| {
                v.id != id && v.owner_user_id == owner_user_id && v.name == upsert.name
            }) {
                return Err(StoreError::Conflict("name taken".into()));
            }
            let pos = views
                .iter()
                .position(|v| v.id == id && v.owner_user_id == owner_user_id)
                .ok_or_else(|| StoreError::NotFound {
                    entity: "project_view",
                    id: id.to_string(),
                })?;
            let v = &mut views[pos];
            v.name = upsert.name.clone();
            v.group_by = upsert.group_by.clone();
            v.filter_clauses = upsert.filter_clauses.clone();
            v.sort = upsert.sort.clone();
            v.visibility = upsert.visibility;
            v.start_date = upsert.start_date;
            v.due_date = upsert.due_date;
            v.categories = upsert.categories.clone();
            v.updated_at = Utc::now();
            Ok(v.clone())
        }

        async fn delete_project_view(
            &self,
            id: Uuid,
            owner_user_id: Uuid,
        ) -> Result<(), StoreError> {
            let mut views = self.views.lock().unwrap();
            let pos = views
                .iter()
                .position(|v| v.id == id && v.owner_user_id == owner_user_id)
                .ok_or_else(|| StoreError::NotFound {
                    entity: "project_view",
                    id: id.to_string(),
                })?;
            views.remove(pos);
            Ok(())
        }

        async fn reorder_project_views(
            &self,
            project_id: Uuid,
            owner_user_id: Uuid,
            ordered_ids: &[Uuid],
        ) -> Result<Vec<ProjectView>, StoreError> {
            let mut views = self.views.lock().unwrap();
            let existing: std::collections::HashSet<Uuid> = views
                .iter()
                .filter(|v| v.project_id == project_id && v.owner_user_id == owner_user_id)
                .map(|v| v.id)
                .collect();
            let req: std::collections::HashSet<Uuid> = ordered_ids.iter().copied().collect();
            if existing != req {
                return Err(StoreError::Invalid("set mismatch".into()));
            }
            for (idx, vid) in ordered_ids.iter().enumerate() {
                if let Some(v) = views
                    .iter_mut()
                    .find(|v| v.id == *vid && v.owner_user_id == owner_user_id)
                {
                    v.position = idx as i32;
                    v.updated_at = Utc::now();
                }
            }
            let mut out: Vec<ProjectView> = views
                .iter()
                .filter(|v| v.project_id == project_id && v.owner_user_id == owner_user_id)
                .cloned()
                .collect();
            out.sort_by_key(|v| v.position);
            Ok(out)
        }

        // --- minimal stubs for the rest of the Store surface --------
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

    // Harness ----------------------------------------------------------

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
            tenant_id: None,
            teams: Vec::new(),
            extra: serde_json::Value::Null,
        };
        project_views_router(app_state)
            .layer(Extension(Principal { actor_user_id: actor }))
            .layer(Extension(spi_principal))
            .layer(Extension(engine))
    }

    async fn json_of(resp: Response) -> serde_json::Value {
        let body = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        serde_json::from_slice(&body).unwrap()
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

    async fn post_json(
        app: &Router,
        uri: &str,
        body: serde_json::Value,
    ) -> Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn patch_json(
        app: &Router,
        uri: &str,
        body: serde_json::Value,
    ) -> Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    fn create_body(name: &str) -> serde_json::Value {
        json!({
            "name": name,
            "group_by": "status",
            "filter_clauses": [
                { "dim": "status", "value": "open" },
            ],
            "sort": "updated_desc",
        })
    }

    // Tests ------------------------------------------------------------

    #[tokio::test]
    async fn create_then_list_returns_position_zero() {
        let store = Arc::new(MemStore::default());
        let project = seed_project(&store);
        let app = build_app(store, Uuid::new_v4());
        let resp = post_json(&app, &format!("/projects/{}/views", project.id), create_body("Active"))
            .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let v = json_of(resp).await;
        assert_eq!(v["name"], "Active");
        assert_eq!(v["position"], 0);
        assert_eq!(v["visibility"], "private");
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}/views", project.id))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert_eq!(v.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn create_appends_positions_in_order() {
        let store = Arc::new(MemStore::default());
        let project = seed_project(&store);
        let app = build_app(store, Uuid::new_v4());
        for name in ["A", "B", "C"] {
            let r = post_json(&app, &format!("/projects/{}/views", project.id), create_body(name))
                .await;
            assert_eq!(r.status(), StatusCode::CREATED);
        }
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}/views", project.id))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let v = json_of(resp).await;
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["name"], "A");
        assert_eq!(arr[0]["position"], 0);
        assert_eq!(arr[2]["name"], "C");
        assert_eq!(arr[2]["position"], 2);
    }

    #[tokio::test]
    async fn duplicate_name_returns_409() {
        let store = Arc::new(MemStore::default());
        let project = seed_project(&store);
        let app = build_app(store, Uuid::new_v4());
        let r = post_json(&app, &format!("/projects/{}/views", project.id), create_body("Dup"))
            .await;
        assert_eq!(r.status(), StatusCode::CREATED);
        let r = post_json(&app, &format!("/projects/{}/views", project.id), create_body("Dup"))
            .await;
        assert_eq!(r.status(), StatusCode::CONFLICT);
        let v = json_of(r).await;
        assert_eq!(v["code"], "view_name_taken");
    }

    #[tokio::test]
    async fn invalid_group_by_returns_400() {
        let store = Arc::new(MemStore::default());
        let project = seed_project(&store);
        let app = build_app(store, Uuid::new_v4());
        let mut body = create_body("X");
        body["group_by"] = json!("mystery");
        let r = post_json(&app, &format!("/projects/{}/views", project.id), body).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        let v = json_of(r).await;
        assert_eq!(v["code"], "invalid_group_by");
    }

    #[tokio::test]
    async fn invalid_filter_dim_returns_400() {
        let store = Arc::new(MemStore::default());
        let project = seed_project(&store);
        let app = build_app(store, Uuid::new_v4());
        let body = json!({
            "name": "X",
            "filter_clauses": [{ "dim": "mystery", "value": "x" }],
            "sort": "updated_desc",
        });
        let r = post_json(&app, &format!("/projects/{}/views", project.id), body).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        let v = json_of(r).await;
        assert_eq!(v["code"], "invalid_filter");
    }

    #[tokio::test]
    async fn invalid_status_value_returns_400() {
        let store = Arc::new(MemStore::default());
        let project = seed_project(&store);
        let app = build_app(store, Uuid::new_v4());
        let body = json!({
            "name": "X",
            "filter_clauses": [{ "dim": "status", "value": "banana" }],
            "sort": "updated_desc",
        });
        let r = post_json(&app, &format!("/projects/{}/views", project.id), body).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        let v = json_of(r).await;
        assert_eq!(v["code"], "invalid_filter");
    }

    #[tokio::test]
    async fn patch_updates_fields() {
        let store = Arc::new(MemStore::default());
        let project = seed_project(&store);
        let app = build_app(store, Uuid::new_v4());
        let r = post_json(&app, &format!("/projects/{}/views", project.id), create_body("A")).await;
        let v = json_of(r).await;
        let view_id = v["id"].as_str().unwrap().to_string();
        let body = json!({
            "name": "A renamed",
            "group_by": null,
            "filter_clauses": [],
            "sort": "title_asc",
        });
        let r = patch_json(
            &app,
            &format!("/projects/{}/views/{}", project.id, view_id),
            body,
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        let v = json_of(r).await;
        assert_eq!(v["name"], "A renamed");
        assert!(v["group_by"].is_null());
        assert_eq!(v["sort"], "title_asc");
    }

    #[tokio::test]
    async fn delete_removes_view_and_returns_204() {
        let store = Arc::new(MemStore::default());
        let project = seed_project(&store);
        let app = build_app(store, Uuid::new_v4());
        let r = post_json(&app, &format!("/projects/{}/views", project.id), create_body("A")).await;
        let v = json_of(r).await;
        let view_id = v["id"].as_str().unwrap().to_string();
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/projects/{}/views/{}", project.id, view_id))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NO_CONTENT);
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}/views/{}", project.id, view_id))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn reorder_rewrites_positions() {
        let store = Arc::new(MemStore::default());
        let project = seed_project(&store);
        let app = build_app(store, Uuid::new_v4());
        let mut ids = Vec::new();
        for name in ["A", "B", "C"] {
            let r = post_json(&app, &format!("/projects/{}/views", project.id), create_body(name))
                .await;
            let v = json_of(r).await;
            ids.push(v["id"].as_str().unwrap().to_string());
        }
        // C, A, B
        let body = json!({ "ordered_ids": [ids[2], ids[0], ids[1]] });
        let r = post_json(
            &app,
            &format!("/projects/{}/views/reorder", project.id),
            body,
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        let v = json_of(r).await;
        let arr = v.as_array().unwrap();
        assert_eq!(arr[0]["name"], "C");
        assert_eq!(arr[0]["position"], 0);
        assert_eq!(arr[1]["name"], "A");
        assert_eq!(arr[2]["name"], "B");
    }

    #[tokio::test]
    async fn reorder_set_mismatch_returns_400() {
        let store = Arc::new(MemStore::default());
        let project = seed_project(&store);
        let app = build_app(store, Uuid::new_v4());
        let r = post_json(&app, &format!("/projects/{}/views", project.id), create_body("A")).await;
        let v = json_of(r).await;
        let real_id = v["id"].as_str().unwrap().to_string();
        let body = json!({
            "ordered_ids": [real_id, Uuid::new_v4().to_string()],
        });
        let r = post_json(
            &app,
            &format!("/projects/{}/views/reorder", project.id),
            body,
        )
        .await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        let v = json_of(r).await;
        assert_eq!(v["code"], "invalid_reorder");
    }

    #[tokio::test]
    async fn unknown_project_returns_404() {
        let store = Arc::new(MemStore::default());
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}/views", Uuid::new_v4()))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
