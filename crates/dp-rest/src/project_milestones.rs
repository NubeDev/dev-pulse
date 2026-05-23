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

use async_trait::async_trait;
use axum::{
    extract::{Extension, Path, Query, State},
    response::Json,
    routing::{get, patch, post},
    Router,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use dp_domain::milestone::{Milestone, MilestoneState, MilestoneUpsert};
use dp_domain::store::StoreError;

use crate::app_permissions::require_issues_write;
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
            Router::new()
                .route(
                    "/projects/{id}/adopt-milestone",
                    post(adopt_milestone),
                )
                .route(
                    "/projects/{id}/milestones",
                    post(create_project_milestone),
                )
                .route(
                    "/projects/{id}/milestones/{milestone_id}",
                    patch(patch_project_milestone).delete(delete_project_milestone),
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
// Create-milestone surface (PROJECT-VIEW.md milestones two-way sync).
// ---------------------------------------------------------------------------

/// Errors a [`MilestoneWriteBackend`] surfaces back to the
/// handler. Mirrors the shape of
/// [`crate::issues_write::IssueWriteError`] so the handler maps
/// each variant to a stable [`ApiError`].
#[derive(Debug)]
pub enum MilestoneWriteError {
    /// GitHub returned a 4xx (validation, missing scope, etc.).
    /// Surfaced as `400 upstream_validation` with GitHub's
    /// verbatim message in `details`.
    Validation(String),
    /// GitHub returned a 5xx, transport failure, or the response
    /// could not be parsed. Surfaced as `502 upstream_unavailable`.
    Upstream(String),
    /// The backend isn't wired in this deployment.
    Unconfigured,
}

impl MilestoneWriteError {
    /// Lift into the wire-side [`ApiError`]. Same shape as
    /// [`crate::issues_write::IssueWriteError::into_api_error`].
    pub fn into_api_error(self) -> ApiError {
        match self {
            Self::Validation(msg) => ApiError::BadRequest {
                code: "upstream_validation",
                message: msg,
            },
            Self::Upstream(msg) => ApiError::BadRequest {
                code: "upstream_unavailable",
                message: msg,
            },
            Self::Unconfigured => ApiError::BadRequest {
                code: "upstream_unavailable",
                message: "milestone write backend not configured".into(),
            },
        }
    }
}

/// GitHub I/O seam for the create-milestone path. Production
/// binaries wire an octocrab-backed implementation from the bin
/// layer; tests pass a fake.
#[async_trait]
pub trait MilestoneWriteBackend: Send + Sync + 'static {
    /// `POST /repos/{owner}/{repo}/milestones`. Returns the full
    /// GitHub-side milestone payload so the handler can parse +
    /// upsert in the same request.
    async fn create_milestone(
        &self,
        owner_login: &str,
        repo_name: &str,
        title: &str,
        description: Option<&str>,
        due_on: Option<NaiveDate>,
    ) -> Result<serde_json::Value, MilestoneWriteError>;

    /// `PATCH /repos/{owner}/{repo}/milestones/{number}`. Forwards
    /// every set field of the patch. Returns the full GitHub-side
    /// payload so the handler can re-upsert the local mirror.
    async fn update_milestone(
        &self,
        owner_login: &str,
        repo_name: &str,
        number: i64,
        patch: &MilestonePatchInput,
    ) -> Result<serde_json::Value, MilestoneWriteError>;

    /// `DELETE /repos/{owner}/{repo}/milestones/{number}`. The
    /// handler hard-deletes the local row afterwards so the strip
    /// doesn't render a row that no longer exists on GitHub.
    async fn delete_milestone(
        &self,
        owner_login: &str,
        repo_name: &str,
        number: i64,
    ) -> Result<(), MilestoneWriteError>;
}

/// Patch shape the handler hands to
/// [`MilestoneWriteBackend::update_milestone`]. Same
/// `Option<Option<_>>` semantics as
/// [`dp_fetcher::client::MilestoneRemotePatch`]: `None` = leave
/// as-is; `Some(None)` = clear; `Some(Some(_))` = replace.
#[derive(Debug, Clone, Default)]
pub struct MilestonePatchInput {
    /// New title.
    pub title: Option<String>,
    /// `"open"` / `"closed"`.
    pub state: Option<String>,
    /// Description.
    pub description: Option<Option<String>>,
    /// Due date.
    pub due_on: Option<Option<NaiveDate>>,
}

/// Default backend — refuses every call. Wired into
/// [`AppState::new`] so deployments that forget to wire a real
/// backend fail loudly instead of silently dropping writes.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnconfiguredMilestoneWriter;

#[async_trait]
impl MilestoneWriteBackend for UnconfiguredMilestoneWriter {
    async fn create_milestone(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: Option<&str>,
        _: Option<NaiveDate>,
    ) -> Result<serde_json::Value, MilestoneWriteError> {
        Err(MilestoneWriteError::Unconfigured)
    }
    async fn update_milestone(
        &self,
        _: &str,
        _: &str,
        _: i64,
        _: &MilestonePatchInput,
    ) -> Result<serde_json::Value, MilestoneWriteError> {
        Err(MilestoneWriteError::Unconfigured)
    }
    async fn delete_milestone(
        &self,
        _: &str,
        _: &str,
        _: i64,
    ) -> Result<(), MilestoneWriteError> {
        Err(MilestoneWriteError::Unconfigured)
    }
}

/// Production [`MilestoneWriteBackend`] backed by the dp-fetcher
/// octocrab client. Bin layer constructs it from the same
/// `dp_fetcher::client::Client` the read path uses so writes
/// share the local request budget.
pub struct FetcherMilestoneWriter {
    client: Arc<dp_fetcher::client::Client>,
}

impl FetcherMilestoneWriter {
    /// Construct from a ready-to-use fetcher client.
    pub fn new(client: Arc<dp_fetcher::client::Client>) -> Self {
        Self { client }
    }
}

impl std::fmt::Debug for FetcherMilestoneWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FetcherMilestoneWriter").finish_non_exhaustive()
    }
}

fn map_gh_milestone_err(
    e: dp_fetcher::client::GhWriteError,
) -> MilestoneWriteError {
    match e {
        dp_fetcher::client::GhWriteError::Validation(m) => {
            MilestoneWriteError::Validation(m)
        }
        dp_fetcher::client::GhWriteError::Upstream(m) => {
            MilestoneWriteError::Upstream(m)
        }
    }
}

#[async_trait]
impl MilestoneWriteBackend for FetcherMilestoneWriter {
    async fn create_milestone(
        &self,
        owner_login: &str,
        repo_name: &str,
        title: &str,
        description: Option<&str>,
        due_on: Option<NaiveDate>,
    ) -> Result<serde_json::Value, MilestoneWriteError> {
        self.client
            .gh_create_milestone(owner_login, repo_name, title, description, due_on)
            .await
            .map_err(map_gh_milestone_err)
    }

    async fn update_milestone(
        &self,
        owner_login: &str,
        repo_name: &str,
        number: i64,
        input: &MilestonePatchInput,
    ) -> Result<serde_json::Value, MilestoneWriteError> {
        let remote = dp_fetcher::client::MilestoneRemotePatch {
            title: input.title.as_deref(),
            state: input.state.as_deref(),
            description: input
                .description
                .as_ref()
                .map(|o| o.as_deref()),
            due_on: input.due_on,
        };
        self.client
            .gh_update_milestone(owner_login, repo_name, number, &remote)
            .await
            .map_err(map_gh_milestone_err)
    }

    async fn delete_milestone(
        &self,
        owner_login: &str,
        repo_name: &str,
        number: i64,
    ) -> Result<(), MilestoneWriteError> {
        self.client
            .gh_delete_milestone(owner_login, repo_name, number)
            .await
            .map_err(map_gh_milestone_err)
    }
}

/// Parse a GitHub REST milestone payload (`POST` response shape)
/// into a [`MilestoneUpsert`]. Symmetrical to the fetcher-side
/// parsers for issues; kept here because the fetcher's mirroring
/// of milestones hasn't shipped yet, so this is the only call
/// site.
///
/// Returns a string error so the handler can surface
/// `502 upstream_unavailable` when GitHub returns a shape we
/// can't decode — a bug in our parsing or an upstream contract
/// change either way.
fn parse_milestone_upsert(
    repo_id: Uuid,
    payload: &serde_json::Value,
) -> Result<MilestoneUpsert, String> {
    let obj = payload
        .as_object()
        .ok_or_else(|| "milestone payload is not an object".to_string())?;
    let github_number = obj
        .get("number")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "milestone payload missing `number`".to_string())?
        as i32;
    let github_node_id = obj
        .get("node_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "milestone payload missing `node_id`".to_string())?
        .to_string();
    let title = obj
        .get("title")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "milestone payload missing `title`".to_string())?
        .to_string();
    let description = obj
        .get("description")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let state_str = obj
        .get("state")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "milestone payload missing `state`".to_string())?;
    let state = MilestoneState::from_str(state_str)
        .ok_or_else(|| format!("milestone payload has unknown state {state_str:?}"))?;
    let due_on = obj
        .get("due_on")
        .and_then(|v| v.as_str())
        .map(|s| {
            // GitHub returns ISO-8601 timestamps; we only keep
            // the date (the `dp_milestones.due_on` column is a
            // calendar DATE — see migration 0030 rationale).
            DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&Utc).date_naive())
                .map_err(|e| format!("invalid `due_on` {s:?}: {e}"))
        })
        .transpose()?;
    let open_issues = obj
        .get("open_issues")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let closed_issues = obj
        .get("closed_issues")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let created_at = obj
        .get("created_at")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .ok_or_else(|| "milestone payload missing `created_at`".to_string())?;
    let updated_at = obj
        .get("updated_at")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or(created_at);
    let closed_at = obj
        .get("closed_at")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));
    Ok(MilestoneUpsert {
        repo_id,
        github_number,
        github_node_id,
        title,
        description,
        state,
        due_on,
        open_issues,
        closed_issues,
        created_at,
        updated_at,
        closed_at,
    })
}

/// Body for [`create_project_milestone`].
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CreateMilestoneRequest {
    /// The repo to create the milestone in. Must be currently
    /// linked to the project (via `dp_project_repos`); otherwise
    /// the handler returns `400 repo_not_linked`.
    pub repo_id: Uuid,
    /// Milestone title (GitHub validates length / uniqueness).
    pub title: String,
    /// Optional long-form description. Markdown.
    #[serde(default)]
    pub description: Option<String>,
    /// Optional due date (`YYYY-MM-DD`).
    #[serde(default)]
    pub due_on: Option<NaiveDate>,
}

/// `POST /projects/{id}/milestones` — create a milestone on a
/// linked repo and mirror it into `dp_milestones`.
///
/// 1. Resolve the project; 404 if missing.
/// 2. Confirm `repo_id` is currently linked to this project; 400
///    `repo_not_linked` otherwise (so a stale UI can't create
///    milestones on arbitrary repos by id).
/// 3. Resolve repo → org for the install-permission check.
/// 4. [`require_issues_write`] gate; failure ⇒
///    `403 writes_not_available_for_org`.
/// 5. Call GitHub via [`MilestoneWriteBackend::create_milestone`].
/// 6. Parse the response with [`parse_milestone_upsert`] and
///    [`Store::upsert_milestone`] the row so the strip refreshes
///    immediately.
/// 7. Audit `project.milestone.create` with target
///    `<project_id>:<repo_id>#<github_number>`.
#[utoipa::path(
    post,
    path = "/projects/{id}/milestones",
    params(("id" = Uuid, Path, description = "Project id")),
    request_body = CreateMilestoneRequest,
    responses(
        (status = 200, description = "Milestone created on GitHub and mirrored", body = MilestoneDto),
        (status = 400, description = "Validation failed at GitHub, or repo not linked"),
        (status = 403, description = "Writes not available for the target org"),
        (status = 404, description = "No such project or repo"),
    ),
    tag = "projects",
)]
pub async fn create_project_milestone(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(project_id): Path<Uuid>,
    Json(body): Json<CreateMilestoneRequest>,
) -> Result<Json<MilestoneDto>, ApiError> {
    // Step 1 — project exists.
    state
        .store
        .get_project(project_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "project_not_found",
            message: format!("no project with id {project_id}"),
        })?;

    // Step 2 — repo is linked to this project.
    let linked = state.store.list_project_repos(project_id).await?;
    if !linked.iter().any(|r| r.repo_id == body.repo_id) {
        return Err(ApiError::BadRequest {
            code: "repo_not_linked",
            message: format!(
                "repo {} is not linked to project {project_id}",
                body.repo_id
            ),
        });
    }

    // Step 3 — resolve repo → org for the install-permission check.
    let repo = state
        .store
        .get_repo(body.repo_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "repo_not_found",
            message: format!("no repo with id {}", body.repo_id),
        })?;
    let org = state
        .store
        .get_org(repo.org_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "org_not_found",
            message: format!("no org with id {}", repo.org_id),
        })?;

    // Step 4 — install-permission gate.
    require_issues_write(&*state.store, &state.github_app, &org).await?;

    // Step 5 — GitHub write.
    let payload = state
        .milestone_writer
        .create_milestone(
            &org.login,
            &repo.name,
            body.title.trim(),
            body.description.as_deref().map(str::trim).filter(|s| !s.is_empty()),
            body.due_on,
        )
        .await
        .map_err(MilestoneWriteError::into_api_error)?;

    // Step 6 — parse + local upsert. If parsing or the local
    // upsert fails the GitHub-side row still exists; we log,
    // return 502, and the next reconciler tick reconciles.
    let upsert = parse_milestone_upsert(body.repo_id, &payload).map_err(|e| {
        tracing::warn!(
            target: "dp_rest::project_milestones",
            project_id = %project_id,
            repo_id = %body.repo_id,
            error = %e,
            "create_milestone: failed to parse GitHub payload",
        );
        ApiError::BadRequest {
            code: "upstream_unavailable",
            message: format!("could not parse GitHub milestone payload: {e}"),
        }
    })?;
    let github_number = upsert.github_number;
    let row = state.store.upsert_milestone(&upsert).await?;

    // Step 7 — audit.
    let target = format!("{project_id}:{}#{github_number}", body.repo_id);
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_MILESTONE_CREATE,
        target,
    )
    .await
    .ok();

    Ok(Json(MilestoneDto::from(row)))
}

// ---------------------------------------------------------------------------
// Update / delete milestone — same two-way-sync pattern as create.
// ---------------------------------------------------------------------------

/// Body for [`patch_project_milestone`]. Mirrors the
/// `Option<Option<_>>` shape of [`MilestonePatchInput`] on the
/// wire so callers can distinguish "leave as-is" (omit the key)
/// from "clear" (send `null`).
///
/// Serde's `#[serde(default, deserialize_with = …)]` with
/// `serde_with::rust::double_option` would be the canonical
/// implementation; we inline a small helper instead to avoid
/// pulling in a dependency for one struct.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct PatchMilestoneRequest {
    /// New title. Omit to leave as-is.
    #[serde(default)]
    pub title: Option<String>,
    /// `"open"` / `"closed"`. Omit to leave as-is.
    #[serde(default)]
    pub state: Option<String>,
    /// Description. Omit to leave as-is; `null` to clear; string
    /// to replace.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    #[schema(value_type = Option<String>, nullable)]
    pub description: Option<Option<String>>,
    /// Due date. Omit / `null` / value — same tri-state as
    /// `description`.
    #[serde(default, deserialize_with = "deserialize_double_option_date")]
    #[schema(value_type = Option<String>, nullable, format = "date")]
    pub due_on: Option<Option<NaiveDate>>,
}

fn deserialize_double_option<'de, D>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // serde calls this deserializer only when the key is present
    // in the JSON object — `serde(default)` short-circuits the
    // absent case to `None`. So if we get here, the key was set
    // (possibly to `null`).
    Ok(Some(Option::<String>::deserialize(deserializer)?))
}

fn deserialize_double_option_date<'de, D>(
    deserializer: D,
) -> Result<Option<Option<NaiveDate>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<NaiveDate>::deserialize(deserializer)?))
}

/// Resolve a milestone id to `(milestone, org, repo)` while
/// enforcing it belongs to a repo currently linked to this
/// project. Returns 404 (`milestone_not_found`) when the id is
/// unknown to this project — the same shape as the strip's
/// "milestone not linked" failure on adopt.
async fn resolve_project_milestone(
    state: &AppState,
    project_id: Uuid,
    milestone_id: Uuid,
) -> Result<(Milestone, dp_domain::org::Org, dp_domain::repo::Repo), ApiError>
{
    let rows = state
        .store
        .list_project_milestones(project_id, true)
        .await?;
    let milestone = rows
        .into_iter()
        .find(|m| m.id == milestone_id)
        .ok_or_else(|| ApiError::NotFound {
            code: "milestone_not_found",
            message: format!(
                "no milestone with id {milestone_id} on project {project_id}"
            ),
        })?;
    let repo = state
        .store
        .get_repo(milestone.repo_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "repo_not_found",
            message: format!("no repo with id {}", milestone.repo_id),
        })?;
    let org = state
        .store
        .get_org(repo.org_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "org_not_found",
            message: format!("no org with id {}", repo.org_id),
        })?;
    Ok((milestone, org, repo))
}

/// `PATCH /projects/{id}/milestones/{milestone_id}` — edit a
/// mirrored milestone on GitHub and refresh the local row.
///
/// The `state` field doubles as the close/reopen verb: setting
/// it to `"closed"` audits as `project.milestone.close`,
/// `"open"` as `project.milestone.reopen`. Any other field
/// change audits as `project.milestone.update`. When `state` is
/// the only field set the handler still re-upserts the local
/// mirror from GitHub's response so `closed_at` lands in the
/// same tick.
#[utoipa::path(
    patch,
    path = "/projects/{id}/milestones/{milestone_id}",
    params(
        ("id" = Uuid, Path, description = "Project id"),
        ("milestone_id" = Uuid, Path, description = "Milestone id"),
    ),
    request_body = PatchMilestoneRequest,
    responses(
        (status = 200, description = "Milestone updated on GitHub and mirrored", body = MilestoneDto),
        (status = 400, description = "Validation failed at GitHub"),
        (status = 403, description = "Writes not available for the target org"),
        (status = 404, description = "No such project or milestone"),
    ),
    tag = "projects",
)]
pub async fn patch_project_milestone(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((project_id, milestone_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchMilestoneRequest>,
) -> Result<Json<MilestoneDto>, ApiError> {
    let (milestone, org, repo) =
        resolve_project_milestone(&state, project_id, milestone_id).await?;
    require_issues_write(&*state.store, &state.github_app, &org).await?;

    // Build the patch input. Trim the title; an explicit empty
    // string for description clears it (matches GitHub's
    // semantics).
    let input = MilestonePatchInput {
        title: body.title.as_ref().map(|t| t.trim().to_string()),
        state: body.state.clone(),
        description: body.description.clone().map(|opt| {
            opt.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
        }),
        due_on: body.due_on,
    };

    let payload = state
        .milestone_writer
        .update_milestone(&org.login, &repo.name, milestone.github_number.into(), &input)
        .await
        .map_err(MilestoneWriteError::into_api_error)?;

    let upsert = parse_milestone_upsert(milestone.repo_id, &payload).map_err(|e| {
        tracing::warn!(
            target: "dp_rest::project_milestones",
            project_id = %project_id,
            milestone_id = %milestone_id,
            error = %e,
            "patch_milestone: failed to parse GitHub payload",
        );
        ApiError::BadRequest {
            code: "upstream_unavailable",
            message: format!("could not parse GitHub milestone payload: {e}"),
        }
    })?;
    let row = state.store.upsert_milestone(&upsert).await?;

    // Pick the audit verb. Mirrors `issue_audit_verb` —
    // state→close/reopen wins over any other field set in the
    // same patch (matches the §8.5 issue convention).
    let verb = match input.state.as_deref() {
        Some("closed") => audit::PROJECT_MILESTONE_CLOSE,
        Some("open") => audit::PROJECT_MILESTONE_REOPEN,
        _ => audit::PROJECT_MILESTONE_UPDATE,
    };
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        verb,
        format!("{project_id}:{milestone_id}"),
    )
    .await
    .ok();

    Ok(Json(MilestoneDto::from(row)))
}

/// `DELETE /projects/{id}/milestones/{milestone_id}` — delete a
/// milestone from GitHub and from the local mirror.
///
/// Hard delete on both sides. `dp_projects.primary_milestone_id`
/// pointers clear via the FK's `ON DELETE SET NULL` (migration
/// 0035). The handler returns 204 on success.
#[utoipa::path(
    delete,
    path = "/projects/{id}/milestones/{milestone_id}",
    params(
        ("id" = Uuid, Path, description = "Project id"),
        ("milestone_id" = Uuid, Path, description = "Milestone id"),
    ),
    responses(
        (status = 204, description = "Milestone deleted on GitHub and locally"),
        (status = 400, description = "GitHub rejected the delete"),
        (status = 403, description = "Writes not available for the target org"),
        (status = 404, description = "No such project or milestone"),
    ),
    tag = "projects",
)]
pub async fn delete_project_milestone(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((project_id, milestone_id)): Path<(Uuid, Uuid)>,
) -> Result<axum::http::StatusCode, ApiError> {
    let (milestone, org, repo) =
        resolve_project_milestone(&state, project_id, milestone_id).await?;
    require_issues_write(&*state.store, &state.github_app, &org).await?;

    state
        .milestone_writer
        .delete_milestone(&org.login, &repo.name, milestone.github_number.into())
        .await
        .map_err(MilestoneWriteError::into_api_error)?;

    // Local mirror — squash NotFound (the row was already gone)
    // because the GitHub delete succeeded, so the user-visible
    // outcome is "deleted" either way.
    if let Err(e) = state.store.delete_milestone(milestone_id).await {
        if !matches!(e, StoreError::NotFound { .. }) {
            return Err(e.into());
        }
    }

    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_MILESTONE_DELETE,
        format!("{project_id}:{milestone_id}"),
    )
    .await
    .ok();

    Ok(axum::http::StatusCode::NO_CONTENT)
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
        orgs: Mutex<Vec<Org>>,
        repos: Mutex<Vec<Repo>>,
        project_repos: Mutex<Vec<dp_domain::project::ProjectRepo>>,
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
        async fn get_repo(&self, id: Uuid) -> Result<Option<Repo>, StoreError> {
            Ok(self.repos.lock().unwrap().iter().find(|r| r.id == id).cloned())
        }
        async fn get_org(&self, id: Uuid) -> Result<Option<Org>, StoreError> {
            Ok(self.orgs.lock().unwrap().iter().find(|o| o.id == id).cloned())
        }
        async fn list_project_repos(
            &self,
            project_id: Uuid,
        ) -> Result<Vec<dp_domain::project::ProjectRepo>, StoreError> {
            Ok(self
                .project_repos
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.project_id == project_id)
                .cloned()
                .collect())
        }
        async fn upsert_milestone(
            &self,
            u: &MilestoneUpsert,
        ) -> Result<Milestone, StoreError> {
            let mut rows = self.milestones.lock().unwrap();
            // Update by `(repo_id, github_number)` if present;
            // else insert a new row with a freshly-minted id.
            if let Some(existing) = rows
                .iter_mut()
                .find(|m| m.repo_id == u.repo_id && m.github_number == u.github_number)
            {
                existing.title = u.title.clone();
                existing.description = u.description.clone();
                existing.state = u.state;
                existing.due_on = u.due_on;
                existing.open_issues = u.open_issues;
                existing.closed_issues = u.closed_issues;
                existing.closed_at = u.closed_at;
                existing.updated_at = u.updated_at;
                existing.fetched_at = Utc::now();
                existing.remote_missing_streak = 0;
                return Ok(existing.clone());
            }
            let m = Milestone {
                id: Uuid::new_v4(),
                repo_id: u.repo_id,
                github_number: u.github_number,
                github_node_id: u.github_node_id.clone(),
                title: u.title.clone(),
                description: u.description.clone(),
                state: u.state,
                due_on: u.due_on,
                open_issues: u.open_issues,
                closed_issues: u.closed_issues,
                created_at: u.created_at,
                updated_at: u.updated_at,
                closed_at: u.closed_at,
                fetched_at: Utc::now(),
                remote_missing_streak: 0,
            };
            rows.push(m.clone());
            Ok(m)
        }
        async fn delete_milestone(&self, id: Uuid) -> Result<(), StoreError> {
            let mut rows = self.milestones.lock().unwrap();
            let before = rows.len();
            rows.retain(|m| m.id != id);
            if rows.len() == before {
                return Err(StoreError::NotFound {
                    entity: "milestone",
                    id: id.to_string(),
                });
            }
            Ok(())
        }
        async fn get_org_app_install(
            &self,
            _: Uuid,
        ) -> Result<Option<dp_domain::app_install::OrgAppInstall>, StoreError> {
            // Tests run with `pat_mode = true`, so this is never
            // consulted — keep the stub trivial.
            Ok(None)
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
        build_app_with_writer(store, actor, None)
    }

    fn build_app_with_writer(
        store: Arc<MemStore>,
        actor: Uuid,
        writer: Option<Arc<dyn MilestoneWriteBackend>>,
    ) -> Router {
        use axum::extract::Extension;
        use crate::app_permissions::GitHubAppConfig;
        use starter_spi::auth::{Principal as SpiPrincipal, Role};
        use starter_spi::authz::{NoopPolicyEngine, PolicyEngine};
        use std::sync::Arc as StdArc;
        let mut state = AppState::new(store).with_github_app(Arc::new(
            // `pat_mode` short-circuits `require_issues_write`'s
            // App-install lookup so write tests can run against
            // the MemStore without seeding `dp_org_app_installs`.
            GitHubAppConfig { pat_mode: true, ..GitHubAppConfig::default() },
        ));
        if let Some(w) = writer {
            state = state.with_milestone_writer(w);
        }
        let app_state = Arc::new(state);
        let engine: StdArc<dyn PolicyEngine> = StdArc::new(NoopPolicyEngine);
        let spi_principal = SpiPrincipal {
            subject: actor.to_string(),
            role: Role::Admin,
            scopes: Vec::new(),
            tenant_id: None,
            teams: Vec::new(),
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

    // -----------------------------------------------------------------
    // Write-surface tests (create / patch / delete)
    // -----------------------------------------------------------------

    /// Records every call the handler made + returns canned GitHub
    /// payloads. The handler uses the payload to drive
    /// `parse_milestone_upsert`, so the canned shape has to match
    /// the live REST response well enough to round-trip every
    /// required field.
    #[derive(Default)]
    struct FakeWriter {
        creates: Mutex<Vec<(String, String, String)>>,
        updates: Mutex<Vec<(String, String, i64, MilestonePatchInput)>>,
        deletes: Mutex<Vec<(String, String, i64)>>,
        /// Next github_number to hand out on `create` so tests
        /// don't all collide on `1`.
        next_number: Mutex<i32>,
    }

    impl FakeWriter {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                next_number: Mutex::new(42),
                ..Default::default()
            })
        }

        fn payload(number: i32, title: &str, state: &str) -> serde_json::Value {
            serde_json::json!({
                "number": number,
                "node_id": format!("MI_{number}"),
                "title": title,
                "description": serde_json::Value::Null,
                "state": state,
                "due_on": serde_json::Value::Null,
                "created_at": "2024-01-01T00:00:00Z",
                "updated_at": "2024-01-02T00:00:00Z",
                "closed_at": serde_json::Value::Null,
            })
        }
    }

    #[async_trait]
    impl MilestoneWriteBackend for FakeWriter {
        async fn create_milestone(
            &self,
            owner: &str,
            repo: &str,
            title: &str,
            _description: Option<&str>,
            _due_on: Option<NaiveDate>,
        ) -> Result<serde_json::Value, MilestoneWriteError> {
            self.creates.lock().unwrap().push((
                owner.to_string(),
                repo.to_string(),
                title.to_string(),
            ));
            let mut n = self.next_number.lock().unwrap();
            *n += 1;
            Ok(FakeWriter::payload(*n, title, "open"))
        }

        async fn update_milestone(
            &self,
            owner: &str,
            repo: &str,
            number: i64,
            patch: &MilestonePatchInput,
        ) -> Result<serde_json::Value, MilestoneWriteError> {
            self.updates.lock().unwrap().push((
                owner.to_string(),
                repo.to_string(),
                number,
                patch.clone(),
            ));
            // Echo the patch back to mimic GitHub's response so
            // `parse_milestone_upsert` can re-upsert the row.
            let state = patch.state.as_deref().unwrap_or("open");
            let title = patch.title.as_deref().unwrap_or("orig");
            Ok(FakeWriter::payload(number as i32, title, state))
        }

        async fn delete_milestone(
            &self,
            owner: &str,
            repo: &str,
            number: i64,
        ) -> Result<(), MilestoneWriteError> {
            self.deletes
                .lock()
                .unwrap()
                .push((owner.to_string(), repo.to_string(), number));
            Ok(())
        }
    }

    fn seed_org(store: &MemStore, login: &str) -> Org {
        let o = Org {
            id: Uuid::new_v4(),
            github_id: 1,
            login: login.into(),
            name: None,
        };
        store.orgs.lock().unwrap().push(o.clone());
        o
    }

    fn seed_repo(store: &MemStore, org_id: Uuid, name: &str) -> Repo {
        let r = Repo {
            id: Uuid::new_v4(),
            org_id,
            github_id: 1,
            name: name.into(),
        };
        store.repos.lock().unwrap().push(r.clone());
        r
    }

    fn link_project_repo(store: &MemStore, project_id: Uuid, repo_id: Uuid) {
        store
            .project_repos
            .lock()
            .unwrap()
            .push(dp_domain::project::ProjectRepo {
                project_id,
                repo_id,
                added_by: None,
                added_at: Utc::now(),
            });
    }

    /// Convenience: seed a project + one linked repo + the
    /// repo's org. Returns `(project, repo, org)`.
    fn seed_project_with_repo(
        store: &MemStore,
    ) -> (Project, Repo, Org) {
        let project = seed_project(store);
        let org = seed_org(store, "acme");
        let repo = seed_repo(store, org.id, "widgets");
        link_project_repo(store, project.id, repo.id);
        (project, repo, org)
    }

    async fn post_json(
        app: &Router,
        path: &str,
        body: serde_json::Value,
    ) -> Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn patch_json(
        app: &Router,
        path: &str,
        body: serde_json::Value,
    ) -> Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn delete_req(app: &Router, path: &str) -> Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(path)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn create_milestone_forwards_to_writer_and_upserts_locally() {
        let store = Arc::new(MemStore::default());
        let (project, repo, _org) = seed_project_with_repo(&store);
        let writer = FakeWriter::new();
        let app = build_app_with_writer(
            store.clone(),
            Uuid::new_v4(),
            Some(writer.clone() as Arc<dyn MilestoneWriteBackend>),
        );

        let resp = post_json(
            &app,
            &format!("/projects/{}/milestones", project.id),
            serde_json::json!({
                "repo_id": repo.id,
                "title": "v0.3",
                "description": null,
                "due_on": null,
            }),
        )
        .await;
        assert_eq!(resp.status(), 200);
        let body = body_json(resp).await;
        assert_eq!(body["title"], "v0.3");
        assert_eq!(body["state"], "open");

        // Writer was called with the right org/repo/title.
        let creates = writer.creates.lock().unwrap();
        assert_eq!(creates.len(), 1);
        assert_eq!(creates[0].0, "acme");
        assert_eq!(creates[0].1, "widgets");
        assert_eq!(creates[0].2, "v0.3");

        // Local mirror has the new row.
        let rows = store.milestones.lock().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "v0.3");
        assert_eq!(rows[0].repo_id, repo.id);
    }

    #[tokio::test]
    async fn create_milestone_rejects_repo_not_linked_to_project() {
        let store = Arc::new(MemStore::default());
        let project = seed_project(&store);
        let org = seed_org(&store, "acme");
        // Note: repo created but *not* linked to the project.
        let repo = seed_repo(&store, org.id, "widgets");
        let writer = FakeWriter::new();
        let app = build_app_with_writer(
            store.clone(),
            Uuid::new_v4(),
            Some(writer.clone() as Arc<dyn MilestoneWriteBackend>),
        );

        let resp = post_json(
            &app,
            &format!("/projects/{}/milestones", project.id),
            serde_json::json!({ "repo_id": repo.id, "title": "x" }),
        )
        .await;
        assert_eq!(resp.status(), 400);
        let body = body_json(resp).await;
        assert_eq!(body["code"], "repo_not_linked");

        // Writer was never called — validation runs before I/O.
        assert!(writer.creates.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn patch_milestone_close_forwards_state_and_audits_close() {
        let store = Arc::new(MemStore::default());
        let (project, repo, _org) = seed_project_with_repo(&store);
        let m = seed_milestone(&store, "v1", None, MilestoneState::Open);
        // Re-target the seeded milestone at the linked repo so
        // `resolve_project_milestone` finds it.
        store.milestones.lock().unwrap()[0].repo_id = repo.id;
        let writer = FakeWriter::new();
        let app = build_app_with_writer(
            store.clone(),
            Uuid::new_v4(),
            Some(writer.clone() as Arc<dyn MilestoneWriteBackend>),
        );

        let resp = patch_json(
            &app,
            &format!("/projects/{}/milestones/{}", project.id, m.id),
            serde_json::json!({ "state": "closed" }),
        )
        .await;
        assert_eq!(resp.status(), 200);
        let body = body_json(resp).await;
        assert_eq!(body["state"], "closed");

        let updates = writer.updates.lock().unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].0, "acme");
        assert_eq!(updates[0].1, "widgets");
        assert_eq!(updates[0].3.state.as_deref(), Some("closed"));
        assert!(updates[0].3.title.is_none());
    }

    #[tokio::test]
    async fn patch_milestone_unknown_returns_404_and_skips_writer() {
        let store = Arc::new(MemStore::default());
        let (project, _repo, _org) = seed_project_with_repo(&store);
        let writer = FakeWriter::new();
        let app = build_app_with_writer(
            store.clone(),
            Uuid::new_v4(),
            Some(writer.clone() as Arc<dyn MilestoneWriteBackend>),
        );

        let resp = patch_json(
            &app,
            &format!("/projects/{}/milestones/{}", project.id, Uuid::new_v4()),
            serde_json::json!({ "title": "x" }),
        )
        .await;
        assert_eq!(resp.status(), 404);
        let body = body_json(resp).await;
        assert_eq!(body["code"], "milestone_not_found");
        assert!(writer.updates.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn delete_milestone_removes_local_row_and_returns_204() {
        let store = Arc::new(MemStore::default());
        let (project, repo, _org) = seed_project_with_repo(&store);
        let m = seed_milestone(&store, "v1", None, MilestoneState::Open);
        store.milestones.lock().unwrap()[0].repo_id = repo.id;
        let writer = FakeWriter::new();
        let app = build_app_with_writer(
            store.clone(),
            Uuid::new_v4(),
            Some(writer.clone() as Arc<dyn MilestoneWriteBackend>),
        );

        let resp = delete_req(
            &app,
            &format!("/projects/{}/milestones/{}", project.id, m.id),
        )
        .await;
        assert_eq!(resp.status(), 204);
        assert_eq!(writer.deletes.lock().unwrap().len(), 1);
        assert!(store.milestones.lock().unwrap().is_empty());
    }
}
