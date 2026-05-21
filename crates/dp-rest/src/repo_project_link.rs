//! `GET/PUT/DELETE /repos/{id}/project-link` — admin CRUD for the
//! §3.10 `dp_repo_project_link` row that wires a NubeIO repo to a
//! GitHub Projects v2 board.
//!
//! Linking a repo is a one-time operator action; the mirror task
//! is the same whether the link landed seconds or weeks ago. The
//! handlers here just persist the `(project_node_id,
//! start_field_node_id, due_field_node_id)` triple — they never
//! call GitHub themselves. The §3.10 `PATCH /issues/{id}/dates`
//! handler reads through `get_repo_project_link` on every save
//! and the [`crate::issue_dates::OctocrabProjectV2Mirror`] adapter
//! does the GraphQL work.
//!
//! A convenience `GET /repos/{id}/projects` reaches into the
//! fetcher's GraphQL surface so the admin pane can render a
//! project + field picker without forcing the operator to paste
//! raw node ids. The picker call is gated behind the same
//! `(issues, write)` pair as the link itself.
//!
//! Authorisation: every route is gated `(issues, write)` — same
//! pair the §3.10 PATCH handler uses, so the admin pane reuses
//! the existing write gate without a new permission lane.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    extract::{Path, State},
    response::Json,
    routing::{delete, get, put},
    Router,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::issue_dates::RepoProjectLink;

use crate::error::ApiError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Projects picker backend — the GraphQL seam for the admin UI
// ---------------------------------------------------------------------------

/// `repository(owner, name) { projectsV2(first: 50) }` projection
/// for the admin pane's project + field picker. Production
/// binaries wire an octocrab-backed implementation; tests pass a
/// fake or skip the route entirely.
///
/// Held behind a trait so the dp-rest layer doesn't import
/// dp-fetcher directly (boundary §0.6).
#[async_trait]
pub trait ProjectsPickerBackend: Send + Sync + 'static {
    /// List Projects v2 boards visible to the authenticated PAT /
    /// installation for `(owner, repo)`. Returns the verbatim
    /// GraphQL `projectsV2` shape (`{ nodes: [...] }`) so the
    /// frontend renders without an extra translation hop.
    async fn list_repo_projects(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<serde_json::Value, ProjectsPickerError>;
}

/// Errors a [`ProjectsPickerBackend`] may surface.
#[derive(Debug, thiserror::Error)]
pub enum ProjectsPickerError {
    /// GitHub GraphQL error (or 4xx-class transport error).
    #[error("github graphql: {0}")]
    GraphQl(String),
    /// 5xx / network transport.
    #[error("github transport: {0}")]
    Transport(String),
    /// Deployment hasn't wired a real backend — the picker route
    /// returns 503 in this case.
    #[error("projects picker not configured")]
    Unconfigured,
}

/// Default backend — refuses every call so deployments without a
/// real picker (test rigs, App-install mode that hasn't grown the
/// GraphQL surface yet) surface a 503 instead of an empty list.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnconfiguredProjectsPicker;

#[async_trait]
impl ProjectsPickerBackend for UnconfiguredProjectsPicker {
    async fn list_repo_projects(
        &self,
        _: &str,
        _: &str,
    ) -> Result<serde_json::Value, ProjectsPickerError> {
        Err(ProjectsPickerError::Unconfigured)
    }
}

/// Production [`ProjectsPickerBackend`] backed by the dp-fetcher
/// octocrab client (the same handle that powers
/// [`crate::issue_dates::OctocrabProjectV2Mirror`]). Thin shim:
/// forwards to [`dp_fetcher::client::Client::gh_list_repo_projectv2`]
/// and maps the [`dp_fetcher::client::GhWriteError`] split into
/// [`ProjectsPickerError`].
pub struct OctocrabProjectsPicker {
    client: Arc<dp_fetcher::client::Client>,
}

impl OctocrabProjectsPicker {
    /// Construct from a ready-to-use fetcher client.
    pub fn new(client: Arc<dp_fetcher::client::Client>) -> Self {
        Self { client }
    }
}

impl std::fmt::Debug for OctocrabProjectsPicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OctocrabProjectsPicker").finish_non_exhaustive()
    }
}

#[async_trait]
impl ProjectsPickerBackend for OctocrabProjectsPicker {
    async fn list_repo_projects(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<serde_json::Value, ProjectsPickerError> {
        use dp_fetcher::client::GhWriteError as G;
        self.client
            .gh_list_repo_projectv2(owner, repo)
            .await
            .map_err(|e| match e {
                G::Validation(m) => ProjectsPickerError::GraphQl(m),
                G::Upstream(m) => ProjectsPickerError::Transport(m),
            })
    }
}

// ---------------------------------------------------------------------------
// Wire DTOs
// ---------------------------------------------------------------------------

/// Wire shape for `dp_repo_project_link`. Mirrors
/// [`RepoProjectLink`] one-for-one.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RepoProjectLinkDto {
    /// The repo this link belongs to.
    pub repo_id: Uuid,
    /// Projects v2 project node id (e.g. `PVT_kwDOABC...`).
    pub project_node_id: String,
    /// Field node id for the start-date column, or `null` when
    /// the project does not configure one.
    pub start_field_node_id: Option<String>,
    /// Field node id for the due-date column, or `null` when the
    /// project does not configure one.
    pub due_field_node_id: Option<String>,
}

impl From<RepoProjectLink> for RepoProjectLinkDto {
    fn from(l: RepoProjectLink) -> Self {
        Self {
            repo_id: l.repo_id,
            project_node_id: l.project_node_id,
            start_field_node_id: l.start_field_node_id,
            due_field_node_id: l.due_field_node_id,
        }
    }
}

/// PUT body — `repo_id` is taken from the URL path so the wire
/// shape only carries the three GraphQL node ids the operator
/// picked. The handler upserts: posting twice with different
/// values rewrites the link.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct PutRepoProjectLinkRequest {
    /// Projects v2 project node id the repo should mirror to.
    pub project_node_id: String,
    /// Optional start-date field node id (omit when the project
    /// does not configure one — the mirror skips that lane).
    #[serde(default)]
    pub start_field_node_id: Option<String>,
    /// Optional due-date field node id (omit when the project
    /// does not configure one).
    #[serde(default)]
    pub due_field_node_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /repos/{id}/project-link` — read the current link, or
/// `404` when the repo is not linked. Returning 404 (rather than
/// `200 null`) lets the admin pane use the same fetch hook for
/// "load existing" vs. "first-time link" without an extra
/// discriminator on the wire.
#[utoipa::path(
    get,
    path = "/repos/{id}/project-link",
    params(("id" = Uuid, Path, description = "Repo id")),
    responses(
        (status = 200, description = "Current link", body = RepoProjectLinkDto),
        (status = 404, description = "Repo not linked"),
    ),
    tag = "repos",
)]
pub async fn get_repo_project_link(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RepoProjectLinkDto>, ApiError> {
    let _ = state.store.get_repo(id).await?.ok_or_else(|| ApiError::NotFound {
        code: "repo_not_found",
        message: format!("no repo with id {id}"),
    })?;
    match state.store.get_repo_project_link(id).await? {
        Some(l) => Ok(Json(RepoProjectLinkDto::from(l))),
        None => Err(ApiError::NotFound {
            code: "repo_project_link_not_found",
            message: format!("repo {id} is not linked to a Projects v2 board"),
        }),
    }
}

/// `PUT /repos/{id}/project-link` — upsert the link. The mirror
/// catches up on the next `PATCH /issues/{id}/dates`; this
/// handler does not back-fill historical issues.
#[utoipa::path(
    put,
    path = "/repos/{id}/project-link",
    params(("id" = Uuid, Path, description = "Repo id")),
    request_body = PutRepoProjectLinkRequest,
    responses(
        (status = 200, description = "Link upserted", body = RepoProjectLinkDto),
        (status = 404, description = "No such repo"),
    ),
    tag = "repos",
)]
pub async fn put_repo_project_link(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PutRepoProjectLinkRequest>,
) -> Result<Json<RepoProjectLinkDto>, ApiError> {
    let _ = state.store.get_repo(id).await?.ok_or_else(|| ApiError::NotFound {
        code: "repo_not_found",
        message: format!("no repo with id {id}"),
    })?;
    // Light validation — empty project id is meaningless and
    // would confuse the mirror adapter.
    if body.project_node_id.trim().is_empty() {
        return Err(ApiError::BadRequest {
            code: "invalid_project_node_id",
            message: "project_node_id must be non-empty".into(),
        });
    }
    let link = RepoProjectLink {
        repo_id: id,
        project_node_id: body.project_node_id,
        start_field_node_id: body
            .start_field_node_id
            .filter(|s| !s.trim().is_empty()),
        due_field_node_id: body.due_field_node_id.filter(|s| !s.trim().is_empty()),
    };
    let out = state.store.upsert_repo_project_link(&link).await?;
    Ok(Json(RepoProjectLinkDto::from(out)))
}

/// `DELETE /repos/{id}/project-link` — unwire the repo. The
/// local `dp_issue_dates` rows stay put (date editing still
/// works locally); subsequent `PATCH /issues/{id}/dates` calls
/// for issues in this repo just skip the mirror.
#[utoipa::path(
    delete,
    path = "/repos/{id}/project-link",
    params(("id" = Uuid, Path, description = "Repo id")),
    responses(
        (status = 204, description = "Link removed (or was absent)"),
    ),
    tag = "repos",
)]
pub async fn delete_repo_project_link(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<axum::http::StatusCode, ApiError> {
    state.store.delete_repo_project_link(id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// `GET /repos/{id}/projects` — list Projects v2 boards visible
/// to the deployment's PAT / installation for this repo, with
/// each project's fields. The admin pane uses this for the
/// project + field picker so operators never paste node ids by
/// hand. Returns the verbatim GraphQL `projectsV2` shape — the
/// frontend renders the picker straight off it.
#[utoipa::path(
    get,
    path = "/repos/{id}/projects",
    params(("id" = Uuid, Path, description = "Repo id")),
    responses(
        (status = 200, description = "GraphQL projectsV2 envelope"),
        (status = 404, description = "No such repo"),
        (status = 503, description = "Projects picker not configured"),
    ),
    tag = "repos",
)]
pub async fn list_repo_projects(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let repo = state.store.get_repo(id).await?.ok_or_else(|| ApiError::NotFound {
        code: "repo_not_found",
        message: format!("no repo with id {id}"),
    })?;
    let org = state.store.get_org(repo.org_id).await?.ok_or_else(|| {
        ApiError::NotFound {
            code: "org_not_found",
            message: format!("no org with id {}", repo.org_id),
        }
    })?;
    match state
        .projects_picker
        .list_repo_projects(&org.login, &repo.name)
        .await
    {
        Ok(v) => Ok(Json(v)),
        Err(ProjectsPickerError::Unconfigured) => Err(ApiError::BadRequest {
            code: "upstream_unavailable",
            message: "projects picker backend not configured".into(),
        }),
        Err(ProjectsPickerError::GraphQl(msg)) => Err(ApiError::BadRequest {
            code: "github_validation_failed",
            message: msg,
        }),
        Err(ProjectsPickerError::Transport(msg)) => Err(ApiError::BadRequest {
            code: "upstream_unavailable",
            message: msg,
        }),
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the repo→project-link router. Gated on `(issues,
/// write)` — admin work on the mirror lives in the same auth
/// lane as the date editor that consumes it.
pub fn repo_project_link_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new()
                .route("/repos/{id}/project-link", get(get_repo_project_link))
                .route("/repos/{id}/project-link", put(put_repo_project_link))
                .route("/repos/{id}/project-link", delete(delete_repo_project_link))
                .route("/repos/{id}/projects", get(list_repo_projects)),
            "issues",
            "write",
        ))
        .with_state(inner)
}
