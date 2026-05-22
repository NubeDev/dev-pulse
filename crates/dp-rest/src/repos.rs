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

/// `GET /repos/{id}/metadata` response — the per-repo snapshot of
/// mutable GitHub-side fields (stars / forks / language / default
/// branch / archival state, …) populated by the fetcher off every
/// webhook delivery's `repository` block.
///
/// All numeric fields default to `0`; nullable fields default to
/// `null`. The endpoint returns `404` when no snapshot row has been
/// recorded for the repo yet (fresh install before the first
/// webhook lands) — the UI shows a "snapshot pending" placeholder
/// rather than rendering zeros.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RepoMetadataDto {
    /// Stars on GitHub.
    pub stars: i64,
    /// Forks on GitHub.
    pub forks: i64,
    /// Watchers (`subscribers_count`, not the legacy alias).
    pub watchers: i64,
    /// Open issues + PRs as GitHub itself counts them. Differs
    /// from the dev-pulse `open_issue_count` (issues only) by the
    /// number of open PRs.
    pub open_issues_remote: i64,
    /// Primary language detected by GitHub, if any.
    pub primary_language: Option<String>,
    /// Default branch name.
    pub default_branch: Option<String>,
    /// Repo description.
    pub description: Option<String>,
    /// Repo homepage URL.
    pub homepage: Option<String>,
    /// GitHub's archived flag.
    pub is_archived: bool,
    /// GitHub's fork flag.
    pub is_fork: bool,
    /// GitHub's private flag.
    pub is_private: bool,
    /// GitHub's `pushed_at` — last push to any branch.
    pub pushed_at: Option<DateTime<Utc>>,
    /// Wall-clock the dev-pulse fetcher last refreshed this row.
    pub metadata_updated_at: DateTime<Utc>,
}

impl From<dp_domain::RepoMetadata> for RepoMetadataDto {
    fn from(m: dp_domain::RepoMetadata) -> Self {
        Self {
            stars: m.stars,
            forks: m.forks,
            watchers: m.watchers,
            open_issues_remote: m.open_issues_remote,
            primary_language: m.primary_language,
            default_branch: m.default_branch,
            description: m.description,
            homepage: m.homepage,
            is_archived: m.is_archived,
            is_fork: m.is_fork,
            is_private: m.is_private,
            pushed_at: m.pushed_at,
            metadata_updated_at: m.metadata_updated_at,
        }
    }
}

/// `GET /repos/{id}/metadata` — repo snapshot for the
/// repo-activity dashboard. Authorisation: `("repos", "read")`.
#[utoipa::path(
    get,
    path = "/repos/{id}/metadata",
    params(("id" = Uuid, Path, description = "Repo id")),
    responses(
        (status = 200, description = "Repo metadata snapshot", body = RepoMetadataDto),
        (status = 404, description = "No such repo, or no snapshot recorded yet"),
    ),
    tag = "repos",
)]
pub async fn get_repo_metadata(
    State(state): State<AppState>,
    Extension(_principal): Extension<Principal>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<RepoMetadataDto>, ApiError> {
    if state.store.get_repo(id).await?.is_none() {
        return Err(ApiError::NotFound {
            code: "repo_not_found",
            message: format!("no repo with id {id}"),
        });
    }
    let m = state.store.get_repo_metadata(id).await?.ok_or(ApiError::NotFound {
        code: "repo_metadata_not_found",
        message: format!("no metadata snapshot recorded for repo {id} yet"),
    })?;
    Ok(Json(m.into()))
}

/// `GET /repos/{id}/pr-size-stats` query — defaults to a rolling
/// 90-day window if `since` / `until` are omitted, capped to a
/// 366-day span so a runaway client can't request a year-and-a-half
/// of percentiles.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PrSizeStatsQuery {
    /// Inclusive window start (UTC). Defaults to `now - 90 days`.
    #[serde(default)]
    pub since: Option<DateTime<Utc>>,
    /// Exclusive window end (UTC). Defaults to `now`.
    #[serde(default)]
    pub until: Option<DateTime<Utc>>,
}

/// p50 / p90 / p95 over a numeric distribution. Every field is
/// `null` when the sample is too small (SCOPE §15.9, `n < 5`); the
/// `sample_n` on the parent response carries the actual count so
/// the UI can render "n too small" instead of zeros.
#[derive(Debug, Clone, Copy, Default, Serialize, ToSchema)]
pub struct PercentileTripleDto {
    /// 50th percentile (median).
    pub p50: Option<f64>,
    /// 90th percentile.
    pub p90: Option<f64>,
    /// 95th percentile.
    pub p95: Option<f64>,
}

impl From<dp_domain::PercentileTriple> for PercentileTripleDto {
    fn from(t: dp_domain::PercentileTriple) -> Self {
        Self {
            p50: t.p50,
            p90: t.p90,
            p95: t.p95,
        }
    }
}

/// `GET /repos/{id}/pr-size-stats` response — repo-level
/// pull-request size distribution. Every field describes the
/// repo, never an individual contributor (SCOPE §4).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RepoPrSizeStatsDto {
    /// Echoed window start.
    pub since: DateTime<Utc>,
    /// Echoed window end.
    pub until: DateTime<Utc>,
    /// Number of merged PRs whose payload carried diff-size fields
    /// inside the window.
    pub sample_n: i64,
    /// `payload->>'additions'`.
    pub additions: PercentileTripleDto,
    /// `payload->>'deletions'`.
    pub deletions: PercentileTripleDto,
    /// `additions + deletions`.
    pub total_lines: PercentileTripleDto,
    /// `payload->>'changed_files'`.
    pub changed_files: PercentileTripleDto,
    /// `payload->>'commits'`.
    pub commits: PercentileTripleDto,
}

/// `GET /repos/{id}/pr-size-stats` — repo-level PR-size
/// distribution. Authorisation: `("repos", "read")`.
#[utoipa::path(
    get,
    path = "/repos/{id}/pr-size-stats",
    params(
        ("id"    = Uuid, Path, description = "Repo id"),
        ("since" = Option<DateTime<Utc>>, Query, description = "Inclusive window start (default: now - 90d)"),
        ("until" = Option<DateTime<Utc>>, Query, description = "Exclusive window end (default: now)"),
    ),
    responses(
        (status = 200, description = "Repo PR-size percentile distribution", body = RepoPrSizeStatsDto),
        (status = 400, description = "Invalid window (since >= until, or span > 366d)"),
        (status = 404, description = "No such repo"),
    ),
    tag = "repos",
)]
pub async fn get_repo_pr_size_stats(
    State(state): State<AppState>,
    Extension(_principal): Extension<Principal>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
    Query(q): Query<PrSizeStatsQuery>,
) -> Result<Json<RepoPrSizeStatsDto>, ApiError> {
    if state.store.get_repo(id).await?.is_none() {
        return Err(ApiError::NotFound {
            code: "repo_not_found",
            message: format!("no repo with id {id}"),
        });
    }
    let until = q.until.unwrap_or_else(Utc::now);
    let since = q.since.unwrap_or_else(|| until - chrono::Duration::days(90));
    if since >= until {
        return Err(ApiError::BadRequest {
            code: "invalid_window",
            message: "since must be < until".to_string(),
        });
    }
    if (until - since) > chrono::Duration::days(366) {
        return Err(ApiError::BadRequest {
            code: "invalid_window",
            message: "window span exceeds the 366-day cap".to_string(),
        });
    }
    let s = state.store.pr_size_stats_for_repo(id, since, until).await?;
    Ok(Json(RepoPrSizeStatsDto {
        since,
        until,
        sample_n: s.sample_n,
        additions: s.additions.into(),
        deletions: s.deletions.into(),
        total_lines: s.total_lines.into(),
        changed_files: s.changed_files.into(),
        commits: s.commits.into(),
    }))
}

/// `GET /repos/{id}/ci-stats` query — same window contract as
/// `pr-size-stats`: defaults to a rolling 90 days, capped at a
/// 366-day span.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CiStatsQuery {
    /// Inclusive window start (UTC). Defaults to `now - 90 days`.
    #[serde(default)]
    pub since: Option<DateTime<Utc>>,
    /// Exclusive window end (UTC). Defaults to `now`.
    #[serde(default)]
    pub until: Option<DateTime<Utc>>,
}

/// `GET /repos/{id}/ci-stats` response — repo-level CI
/// workflow-run health. Every field describes the repo's CI, not
/// an individual contributor (SCOPE §4).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RepoCiStatsDto {
    /// Echoed window start.
    pub since: DateTime<Utc>,
    /// Echoed window end.
    pub until: DateTime<Utc>,
    /// Total workflow runs in the window.
    pub total_runs: i64,
    /// `conclusion = 'success'`.
    pub success: i64,
    /// `conclusion = 'failure'`.
    pub failure: i64,
    /// `conclusion = 'cancelled'`.
    pub cancelled: i64,
    /// `skipped`, `neutral`, `timed_out`, `action_required`, `stale`.
    pub other: i64,
    /// `success / (success + failure)` ∈ `[0.0, 1.0]`, or `null`
    /// when there were no terminal success / failure runs.
    pub success_rate: Option<f64>,
    /// Runs whose payload carried both `run_started_at` and
    /// `updated_at` and a strictly-positive delta.
    pub duration_sample_n: i64,
    /// p50 / p90 / p95 over `updated_at - run_started_at`, in
    /// seconds. `null` triple when `duration_sample_n < 5`.
    pub duration_seconds: PercentileTripleDto,
}

/// `GET /repos/{id}/ci-stats` — repo-level CI workflow-run
/// statistics. Authorisation: `("repos", "read")`.
#[utoipa::path(
    get,
    path = "/repos/{id}/ci-stats",
    params(
        ("id"    = Uuid, Path, description = "Repo id"),
        ("since" = Option<DateTime<Utc>>, Query, description = "Inclusive window start (default: now - 90d)"),
        ("until" = Option<DateTime<Utc>>, Query, description = "Exclusive window end (default: now)"),
    ),
    responses(
        (status = 200, description = "Repo CI run statistics", body = RepoCiStatsDto),
        (status = 400, description = "Invalid window (since >= until, or span > 366d)"),
        (status = 404, description = "No such repo"),
    ),
    tag = "repos",
)]
pub async fn get_repo_ci_stats(
    State(state): State<AppState>,
    Extension(_principal): Extension<Principal>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
    Query(q): Query<CiStatsQuery>,
) -> Result<Json<RepoCiStatsDto>, ApiError> {
    if state.store.get_repo(id).await?.is_none() {
        return Err(ApiError::NotFound {
            code: "repo_not_found",
            message: format!("no repo with id {id}"),
        });
    }
    let until = q.until.unwrap_or_else(Utc::now);
    let since = q.since.unwrap_or_else(|| until - chrono::Duration::days(90));
    if since >= until {
        return Err(ApiError::BadRequest {
            code: "invalid_window",
            message: "since must be < until".to_string(),
        });
    }
    if (until - since) > chrono::Duration::days(366) {
        return Err(ApiError::BadRequest {
            code: "invalid_window",
            message: "window span exceeds the 366-day cap".to_string(),
        });
    }
    let s = state.store.ci_stats_for_repo(id, since, until).await?;
    Ok(Json(RepoCiStatsDto {
        since,
        until,
        total_runs: s.total_runs,
        success: s.success,
        failure: s.failure,
        cancelled: s.cancelled,
        other: s.other,
        success_rate: s.success_rate,
        duration_sample_n: s.duration_sample_n,
        duration_seconds: s.duration_seconds.into(),
    }))
}

/// `GET /repos/{id}/activity-heatmap` query parameters. Same
/// window contract as the other repo-level aggregators; adds a
/// `timezone` knob so heatmap cells line up with the viewer's
/// local day.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ActivityHeatmapQuery {
    /// Inclusive window start (UTC). Defaults to `now - 90 days`.
    #[serde(default)]
    pub since: Option<DateTime<Utc>>,
    /// Exclusive window end (UTC). Defaults to `now`.
    #[serde(default)]
    pub until: Option<DateTime<Utc>>,
    /// IANA timezone for bucketing (e.g. `America/Los_Angeles`).
    /// Defaults to `UTC`.
    #[serde(default)]
    pub timezone: Option<String>,
}

/// One `(dow, hour)` cell in a [`RepoActivityHeatmapDto`].
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct HeatmapBucketDto {
    /// 0 = Monday … 6 = Sunday (ISO 8601).
    pub dow: i16,
    /// 0..=23 in the response's timezone.
    pub hour: i16,
    /// Event count in this bucket.
    pub count: i64,
}

impl From<dp_domain::HeatmapBucket> for HeatmapBucketDto {
    fn from(b: dp_domain::HeatmapBucket) -> Self {
        Self { dow: b.dow, hour: b.hour, count: b.count }
    }
}

/// `GET /repos/{id}/activity-heatmap` response — dense 168-cell
/// `(dow, hour)` grid of activity events for the repo. Describes
/// the repo's collaboration cadence, never an individual's
/// (SCOPE §4).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RepoActivityHeatmapDto {
    /// Echoed window start.
    pub since: DateTime<Utc>,
    /// Echoed window end.
    pub until: DateTime<Utc>,
    /// Echoed IANA timezone used for bucketing.
    pub timezone: String,
    /// Total events across all buckets in the window.
    pub total: i64,
    /// 168 cells, ordered by `(dow, hour)`.
    pub buckets: Vec<HeatmapBucketDto>,
}

/// `GET /repos/{id}/activity-heatmap` — repo-level activity
/// `(day_of_week, hour_of_day)` distribution.
/// Authorisation: `("repos", "read")`.
#[utoipa::path(
    get,
    path = "/repos/{id}/activity-heatmap",
    params(
        ("id"       = Uuid,                    Path,  description = "Repo id"),
        ("since"    = Option<DateTime<Utc>>,   Query, description = "Inclusive window start (default: now - 90d)"),
        ("until"    = Option<DateTime<Utc>>,   Query, description = "Exclusive window end (default: now)"),
        ("timezone" = Option<String>,          Query, description = "IANA timezone for bucketing (default: UTC)"),
    ),
    responses(
        (status = 200, description = "Repo activity heatmap", body = RepoActivityHeatmapDto),
        (status = 400, description = "Invalid window or timezone"),
        (status = 404, description = "No such repo"),
    ),
    tag = "repos",
)]
pub async fn get_repo_activity_heatmap(
    State(state): State<AppState>,
    Extension(_principal): Extension<Principal>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
    Query(q): Query<ActivityHeatmapQuery>,
) -> Result<Json<RepoActivityHeatmapDto>, ApiError> {
    if state.store.get_repo(id).await?.is_none() {
        return Err(ApiError::NotFound {
            code: "repo_not_found",
            message: format!("no repo with id {id}"),
        });
    }
    let until = q.until.unwrap_or_else(Utc::now);
    let since = q.since.unwrap_or_else(|| until - chrono::Duration::days(90));
    if since >= until {
        return Err(ApiError::BadRequest {
            code: "invalid_window",
            message: "since must be < until".to_string(),
        });
    }
    if (until - since) > chrono::Duration::days(366) {
        return Err(ApiError::BadRequest {
            code: "invalid_window",
            message: "window span exceeds the 366-day cap".to_string(),
        });
    }
    let tz = q.timezone.as_deref().unwrap_or("UTC");
    // Validate the IANA name client-side so PG never sees a
    // malformed value (which it would surface as a generic
    // `invalid_parameter_value`). `chrono-tz` reuses the same
    // tz database PG ships with, so anything parseable here is
    // also valid there.
    if tz.parse::<chrono_tz::Tz>().is_err() {
        return Err(ApiError::BadRequest {
            code: "invalid_timezone",
            message: format!("'{tz}' is not a recognised IANA timezone"),
        });
    }
    let h = state.store.activity_heatmap_for_repo(id, since, until, tz).await?;
    Ok(Json(RepoActivityHeatmapDto {
        since,
        until,
        timezone: h.timezone,
        total: h.total,
        buckets: h.buckets.into_iter().map(Into::into).collect(),
    }))
}

/// `GET /repos/{id}/review-velocity` query — same window
/// contract as the other repo-level aggregators: defaults to a
/// rolling 90 days, capped at a 366-day span.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReviewVelocityQuery {
    /// Inclusive window start (UTC). Defaults to `now - 90 days`.
    #[serde(default)]
    pub since: Option<DateTime<Utc>>,
    /// Exclusive window end (UTC). Defaults to `now`.
    #[serde(default)]
    pub until: Option<DateTime<Utc>>,
}

/// `GET /repos/{id}/review-velocity` response — repo-level
/// time-to-merge percentile distribution. Every field describes
/// the repo's merge cadence, not an individual contributor's
/// (SCOPE §4).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RepoReviewVelocityDto {
    /// Echoed window start.
    pub since: DateTime<Utc>,
    /// Echoed window end.
    pub until: DateTime<Utc>,
    /// Merged PRs in the window whose payload carried both
    /// `created_at` and `merged_at` with a positive delta.
    pub sample_n: i64,
    /// p50 / p90 / p95 over `merged_at - created_at`, in seconds.
    /// `null` triple when `sample_n < 5`.
    pub time_to_merge_seconds: PercentileTripleDto,
}

/// `GET /repos/{id}/review-velocity` — repo-level time-to-merge
/// percentile distribution. Authorisation: `("repos", "read")`.
#[utoipa::path(
    get,
    path = "/repos/{id}/review-velocity",
    params(
        ("id"    = Uuid, Path, description = "Repo id"),
        ("since" = Option<DateTime<Utc>>, Query, description = "Inclusive window start (default: now - 90d)"),
        ("until" = Option<DateTime<Utc>>, Query, description = "Exclusive window end (default: now)"),
    ),
    responses(
        (status = 200, description = "Repo time-to-merge percentiles", body = RepoReviewVelocityDto),
        (status = 400, description = "Invalid window (since >= until, or span > 366d)"),
        (status = 404, description = "No such repo"),
    ),
    tag = "repos",
)]
pub async fn get_repo_review_velocity(
    State(state): State<AppState>,
    Extension(_principal): Extension<Principal>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
    Query(q): Query<ReviewVelocityQuery>,
) -> Result<Json<RepoReviewVelocityDto>, ApiError> {
    if state.store.get_repo(id).await?.is_none() {
        return Err(ApiError::NotFound {
            code: "repo_not_found",
            message: format!("no repo with id {id}"),
        });
    }
    let until = q.until.unwrap_or_else(Utc::now);
    let since = q.since.unwrap_or_else(|| until - chrono::Duration::days(90));
    if since >= until {
        return Err(ApiError::BadRequest {
            code: "invalid_window",
            message: "since must be < until".to_string(),
        });
    }
    if (until - since) > chrono::Duration::days(366) {
        return Err(ApiError::BadRequest {
            code: "invalid_window",
            message: "window span exceeds the 366-day cap".to_string(),
        });
    }
    let v = state.store.review_velocity_for_repo(id, since, until).await?;
    Ok(Json(RepoReviewVelocityDto {
        since,
        until,
        sample_n: v.sample_n,
        time_to_merge_seconds: v.time_to_merge_seconds.into(),
    }))
}

/// `GET /repos/{id}/contributor-diversity` query — same window
/// contract as the other repo-level aggregators.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ContributorDiversityQuery {
    /// Inclusive window start (UTC). Defaults to `now - 90 days`.
    #[serde(default)]
    pub since: Option<DateTime<Utc>>,
    /// Exclusive window end (UTC). Defaults to `now`.
    #[serde(default)]
    pub until: Option<DateTime<Utc>>,
}

/// `GET /repos/{id}/contributor-diversity` response —
/// repo-level "bus factor" view. Every field describes the
/// repo's concentration risk, not an individual contributor
/// (SCOPE §4). The wire shape deliberately carries no user
/// identifiers.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RepoContributorDiversityDto {
    /// Echoed window start.
    pub since: DateTime<Utc>,
    /// Echoed window end.
    pub until: DateTime<Utc>,
    /// Total (merged-PR, author) pairs in the window.
    pub sample_n: i64,
    /// Distinct PR authors observed.
    pub distinct_authors: i64,
    /// Share of `sample_n` from the single top author, in
    /// `[0.0, 1.0]`. `null` when `sample_n < 5`.
    pub top1_share: Option<f64>,
    /// Share of `sample_n` from the top 3 authors combined.
    /// `null` when `sample_n < 5`.
    pub top3_share: Option<f64>,
}

/// `GET /repos/{id}/contributor-diversity` — repo-level
/// "bus factor" view. Authorisation: `("repos", "read")`.
#[utoipa::path(
    get,
    path = "/repos/{id}/contributor-diversity",
    params(
        ("id"    = Uuid, Path, description = "Repo id"),
        ("since" = Option<DateTime<Utc>>, Query, description = "Inclusive window start (default: now - 90d)"),
        ("until" = Option<DateTime<Utc>>, Query, description = "Exclusive window end (default: now)"),
    ),
    responses(
        (status = 200, description = "Repo contributor-diversity stats", body = RepoContributorDiversityDto),
        (status = 400, description = "Invalid window (since >= until, or span > 366d)"),
        (status = 404, description = "No such repo"),
    ),
    tag = "repos",
)]
pub async fn get_repo_contributor_diversity(
    State(state): State<AppState>,
    Extension(_principal): Extension<Principal>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
    Query(q): Query<ContributorDiversityQuery>,
) -> Result<Json<RepoContributorDiversityDto>, ApiError> {
    if state.store.get_repo(id).await?.is_none() {
        return Err(ApiError::NotFound {
            code: "repo_not_found",
            message: format!("no repo with id {id}"),
        });
    }
    let until = q.until.unwrap_or_else(Utc::now);
    let since = q.since.unwrap_or_else(|| until - chrono::Duration::days(90));
    if since >= until {
        return Err(ApiError::BadRequest {
            code: "invalid_window",
            message: "since must be < until".to_string(),
        });
    }
    if (until - since) > chrono::Duration::days(366) {
        return Err(ApiError::BadRequest {
            code: "invalid_window",
            message: "window span exceeds the 366-day cap".to_string(),
        });
    }
    let d = state.store.contributor_diversity_for_repo(id, since, until).await?;
    Ok(Json(RepoContributorDiversityDto {
        since,
        until,
        sample_n: d.sample_n,
        distinct_authors: d.distinct_authors,
        top1_share: d.top1_share,
        top3_share: d.top3_share,
    }))
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
    Extension(principal): Extension<Principal>,
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
    // Audit the *request* (the tick itself is async; the audit log
    // captures operator intent, not the outcome). Failures here
    // never block the 202 the caller already expects.
    crate::audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        crate::audit::REPO_SYNC_REQUESTED,
        repo.id.to_string(),
    )
    .await
    .ok();
    Ok((
        axum::http::StatusCode::ACCEPTED,
        Json(RepoSyncQueuedDto { queued: true }),
    ))
}

/// Request body for `POST /repos/sync` — operator-triggered
/// per-repo reconciler tick keyed by `(org_login, name)` rather
/// than the internal repo `id`. Useful for callers that already
/// know the GitHub slug (CLI, webhooks, ad-hoc curl) and don't
/// want a round-trip through `GET /repos` to resolve the UUID.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct RepoSyncByNameRequest {
    /// GitHub org login (e.g. `acme-corp`). Case-insensitive.
    pub org: String,
    /// Repo name without the `owner/` prefix (e.g. `dev-pulse`).
    /// Case-insensitive.
    pub name: String,
}

/// `POST /repos/sync` — operator-triggered per-repo reconciler
/// tick keyed by `(org, name)` instead of the internal repo
/// `id`. Same coalescing / audit behaviour as
/// [`request_repo_sync`]; the response carries the resolved
/// `repo_id` so the caller can follow up on `GET
/// /repos/{id}/sync-status`. Authorisation: `("repos", "sync")`.
#[utoipa::path(
    post,
    path = "/repos/sync",
    request_body = RepoSyncByNameRequest,
    responses(
        (status = 202, description = "Sync queued", body = RepoSyncQueuedByNameDto),
        (status = 404, description = "No such org or repo"),
        (status = 503, description = "Reconciler scheduler not configured in this deployment"),
    ),
    tag = "repos",
)]
pub async fn request_repo_sync_by_name(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<RepoSyncByNameRequest>,
) -> Result<(axum::http::StatusCode, Json<RepoSyncQueuedByNameDto>), ApiError> {
    let org_login = body.org.trim();
    let repo_name = body.name.trim();
    if org_login.is_empty() || repo_name.is_empty() {
        return Err(ApiError::BadRequest {
            code: "invalid_repo_slug",
            message: "both `org` and `name` are required".to_string(),
        });
    }
    let org = state
        .store
        .list_orgs()
        .await?
        .into_iter()
        .find(|o| o.login.eq_ignore_ascii_case(org_login))
        .ok_or_else(|| ApiError::NotFound {
            code: "org_not_found",
            message: format!("no org with login '{org_login}'"),
        })?;
    let filter = RepoListFilter {
        org_id: Some(org.id),
        q: Some(repo_name.to_string()),
        limit: MAX_LIST_LIMIT,
        offset: 0,
    };
    let repo = state
        .store
        .list_repos(&filter)
        .await?
        .into_iter()
        .find(|r| r.name.eq_ignore_ascii_case(repo_name))
        .ok_or_else(|| ApiError::NotFound {
            code: "repo_not_found",
            message: format!("no repo '{org_login}/{repo_name}'"),
        })?;
    let Some(scheduler) = state.scheduler.clone() else {
        return Err(ApiError::BadRequest {
            code: "reconciler_unavailable",
            message: "reconciler scheduler not configured".to_string(),
        });
    };
    let org_id = repo.org_id;
    let repo_id = repo.id;
    tokio::spawn(async move {
        let scope = dp_fetcher::reconciler::Scope::Repo { org_id, repo_id };
        if let Err(e) = scheduler.try_trigger_now(scope).await {
            tracing::warn!(error = %e, repo_id = %repo_id, "per-repo sync trigger failed");
        }
    });
    crate::audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        crate::audit::REPO_SYNC_REQUESTED,
        repo_id.to_string(),
    )
    .await
    .ok();
    Ok((
        axum::http::StatusCode::ACCEPTED,
        Json(RepoSyncQueuedByNameDto {
            queued: true,
            repo_id,
        }),
    ))
}

/// Wire envelope for `POST /repos/sync`. Carries the resolved
/// `repo_id` alongside the `queued` sentinel so the caller can
/// follow up on `GET /repos/{id}/sync-status` without a second
/// lookup.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RepoSyncQueuedByNameDto {
    /// Sentinel — always `true`.
    pub queued: bool,
    /// Internal repo id resolved from `(org, name)`.
    pub repo_id: Uuid,
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
            .route("/repos/{id}/metadata", get(get_repo_metadata))
            .route("/repos/{id}/pr-size-stats", get(get_repo_pr_size_stats))
            .route("/repos/{id}/ci-stats", get(get_repo_ci_stats))
            .route("/repos/{id}/activity-heatmap", get(get_repo_activity_heatmap))
            .route("/repos/{id}/review-velocity", get(get_repo_review_velocity))
            .route("/repos/{id}/contributor-diversity", get(get_repo_contributor_diversity))
            .route("/repos/{id}/sync-status", get(get_repo_sync_status)),
        "repos",
        "read",
    );
    let writes = with_permission(
        Router::new()
            .route("/repos/{id}/sync", axum::routing::post(request_repo_sync))
            .route("/repos/sync", axum::routing::post(request_repo_sync_by_name)),
        "repos",
        "sync",
    );
    Router::new().merge(reads).merge(writes).with_state(inner)
}
