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

/// Wire envelope for `GET /repos/{id}/sync-status`. See
/// `linear-projects-idea.md` §5.9. `queued` is `false` for now;
/// the scheduler does not expose per-repo in-flight introspection
/// so the badge UX treats "queued" as a transient client-side
/// flag set after a successful POST.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RepoSyncStatusDto {
    /// When the last successful sync landed.
    pub last_synced_at: Option<DateTime<Utc>>,
    /// When the last attempt finished — same value as
    /// `last_synced_at` until the migration grows an
    /// `attempted_at` column.
    pub last_attempt_at: Option<DateTime<Utc>>,
    /// Last error, or `null` if the latest sync succeeded.
    pub last_error: Option<String>,
    /// `true` if the scheduler is in the middle of reconciling
    /// this repo. Currently always `false` — see module comment.
    pub queued: bool,
}

/// Wire envelope for `POST /repos/{id}/sync`. Always
/// `{ "queued": true }` on the 202 reply.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RepoSyncQueuedDto {
    /// Sentinel — always `true`.
    pub queued: bool,
}

/// `GET /repos/{id}/sync-status` — sync freshness badge data.
/// Authorisation: `("repos", "read")`.
#[utoipa::path(
    get,
    path = "/repos/{id}/sync-status",
    params(("id" = Uuid, Path, description = "Repo id")),
    responses(
        (status = 200, description = "Sync freshness", body = RepoSyncStatusDto),
        (status = 404, description = "No such repo"),
    ),
    tag = "repos",
)]
pub async fn get_repo_sync_status(
    State(state): State<AppState>,
    Extension(_principal): Extension<Principal>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<RepoSyncStatusDto>, ApiError> {
    if state.store.get_repo(id).await?.is_none() {
        return Err(ApiError::NotFound {
            code: "repo_not_found",
            message: format!("no repo with id {id}"),
        });
    }
    let s = state.store.get_repo_sync_status(id).await?.unwrap_or(
        dp_domain::store::RepoSyncStatus {
            last_synced_at: None,
            last_attempt_at: None,
            last_error: None,
        },
    );
    Ok(Json(RepoSyncStatusDto {
        last_synced_at: s.last_synced_at,
        last_attempt_at: s.last_attempt_at,
        last_error: s.last_error,
        queued: false,
    }))
}

/// `POST /repos/{id}/sync` — operator-triggered per-repo
/// reconciler tick. Idempotent: if the scheduler is already
/// running a tick the call coalesces and the body is still
/// `{ "queued": true }` (the user's *intent* is queued even if
/// the scheduler decided to coalesce against an in-flight run).
/// Authorisation: `("repos", "sync")` — the one new auth pair in
/// slice 2.
#[utoipa::path(
    post,
    path = "/repos/{id}/sync",
    params(("id" = Uuid, Path, description = "Repo id")),
    responses(
        (status = 202, description = "Sync queued", body = RepoSyncQueuedDto),
        (status = 404, description = "No such repo"),
        (status = 503, description = "Reconciler scheduler not configured in this deployment"),
    ),
    tag = "repos",
)]
pub async fn request_repo_sync(
    State(state): State<AppState>,
    Extension(_principal): Extension<Principal>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<(axum::http::StatusCode, Json<RepoSyncQueuedDto>), ApiError> {
    let repo = state.store.get_repo(id).await?.ok_or(ApiError::NotFound {
        code: "repo_not_found",
        message: format!("no repo with id {id}"),
    })?;
    let Some(scheduler) = state.scheduler.clone() else {
        return Err(ApiError::BadRequest {
            code: "reconciler_unavailable",
            message: "reconciler scheduler not configured".to_string(),
        });
    };
    // Spawn so the request returns 202 immediately; the scheduler
    // coalesces against any in-flight tick. Errors from the tick
    // are logged but never surface — the caller has already
    // returned.
    tokio::spawn(async move {
        let scope = dp_fetcher::reconciler::Scope::Repo {
            org_id: repo.org_id,
            repo_id: repo.id,
        };
        if let Err(e) = scheduler.try_trigger_now(scope).await {
            tracing::warn!(error = %e, repo_id = %repo.id, "per-repo sync trigger failed");
        }
    });
    Ok((
        axum::http::StatusCode::ACCEPTED,
        Json(RepoSyncQueuedDto { queued: true }),
    ))
}

/// Build the repos router fragment. Same wrapping pattern as
/// [`crate::directory::directory_router`] — `repos.read` is the
/// authz pair the workflow gate matches on; the `POST
/// /repos/{id}/sync` route is gated on the new `("repos", "sync")`
/// pair (§5.9 — the one new auth pair in slice 2).
pub fn repos_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    let reads = with_permission(
        Router::new()
            .route("/repos", get(list_repos))
            .route("/repos/{id}/sync-status", get(get_repo_sync_status)),
        "repos",
        "read",
    );
    let writes = with_permission(
        Router::new().route("/repos/{id}/sync", axum::routing::post(request_repo_sync)),
        "repos",
        "sync",
    );
    Router::new().merge(reads).merge(writes).with_state(inner)
}
