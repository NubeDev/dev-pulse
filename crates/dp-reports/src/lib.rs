//! `dp-reports` — report query layer (TODO §Phase 3, SCOPE §8).
//!
//! Implements the three org-scope lenses (SCOPE §8.1) with
//! `event_actors`-aware de-dup (TODO §0.2). Every report accepts the
//! single [`ReportRequest`] envelope and the server resolves
//! `(label, tz, anchor)` into a concrete UTC `[start, end)` via
//! [`resolve_window`] (TODO §0.4) — never the frontend.
//!
//! Boundary rule (TODO §0.6): zero `starter_*` imports. Verified by
//! `scripts/check-boundaries.sh` in CI.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod aggregate;
pub mod envelope;
pub mod freshness;
pub mod lenses;
pub mod tag_filter;

pub use aggregate::{
    compute_percentiles, count_by_bucket, count_by_org, count_by_repo, count_by_team,
    count_by_user, filter_rows_for_metric, percentile_cont_sql, pick_trend_bucket,
    truncate_to_bucket, CountMetric, DurationMetric, MetricRoleEntry, Percentiles, TrendBucket,
    METRIC_ROLE_MAP, MIN_PERCENTILE_SAMPLE_N,
};
pub use envelope::{
    resolve_window, resolve_window_at, GroupBy, ReportRequest, ResolveError, ScopeMode,
    WindowLabel, WindowSpec, MAX_TAGS_FOR_GROUP_BY_TAG,
};
pub use tag_filter::{
    empty_reason_for_tag_filter, is_issue_centric_event_kind, tag_link_kinds_match_event_kind,
    EMPTY_REASON_TAG_KIND_MISMATCH,
};
pub use freshness::{pick_headline as pick_freshness_headline, DataAsOf, DataAsOfExt};

// Re-export the resolved Window type from dp-domain so callers only
// need to depend on dp-reports for the request/response shapes.
pub use dp_domain::window::{Window, WindowAnchor};

// Re-export the EventActorRow projection the lenses operate on, so
// downstream report code can `use dp_reports::EventActorRow` without
// pulling in dp-domain directly.
pub use dp_domain::store::EventActorRow;
