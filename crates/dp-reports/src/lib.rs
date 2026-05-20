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
pub mod leaderboard;
pub mod lenses;
pub mod my_standing;

pub use aggregate::{
    compute_percentiles, count_by_bucket, count_by_org, count_by_repo, count_by_team,
    count_by_user, filter_rows_for_metric, percentile_cont_sql, pick_trend_bucket,
    truncate_to_bucket, CountMetric, DurationMetric, MetricRoleEntry, Percentiles, TrendBucket,
    METRIC_ROLE_MAP, MIN_PERCENTILE_SAMPLE_N,
};
pub use envelope::{
    resolve_window, resolve_window_at, GroupBy, ReportRequest, ResolveError, ScopeMode,
    WindowLabel, WindowSpec,
};
pub use freshness::{pick_headline as pick_freshness_headline, DataAsOf, DataAsOfExt};
pub use leaderboard::{
    build_leaderboard_sql, build_next_cursor, build_paginated_leaderboard_sql,
    build_subject_ids_leaderboard_sql, build_user_single_org_sql,
    check_reconciliation_identity, debug_assert_reconciliation_identity, effective_page_size,
    resolve_leaderboard_envelope, validate_also_compute, validate_page_request,
    validate_subject_ids, validate_subject_scope_combo, LeaderboardContext, LeaderboardEnvelope,
    LeaderboardError, LeaderboardFooter, LeaderboardHeadline, LeaderboardPage, LeaderboardPrimary,
    LeaderboardResponse, LeaderboardRow, MetricId, PageCursor, PageRequest,
    ResolvedLeaderboardEnvelope, SubjectKind, HOME_ORG_LABEL_UNLABELED_BUCKET,
    HOME_ORG_LABEL_UNLABELED_LABEL, LEADERBOARD_ALSO_COMPUTE_CAP, LEADERBOARD_BIND_ORDER,
    LEADERBOARD_BIND_ORDER_PAGED, LEADERBOARD_BIND_ORDER_PAGED_WITH_CURSOR,
    LEADERBOARD_BIND_ORDER_SUBJECT_IDS, LEADERBOARD_PAGE_SIZE_DEFAULT, LEADERBOARD_PAGE_SIZE_MAX,
    LEADERBOARD_SUBJECT_IDS_CAP, LEADERBOARD_TIE_BREAK_ORDER_BY_CLAUSE,
    USER_SINGLE_ORG_BIND_ORDER,
};
pub use my_standing::{
    anonymise_neighbour_row, build_my_standing_sql, compute_visible_headline,
    effective_neighbor_radius, resolve_my_standing_envelope, validate_my_standing_permission,
    MyStandingEnvelope, MyStandingError, MyStandingResponse, ResolvedMyStandingEnvelope,
    MY_STANDING_BIND_ORDER, MY_STANDING_NEIGHBOUR_ANONYMISED_LABEL,
    MY_STANDING_NEIGHBOUR_ANONYMISED_SUBJECT_ID, MY_STANDING_NEIGHBOR_RADIUS_DEFAULT,
    MY_STANDING_NEIGHBOR_RADIUS_MAX,
};

// Re-export the resolved Window type from dp-domain so callers only
// need to depend on dp-reports for the request/response shapes.
pub use dp_domain::window::{Window, WindowAnchor};

// Re-export the EventActorRow projection the lenses operate on, so
// downstream report code can `use dp_reports::EventActorRow` without
// pulling in dp-domain directly.
pub use dp_domain::store::EventActorRow;
