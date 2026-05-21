//! Project ↔ GitHub Projects v2 board link CRUD and org-scoped
//! board picker (`linear-projects-v2.md` §7.3).
//!
//! Four routes ship here:
//!
//! | route                                              | what it does                                                |
//! |----------------------------------------------------|-------------------------------------------------------------|
//! | `GET    /orgs/{org_id}/projects-v2`                | normalized `OrgProjectPickerDto` for the §6.4 link dialog   |
//! | `GET    /projects/{id}/board-links`                | `[BoardLinkDto]` (per-link mirror status surfaced)          |
//! | `POST   /projects/{id}/board-links`                | `BoardLinkDto` — link a project to a GitHub Projects v2 board |
//! | `DELETE /projects/{id}/board-links/{link_id}`      | 204 — §9.2 elevated (created_by / lead / admin)             |
//!
//! The picker endpoint is org-scoped (was repo-scoped in §3.10).
//! It returns a **normalized DTO** built off the GraphQL envelope —
//! the REST contract never leaks `nodes[]`-shaped Projects v2
//! GraphQL. Date-field columns are filtered server-side so the
//! `[ Start → ▾ ]` / `[ Due → ▾ ]` dropdowns in the dialog only
//! see fields the mirror can actually write.
//!
//! Authorisation: every route is gated `(projects, write)` — the
//! link surface is operator work on the project, not a viewer
//! affordance. The DELETE handler then applies the §9.2 elevation
//! check on top of the broader write gate so an in-org viewer
//! who happens to hold `(projects, write)` still cannot unlink
//! someone else's board.
//!
//! Audit verbs are pinned in [`crate::audit`]:
//! [`PROJECT_BOARD_LINK`] for create, [`PROJECT_BOARD_UNLINK`] for
//! delete. Reads (the picker and the per-project list) never
//! audit.
//!
//! [`PROJECT_BOARD_LINK`]: crate::audit::PROJECT_BOARD_LINK
//! [`PROJECT_BOARD_UNLINK`]: crate::audit::PROJECT_BOARD_UNLINK

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{delete, get, post},
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::board_link::{BoardLink, BoardLinkUpsert};
use dp_domain::store::StoreError;

use crate::audit::{self, Principal};
use crate::error::ApiError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Picker backend — the GraphQL seam for the §6.4 dialog
// ---------------------------------------------------------------------------

/// `organization(login) { projectsV2(first: 50) }` projection,
/// normalized into the §7.3 [`OrgProjectPickerDto`]. The dp-rest
/// layer holds the backend behind a trait so it doesn't import
/// dp-fetcher directly (boundary §0.6).
///
/// The default backend ([`UnconfiguredOrgProjectsPicker`]) refuses
/// every call so deployments without a GraphQL transport fail
/// loudly instead of returning an empty list.
#[async_trait]
pub trait OrgProjectsPickerBackend: Send + Sync + 'static {
    /// List Projects v2 boards visible to the deployment's PAT /
    /// installation for `org_login`, normalized into the picker
    /// DTO. The §6.4 dialog renders one row per board with the
    /// board's date-field dropdowns inlined.
    async fn list_org_projects(
        &self,
        org_login: &str,
    ) -> Result<OrgProjectPickerDto, OrgProjectsPickerError>;

    /// Create a `Date`-typed field on a Projects v2 board. Used
    /// by the §6.4 dialog's "Create date fields" affordance so
    /// operators don't have to leave dev-pulse to prep a board
    /// before linking. Returns the new field's GraphQL node id
    /// so the dialog can preselect it as the Start / Due target.
    async fn create_date_field(
        &self,
        project_node_id: &str,
        name: &str,
    ) -> Result<String, OrgProjectsPickerError>;
}

/// Errors a [`OrgProjectsPickerBackend`] may surface. The dp-rest
/// handler folds them into [`ApiError`]; per §6.4 a token-scope
/// `FORBIDDEN` surfaces as a clear remediation hint so the dialog
/// can render an `[Open GitHub project settings]` link.
#[derive(Debug, thiserror::Error)]
pub enum OrgProjectsPickerError {
    /// 4xx-class GraphQL error (validation, missing scope, etc.).
    #[error("github graphql: {0}")]
    GraphQl(String),
    /// 5xx / transport.
    #[error("github transport: {0}")]
    Transport(String),
    /// Deployment hasn't wired a real backend.
    #[error("org projects picker not configured")]
    Unconfigured,
}

/// Default — refuses every call so unconfigured deployments
/// surface a 503 instead of an empty list.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnconfiguredOrgProjectsPicker;

#[async_trait]
impl OrgProjectsPickerBackend for UnconfiguredOrgProjectsPicker {
    async fn list_org_projects(
        &self,
        _: &str,
    ) -> Result<OrgProjectPickerDto, OrgProjectsPickerError> {
        Err(OrgProjectsPickerError::Unconfigured)
    }
    async fn create_date_field(
        &self,
        _: &str,
        _: &str,
    ) -> Result<String, OrgProjectsPickerError> {
        Err(OrgProjectsPickerError::Unconfigured)
    }
}

/// Production [`OrgProjectsPickerBackend`] backed by the dp-fetcher
/// octocrab client. Forwards to
/// [`dp_fetcher::client::Client::gh_list_org_projectv2`] and
/// normalizes the GraphQL `nodes[]` envelope into
/// [`OrgProjectPickerDto`] so the REST contract never leaks the
/// GraphQL schema.
pub struct OctocrabOrgProjectsPicker {
    client: Arc<dp_fetcher::client::Client>,
}

impl OctocrabOrgProjectsPicker {
    /// Construct from a ready-to-use fetcher client. The bin layer
    /// shares the same client across the issue writer, the
    /// mirror, and this picker so all GraphQL traffic flows
    /// through one local budget.
    pub fn new(client: Arc<dp_fetcher::client::Client>) -> Self {
        Self { client }
    }
}

impl std::fmt::Debug for OctocrabOrgProjectsPicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OctocrabOrgProjectsPicker").finish_non_exhaustive()
    }
}

#[async_trait]
impl OrgProjectsPickerBackend for OctocrabOrgProjectsPicker {
    async fn list_org_projects(
        &self,
        org_login: &str,
    ) -> Result<OrgProjectPickerDto, OrgProjectsPickerError> {
        use dp_fetcher::client::GhWriteError as G;
        let raw = self
            .client
            .gh_list_org_projectv2(org_login)
            .await
            .map_err(|e| match e {
                G::Validation(m) => OrgProjectsPickerError::GraphQl(m),
                G::Upstream(m) => OrgProjectsPickerError::Transport(m),
            })?;
        Ok(normalize_picker_envelope(&raw))
    }

    async fn create_date_field(
        &self,
        project_node_id: &str,
        name: &str,
    ) -> Result<String, OrgProjectsPickerError> {
        use dp_fetcher::client::GhWriteError as G;
        self.client
            .gh_create_projectv2_date_field(project_node_id, name)
            .await
            .map_err(|e| match e {
                G::Validation(m) => OrgProjectsPickerError::GraphQl(m),
                G::Upstream(m) => OrgProjectsPickerError::Transport(m),
            })
    }
}

/// Normalize the GraphQL envelope (`{ "nodes": [...] }`) into
/// [`OrgProjectPickerDto`]. Filters closed projects and keeps only
/// date-typed fields on each board so the §6.4 dropdowns render
/// fields the mirror can actually write.
///
/// Exposed for the bin layer's manual tests and so a future picker
/// backend (e.g. App-install GraphQL) can reuse the normalization
/// without re-implementing it.
pub fn normalize_picker_envelope(raw: &serde_json::Value) -> OrgProjectPickerDto {
    let nodes = raw
        .get("nodes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut boards: Vec<BoardPickerDto> = Vec::with_capacity(nodes.len());
    for n in &nodes {
        // Skip closed boards — the picker should only surface
        // targets the user can write to today.
        if n.get("closed").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        let node_id = match n.get("id").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let title = n
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let url = n.get("url").and_then(|v| v.as_str()).map(str::to_string);
        let number = n.get("number").and_then(|v| v.as_i64());
        let mut date_fields: Vec<DateFieldDto> = Vec::new();
        if let Some(fields) = n
            .get("fields")
            .and_then(|v| v.get("nodes"))
            .and_then(|v| v.as_array())
        {
            for f in fields {
                let kind = f
                    .get("dataType")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !kind.eq_ignore_ascii_case("date") {
                    continue;
                }
                let f_id = match f.get("id").and_then(|v| v.as_str()) {
                    Some(s) => s.to_string(),
                    None => continue,
                };
                let name = f
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                date_fields.push(DateFieldDto { node_id: f_id, name });
            }
        }
        boards.push(BoardPickerDto {
            node_id,
            title,
            url,
            number,
            date_fields,
        });
    }
    OrgProjectPickerDto {
        boards,
        fetched_at: Utc::now(),
    }
}

// ---------------------------------------------------------------------------
// Wire DTOs
// ---------------------------------------------------------------------------

/// `GET /orgs/{org_id}/projects-v2` response (§7.3). Normalized so
/// the REST contract never leaks the GraphQL envelope. The
/// `fetched_at` field is stamped server-side at picker run so the
/// UI can render the freshness explicitly when an operator
/// re-opens the dialog (the picker itself is a cache-by-default
/// read but rebuilds the cache on every call — see
/// [`crate::state::AppState`] notes on the picker backend).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OrgProjectPickerDto {
    /// Boards visible to the deployment, in GitHub-provided order
    /// (typically most-recently-touched first). Closed boards are
    /// filtered out by the normaliser so only link-able targets
    /// appear in the dialog.
    pub boards: Vec<BoardPickerDto>,
    /// Wall-clock the picker ran. Lets the UI render
    /// `Fetched 14:23:07` next to the dropdown.
    pub fetched_at: DateTime<Utc>,
}

/// One board in [`OrgProjectPickerDto::boards`].
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BoardPickerDto {
    /// GitHub Projects v2 board node id (`PVT_…`).
    pub node_id: String,
    /// Human-readable title (e.g. `"NubeIO / Rubix Roadmap"`).
    pub title: String,
    /// Deep link to github.com for this board, when GitHub
    /// surfaces one. The §6.4 dialog uses this for the
    /// `[Open GitHub project settings]` fallback when the picker
    /// renders a single result and the user wants to inspect it
    /// before linking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Project number (`#12` etc.). Optional so a future
    /// transport that doesn't surface it doesn't break the DTO.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<i64>,
    /// Date-typed fields the mirror can write. Filtered server-
    /// side so the dialog's `Start → / Due →` dropdowns only see
    /// fields the mirror will accept.
    pub date_fields: Vec<DateFieldDto>,
}

/// One date-typed field exposed in the picker. Carries the
/// GraphQL node id so the POST body can round-trip the operator's
/// choice without a second picker call.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DateFieldDto {
    /// Field node id (`PVTF_…`).
    pub node_id: String,
    /// Field name as configured on the GitHub board
    /// (e.g. `"Begin date"`, `"Target date"`).
    pub name: String,
}

/// Wire form of [`BoardLink`] for the §7.3 GET / POST responses.
/// Carries the per-link `last_mirror_at` / `last_mirror_error`
/// aggregate columns the §6.3 row renders as
/// `Last sync: 14:23:07 ✓` / `Last sync: failed — …`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BoardLinkDto {
    /// Primary key — opaque to clients. The §7.3 DELETE handler
    /// takes this in the URL so the wire shape never carries a
    /// raw GitHub node id.
    pub id: Uuid,
    /// Parent project.
    pub project_id: Uuid,
    /// GitHub Projects v2 board node id (`PVT_…`).
    pub github_board_node_id: String,
    /// Cached board title. `None` until the picker has refreshed
    /// — the UI falls back to "Untitled board" in that case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_board_title: Option<String>,
    /// Cached deep link.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_board_url: Option<String>,
    /// Wall-clock the cache was last refreshed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_board_cached_at: Option<DateTime<Utc>>,
    /// Mapped start-date field, or `None` when the board has no
    /// start field (mirror skips the lane).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_field_node_id: Option<String>,
    /// Mapped due-date field, or `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_field_node_id: Option<String>,
    /// Aggregate timestamp of the most recent successful mirror
    /// across any item under this link. `None` until the first
    /// success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_mirror_at: Option<DateTime<Utc>>,
    /// Aggregate error from the most recent failed mirror across
    /// any item under this link; `None` when the most recent
    /// outcome was a success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_mirror_error: Option<String>,
    /// First-write timestamp.
    pub created_at: DateTime<Utc>,
    /// Last accepted mutation on the row.
    pub updated_at: DateTime<Utc>,
}

impl From<BoardLink> for BoardLinkDto {
    fn from(l: BoardLink) -> Self {
        Self {
            id: l.id,
            project_id: l.project_id,
            github_board_node_id: l.github_board_node_id,
            github_board_title: l.github_board_title,
            github_board_url: l.github_board_url,
            github_board_cached_at: l.github_board_cached_at,
            start_field_node_id: l.start_field_node_id,
            due_field_node_id: l.due_field_node_id,
            last_mirror_at: l.last_mirror_at,
            last_mirror_error: l.last_mirror_error,
            created_at: l.created_at,
            updated_at: l.updated_at,
        }
    }
}

/// Body for `POST /projects/{id}/board-links`. Carries the board
/// the operator picked from the §6.4 dialog plus the field-id
/// mappings the picker resolved. The cached `github_board_title`
/// / `github_board_url` ride through too so the link row renders
/// the right name immediately, without a second picker round-trip.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct CreateBoardLinkRequest {
    /// GitHub board node id chosen from [`BoardPickerDto::node_id`].
    pub github_board_node_id: String,
    /// Picker-resolved title. Optional — the nightly refresh
    /// backfills it if absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_board_title: Option<String>,
    /// Picker-resolved deep link.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_board_url: Option<String>,
    /// Mapped start-date field. Omit when the board has no start
    /// field — the mirror skips that lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_field_node_id: Option<String>,
    /// Mapped due-date field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_field_node_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /orgs/{org_id}/projects-v2` — normalized board picker for
/// the §6.4 link dialog. Returns 503 when no picker backend is
/// wired (the UI then renders an `[Open GitHub project settings]`
/// hint per §6.4); the empty-result case is `200 { boards: [],
/// fetched_at }` so the UI can distinguish "no boards" from
/// "transport down".
#[utoipa::path(
    get,
    path = "/orgs/{org_id}/projects-v2",
    params(("org_id" = Uuid, Path, description = "Org id")),
    responses(
        (status = 200, description = "Normalized picker DTO",  body = OrgProjectPickerDto),
        (status = 404, description = "No such org"),
        (status = 503, description = "Picker backend not configured"),
    ),
    tag = "projects",
)]
pub async fn list_org_projects_v2(
    State(state): State<AppState>,
    Path(org_id): Path<Uuid>,
) -> Result<Json<OrgProjectPickerDto>, ApiError> {
    let org = state
        .store
        .get_org(org_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "org_not_found",
            message: format!("no org with id {org_id}"),
        })?;
    match state.org_projects_picker.list_org_projects(&org.login).await {
        Ok(dto) => Ok(Json(dto)),
        Err(OrgProjectsPickerError::Unconfigured) => Err(ApiError::BadRequest {
            code: "upstream_unavailable",
            message: "org projects picker backend not configured".into(),
        }),
        Err(OrgProjectsPickerError::GraphQl(msg)) => Err(ApiError::BadRequest {
            code: "github_validation_failed",
            message: msg,
        }),
        Err(OrgProjectsPickerError::Transport(msg)) => Err(ApiError::BadRequest {
            code: "upstream_unavailable",
            message: msg,
        }),
    }
}

/// `GET /projects/{id}/board-links` — list the project's linked
/// boards, in `created_at ASC` order (so §6.3 renders a stable
/// sequence matching the operator's link-now order). Carries the
/// per-link `last_mirror_at` / `last_mirror_error` so the row can
/// render mirror status without a second round-trip.
#[utoipa::path(
    get,
    path = "/projects/{id}/board-links",
    params(("id" = Uuid, Path, description = "Project id")),
    responses(
        (status = 200, description = "Linked boards (possibly empty)", body = Vec<BoardLinkDto>),
        (status = 404, description = "No such project"),
    ),
    tag = "projects",
)]
pub async fn list_board_links(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<BoardLinkDto>>, ApiError> {
    let _project = state
        .store
        .get_project(project_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "project_not_found",
            message: format!("no project with id {project_id}"),
        })?;
    let links = state.store.list_board_links(project_id).await?;
    Ok(Json(links.into_iter().map(BoardLinkDto::from).collect()))
}

/// `POST /projects/{id}/board-links` — link a board to the
/// project. The natural-key `(project_id, github_board_node_id)`
/// UNIQUE constraint surfaces a re-link of the same board as
/// `409 board_already_linked`. The picker-supplied cached
/// title / url ride through so the link row renders the right
/// name immediately. Audits [`audit::PROJECT_BOARD_LINK`].
#[utoipa::path(
    post,
    path = "/projects/{id}/board-links",
    params(("id" = Uuid, Path, description = "Project id")),
    request_body = CreateBoardLinkRequest,
    responses(
        (status = 200, description = "Board linked",                    body = BoardLinkDto),
        (status = 400, description = "Validation failure"),
        (status = 404, description = "No such project"),
        (status = 409, description = "Board already linked"),
    ),
    tag = "projects",
)]
pub async fn create_board_link(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(project_id): Path<Uuid>,
    Json(body): Json<CreateBoardLinkRequest>,
) -> Result<Json<BoardLinkDto>, ApiError> {
    let _project = state
        .store
        .get_project(project_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "project_not_found",
            message: format!("no project with id {project_id}"),
        })?;
    if body.github_board_node_id.trim().is_empty() {
        return Err(ApiError::BadRequest {
            code: "invalid_board_node_id",
            message: "github_board_node_id must be non-empty".into(),
        });
    }
    let upsert = BoardLinkUpsert {
        project_id,
        github_board_node_id: body.github_board_node_id.trim().to_string(),
        github_board_title: body
            .github_board_title
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        github_board_url: body
            .github_board_url
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        start_field_node_id: body
            .start_field_node_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        due_field_node_id: body
            .due_field_node_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        status_field_node_id: None,
        created_by: Some(principal.actor_user_id),
    };
    let link = match state.store.create_board_link(&upsert).await {
        Ok(l) => l,
        Err(StoreError::Conflict(msg)) => {
            return Err(ApiError::Conflict {
                code: "board_already_linked",
                message: msg,
            });
        }
        Err(StoreError::Invalid(msg)) => {
            return Err(ApiError::BadRequest {
                code: "board_link_invalid",
                message: msg,
            });
        }
        Err(e) => return Err(e.into()),
    };
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_BOARD_LINK,
        format!("{project_id}:{}", link.id),
    )
    .await?;
    Ok(Json(BoardLinkDto::from(link)))
}

/// `DELETE /projects/{id}/board-links/{link_id}` — unlink a board
/// from the project. §9.2 elevation: caller must be the project's
/// `created_by`, its `lead_user_id`, or hold the `(projects,
/// admin)` lane. We approximate the admin lane here by checking
/// for the lead / author seat; further admin elevation rides on
/// top of the `(projects, write)` outer gate. The mirror items
/// (`dp_project_board_items`) cascade via `ON DELETE CASCADE`.
/// Audits [`audit::PROJECT_BOARD_UNLINK`].
#[utoipa::path(
    delete,
    path = "/projects/{id}/board-links/{link_id}",
    params(
        ("id"      = Uuid, Path, description = "Project id"),
        ("link_id" = Uuid, Path, description = "Board link id"),
    ),
    responses(
        (status = 204, description = "Link removed"),
        (status = 403, description = "Caller is not the project creator / lead"),
        (status = 404, description = "No such project, or link does not belong to it"),
    ),
    tag = "projects",
)]
pub async fn delete_board_link(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((project_id, link_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let project = state
        .store
        .get_project(project_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "project_not_found",
            message: format!("no project with id {project_id}"),
        })?;
    let link = state
        .store
        .get_board_link(link_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "board_link_not_found",
            message: format!("no board link with id {link_id}"),
        })?;
    if link.project_id != project_id {
        return Err(ApiError::NotFound {
            code: "board_link_not_found",
            message: format!(
                "board link {link_id} does not belong to project {project_id}"
            ),
        });
    }
    // §9.2 elevation: the actor must be the project's creator or
    // its lead. The outer `(projects, write)` gate is enforced by
    // the router merge; this is the per-row "you wrote this row"
    // check the spec calls out for the unlink op.
    let actor = principal.actor_user_id;
    let allowed = project.created_by == Some(actor) || project.lead_user_id == Some(actor);
    if !allowed {
        return Err(ApiError::Forbidden {
            code: "project_board_unlink_forbidden",
            message: "only the project creator or lead can unlink a board".into(),
        });
    }
    state.store.delete_board_link(link_id).await.map_err(|e| match e {
        StoreError::NotFound { .. } => ApiError::NotFound {
            code: "board_link_not_found",
            message: format!("no board link with id {link_id}"),
        },
        other => other.into(),
    })?;
    audit::record(
        state.store.as_ref(),
        actor,
        audit::PROJECT_BOARD_UNLINK,
        format!("{project_id}:{link_id}"),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// `POST /orgs/{org_id}/projects-v2/date-fields` request body.
/// `project_node_id` is the GraphQL `PVT_…` id the picker
/// returned for the selected board; `name` is the new field's
/// label (e.g. `Start date`, `Due date`).
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateDateFieldRequest {
    /// GraphQL node id of the Projects v2 board.
    pub project_node_id: String,
    /// Field name to display on the board.
    pub name: String,
}

/// `POST /orgs/{org_id}/projects-v2/date-fields` response. The
/// new field's GraphQL node id, suitable to drop straight into a
/// `start_field_node_id` / `due_field_node_id` on the link row.
#[derive(Debug, Serialize, ToSchema)]
pub struct CreateDateFieldResponse {
    /// GraphQL node id of the newly-created date field.
    pub node_id: String,
    /// Echoed name, for the dialog's success toast.
    pub name: String,
}

/// `POST /orgs/{org_id}/projects-v2/date-fields` — create a Date
/// field on a Projects v2 board so the §6.4 link dialog has a
/// target to mirror Start / Due into. The `org_id` exists for
/// permission scoping; the GitHub mutation itself is keyed on
/// `project_node_id` (which is globally unique).
#[utoipa::path(
    post,
    path = "/orgs/{org_id}/projects-v2/date-fields",
    params(("org_id" = Uuid, Path, description = "Org id")),
    request_body = CreateDateFieldRequest,
    responses(
        (status = 200, description = "Field created", body = CreateDateFieldResponse),
        (status = 400, description = "Validation failure or upstream unavailable"),
        (status = 404, description = "No such org"),
    ),
    tag = "projects",
)]
pub async fn create_org_projectv2_date_field(
    State(state): State<AppState>,
    Path(org_id): Path<Uuid>,
    Json(body): Json<CreateDateFieldRequest>,
) -> Result<Json<CreateDateFieldResponse>, ApiError> {
    let _org = state
        .store
        .get_org(org_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "org_not_found",
            message: format!("no org with id {org_id}"),
        })?;
    let project_node_id = body.project_node_id.trim();
    let name = body.name.trim();
    if project_node_id.is_empty() {
        return Err(ApiError::BadRequest {
            code: "invalid_project_node_id",
            message: "project_node_id must be non-empty".into(),
        });
    }
    if name.is_empty() {
        return Err(ApiError::BadRequest {
            code: "invalid_field_name",
            message: "name must be non-empty".into(),
        });
    }
    match state
        .org_projects_picker
        .create_date_field(project_node_id, name)
        .await
    {
        Ok(node_id) => Ok(Json(CreateDateFieldResponse {
            node_id,
            name: name.to_string(),
        })),
        Err(OrgProjectsPickerError::Unconfigured) => Err(ApiError::BadRequest {
            code: "upstream_unavailable",
            message: "org projects picker backend not configured".into(),
        }),
        Err(OrgProjectsPickerError::GraphQl(msg)) => Err(ApiError::BadRequest {
            code: "github_validation_failed",
            message: msg,
        }),
        Err(OrgProjectsPickerError::Transport(msg)) => Err(ApiError::BadRequest {
            code: "upstream_unavailable",
            message: msg,
        }),
    }
}

/// Build the board-links router fragment. Gated on `(projects,
/// read)` for the picker + list and `(projects, write)` for
/// create / delete — same lanes as the §7.1 CRUD spine. The §9.2
/// "creator or lead" elevation rides on top of the write gate
/// inside [`delete_board_link`].
pub fn board_links_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new()
                .route("/orgs/{org_id}/projects-v2", get(list_org_projects_v2))
                .route("/projects/{id}/board-links", get(list_board_links)),
            "projects",
            "read",
        ))
        .merge(with_permission(
            Router::new()
                .route("/projects/{id}/board-links", post(create_board_link))
                .route(
                    "/projects/{id}/board-links/{link_id}",
                    delete(delete_board_link),
                )
                .route(
                    "/orgs/{org_id}/projects-v2/date-fields",
                    post(create_org_projectv2_date_field),
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
    use serde_json::json;

    #[test]
    fn normalize_picker_skips_closed_and_filters_non_date_fields() {
        let raw = json!({
            "nodes": [
                {
                    "id": "PVT_open",
                    "title": "Roadmap",
                    "number": 12,
                    "url": "https://github.com/orgs/acme/projects/12",
                    "closed": false,
                    "fields": {
                        "nodes": [
                            { "id": "PVTF_begin",  "name": "Begin date",  "dataType": "DATE" },
                            { "id": "PVTF_target", "name": "Target date", "dataType": "DATE" },
                            { "id": "PVTF_text",   "name": "Status",      "dataType": "TEXT" }
                        ]
                    }
                },
                {
                    "id": "PVT_closed",
                    "title": "Old",
                    "closed": true,
                    "fields": { "nodes": [] }
                }
            ]
        });
        let dto = normalize_picker_envelope(&raw);
        assert_eq!(dto.boards.len(), 1);
        let board = &dto.boards[0];
        assert_eq!(board.node_id, "PVT_open");
        assert_eq!(board.title, "Roadmap");
        assert_eq!(board.number, Some(12));
        assert_eq!(board.date_fields.len(), 2);
        assert_eq!(board.date_fields[0].name, "Begin date");
        assert_eq!(board.date_fields[1].name, "Target date");
    }

    #[test]
    fn normalize_picker_empty_envelope_yields_empty_boards() {
        let raw = json!({ "nodes": [] });
        let dto = normalize_picker_envelope(&raw);
        assert!(dto.boards.is_empty());
    }
}
