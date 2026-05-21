//! Project portfolio report — SCOPE-PROJECT-REPORTS.md.
//!
//! "Across every project I can see — which are on track, which are
//! slipping, which issues are doing the slipping?" One row per visible
//! project + a portfolio-level KPI rollup, in a single round trip.
//!
//! ## Surfaces this module is shared across
//!
//! REST (`POST /reports/project-portfolio`), MCP (`project_portfolio`
//! tool, phase 5), and the frontend `useReportProjectPortfolio` hook
//! all consume the same [`ProjectPortfolioRequest`] /
//! [`ProjectPortfolioResponse`] pair so they cannot diverge — same
//! shape-locking rule as the leaderboard envelope.
//!
//! ## Boundary
//!
//! Pure types + a pure SQL string builder (added in S2). No `sqlx`, no
//! `dp-store-pg` import. The store layer binds parameters in the order
//! documented on [`build_project_portfolio_sql`] (S2).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use dp_domain::project::ProjectStatus;
pub use dp_domain::project::{PortfolioQueryFilter, PortfolioRawRow, PortfolioSort};
use dp_domain::window::Window;

use crate::envelope::WindowSpec;

// ---------------------------------------------------------------------------
// Pagination defaults (spec §6 — same shape as `GET /projects`)
// ---------------------------------------------------------------------------

/// Default `limit` for the portfolio page — matches the §6.2 list
/// default for `GET /projects`. Kept as a small constant so the REST
/// handler, MCP tool, and frontend all share one source of truth.
pub const PORTFOLIO_LIMIT_DEFAULT: u32 = 50;

/// Hard ceiling — the design budget (§15) is `total < 1000`; a single
/// page above this is almost certainly a caller bug.
pub const PORTFOLIO_LIMIT_MAX: u32 = 200;

fn default_limit() -> u32 {
    PORTFOLIO_LIMIT_DEFAULT
}

// ---------------------------------------------------------------------------
// Request envelope (spec §6)
// ---------------------------------------------------------------------------

/// Inputs to the portfolio report.
///
/// Distinct from [`crate::envelope::ReportRequest`] on purpose: the
/// unit of measurement here is a *planned window* (`start_at` →
/// `due_at`), not the `event_actors` stream. See spec §2 for why this
/// is not bolted onto the §15.6 activity envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectPortfolioRequest {
    /// Org filter. Empty ⇒ every org the caller can see (per the auth
    /// layer narrowing — same rule as `GET /projects`).
    #[serde(default)]
    pub orgs: Vec<Uuid>,

    /// Restrict to the listed statuses. Empty ⇒ `[Active, Backlog]`
    /// (the sidebar default; archived projects only surface when the
    /// caller explicitly opts in via [`ProjectStatus::Archived`]).
    #[serde(default)]
    pub statuses: Vec<ProjectStatus>,

    /// Optional planned-window filter. When `Some(_)`, a project is
    /// included iff its `[start_at, due_at]` overlaps the resolved
    /// `(start, end)`. `None` ⇒ no timeline filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowSpec>,

    /// `false` (default) ⇒ include projects whose `due_at` is past
    /// but whose `status` is still `active` (slipping). `true` ⇒ hide
    /// them. Mirrors the sidebar "Hide done" toggle.
    #[serde(default)]
    pub hide_overdue: bool,

    /// Sort key. Default `due_asc_nulls_last`.
    #[serde(default)]
    pub sort: PortfolioSort,

    /// 1-based pagination — same envelope shape as `GET /projects`.
    #[serde(default = "default_limit")]
    pub limit: u32,

    /// Page offset; `0` ⇒ first page.
    #[serde(default)]
    pub offset: u32,
}

impl Default for ProjectPortfolioRequest {
    fn default() -> Self {
        Self {
            orgs: Vec::new(),
            statuses: Vec::new(),
            window: None,
            hide_overdue: false,
            sort: PortfolioSort::default(),
            limit: PORTFOLIO_LIMIT_DEFAULT,
            offset: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// User chip (spec §7)
// ---------------------------------------------------------------------------

/// Minimal user projection for the "Lead" column. Intentionally
/// narrow — full [`dp_domain::user::User`] would force the report to
/// carry email / soft-delete state it doesn't need. Soft-deleted
/// leads are pseudonymised the same way every other surface
/// pseudonymises them (spec §14); that translation happens in the
/// REST layer, not here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserChip {
    /// `dp_users.id`.
    pub id: Uuid,
    /// GitHub login (already pseudonymised if the user is soft-deleted).
    pub login: String,
}

// ---------------------------------------------------------------------------
// Response row (spec §7)
// ---------------------------------------------------------------------------

/// One row per visible project. Locked to the SCOPE-PROJECT-REPORTS
/// §9 metric definitions — REST, MCP, and the frontend must never
/// recompute these locally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectPortfolioRow {
    /// `dp_projects.id`.
    pub id: Uuid,
    /// Owning org (`dp_projects.org_id`).
    pub org_id: Uuid,
    /// Owning org's GitHub login slug.
    pub org_login: String,
    /// Project name.
    pub name: String,
    /// Lifecycle status.
    pub status: ProjectStatus,

    /// Planned start, UTC. `None` ⇒ unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_at: Option<DateTime<Utc>>,

    /// Planned due, UTC. `None` ⇒ unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_at: Option<DateTime<Utc>>,

    /// Total attached issues (`dp_projects.issue_count`).
    pub issue_count: i32,
    /// Closed attached issues (`dp_projects.closed_issue_count`).
    pub closed_issue_count: i32,

    /// `round(closed_issue_count * 100 / issue_count)` when
    /// `issue_count > 0`; otherwise `0`. Matches the §6.3 KPI tile.
    pub progress_pct: i32,

    /// `floor((due_at - now) / 1 day)` in UTC.
    ///
    /// - Positive ⇒ days remaining.
    /// - Negative ⇒ days overdue (status may still be `active`).
    /// - `None`   ⇒ no `due_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slip_days: Option<i32>,

    /// Open issues attached to the project that are overdue per
    /// spec §9: own `due_at < now`, **or** no own `due_at` and the
    /// project's `due_at < now`.
    pub issue_overdue_count: i32,

    /// Lead chip — `None` when `lead_user_id IS NULL`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<UserChip>,

    /// `true` iff there is at least one `dp_project_board_links` row
    /// for this project.
    pub mirrored_to_github: bool,

    /// CAS token — echoed through so a row click can deep-link into
    /// the §6.3 detail page without a fresh `GET /projects/{id}`.
    pub version: i64,
}

// ---------------------------------------------------------------------------
// Rollup KPIs (spec §7 + §9)
// ---------------------------------------------------------------------------

/// Portfolio-level rollups, computed across the **visible rows**
/// (not `total`), so the figures are honest about the current page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortfolioKpis {
    /// `rows.len()` — same as the page size when full.
    pub total_projects: i32,

    /// `due_at IS NULL OR due_at >= now` (or `status = done`).
    pub on_track: i32,

    /// `status IN (active, backlog) AND due_at < now`.
    pub overdue: i32,

    /// `status = done`.
    pub completed: i32,

    /// Average integer percent across rows with `issue_count > 0`;
    /// `0` when every visible row is empty.
    pub avg_progress_pct: i32,

    /// Sum of `issue_count - closed_issue_count` across visible rows.
    pub total_issues_open: i32,

    /// Sum of `issue_overdue_count` across visible rows.
    pub total_issues_overdue: i32,
}

impl Default for PortfolioKpis {
    fn default() -> Self {
        Self {
            total_projects: 0,
            on_track: 0,
            overdue: 0,
            completed: 0,
            avg_progress_pct: 0,
            total_issues_open: 0,
            total_issues_overdue: 0,
        }
    }
}

/// Compute [`PortfolioKpis`] from a slice of rows and the resolved
/// `now`. Centralised so the REST handler, MCP tool, and any future
/// caller never roll their own.
pub fn rollup_kpis(rows: &[ProjectPortfolioRow], now: DateTime<Utc>) -> PortfolioKpis {
    let mut kpis = PortfolioKpis::default();
    let mut progress_sum: i64 = 0;
    let mut progress_n: i32 = 0;

    for row in rows {
        kpis.total_projects += 1;

        match (row.status, row.due_at) {
            (ProjectStatus::Done, _) => kpis.completed += 1,
            (ProjectStatus::Active | ProjectStatus::Backlog, Some(due)) if due < now => {
                kpis.overdue += 1;
            }
            _ => kpis.on_track += 1,
        }

        if row.issue_count > 0 {
            progress_sum += i64::from(row.progress_pct);
            progress_n += 1;
        }

        kpis.total_issues_open += row.issue_count - row.closed_issue_count;
        kpis.total_issues_overdue += row.issue_overdue_count;
    }

    kpis.avg_progress_pct = if progress_n > 0 {
        i32::try_from(progress_sum / i64::from(progress_n)).unwrap_or(0)
    } else {
        0
    };

    kpis
}

// ---------------------------------------------------------------------------
// Response envelope (spec §7)
// ---------------------------------------------------------------------------

/// Outputs from the portfolio report. Mirrors the request paging
/// shape and echoes the resolved `(start, end)` + `now` so callers
/// never have to derive them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectPortfolioResponse {
    /// One row per visible project, page-bounded.
    pub rows: Vec<ProjectPortfolioRow>,

    /// Echoed resolved window, per §0.4. `None` when the request
    /// omitted `window`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_window: Option<Window>,

    /// Server-resolved `now` used by every `*_days` field.
    pub now: DateTime<Utc>,

    /// Total matching rows across pages — same envelope shape as
    /// `GET /projects`.
    pub total: u32,
    /// Echoed page size from the request.
    pub limit: u32,
    /// Echoed page offset from the request.
    pub offset: u32,

    /// Rollups computed across the visible rows (not `total`).
    pub kpis: PortfolioKpis,
}

// ---------------------------------------------------------------------------
// Raw-row → wire-row mapping (spec §7)
// ---------------------------------------------------------------------------

impl From<PortfolioRawRow> for ProjectPortfolioRow {
    fn from(raw: PortfolioRawRow) -> Self {
        Self {
            id: raw.id,
            org_id: raw.org_id,
            org_login: raw.org_login,
            name: raw.name,
            status: raw.status,
            start_at: raw.start_at,
            due_at: raw.due_at,
            issue_count: raw.issue_count,
            closed_issue_count: raw.closed_issue_count,
            progress_pct: raw.progress_pct,
            slip_days: raw.slip_days,
            issue_overdue_count: raw.issue_overdue_count,
            lead: raw.lead.map(|(id, login)| UserChip { id, login }),
            mirrored_to_github: raw.mirrored_to_github,
            version: raw.version,
        }
    }
}

// ---------------------------------------------------------------------------
// SQL builder (spec §10)
// ---------------------------------------------------------------------------

/// Parameter bind order for [`build_project_portfolio_sql`].
///
/// Unified across every sort variant so the store adapter binds the
/// same way regardless of the chosen ordering. The status / org / window
/// filters all use the `cardinality($n::T[]) = 0 OR col = ANY($n)`
/// pattern so a single SQL string handles "all" and "filtered" without
/// branching. The status array is `text[]` because `dp_projects.status`
/// is a `TEXT` column with a `CHECK (status IN (...))` constraint —
/// NOT a Postgres enum (per 0022_projects.sql).
pub const PROJECT_PORTFOLIO_BIND_ORDER: &[&str] = &[
    "$1 orgs (uuid[]; cardinality 0 == no filter)",
    "$2 statuses (text[]; cardinality 0 == default to ['active','backlog'] resolved caller-side)",
    "$3 window_start (timestamptz, nullable; NULL == no window filter)",
    "$4 window_end (timestamptz exclusive, nullable; NULL == no window filter)",
    "$5 hide_overdue (bool; true ⇒ drop status IN (active,backlog) AND due_at < now)",
    "$6 now (timestamptz; used for slip_days, issue_overdue_count, hide_overdue)",
    "$7 limit (int)",
    "$8 offset (int)",
];

/// `ORDER BY` clause for a given sort. Whitelisted constants — no
/// user input is ever concatenated.
const fn order_by_clause(sort: PortfolioSort) -> &'static str {
    match sort {
        PortfolioSort::DueAscNullsLast => "ORDER BY p.due_at ASC NULLS LAST, p.name ASC",
        PortfolioSort::DueDescNullsLast => "ORDER BY p.due_at DESC NULLS LAST, p.name ASC",
        // Most overdue (largest negative slip) first; NULL due last.
        PortfolioSort::SlipDaysDesc => {
            "ORDER BY (p.due_at IS NULL) ASC, p.due_at ASC, p.name ASC"
        }
        PortfolioSort::ProgressAsc => {
            "ORDER BY (CASE WHEN p.issue_count = 0 THEN 0 \
                            ELSE (p.closed_issue_count * 100 / p.issue_count) END) ASC, \
                      p.name ASC"
        }
        PortfolioSort::NameAsc => "ORDER BY p.name ASC",
        PortfolioSort::UpdatedDesc => "ORDER BY p.updated_at DESC, p.name ASC",
    }
}

/// Build the portfolio SQL for a given sort.
///
/// The returned string assumes [`PROJECT_PORTFOLIO_BIND_ORDER`]. One
/// round trip: a CTE materialises the filtered project set; the outer
/// select projects the row shape plus `COUNT(*) OVER ()` so the store
/// learns `total` without a second query. `issue_overdue_count` and
/// `mirrored_to_github` are computed via correlated subqueries — the
/// dominant scan path on `dp_projects (org_id, status, due_at)` keeps
/// the planner honest at the v1 design budget (`total < 1000`, spec
/// §15.2).
///
/// The status filter does *not* default to `['active','backlog']` here
/// — the caller (REST/MCP) is responsible for applying that default
/// before binding, because the empty-array sentinel ("no filter")
/// already means "all statuses". Keeping that decision in one place
/// (the handler) prevents a silent disagreement between SQL behaviour
/// and the documented sidebar default.
pub fn build_project_portfolio_sql(sort: PortfolioSort) -> String {
    let order_by = order_by_clause(sort);
    format!(
        "WITH filtered AS ( \
             SELECT p.id, p.org_id, p.name, p.status, p.start_at, p.due_at, \
                    p.issue_count, p.closed_issue_count, p.lead_user_id, \
                    p.version, p.updated_at \
               FROM dp_projects p \
              WHERE (cardinality($1::uuid[]) = 0 OR p.org_id = ANY($1)) \
                AND (cardinality($2::text[]) = 0 OR p.status = ANY($2)) \
                AND ($3::timestamptz IS NULL OR p.start_at IS NULL OR p.start_at < $4) \
                AND ($4::timestamptz IS NULL OR p.due_at   IS NULL OR p.due_at   >= $3) \
                AND (NOT $5 OR NOT (p.status IN ('active','backlog') AND p.due_at < $6)) \
         ) \
         SELECT p.id, \
                p.org_id, \
                o.login                            AS org_login, \
                p.name, \
                p.status, \
                p.start_at, \
                p.due_at, \
                p.issue_count, \
                p.closed_issue_count, \
                CASE WHEN p.issue_count = 0 THEN 0 \
                     ELSE (p.closed_issue_count * 100 / p.issue_count) END  AS progress_pct, \
                CASE WHEN p.due_at IS NULL THEN NULL \
                     ELSE FLOOR(EXTRACT(EPOCH FROM (p.due_at - $6)) / 86400.0)::int \
                END                                AS slip_days, \
                ( \
                    SELECT COUNT(*)::int \
                      FROM dp_project_issues pi \
                      JOIN dp_issues i ON i.id = pi.issue_id \
                 LEFT JOIN dp_issue_dates idt ON idt.issue_id = i.id \
                     WHERE pi.project_id = p.id \
                       AND i.state = 'open' \
                       AND ( \
                              idt.due_at < $6 \
                           OR (idt.due_at IS NULL AND p.due_at IS NOT NULL AND p.due_at < $6) \
                       ) \
                )                                  AS issue_overdue_count, \
                u.id                               AS lead_user_id, \
                u.login                            AS lead_login, \
                EXISTS ( \
                    SELECT 1 FROM dp_project_board_links bl \
                     WHERE bl.project_id = p.id \
                )                                  AS mirrored_to_github, \
                p.version, \
                COUNT(*) OVER ()                   AS total \
           FROM filtered p \
           JOIN dp_orgs o ON o.id = p.org_id \
      LEFT JOIN dp_users u ON u.id = p.lead_user_id \
          {order_by} \
          LIMIT $7 OFFSET $8"
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 0, 0, 0).single().unwrap()
    }

    fn sample_row(status: ProjectStatus, due: Option<DateTime<Utc>>) -> ProjectPortfolioRow {
        ProjectPortfolioRow {
            id: Uuid::nil(),
            org_id: Uuid::nil(),
            org_login: "acme".into(),
            name: "p".into(),
            status,
            start_at: None,
            due_at: due,
            issue_count: 10,
            closed_issue_count: 4,
            progress_pct: 40,
            slip_days: None,
            issue_overdue_count: 0,
            lead: None,
            mirrored_to_github: false,
            version: 1,
        }
    }

    #[test]
    fn request_round_trips_through_json_with_defaults() {
        let req = ProjectPortfolioRequest::default();
        let json = serde_json::to_string(&req).unwrap();
        let back: ProjectPortfolioRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn sort_serialises_in_snake_case() {
        let s = PortfolioSort::DueAscNullsLast;
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(j, "\"due_asc_nulls_last\"");
    }

    #[test]
    fn rollup_bucketing_is_mutually_exclusive() {
        let now = utc(2026, 5, 21);
        let rows = vec![
            sample_row(ProjectStatus::Done, Some(utc(2026, 5, 1))), // completed
            sample_row(ProjectStatus::Active, Some(utc(2026, 5, 1))), // overdue
            sample_row(ProjectStatus::Active, Some(utc(2026, 6, 1))), // on_track
            sample_row(ProjectStatus::Backlog, None),                // on_track (NULL due)
        ];
        let kpis = rollup_kpis(&rows, now);
        assert_eq!(kpis.total_projects, 4);
        assert_eq!(kpis.completed, 1);
        assert_eq!(kpis.overdue, 1);
        assert_eq!(kpis.on_track, 2);
        assert_eq!(
            kpis.completed + kpis.overdue + kpis.on_track,
            kpis.total_projects,
            "buckets must partition total_projects",
        );
    }

    #[test]
    fn rollup_avg_progress_ignores_zero_issue_projects() {
        let now = utc(2026, 5, 21);
        let mut empty = sample_row(ProjectStatus::Active, None);
        empty.issue_count = 0;
        empty.closed_issue_count = 0;
        empty.progress_pct = 0;
        let mut filled = sample_row(ProjectStatus::Active, None);
        filled.progress_pct = 80;
        let rows = vec![empty, filled];
        let kpis = rollup_kpis(&rows, now);
        assert_eq!(kpis.avg_progress_pct, 80, "empty project must not pull avg down");
    }

    #[test]
    fn rollup_sums_open_and_overdue_issues() {
        let now = utc(2026, 5, 21);
        let mut a = sample_row(ProjectStatus::Active, None);
        a.issue_count = 10;
        a.closed_issue_count = 3;
        a.issue_overdue_count = 2;
        let mut b = sample_row(ProjectStatus::Active, None);
        b.issue_count = 5;
        b.closed_issue_count = 5;
        b.issue_overdue_count = 0;
        let kpis = rollup_kpis(&[a, b], now);
        assert_eq!(kpis.total_issues_open, 7);
        assert_eq!(kpis.total_issues_overdue, 2);
    }

    #[test]
    fn empty_rows_produces_zeroed_kpis() {
        let kpis = rollup_kpis(&[], utc(2026, 5, 21));
        assert_eq!(kpis, PortfolioKpis::default());
    }

    // -----------------------------------------------------------------
    // SQL builder
    // -----------------------------------------------------------------

    #[test]
    fn bind_order_has_eight_slots() {
        assert_eq!(PROJECT_PORTFOLIO_BIND_ORDER.len(), 8);
    }

    #[test]
    fn sql_contains_every_param_placeholder() {
        let sql = build_project_portfolio_sql(PortfolioSort::DueAscNullsLast);
        for n in 1..=8 {
            let placeholder = format!("${n}");
            assert!(
                sql.contains(&placeholder),
                "missing param placeholder {placeholder} in SQL: {sql}",
            );
        }
    }

    #[test]
    fn sql_joins_all_required_tables() {
        let sql = build_project_portfolio_sql(PortfolioSort::NameAsc);
        for table in [
            "dp_projects",
            "dp_orgs",
            "dp_users",
            "dp_project_issues",
            "dp_issues",
            "dp_issue_dates",
            "dp_project_board_links",
        ] {
            assert!(sql.contains(table), "missing table {table} in SQL");
        }
    }

    #[test]
    fn sql_emits_count_over_for_total() {
        let sql = build_project_portfolio_sql(PortfolioSort::DueAscNullsLast);
        assert!(
            sql.contains("COUNT(*) OVER ()"),
            "total must come from window function so we do one round trip: {sql}",
        );
    }

    #[test]
    fn sort_variants_emit_distinct_order_by() {
        let variants = [
            PortfolioSort::DueAscNullsLast,
            PortfolioSort::DueDescNullsLast,
            PortfolioSort::SlipDaysDesc,
            PortfolioSort::ProgressAsc,
            PortfolioSort::NameAsc,
            PortfolioSort::UpdatedDesc,
        ];
        let mut clauses: Vec<&str> = variants.iter().map(|s| order_by_clause(*s)).collect();
        clauses.sort_unstable();
        clauses.dedup();
        assert_eq!(clauses.len(), variants.len(), "all sort variants must be distinct");
    }

    #[test]
    fn issue_overdue_subquery_matches_spec_section_9() {
        let sql = build_project_portfolio_sql(PortfolioSort::DueAscNullsLast);
        // §9 predicate: own due_at < now OR (own due_at IS NULL AND project due_at < now).
        assert!(sql.contains("i.state = 'open'"), "open-state filter required");
        assert!(
            sql.contains("idt.due_at IS NULL AND p.due_at IS NOT NULL AND p.due_at < $6"),
            "fall-through to project due_at required (spec §9)",
        );
    }

    #[test]
    fn status_filter_uses_text_array() {
        // dp_projects.status is TEXT with a CHECK, not a pg enum (per 0022_projects.sql).
        let sql = build_project_portfolio_sql(PortfolioSort::DueAscNullsLast);
        assert!(sql.contains("$2::text[]"), "status must bind as text[], got: {sql}");
    }

    #[test]
    fn no_user_input_reaches_order_by() {
        // Every variant of PortfolioSort maps to a constant string.
        // This is the security invariant: no string formatting of
        // arbitrary input ever lands in ORDER BY.
        let sort: PortfolioSort = serde_json::from_str("\"name_asc\"").unwrap();
        let clause = order_by_clause(sort);
        assert!(clause.starts_with("ORDER BY"));
    }
}
