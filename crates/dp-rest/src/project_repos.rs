//! Project ↔ repo association REST surface.
//!
//! Three routes ship here:
//!
//! | route                                  | what it does                                              |
//! |----------------------------------------|-----------------------------------------------------------|
//! | `GET    /projects/{id}/repos`          | list repos associated with a project                      |
//! | `PUT    /projects/{id}/repos/{repo_id}`| idempotently associate a repo with the project            |
//! | `DELETE /projects/{id}/repos/{repo_id}`| remove the association                                    |
//!
//! Soft scoping: associating a repo with a project does **not**
//! gate which issues the project can hold (the §7.2 bulk-add still
//! accepts issues from any repo). It exists so the §6.3 "Add
//! issues" dialog can narrow the issue picker to repos the operator
//! has explicitly associated with the project.
//!
//! Authorisation: `(projects, read)` for the GET and `(projects,
//! write)` for PUT / DELETE — same lanes as the §7.1 CRUD spine.

use std::sync::Arc;

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{delete, get, put},
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::project::ProjectRepo;

use crate::audit::{self, Principal};
use crate::error::ApiError;
use crate::state::AppState;

/// One row in [`list_project_repos`]'s response. Enriched with the
/// repo's `org_id` and `name` so the §6.3 UI does not have to fan
/// out a `GET /repos/{id}` per row.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectRepoDto {
    /// The project this row associates the repo with.
    pub project_id: Uuid,
    /// The repo id.
    pub repo_id: Uuid,
    /// Enriched repo org id (matches `dp_repos.org_id`).
    pub repo_org_id: Uuid,
    /// Enriched repo org login — used by the frontend to build
    /// `https://github.com/{org_login}/{repo_name}` links.
    pub repo_org_login: String,
    /// Enriched repo name (matches `dp_repos.name`).
    pub repo_name: String,
    /// User who created the association, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_by: Option<Uuid>,
    /// When the association was created.
    pub added_at: DateTime<Utc>,
}

async fn enrich(
    state: &AppState,
    rows: Vec<ProjectRepo>,
) -> Result<Vec<ProjectRepoDto>, ApiError> {
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let repo = state.store.get_repo(r.repo_id).await?;
        let (repo_org_id, repo_name) = match &repo {
            Some(r2) => (r2.org_id, r2.name.clone()),
            // Should be impossible thanks to FK ON DELETE CASCADE,
            // but degrade gracefully if it ever happens.
            None => (Uuid::nil(), String::new()),
        };
        let repo_org_login = match state.store.get_org(repo_org_id).await? {
            Some(org) => org.login,
            None => String::new(),
        };
        out.push(ProjectRepoDto {
            project_id: r.project_id,
            repo_id: r.repo_id,
            repo_org_id,
            repo_org_login,
            repo_name,
            added_by: r.added_by,
            added_at: r.added_at,
        });
    }
    Ok(out)
}

/// `GET /projects/{id}/repos` — list repos associated with the
/// project, in `added_at ASC` order.
#[utoipa::path(
    get,
    path = "/projects/{id}/repos",
    params(("id" = Uuid, Path, description = "Project id")),
    responses(
        (status = 200, description = "Repos associated with the project", body = Vec<ProjectRepoDto>),
        (status = 404, description = "No such project"),
    ),
    tag = "projects",
)]
pub async fn list_project_repos(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<ProjectRepoDto>>, ApiError> {
    state
        .store
        .get_project(project_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "project_not_found",
            message: format!("no project with id {project_id}"),
        })?;
    let rows = state.store.list_project_repos(project_id).await?;
    let dtos = enrich(&state, rows).await?;
    Ok(Json(dtos))
}

/// `PUT /projects/{id}/repos/{repo_id}` — idempotently associate a
/// repo with the project. Enforces that the repo's `org_id` matches
/// the project's `org_id` (v1: one org per project, §4).
#[utoipa::path(
    put,
    path = "/projects/{id}/repos/{repo_id}",
    params(
        ("id"      = Uuid, Path, description = "Project id"),
        ("repo_id" = Uuid, Path, description = "Repo id"),
    ),
    responses(
        (status = 200, description = "Association exists (newly created or pre-existing)", body = ProjectRepoDto),
        (status = 400, description = "Cross-org repo or invalid id"),
        (status = 404, description = "No such project or repo"),
    ),
    tag = "projects",
)]
pub async fn add_project_repo(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((project_id, repo_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ProjectRepoDto>, ApiError> {
    let project = state
        .store
        .get_project(project_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "project_not_found",
            message: format!("no project with id {project_id}"),
        })?;
    let repo = state
        .store
        .get_repo(repo_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "repo_not_found",
            message: format!("no repo with id {repo_id}"),
        })?;
    if repo.org_id != project.org_id {
        return Err(ApiError::BadRequest {
            code: "cross_org_repo",
            message:
                "repo must belong to the same org as the project".into(),
        });
    }
    let row = state
        .store
        .add_project_repo(project_id, repo_id, Some(principal.actor_user_id))
        .await?;
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_REPO_ADD,
        format!("{project_id}:{repo_id}"),
    )
    .await?;
    let dtos = enrich(&state, vec![row]).await?;
    Ok(Json(dtos.into_iter().next().expect("enrich preserves rows")))
}

/// `DELETE /projects/{id}/repos/{repo_id}` — remove the
/// association. Idempotent (a no-op delete returns 204).
#[utoipa::path(
    delete,
    path = "/projects/{id}/repos/{repo_id}",
    params(
        ("id"      = Uuid, Path, description = "Project id"),
        ("repo_id" = Uuid, Path, description = "Repo id"),
    ),
    responses(
        (status = 204, description = "Association removed (or already absent)"),
    ),
    tag = "projects",
)]
pub async fn remove_project_repo(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((project_id, repo_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    state
        .store
        .remove_project_repo(project_id, repo_id)
        .await?;
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_REPO_REMOVE,
        format!("{project_id}:{repo_id}"),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Build the project ↔ repo router fragment.
pub fn project_repos_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new()
                .route("/projects/{id}/repos", get(list_project_repos)),
            "projects",
            "read",
        ))
        .merge(with_permission(
            Router::new()
                .route("/projects/{id}/repos/{repo_id}", put(add_project_repo))
                .route(
                    "/projects/{id}/repos/{repo_id}",
                    delete(remove_project_repo),
                ),
            "projects",
            "write",
        ))
        .with_state(inner)
}
