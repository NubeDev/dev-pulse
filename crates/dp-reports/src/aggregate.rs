//! Aggregation layer (TODO §Phase 3 — counts + percentiles + group-by).
//!
//! Two metric families, one set of group-by reducers, one trend-bucket
//! picker.
//!
//! # Count metrics — [`CountMetric`]
//!
//! Each variant carries the [`SCOPE §15.7`][s15.7] role→metric mapping
//! as data: the [`EventKind`]s it applies to and the **default**
//! `actor_roles` filter. The mapping table is materialised as the
//! [`METRIC_ROLE_MAP`] const so the Phase 4 / Phase 5 surfaces, the
//! frontend, and the spot-check fixture harness can all read the same
//! single source of truth. Callers who want a non-default lens (e.g.
//! "PRs I authored *or* co-authored") pass `actor_roles` explicitly on
//! the [`ReportRequest`][crate::ReportRequest] envelope and bypass the
//! default — see [`filter_rows_for_metric`].
//!
//! Bot users (SCOPE §6 caveat) and unattributed events (`user_id IS
//! NULL`) are **not** filtered here — that's a store-side / UI
//! concern. This layer is role-pure.
//!
//! # Duration metrics — [`DurationMetric`]
//!
//! `percentile_cont` over `duration_seconds`, p50/p90/p95 only — no
//! means anywhere (SCOPE §6 long-tail distortion).
//!
//! Sample-size guard (SCOPE §15.9): when `n < 5` all three percentile
//! fields are `None` (the UI shows `—`) and the actual `sample_n` is
//! surfaced so the response can carry "n too small" rather than a
//! noisy single-data-point percentile.
//!
//! Two ways to compute the triple:
//!
//! * [`percentile_cont_sql`] — emits the SQL fragment for the
//!   `dp-store-pg` layer to embed in its duration queries. One helper,
//!   one place that knows the column name and the three percentile
//!   constants.
//! * [`compute_percentiles`] — pure-Rust equivalent that produces the
//!   same numbers Postgres `percentile_cont` would, used by the
//!   spot-check harness and by unit tests so the SQL contract is
//!   verified without a live database.
//!
//! # Group-by reducers
//!
//! Pure reducers over [`EventActorRow`] for the dimensions reports
//! actually surface today: [`count_by_user`], [`count_by_repo`],
//! [`count_by_org`], [`count_by_bucket`] (day / week / month, anchored
//! in the [`Window`]'s TZ), plus the team variant
//! [`count_by_team`] which takes a `user_id → team_id` resolver
//! because the [`EventActorRow`] projection doesn't carry team
//! membership.
//!
//! All reducers operate on **already-lensed** input — caller is
//! expected to apply the appropriate [`ScopeMode`][crate::ScopeMode]
//! lens from [`lenses`][crate::lenses] before counting, so the
//! `(user_id, event_id)` dedup rule for `AllOrgsCombined` happens
//! exactly once.
//!
//! # Trend bucket — [`pick_trend_bucket`]
//!
//! Window-length driven per SCOPE §15.8: `≤ 31d → Day`,
//! `32–183d → Week`, `> 183d → Month`. Server-side decision; the
//! resolved bucket is echoed in the response so every surface
//! (REST / MCP / frontend) renders identical bars.
//!
//! [s15.7]: https://github.com/NubeDev/dev-pulse/blob/main/SCOPE.md#157-role--metric-mapping-one-filter-per-metric-no-overlap

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use dp_domain::event::{ActorRole, EventKind};
use dp_domain::store::EventActorRow;
use dp_domain::window::Window;

// ---------------------------------------------------------------------------
// Count metrics (SCOPE §15.7)
// ---------------------------------------------------------------------------

/// Discrete-event count metrics. Each variant resolves to one
/// [`EventKind`] and a default `actor_roles` filter — see
/// [`METRIC_ROLE_MAP`].
///
/// Variants intentionally cover only the metrics whose underlying
/// [`EventKind`] is currently modelled in [`dp_domain`]. SCOPE §15.7
/// also lists `issues assigned` and `review requests received`; those
/// land when [`EventKind`] gains the matching variants (Phase 2
/// ingestion follow-up), at which point this enum and
/// [`METRIC_ROLE_MAP`] get *additive* rows, never edits to existing
/// rows (SCOPE §15.7 revisit trigger).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CountMetric {
    /// `commits authored` — the **only** default-union metric
    /// (`role IN (author, co_author)`), per SCOPE §6 co-author credit.
    CommitsAuthored,
    /// `commits committed` — `role = committer`; split from author
    /// for squash-merges (SCOPE §6).
    CommitsCommitted,
    /// `PRs opened` — `role = author`.
    PullRequestsOpened,
    /// `PRs merged` — `role = merger`.
    PullRequestsMerged,
    /// `PRs closed without merge` — `role = closer`.
    PullRequestsClosedUnmerged,
    /// `PRs reviewed` — `role = reviewer`.
    PullRequestsReviewed,
    /// `PR review comments` — `role = commenter`.
    ReviewComments,
    /// `issues opened` — `role = author`.
    IssuesOpened,
    /// `issues closed` — `role = closer`.
    IssuesClosed,
    /// `issues commented` — `role = commenter`.
    IssuesCommented,
    /// `workflow runs triggered` — `role = author`.
    WorkflowRunsTriggered,
    /// `deployments cut` — `role = author`.
    DeploymentsCut,
    /// `releases cut` — `role = author`.
    ReleasesCut,
}

/// One row of the role→metric mapping table.
#[derive(Debug, Clone, Copy)]
pub struct MetricRoleEntry {
    /// Which metric.
    pub metric: CountMetric,
    /// The [`EventKind`] the metric counts.
    pub kind: EventKind,
    /// The default `actor_roles` filter for that count.
    pub default_roles: &'static [ActorRole],
}

/// The role→metric mapping locked in SCOPE §15.7, materialised as a
/// const so every surface (Phase 3 reports, Phase 4 REST, Phase 5 MCP,
/// the frontend, and the spot-check harness) reads the *same* table.
///
/// Order matches the SCOPE §15.7 table top-to-bottom. Adding a new
/// metric goes at the bottom — never re-order, the index is not
/// stable but humans diff this by row position.
pub const METRIC_ROLE_MAP: &[MetricRoleEntry] = &[
    MetricRoleEntry {
        metric: CountMetric::CommitsAuthored,
        kind: EventKind::Commit,
        // The only default-union row in the whole table; SCOPE §6
        // mandates co-author credit for `commits authored`.
        default_roles: &[ActorRole::Author, ActorRole::CoAuthor],
    },
    MetricRoleEntry {
        metric: CountMetric::CommitsCommitted,
        kind: EventKind::Commit,
        default_roles: &[ActorRole::Committer],
    },
    MetricRoleEntry {
        metric: CountMetric::PullRequestsOpened,
        kind: EventKind::PullRequestOpened,
        default_roles: &[ActorRole::Author],
    },
    MetricRoleEntry {
        metric: CountMetric::PullRequestsMerged,
        kind: EventKind::PullRequestMerged,
        default_roles: &[ActorRole::Merger],
    },
    MetricRoleEntry {
        metric: CountMetric::PullRequestsClosedUnmerged,
        kind: EventKind::PullRequestClosed,
        default_roles: &[ActorRole::Closer],
    },
    MetricRoleEntry {
        metric: CountMetric::PullRequestsReviewed,
        kind: EventKind::Review,
        default_roles: &[ActorRole::Reviewer],
    },
    MetricRoleEntry {
        metric: CountMetric::ReviewComments,
        kind: EventKind::ReviewComment,
        default_roles: &[ActorRole::Commenter],
    },
    MetricRoleEntry {
        metric: CountMetric::IssuesOpened,
        kind: EventKind::IssueOpened,
        default_roles: &[ActorRole::Author],
    },
    MetricRoleEntry {
        metric: CountMetric::IssuesClosed,
        kind: EventKind::IssueClosed,
        default_roles: &[ActorRole::Closer],
    },
    MetricRoleEntry {
        metric: CountMetric::IssuesCommented,
        kind: EventKind::IssueComment,
        default_roles: &[ActorRole::Commenter],
    },
    MetricRoleEntry {
        metric: CountMetric::WorkflowRunsTriggered,
        kind: EventKind::WorkflowRun,
        default_roles: &[ActorRole::Author],
    },
    MetricRoleEntry {
        metric: CountMetric::DeploymentsCut,
        kind: EventKind::Deployment,
        default_roles: &[ActorRole::Author],
    },
    MetricRoleEntry {
        metric: CountMetric::ReleasesCut,
        kind: EventKind::Release,
        default_roles: &[ActorRole::Author],
    },
];

impl CountMetric {
    /// Look up this metric's [`MetricRoleEntry`].
    ///
    /// `panic`s if [`METRIC_ROLE_MAP`] is missing a row for a variant
    /// — that's a programming bug (someone added an enum variant
    /// without a const-table row) and we want it loud, not silently
    /// returning zero counts.
    pub fn role_entry(self) -> &'static MetricRoleEntry {
        METRIC_ROLE_MAP
            .iter()
            .find(|e| e.metric == self)
            .unwrap_or_else(|| {
                panic!("METRIC_ROLE_MAP missing row for {:?} — extend the const", self)
            })
    }

    /// Convenience: the [`EventKind`] this metric counts.
    pub fn event_kind(self) -> EventKind {
        self.role_entry().kind
    }

    /// Convenience: the default `actor_roles` filter.
    pub fn default_actor_roles(self) -> &'static [ActorRole] {
        self.role_entry().default_roles
    }
}

/// Filter `rows` to the subset that satisfies a count metric.
///
/// * Always filters by `kind == metric.event_kind()`.
/// * Filters by `role IN actor_roles_override` if `Some`; otherwise by
///   `role IN metric.default_actor_roles()`.
///
/// The override channel is the one SCOPE §15.6 / §15.7 lever — the
/// envelope's `actor_roles` field lets the caller widen "PRs opened
/// (author only)" to "PRs I touched in any role" without changing the
/// const table.
///
/// Returns owned `Vec<EventActorRow>` because every downstream pass
/// (lens, group-by) wants the same clones. If profiling ever shows
/// this clone hurting, the obvious move is `Vec<&EventActorRow>` —
/// defer until benches demand it.
pub fn filter_rows_for_metric(
    rows: &[EventActorRow],
    metric: CountMetric,
    actor_roles_override: Option<&[ActorRole]>,
) -> Vec<EventActorRow> {
    let kind = metric.event_kind();
    let roles: &[ActorRole] =
        actor_roles_override.unwrap_or_else(|| metric.default_actor_roles());
    rows.iter()
        .filter(|r| r.kind == kind && roles.contains(&r.role))
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// Duration metrics + percentiles (SCOPE §15.9)
// ---------------------------------------------------------------------------

/// Duration-style metrics aggregated with `percentile_cont`. No means.
///
/// The actual computation (joining the `(open_event, close_event)`
/// pair into a `duration_seconds`) lives in `dp-store-pg`; this enum
/// only names the metrics and serves as the contract that future store
/// methods must satisfy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurationMetric {
    /// PR opened → first review submitted (any reviewer).
    TimeToFirstReview,
    /// PR opened → PR merged. (Closed-without-merge has no merge ts
    /// and is excluded from the sample.)
    TimeToMerge,
    /// First review submitted → last review submitted on the same PR
    /// (a.k.a. review turnaround / round-trip).
    ReviewTurnaround,
}

/// Below this sample size, all three percentiles are reported as
/// `None` and the UI renders `—` (SCOPE §15.9 sample-size floor).
pub const MIN_PERCENTILE_SAMPLE_N: usize = 5;

/// p50/p90/p95 over a duration sample, with sample-size guard.
///
/// `p50/p90/p95` are `None` when `sample_n < MIN_PERCENTILE_SAMPLE_N`
/// — *not* zero, *not* omitted. The wire format keeps the keys so the
/// frontend can render "—" rather than infer absence-vs-zero.
///
/// Values are seconds (matches the column we percentile over). The
/// frontend formats to "1h 23m" etc.; rendering is not our concern.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Percentiles {
    /// 50th percentile (seconds), or `None` if `sample_n < 5`.
    pub p50: Option<f64>,
    /// 90th percentile (seconds), or `None` if `sample_n < 5`.
    pub p90: Option<f64>,
    /// 95th percentile (seconds), or `None` if `sample_n < 5`.
    pub p95: Option<f64>,
    /// Actual observation count that produced (or suppressed) the
    /// percentile triple. Always reported.
    pub sample_n: u64,
}

impl Percentiles {
    /// The shape returned when there are fewer than
    /// [`MIN_PERCENTILE_SAMPLE_N`] observations.
    pub fn too_small(sample_n: usize) -> Self {
        Self {
            p50: None,
            p90: None,
            p95: None,
            sample_n: sample_n as u64,
        }
    }
}

/// Pure-Rust equivalent of `percentile_cont(p) WITHIN GROUP (ORDER BY
/// duration_seconds)` for `p ∈ {0.50, 0.90, 0.95}`, with the SCOPE
/// §15.9 `n < 5 → None` guard applied.
///
/// Matches Postgres's interpolation semantics: rank `= p * (n - 1)`,
/// linear blend between the two flanking ranked values. Used by the
/// spot-check harness to verify the SQL helper matches recorded GitHub
/// numbers without standing up a live database.
pub fn compute_percentiles(durations_seconds: &[i64]) -> Percentiles {
    let n = durations_seconds.len();
    if n < MIN_PERCENTILE_SAMPLE_N {
        return Percentiles::too_small(n);
    }
    let mut sorted: Vec<i64> = durations_seconds.to_vec();
    sorted.sort_unstable();
    Percentiles {
        p50: Some(percentile_cont(&sorted, 0.50)),
        p90: Some(percentile_cont(&sorted, 0.90)),
        p95: Some(percentile_cont(&sorted, 0.95)),
        sample_n: n as u64,
    }
}

/// Linear-interpolated percentile (a.k.a. Postgres `percentile_cont`).
/// Pre-condition: `sorted` is sorted ascending and non-empty.
fn percentile_cont(sorted: &[i64], p: f64) -> f64 {
    debug_assert!(!sorted.is_empty(), "compute_percentiles guards n ≥ 5");
    debug_assert!((0.0..=1.0).contains(&p), "percentile must be in [0,1]");
    let n = sorted.len();
    let rank = p * (n as f64 - 1.0);
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    if lo == hi {
        return sorted[lo] as f64;
    }
    let frac = rank - lo as f64;
    let a = sorted[lo] as f64;
    let b = sorted[hi] as f64;
    a + frac * (b - a)
}

/// Emit the SQL fragment for `p50/p90/p95/sample_n` over `column`.
///
/// One helper so the column-name + percentile-constant tuple lives in
/// exactly one place across `dp-store-pg`. Embed inline:
///
/// ```text
/// SELECT user_id, {percentile_cont_sql("duration_seconds")}
/// FROM …
/// GROUP BY user_id
/// ```
///
/// The `n < 5` guard is **not** applied in SQL — the caller (the
/// store's row-mapper) applies it row-by-row by inspecting `sample_n`
/// before serialising, because Postgres still returns the interpolated
/// value at n = 1..4 and we want to suppress it deterministically in
/// one place (next to [`compute_percentiles`]).
///
/// `column` is interpolated as-is — pass a hard-coded column name, not
/// user input.
pub fn percentile_cont_sql(column: &str) -> String {
    format!(
        "percentile_cont(0.50) WITHIN GROUP (ORDER BY {col}) AS p50, \
         percentile_cont(0.90) WITHIN GROUP (ORDER BY {col}) AS p90, \
         percentile_cont(0.95) WITHIN GROUP (ORDER BY {col}) AS p95, \
         count(*)::bigint                                  AS sample_n",
        col = column,
    )
}

// ---------------------------------------------------------------------------
// Trend bucket (SCOPE §15.8)
// ---------------------------------------------------------------------------

/// Trend-chart bucket size, picked server-side from the resolved
/// window length per SCOPE §15.8. Echoed in the response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrendBucket {
    /// Resolved window ≤ 31 days.
    Day,
    /// Resolved window 32–183 days.
    Week,
    /// Resolved window > 183 days.
    Month,
}

/// Pick the bucket size from the window's UTC length, per
/// SCOPE §15.8: `≤ 31d → Day`, `32–183d → Week`, `> 183d → Month`.
pub fn pick_trend_bucket(window: &Window) -> TrendBucket {
    let days = (window.end - window.start).num_days();
    // `num_days` truncates toward zero; for a `[start, end)` half-open
    // window of `D` calendar days the result is `D` (or `D - 1` if the
    // anchor TZ has a fall-back DST). Treat the 31/183 boundaries
    // inclusively on the *day* side so a "last_30_days" window
    // (exactly 30) stays in `Day`.
    if days <= 31 {
        TrendBucket::Day
    } else if days <= 183 {
        TrendBucket::Week
    } else {
        TrendBucket::Month
    }
}

/// Truncate `ts` to the start of its containing bucket, with the
/// truncation performed in `tz` and the result returned in UTC.
///
/// Mirrors the SCOPE §15.8 SQL contract:
/// `date_trunc('<bucket>', ts AT TIME ZONE tz)` — interpret the
/// timestamp in the window TZ, snap to bucket start, convert back to
/// UTC for the response.
///
/// DST corner: if the bucket-start instant is in a skipped local hour
/// (rare — only for spring-forward at midnight), we fall back to the
/// raw UTC midnight of the same date. The trend chart never shows a
/// hole this way.
pub fn truncate_to_bucket(
    ts: DateTime<Utc>,
    bucket: TrendBucket,
    tz: &Tz,
) -> DateTime<Utc> {
    let local = ts.with_timezone(tz).date_naive();
    let snapped: NaiveDate = match bucket {
        TrendBucket::Day => local,
        TrendBucket::Week => {
            // ISO week starts Monday; matches Postgres `date_trunc('week', …)`.
            let dow = local.weekday().num_days_from_monday() as u64;
            local
                .checked_sub_days(chrono::Days::new(dow))
                .expect("week-snap within NaiveDate range")
        }
        TrendBucket::Month => NaiveDate::from_ymd_opt(local.year(), local.month(), 1)
            .expect("year/month always has day 1"),
    };
    let midnight = chrono::NaiveDateTime::new(
        snapped,
        NaiveTime::from_hms_opt(0, 0, 0).expect("00:00:00 is valid"),
    );
    match tz.from_local_datetime(&midnight) {
        chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc),
        chrono::LocalResult::Ambiguous(earliest, _) => earliest.with_timezone(&Utc),
        chrono::LocalResult::None => Utc.from_utc_datetime(&midnight),
    }
}

// ---------------------------------------------------------------------------
// Group-by reducers
// ---------------------------------------------------------------------------

/// Count rows grouped by `user_id`.
///
/// `BTreeMap` so iteration order is stable for snapshot tests and the
/// SCOPE §11.4 spot-check fixtures. Caller is expected to have applied
/// the relevant lens already (the [`crate::ScopeMode::AllOrgsCombined`]
/// `(user_id, event_id)` dedup happens in [`crate::lenses`]).
pub fn count_by_user(rows: &[EventActorRow]) -> BTreeMap<Uuid, u64> {
    let mut out: BTreeMap<Uuid, u64> = BTreeMap::new();
    for r in rows {
        *out.entry(r.user_id).or_insert(0) += 1;
    }
    out
}

/// Count rows grouped by `repo_id`.
pub fn count_by_repo(rows: &[EventActorRow]) -> BTreeMap<Uuid, u64> {
    let mut out: BTreeMap<Uuid, u64> = BTreeMap::new();
    for r in rows {
        *out.entry(r.repo_id).or_insert(0) += 1;
    }
    out
}

/// Count rows grouped by `org_id`.
pub fn count_by_org(rows: &[EventActorRow]) -> BTreeMap<Uuid, u64> {
    let mut out: BTreeMap<Uuid, u64> = BTreeMap::new();
    for r in rows {
        *out.entry(r.org_id).or_insert(0) += 1;
    }
    out
}

/// Count rows grouped by team. The [`EventActorRow`] projection
/// doesn't carry team membership, so the caller supplies a resolver
/// `user_id → team_id`. Users with no team membership (`None`) are
/// skipped — the report's "unaffiliated" bucket is a frontend concern.
pub fn count_by_team<F>(rows: &[EventActorRow], mut team_of: F) -> BTreeMap<Uuid, u64>
where
    F: FnMut(Uuid) -> Option<Uuid>,
{
    let mut out: BTreeMap<Uuid, u64> = BTreeMap::new();
    for r in rows {
        if let Some(team) = team_of(r.user_id) {
            *out.entry(team).or_insert(0) += 1;
        }
    }
    out
}

/// Count rows grouped by trend bucket (Day/Week/Month), anchored in
/// `tz`. The map key is the bucket-start in UTC (matches SCOPE §15.8
/// "bucket-start converted back to UTC").
///
/// Empty buckets are **not** filled in here — that's the response-
/// shaping layer's job (it knows the resolved window and so can emit
/// zeros for the missing buckets between min and max).
pub fn count_by_bucket(
    rows: &[EventActorRow],
    bucket: TrendBucket,
    tz: &Tz,
) -> BTreeMap<DateTime<Utc>, u64> {
    let mut out: BTreeMap<DateTime<Utc>, u64> = BTreeMap::new();
    for r in rows {
        let k = truncate_to_bucket(r.ts, bucket, tz);
        *out.entry(k).or_insert(0) += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use dp_domain::window::WindowAnchor;

    fn uid(b: u8) -> Uuid {
        Uuid::from_bytes([b; 16])
    }

    fn utc(y: i32, m: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, mi, s).single().unwrap()
    }

    fn row(
        event_id: Uuid,
        user_id: Uuid,
        role: ActorRole,
        org_id: Uuid,
        repo_id: Uuid,
        kind: EventKind,
        ts: DateTime<Utc>,
    ) -> EventActorRow {
        EventActorRow {
            event_id,
            user_id,
            role,
            org_id,
            repo_id,
            kind,
            ts,
        }
    }

    fn win(start: DateTime<Utc>, end: DateTime<Utc>, tz: &str) -> Window {
        Window {
            start,
            end,
            label: "custom".into(),
            tz: tz.into(),
            anchor: WindowAnchor::Viewer,
        }
    }

    // -- METRIC_ROLE_MAP -----------------------------------------------

    #[test]
    fn metric_role_map_covers_every_count_metric_variant() {
        // If a new CountMetric is added without a const-table row,
        // role_entry() panics — catch it here at test time, loudly.
        for m in [
            CountMetric::CommitsAuthored,
            CountMetric::CommitsCommitted,
            CountMetric::PullRequestsOpened,
            CountMetric::PullRequestsMerged,
            CountMetric::PullRequestsClosedUnmerged,
            CountMetric::PullRequestsReviewed,
            CountMetric::ReviewComments,
            CountMetric::IssuesOpened,
            CountMetric::IssuesClosed,
            CountMetric::IssuesCommented,
            CountMetric::WorkflowRunsTriggered,
            CountMetric::DeploymentsCut,
            CountMetric::ReleasesCut,
        ] {
            let entry = m.role_entry();
            assert_eq!(entry.metric, m, "{:?} role_entry returned wrong row", m);
            assert!(
                !entry.default_roles.is_empty(),
                "{:?} must have at least one default role",
                m
            );
        }
    }

    #[test]
    fn commits_authored_is_the_only_default_union_row() {
        // SCOPE §6 / §15.7: co-author credit unions Author + CoAuthor
        // for `commits authored`, and this is the *only* default-union
        // metric in the table. If a future edit changes that, this
        // test must be updated *and* the SCOPE doc updated.
        let union_metrics: Vec<_> = METRIC_ROLE_MAP
            .iter()
            .filter(|e| e.default_roles.len() > 1)
            .collect();
        assert_eq!(union_metrics.len(), 1, "only one default-union metric");
        assert_eq!(union_metrics[0].metric, CountMetric::CommitsAuthored);
        assert_eq!(
            union_metrics[0].default_roles,
            &[ActorRole::Author, ActorRole::CoAuthor],
        );
    }

    // -- filter_rows_for_metric ----------------------------------------

    #[test]
    fn filter_rows_for_metric_default_roles_match_table() {
        let user = uid(0x11);
        let org = uid(0xA);
        let repo = uid(0x22);
        let event = uid(1);
        let rows = vec![
            row(event, user, ActorRole::Author, org, repo, EventKind::Commit, utc(2025, 1, 1, 0, 0, 0)),
            row(event, user, ActorRole::CoAuthor, org, repo, EventKind::Commit, utc(2025, 1, 1, 0, 0, 0)),
            row(event, user, ActorRole::Committer, org, repo, EventKind::Commit, utc(2025, 1, 1, 0, 0, 0)),
            // Different kind: a PR open by the same user — must NOT
            // appear under `commits authored`.
            row(uid(2), user, ActorRole::Author, org, repo, EventKind::PullRequestOpened, utc(2025, 1, 2, 0, 0, 0)),
        ];

        // `commits authored` default = Author OR CoAuthor on Commit.
        let kept = filter_rows_for_metric(&rows, CountMetric::CommitsAuthored, None);
        assert_eq!(kept.len(), 2);
        let roles: Vec<ActorRole> = kept.iter().map(|r| r.role).collect();
        assert!(roles.contains(&ActorRole::Author));
        assert!(roles.contains(&ActorRole::CoAuthor));

        // `commits committed` default = Committer only on Commit.
        let kept = filter_rows_for_metric(&rows, CountMetric::CommitsCommitted, None);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].role, ActorRole::Committer);

        // `PRs opened` default = Author on PullRequestOpened.
        let kept = filter_rows_for_metric(&rows, CountMetric::PullRequestsOpened, None);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].kind, EventKind::PullRequestOpened);
    }

    #[test]
    fn filter_rows_for_metric_actor_roles_override_widens_the_lens() {
        // Envelope override = "PRs I touched in any role".
        let user = uid(0x11);
        let org = uid(0xA);
        let repo = uid(0x22);
        let rows = vec![
            row(uid(1), user, ActorRole::Author, org, repo, EventKind::PullRequestOpened, utc(2025, 1, 1, 0, 0, 0)),
            row(uid(2), user, ActorRole::Reviewer, org, repo, EventKind::PullRequestOpened, utc(2025, 1, 2, 0, 0, 0)),
        ];
        let widened = filter_rows_for_metric(
            &rows,
            CountMetric::PullRequestsOpened,
            Some(&[ActorRole::Author, ActorRole::Reviewer]),
        );
        assert_eq!(widened.len(), 2);

        // And the default narrows it back.
        let narrow = filter_rows_for_metric(&rows, CountMetric::PullRequestsOpened, None);
        assert_eq!(narrow.len(), 1);
        assert_eq!(narrow[0].role, ActorRole::Author);
    }

    // -- compute_percentiles -------------------------------------------

    #[test]
    fn percentiles_below_floor_are_none_with_sample_n_preserved() {
        for n in 0..MIN_PERCENTILE_SAMPLE_N {
            let v: Vec<i64> = (0..n as i64).collect();
            let p = compute_percentiles(&v);
            assert_eq!(p.p50, None, "n={} p50 must be None", n);
            assert_eq!(p.p90, None);
            assert_eq!(p.p95, None);
            assert_eq!(p.sample_n, n as u64, "sample_n always reported");
        }
    }

    #[test]
    fn percentiles_at_floor_compute_postgres_compatible_interpolation() {
        // n = 5 sample: [0, 100, 200, 300, 400]. Postgres
        // percentile_cont yields rank = p * (n - 1) = p * 4:
        //   p50: rank 2.0 → exactly 200
        //   p90: rank 3.6 → 300 + 0.6 * (400-300) = 360
        //   p95: rank 3.8 → 380
        let p = compute_percentiles(&[0, 100, 200, 300, 400]);
        assert_eq!(p.sample_n, 5);
        assert!((p.p50.unwrap() - 200.0).abs() < 1e-9);
        assert!((p.p90.unwrap() - 360.0).abs() < 1e-9);
        assert!((p.p95.unwrap() - 380.0).abs() < 1e-9);
    }

    #[test]
    fn percentiles_unsorted_input_is_sorted_internally() {
        let p = compute_percentiles(&[400, 0, 300, 200, 100]);
        assert!((p.p50.unwrap() - 200.0).abs() < 1e-9);
    }

    #[test]
    fn percentile_cont_sql_mentions_all_three_p_levels_and_sample_n() {
        let sql = percentile_cont_sql("duration_seconds");
        assert!(sql.contains("percentile_cont(0.50)"));
        assert!(sql.contains("percentile_cont(0.90)"));
        assert!(sql.contains("percentile_cont(0.95)"));
        assert!(sql.contains("ORDER BY duration_seconds"));
        assert!(sql.contains("AS p50"));
        assert!(sql.contains("AS p90"));
        assert!(sql.contains("AS p95"));
        assert!(sql.contains("AS sample_n"));
    }

    // -- pick_trend_bucket ---------------------------------------------

    #[test]
    fn trend_bucket_boundaries_match_scope_15_8() {
        // 30d → Day
        let w = win(utc(2025, 1, 1, 0, 0, 0), utc(2025, 1, 31, 0, 0, 0), "UTC");
        assert_eq!(pick_trend_bucket(&w), TrendBucket::Day);
        // 31d exact → still Day (inclusive boundary).
        let w = win(utc(2025, 1, 1, 0, 0, 0), utc(2025, 2, 1, 0, 0, 0), "UTC");
        assert_eq!(pick_trend_bucket(&w), TrendBucket::Day);
        // 32d → Week
        let w = win(utc(2025, 1, 1, 0, 0, 0), utc(2025, 2, 2, 0, 0, 0), "UTC");
        assert_eq!(pick_trend_bucket(&w), TrendBucket::Week);
        // 183d exact → Week (inclusive)
        let w = win(utc(2025, 1, 1, 0, 0, 0), utc(2025, 7, 3, 0, 0, 0), "UTC");
        assert_eq!(pick_trend_bucket(&w), TrendBucket::Week);
        // 184d → Month
        let w = win(utc(2025, 1, 1, 0, 0, 0), utc(2025, 7, 4, 0, 0, 0), "UTC");
        assert_eq!(pick_trend_bucket(&w), TrendBucket::Month);
        // ~1y → Month
        let w = win(utc(2025, 1, 1, 0, 0, 0), utc(2026, 1, 1, 0, 0, 0), "UTC");
        assert_eq!(pick_trend_bucket(&w), TrendBucket::Month);
    }

    // -- truncate_to_bucket --------------------------------------------

    #[test]
    fn truncate_to_bucket_day_in_window_tz_then_back_to_utc() {
        let tz: Tz = "Australia/Sydney".parse().unwrap();
        // 2025-06-15 22:00Z = 2025-06-16 08:00 +10 (Sydney AEST in June).
        // Day-trunc in Sydney → 2025-06-16 00:00 +10 → 2025-06-15 14:00Z.
        let snapped = truncate_to_bucket(utc(2025, 6, 15, 22, 0, 0), TrendBucket::Day, &tz);
        assert_eq!(snapped, utc(2025, 6, 15, 14, 0, 0));
    }

    #[test]
    fn truncate_to_bucket_week_uses_monday_start() {
        let tz: Tz = "UTC".parse().unwrap();
        // 2025-01-08 = Wednesday. Week start = Mon 2025-01-06.
        let snapped = truncate_to_bucket(utc(2025, 1, 8, 12, 0, 0), TrendBucket::Week, &tz);
        assert_eq!(snapped, utc(2025, 1, 6, 0, 0, 0));
    }

    #[test]
    fn truncate_to_bucket_month_uses_first_of_month() {
        let tz: Tz = "UTC".parse().unwrap();
        let snapped = truncate_to_bucket(utc(2025, 6, 15, 12, 0, 0), TrendBucket::Month, &tz);
        assert_eq!(snapped, utc(2025, 6, 1, 0, 0, 0));
    }

    // -- group-by reducers ---------------------------------------------

    #[test]
    fn count_by_user_repo_org_bucket_all_consistent() {
        let u1 = uid(0x11);
        let u2 = uid(0x22);
        let org_a = uid(0xA);
        let org_b = uid(0xB);
        let r1 = uid(0x31);
        let r2 = uid(0x32);

        let rows = vec![
            row(uid(1), u1, ActorRole::Author, org_a, r1, EventKind::Commit, utc(2025, 1, 6, 12, 0, 0)),
            row(uid(2), u1, ActorRole::Author, org_a, r1, EventKind::Commit, utc(2025, 1, 7, 12, 0, 0)),
            row(uid(3), u2, ActorRole::Author, org_b, r2, EventKind::Commit, utc(2025, 1, 6, 12, 0, 0)),
        ];

        // by_user
        let by_user = count_by_user(&rows);
        assert_eq!(by_user[&u1], 2);
        assert_eq!(by_user[&u2], 1);

        // by_repo
        let by_repo = count_by_repo(&rows);
        assert_eq!(by_repo[&r1], 2);
        assert_eq!(by_repo[&r2], 1);

        // by_org
        let by_org = count_by_org(&rows);
        assert_eq!(by_org[&org_a], 2);
        assert_eq!(by_org[&org_b], 1);

        // by_bucket (Day, UTC)
        let tz: Tz = "UTC".parse().unwrap();
        let by_day = count_by_bucket(&rows, TrendBucket::Day, &tz);
        assert_eq!(by_day[&utc(2025, 1, 6, 0, 0, 0)], 2);
        assert_eq!(by_day[&utc(2025, 1, 7, 0, 0, 0)], 1);

        // Totals match across dimensions.
        let total: u64 = by_user.values().sum();
        assert_eq!(total, rows.len() as u64);
        assert_eq!(total, by_repo.values().sum::<u64>());
        assert_eq!(total, by_org.values().sum::<u64>());
        assert_eq!(total, by_day.values().sum::<u64>());
    }

    #[test]
    fn count_by_team_uses_resolver_and_skips_unaffiliated() {
        let u1 = uid(0x11);
        let u2 = uid(0x22);
        let u3 = uid(0x33);
        let team_a = uid(0xAA);
        let team_b = uid(0xBB);

        let teams: BTreeMap<Uuid, Uuid> = [(u1, team_a), (u2, team_b)].into_iter().collect();

        let rows = vec![
            row(uid(1), u1, ActorRole::Author, uid(0xA), uid(0x33), EventKind::Commit, utc(2025, 1, 1, 0, 0, 0)),
            row(uid(2), u1, ActorRole::Author, uid(0xA), uid(0x33), EventKind::Commit, utc(2025, 1, 2, 0, 0, 0)),
            row(uid(3), u2, ActorRole::Author, uid(0xA), uid(0x33), EventKind::Commit, utc(2025, 1, 3, 0, 0, 0)),
            // u3 has no team → must be skipped.
            row(uid(4), u3, ActorRole::Author, uid(0xA), uid(0x33), EventKind::Commit, utc(2025, 1, 4, 0, 0, 0)),
        ];

        let by_team = count_by_team(&rows, |u| teams.get(&u).copied());
        assert_eq!(by_team.len(), 2, "u3 is unaffiliated and skipped");
        assert_eq!(by_team[&team_a], 2);
        assert_eq!(by_team[&team_b], 1);
    }

    #[test]
    fn count_by_bucket_week_groups_same_iso_week_together() {
        let tz: Tz = "UTC".parse().unwrap();
        let user = uid(0x11);
        let rows = vec![
            // Mon 2025-01-06
            row(uid(1), user, ActorRole::Author, uid(0xA), uid(0x33), EventKind::Commit, utc(2025, 1, 6, 0, 1, 0)),
            // Wed 2025-01-08
            row(uid(2), user, ActorRole::Author, uid(0xA), uid(0x33), EventKind::Commit, utc(2025, 1, 8, 12, 0, 0)),
            // Sun 2025-01-12 — still same ISO week
            row(uid(3), user, ActorRole::Author, uid(0xA), uid(0x33), EventKind::Commit, utc(2025, 1, 12, 23, 0, 0)),
            // Mon 2025-01-13 — next week
            row(uid(4), user, ActorRole::Author, uid(0xA), uid(0x33), EventKind::Commit, utc(2025, 1, 13, 0, 5, 0)),
        ];

        let by_week = count_by_bucket(&rows, TrendBucket::Week, &tz);
        assert_eq!(by_week.len(), 2);
        assert_eq!(by_week[&utc(2025, 1, 6, 0, 0, 0)], 3);
        assert_eq!(by_week[&utc(2025, 1, 13, 0, 0, 0)], 1);
    }

    #[test]
    fn count_reducers_on_empty_input_return_empty_maps() {
        assert!(count_by_user(&[]).is_empty());
        assert!(count_by_repo(&[]).is_empty());
        assert!(count_by_org(&[]).is_empty());
        let tz: Tz = "UTC".parse().unwrap();
        assert!(count_by_bucket(&[], TrendBucket::Day, &tz).is_empty());
        assert!(count_by_team(&[], |_| None::<Uuid>).is_empty());
    }
}
