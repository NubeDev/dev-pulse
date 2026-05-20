//! Issue read surface — list + detail handlers.
//!
//! The §8 write path (acquire / commit / rollback / sweeper) lives
//! in [`crate::issues`]; this module only handles **reads** off
//! `dp_issues` so the workflow UI can render its paginated drill-
//! down from repo → issues → one-issue detail without making the
//! frontend re-hydrate from GitHub.
//!
//! | route                                  | shape                              |
//! |----------------------------------------|------------------------------------|
//! | `GET /issues`                          | paginated `IssueListResponse`      |
//! | `GET /issues/{id}`                     | one `IssueDto`                     |
//! | `GET /repos/{repo_id}/issues/{number}` | one `IssueDto` (deep-link form)    |
//!
//! All three routes wear the `issues.read` authz pair so they can
//! be gated independently of the §8 write surface (`issues.write`).
//! Reads are not audited (low-sensitivity directory traversal, same
//! rationale as `GET /repos` / `GET /users`).

use std::sync::Arc;

use axum::{
    extract::{Extension, Path, Query, State},
    response::Json,
    routing::get,
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::issue::{Issue, IssueState};
use dp_domain::store::IssueListFilter;

use crate::audit::Principal;
use crate::error::ApiError;
use crate::repos::{clamp_limit, clamp_offset};
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Wire DTOs
// ---------------------------------------------------------------------------

/// Wire form of [`IssueState`]. Lower-case to match GitHub's wire
/// form (`"open"` / `"closed"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum IssueStateDto {
    /// Issue is open.
    Open,
    /// Issue is closed.
    Closed,
}

impl From<IssueState> for IssueStateDto {
    fn from(s: IssueState) -> Self {
        match s {
            IssueState::Open => Self::Open,
            IssueState::Closed => Self::Closed,
        }
    }
}

impl From<IssueStateDto> for IssueState {
    fn from(s: IssueStateDto) -> Self {
        match s {
            IssueStateDto::Open => Self::Open,
            IssueStateDto::Closed => Self::Closed,
        }
    }
}

/// Full issue projection. Matches the shape the frontend's
/// `IssueDto` already declared (`api/client.ts`) so the existing
/// §8.3 detail pane wires up without DTO shape changes.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct IssueDto {
    /// Internal id.
    pub id: Uuid,
    /// Parent repo id.
    pub repo_id: Uuid,
    /// Parent org id.
    pub org_id: Uuid,
    /// Repo-relative issue number.
    pub number: i64,
    /// Title.
    pub title: String,
    /// Body, when present.
    pub body: Option<String>,
    /// State.
    pub state: IssueStateDto,
    /// Labels as strings.
    pub labels: Vec<String>,
    /// Assignee logins.
    pub assignees: Vec<String>,
    /// Milestone title, when set.
    pub milestone: Option<String>,
    /// §8 CAS token.
    pub version: i64,
    /// Last update.
    pub updated_at: DateTime<Utc>,
}

impl From<Issue> for IssueDto {
    fn from(i: Issue) -> Self {
        Self {
            id: i.id,
            repo_id: i.repo_id,
            org_id: i.org_id,
            number: i.number,
            title: i.title,
            body: i.body,
            state: i.state.into(),
            labels: i.labels,
            assignees: i.assignees,
            milestone: i.milestone,
            version: i.version,
            updated_at: i.updated_at,
        }
    }
}

/// Paginated envelope mirroring `RepoListResponse`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct IssueListResponse {
    /// Issues on this page.
    pub rows: Vec<IssueDto>,
    /// Total matching the filter, ignoring pagination.
    pub total: i64,
    /// Echoed limit.
    pub limit: i64,
    /// Echoed offset.
    pub offset: i64,
}

/// State filter accepted on the wire. `Open` is the v1 default; the
/// store layer treats `None` as "open + closed" so `All` maps
/// straight through.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StateFilter {
    /// Only `state = 'open'`.
    #[default]
    Open,
    /// Only `state = 'closed'`.
    Closed,
    /// Both states.
    All,
}

impl StateFilter {
    fn to_store(self) -> Option<IssueState> {
        match self {
            Self::Open => Some(IssueState::Open),
            Self::Closed => Some(IssueState::Closed),
            Self::All => None,
        }
    }
}

/// Query params for `GET /issues`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListIssuesQuery {
    /// Restrict to one repo.
    #[serde(default)]
    pub repo_id: Option<Uuid>,
    /// Restrict to one org.
    #[serde(default)]
    pub org_id: Option<Uuid>,
    /// State filter. Defaults to `open`.
    #[serde(default)]
    pub state: StateFilter,
    /// Assignee login (exact match).
    #[serde(default)]
    pub assignee: Option<String>,
    /// Case-insensitive substring on title.
    #[serde(default)]
    pub q: Option<String>,
    /// Page size; clamped server-side.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Page offset (`0`-based).
    #[serde(default)]
    pub offset: Option<i64>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /issues` — paginated, filterable issue list. Ordered by
/// `updated_at DESC` (matches GitHub's default "recently updated"
/// view).
#[utoipa::path(
    get,
    path = "/issues",
    params(
        ("repo_id"  = Option<Uuid>, Query, description = "Restrict to one repo"),
        ("org_id"   = Option<Uuid>, Query, description = "Restrict to one org"),
        ("state"    = Option<String>, Query, description = "open|closed|all (default open)"),
        ("assignee" = Option<String>, Query, description = "Assignee login (exact)"),
        ("q"        = Option<String>, Query, description = "Substring search on title"),
        ("limit"    = Option<i64>, Query, description = "Page size (1..=200, default 50)"),
        ("offset"   = Option<i64>, Query, description = "Page offset (default 0)"),
    ),
    responses(
        (status = 200, description = "Paginated issue list", body = IssueListResponse),
    ),
    tag = "issues",
)]
pub async fn list_issues(
    State(state): State<AppState>,
    Extension(_principal): Extension<Principal>,
    Query(q): Query<ListIssuesQuery>,
) -> Result<Json<IssueListResponse>, ApiError> {
    let filter = IssueListFilter {
        repo_id: q.repo_id,
        org_id: q.org_id,
        state: q.state.to_store(),
        assignee: q.assignee.clone().filter(|s| !s.is_empty()),
        q: q.q.clone(),
        limit: clamp_limit(q.limit),
        offset: clamp_offset(q.offset),
    };
    let rows = state.store.list_issues(&filter).await?;
    let total = state.store.count_issues(&filter).await?;
    Ok(Json(IssueListResponse {
        rows: rows.into_iter().map(IssueDto::from).collect(),
        total,
        limit: filter.limit,
        offset: filter.offset,
    }))
}

/// `GET /issues/{id}` — single issue by id.
#[utoipa::path(
    get,
    path = "/issues/{id}",
    params(("id" = Uuid, Path, description = "Issue id")),
    responses(
        (status = 200, description = "Issue detail", body = IssueDto),
        (status = 404, description = "No such issue"),
    ),
    tag = "issues",
)]
pub async fn get_issue_by_id(
    State(state): State<AppState>,
    Extension(_principal): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> Result<Json<IssueDto>, ApiError> {
    let issue = state.store.get_issue(id).await?;
    match issue {
        Some(i) => Ok(Json(IssueDto::from(i))),
        None => Err(ApiError::NotFound {
            code: "issue_not_found",
            message: format!("no issue with id {id}"),
        }),
    }
}

/// `GET /repos/{repo_id}/issues/{number}` — single issue via the
/// canonical deep-link shape audit log entries already record.
#[utoipa::path(
    get,
    path = "/repos/{repo_id}/issues/{number}",
    params(
        ("repo_id" = Uuid, Path, description = "Repo id"),
        ("number"  = i64,  Path, description = "Repo-relative issue number"),
    ),
    responses(
        (status = 200, description = "Issue detail", body = IssueDto),
        (status = 404, description = "No such issue"),
    ),
    tag = "issues",
)]
pub async fn get_issue_by_number(
    State(state): State<AppState>,
    Extension(_principal): Extension<Principal>,
    Path((repo_id, number)): Path<(Uuid, i64)>,
) -> Result<Json<IssueDto>, ApiError> {
    let issue = state
        .store
        .get_issue_by_repo_and_number(repo_id, number)
        .await?;
    match issue {
        Some(i) => Ok(Json(IssueDto::from(i))),
        None => Err(ApiError::NotFound {
            code: "issue_not_found",
            message: format!("no issue #{number} in repo {repo_id}"),
        }),
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the issue-read router. Gated on `issues.read` so the §8
/// write surface (`issues.write`) can be toggled separately.
pub fn issues_read_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new()
                .route("/issues", get(list_issues))
                .route("/issues/{id}", get(get_issue_by_id))
                .route("/repos/{repo_id}/issues/{number}", get(get_issue_by_number)),
            "issues",
            "read",
        ))
        .with_state(inner)
}
