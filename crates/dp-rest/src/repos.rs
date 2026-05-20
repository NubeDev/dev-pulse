//! Repos handlers — workflow drill-down master list.
//!
//! `GET /repos` is the entry point into the workflow surface. The
//! UI lands here with a paginated, searchable list of every repo
//! dev-pulse knows about (potentially in the hundreds), filtered
//! optionally by org or by a free-text query that matches
//! `<owner>/<repo>` substrings. Each row carries the open-issue
//! count and the most recent issue activity so the operator can
//! pick a target repo without a per-row roundtrip.
//!
//! Sort is fixed: `last_activity_at DESC NULLS LAST, org, name`.
//! Hottest-touched repos come first; quiet repos sink. The store
//! layer owns the SQL (see `dp_store_pg::list_repos`).
//!
//! Reads only. The §8 write path lives in [`crate::issues`].

use std::sync::Arc;

use axum::{
    extract::{Extension, Query, State},
    response::Json,
    routing::get,
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::issue::RepoSummary;
use dp_domain::store::{RepoListFilter, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT};

use crate::audit::Principal;
use crate::error::ApiError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Wire DTOs
// ---------------------------------------------------------------------------

/// Row in `GET /repos`. Carries the join from `dp_orgs` so callers
/// don't have to round-trip for `org_login`, plus the two counters
/// the workflow list pane renders inline.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RepoSummaryDto {
    /// Internal repo id.
    pub id: Uuid,
    /// Parent org id.
    pub org_id: Uuid,
    /// Parent org login (joined from `dp_orgs`).
    pub org_login: String,
    /// Repo name (no `owner/` prefix).
    pub name: String,
    /// `org_login/name` for convenience.
    pub slug: String,
    /// Number of open issues in this repo.
    pub open_issue_count: i64,
    /// Most recent issue `updated_at`; `null` if the repo has no
    /// issues yet.
    pub last_activity_at: Option<DateTime<Utc>>,
}

impl From<RepoSummary> for RepoSummaryDto {
    fn from(r: RepoSummary) -> Self {
        let slug = format!("{}/{}", r.org_login, r.name);
        Self {
            id: r.id,
            org_id: r.org_id,
            org_login: r.org_login,
            name: r.name,
            slug,
            open_issue_count: r.open_issue_count,
            last_activity_at: r.last_activity_at,
        }
    }
}

/// Paginated envelope. Carries `total` so the UI can render
/// `Showing X–Y of Z` without a second round-trip.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RepoListResponse {
    /// Repos on this page.
    pub rows: Vec<RepoSummaryDto>,
    /// Total matching the filter, ignoring pagination.
    pub total: i64,
    /// Echoed back so the client can confirm what it asked for.
    pub limit: i64,
    /// Echoed back so the client can confirm what it asked for.
    pub offset: i64,
}

/// Query params for `GET /repos`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListReposQuery {
    /// Restrict to one org. Omit for every org.
    #[serde(default)]
    pub org_id: Option<Uuid>,
    /// Case-insensitive substring search on org login and repo
    /// name.
    #[serde(default)]
    pub q: Option<String>,
    /// Page size; clamped server-side to 1..=[`MAX_LIST_LIMIT`].
    #[serde(default)]
    pub limit: Option<i64>,
    /// Page offset (`0`-based).
    #[serde(default)]
    pub offset: Option<i64>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /repos` — paginated repo list with open-issue counts. Reads
/// only; not audited (low-sensitivity directory traversal, same
/// rationale as `GET /users` in [`crate::directory`]).
#[utoipa::path(
    get,
    path = "/repos",
    params(
        ("org_id" = Option<Uuid>, Query, description = "Restrict to one org"),
        ("q"      = Option<String>, Query, description = "Substring search on `org/name`"),
        ("limit"  = Option<i64>, Query, description = "Page size (1..=200, default 50)"),
        ("offset" = Option<i64>, Query, description = "Page offset (default 0)"),
    ),
    responses(
        (status = 200, description = "Paginated repo list", body = RepoListResponse),
    ),
    tag = "repos",
)]
pub async fn list_repos(
    State(state): State<AppState>,
    Extension(_principal): Extension<Principal>,
    Query(q): Query<ListReposQuery>,
) -> Result<Json<RepoListResponse>, ApiError> {
    let filter = RepoListFilter {
        org_id: q.org_id,
        q: q.q.clone(),
        limit: clamp_limit(q.limit),
        offset: clamp_offset(q.offset),
    };
    let rows = state.store.list_repos(&filter).await?;
    let total = state.store.count_repos(&filter).await?;
    Ok(Json(RepoListResponse {
        rows: rows.into_iter().map(RepoSummaryDto::from).collect(),
        total,
        limit: filter.limit,
        offset: filter.offset,
    }))
}

// ---------------------------------------------------------------------------
// Helpers shared with `crate::issues` list handler.
// ---------------------------------------------------------------------------

/// Clamp a caller-supplied `limit` into `1..=MAX_LIST_LIMIT`.
/// Missing / non-positive values default to [`DEFAULT_LIST_LIMIT`].
pub(crate) fn clamp_limit(v: Option<i64>) -> i64 {
    match v {
        Some(n) if n > 0 => n.min(MAX_LIST_LIMIT),
        _ => DEFAULT_LIST_LIMIT,
    }
}

/// Clamp a caller-supplied `offset` to `>= 0`.
pub(crate) fn clamp_offset(v: Option<i64>) -> i64 {
    v.unwrap_or(0).max(0)
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the repos router fragment. Same wrapping pattern as
/// [`crate::directory::directory_router`] — `repos.read` is the
/// authz pair the workflow gate matches on.
pub fn repos_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new().route("/repos", get(list_repos)),
            "repos",
            "read",
        ))
        .with_state(inner)
}
