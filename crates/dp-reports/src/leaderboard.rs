//! Leaderboard report kind — scaffold (ORG-REPORTS.md §1–§6).
//!
//! Stage 3 lays down the type surface and a thin SQL builder for
//! `subject = user` in single-org mode only. Pagination
//! (ORG-REPORTS §6.5), `also_compute` (§6.3), `subject_ids` (§6.10),
//! the reconciliation footer (§6.2), and the `my_standing` companion
//! endpoint (§6.9) land in later stages.
//!
//! ## Surfaces this module is shared across
//!
//! REST (Phase 4), MCP (Phase 5), and the frontend wiring all consume
//! the same [`LeaderboardEnvelope`] / [`LeaderboardResponse`] pair so
//! they cannot diverge — a SCOPE.md §11.4 trust requirement. Adding a
//! field for one surface means adding it here first.
//!
//! ## Boundary
//!
//! Pure types + pure SQL string builder. No `sqlx`, no `dp-store-pg`
//! import — the store layer interpolates the SQL emitted by
//! [`build_user_single_org_sql`] and binds the parameter list
//! documented on that function. Keeps the §15.7 metric layer reusable
//! and unit-testable without a live database (STAGE-1-COMPOSABILITY).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use dp_domain::event::ActorRole;
use dp_domain::window::Window;

use crate::aggregate::CountMetric;
use crate::envelope::{resolve_window_at, ResolveError, ScopeMode, WindowSpec};

// ---------------------------------------------------------------------------
// Subject axis (ORG-REPORTS §2)
// ---------------------------------------------------------------------------

/// What a leaderboard row represents.
///
/// Orthogonal to the time / org / repo / team dimensions already in
/// SCOPE.md §8. Stage 3 only wires [`SubjectKind::User`]; the other
/// three variants land in stage 4 with the full
/// [`ScopeMode`] fan-out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectKind {
    /// One row per `users.id`.
    User,
    /// One row per `teams.id` (org-scoped by definition).
    Team,
    /// One row per `orgs.id`.
    Org,
    /// One row per distinct `users.home_org_label` value, with NULL
    /// pooled into the synthetic `__unlabeled__` bucket (§6.8).
    HomeOrgLabel,
}

// ---------------------------------------------------------------------------
// Metric identity (ORG-REPORTS §3)
// ---------------------------------------------------------------------------

/// A reference to exactly one row of the SCOPE.md §15.7 metric table.
///
/// Wraps the existing [`CountMetric`] enum and reserves shape for
/// `DurationMetric` once the store-side `list_duration_samples_in_window`
/// fetch exists (STAGE-1-COMPOSABILITY §3 — flagged as a Phase-3
/// follow-up, not a leaderboard blocker). The wire form is internally
/// tagged so adding the duration variant is non-breaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "family", content = "id")]
pub enum MetricId {
    /// One of the §15.7 count metrics.
    Count(CountMetric),
    // Duration(DurationMetric) — added once the store fetch lands.
}

// ---------------------------------------------------------------------------
// Request envelope (ORG-REPORTS §3)
// ---------------------------------------------------------------------------

/// Default for `include_bots` — false, per ORG-REPORTS §6.4.
fn default_include_bots() -> bool {
    false
}

/// Inputs to the leaderboard endpoint.
///
/// Mirrors ORG-REPORTS §3 minus the not-yet-wired stage 6/7/8 fields
/// (`also_compute`, `subject_ids`, `page`). Those are scaffolded into
/// dedicated stages so the wire form grows additively — no field
/// added here changes meaning when those stages land.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaderboardEnvelope {
    /// Window spec, resolved server-side via [`resolve_window_at`].
    pub window: WindowSpec,
    /// Org-scope lens (SCOPE §8.1). Stage 3 only honours
    /// [`ScopeMode::SingleOrg`].
    pub scope_mode: ScopeMode,
    /// Orgs in scope. Empty means "all orgs the principal can see"
    /// (the auth layer narrows the set in Phase 4). Single-org mode
    /// expects exactly one entry; stage 4 broadens this.
    #[serde(default)]
    pub orgs: Vec<Uuid>,
    /// Repo filter. Empty == no filter.
    #[serde(default)]
    pub repos: Vec<Uuid>,
    /// Team filter. Empty == no filter.
    #[serde(default)]
    pub teams: Vec<Uuid>,
    /// `actor_roles` override (SCOPE §15.7). Empty == use the
    /// `rank_by` metric's default-role set from
    /// [`CountMetric::default_actor_roles`].
    #[serde(default)]
    pub actor_roles: Vec<ActorRole>,
    /// Subject axis (§2). Stage 3 only honours [`SubjectKind::User`].
    pub subject: SubjectKind,
    /// The one §15.7 metric used to sort + paginate.
    pub rank_by: MetricId,
    /// Bot suppression (§6.4). Defaults `false`.
    #[serde(default = "default_include_bots")]
    pub include_bots: bool,
}

// ---------------------------------------------------------------------------
// Response shape (ORG-REPORTS §4)
// ---------------------------------------------------------------------------

/// Echo of the resolved request that travels back on every response.
///
/// `resolved_at` + `resolved_window` together pin "identical input +
/// identical resolved_at must produce identical output" (ORG-REPORTS
/// §4 / §6.5). The §6.5 stable cursor is derived from these two
/// fields plus `(rank_by_value, subject_id)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedLeaderboardEnvelope {
    /// Wall-clock instant the server resolved this request at.
    pub resolved_at: DateTime<Utc>,
    /// Resolved window (`[start, end)` in UTC, plus label/tz/anchor
    /// echo).
    pub resolved_window: Window,
    /// Org-scope lens echoed back.
    pub scope_mode: ScopeMode,
    /// Subject axis echoed back.
    pub subject: SubjectKind,
    /// `rank_by` echoed back.
    pub rank_by: MetricId,
}

/// Headline counters above the table (ORG-REPORTS §4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaderboardHeadline {
    /// Number of subjects with at least one event in the resolved
    /// window after lensing + bot filter.
    pub total_subjects: u64,
    /// Total events the headline accounts for. Used by the §6.2
    /// reconciliation identity wired in stage 5.
    pub events_total: u64,
}

/// The metric + value carried by every row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaderboardPrimary {
    /// Which §15.7 metric — echoes `envelope.rank_by`.
    pub metric: MetricId,
    /// Count for count metrics. (Duration metrics will switch to a
    /// `Percentiles` triple once that fetch lands; tagged via
    /// [`MetricId`] so the wire form is unambiguous.)
    pub value: i64,
}

/// One ranked row.
///
/// `subject_org` is `Some` **only** in `per_org_split` mode (§5); it
/// is omitted from the wire form in single-org and all-orgs-combined
/// to make accidental misuse loud rather than silent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaderboardRow {
    /// 1-indexed rank within the visible page, per §6.1 tie-break.
    pub rank: u32,
    /// Subject identifier. UUID for `user`/`team`/`org`; opaque
    /// string (e.g. `"__unlabeled__"`) for `home_org_label`.
    pub subject_id: String,
    /// Subject axis echoed back per row (so a heterogeneous client
    /// cache can key on it).
    pub subject_kind: SubjectKind,
    /// Human-friendly label (login, team slug, org name, home-org
    /// label, …).
    pub subject_label: String,
    /// `(subject, org)` disambiguator — populated **only** in
    /// `per_org_split` mode (§5). Frontend must visually group rows
    /// sharing a `subject_id` under their `subject_org`s or §8.1's
    /// "spread thin" insight is lost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_org: Option<Uuid>,
    /// Primary metric value (`envelope.rank_by`).
    pub primary: LeaderboardPrimary,
    /// §15.7 metadata + future `also_compute` payload.
    pub context: LeaderboardContext,
    /// Per-bucket counts (SCOPE §15.8 trend bucket). Empty in stage 3
    /// — wired in stage 4 once `truncate_to_bucket` is plumbed.
    #[serde(default)]
    pub sparkline: Vec<i64>,
    /// How many distinct orgs this subject appeared in within the
    /// resolved window. `1` in single-org mode by construction; only
    /// interesting in cross-org modes (§6.1).
    pub active_orgs: u32,
}

/// §15.7 metadata always present + `also_compute` payload added in
/// stage 7.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LeaderboardContext {
    /// Distinct days (in the resolved window's TZ) the subject was
    /// active on. Drives the §6.1 secondary sort key.
    pub active_days: u32,
    /// Distinct repos the subject touched in the resolved window.
    pub repos_touched: u32,
    /// Additional §15.7 metrics requested via `also_compute`
    /// (stage 7). Empty until stage 7 lands; omitted from the wire
    /// form when empty so the stage-3 shape stays stable.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extras: serde_json::Map<String, serde_json::Value>,
}

/// Footer counters (ORG-REPORTS §4).
///
/// Stage 3 zeroes every field; stage 5 wires the §6.2 reconciliation
/// identity and the §6.4 bot split, stage 6 wires `insufficient_data`
/// for duration metrics. The wire shape is locked here so later
/// stages don't reshape the response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaderboardFooter {
    /// Total unattributed events in the resolved window (matches the
    /// headline report). §6.2.
    pub unattributed_events: u64,
    /// Unattributed events that would have contributed to `rank_by`
    /// if attributed. §6.2.
    pub unattributed_events_metric: u64,
    /// Subjects below the §15.9 sufficiency threshold for the chosen
    /// duration metric. Always 0 for count metrics. §6.6.
    pub insufficient_data: u64,
    /// Bot subjects hidden from `rows`. §6.4.
    pub bots_suppressed: u64,
    /// Events those bots contributed (needed by the §6.2
    /// reconciliation identity). §6.4.
    pub bots_suppressed_events: u64,
}

/// The full response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaderboardResponse {
    /// Resolved request echo.
    pub envelope: ResolvedLeaderboardEnvelope,
    /// Above-the-table counters.
    pub headline: LeaderboardHeadline,
    /// Ranked rows (1-indexed `rank` per §6.1).
    pub rows: Vec<LeaderboardRow>,
    /// Reconciliation + bot + sufficiency footer.
    pub footer: LeaderboardFooter,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Failure modes the scaffold rejects today.
///
/// Other stages add `CursorWindowMismatch` (§6.5), `SubjectIdsTooLarge`
/// (§6.10), and the per-metric reconciliation-violation assertion
/// (§6.2 debug build). They share this error enum so the REST and MCP
/// surfaces map every leaderboard failure through one match.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum LeaderboardError {
    /// Subject axis not yet wired in this stage. Lifts in stage 4.
    #[error("leaderboard subject={0:?} not yet implemented (stage 3 only honours subject=user)")]
    SubjectNotYetWired(SubjectKind),
    /// Scope mode not yet wired in this stage. Lifts in stage 4.
    #[error("leaderboard scope_mode={0:?} not yet implemented (stage 3 only honours single_org)")]
    ScopeModeNotYetWired(ScopeMode),
    /// `orgs` must contain exactly one id in single-org mode.
    #[error("single_org scope requires exactly one org id, got {0}")]
    SingleOrgRequiresOneOrg(usize),
    /// Window spec failed to resolve.
    #[error(transparent)]
    Resolve(#[from] ResolveError),
}

// ---------------------------------------------------------------------------
// Envelope resolution
// ---------------------------------------------------------------------------

/// Resolve a [`LeaderboardEnvelope`] at `now`, returning the
/// [`ResolvedLeaderboardEnvelope`] echoed in the response.
///
/// Stage 3 enforces the `subject = user` + `scope_mode = single_org`
/// gate so unwired combinations fail fast with a typed error instead
/// of silently producing the user-single-org SQL for a different
/// subject. Stage 4 widens this check.
pub fn resolve_leaderboard_envelope(
    env: &LeaderboardEnvelope,
    now: DateTime<Utc>,
) -> Result<ResolvedLeaderboardEnvelope, LeaderboardError> {
    if env.subject != SubjectKind::User {
        return Err(LeaderboardError::SubjectNotYetWired(env.subject));
    }
    if env.scope_mode != ScopeMode::SingleOrg {
        return Err(LeaderboardError::ScopeModeNotYetWired(env.scope_mode));
    }
    if env.orgs.len() != 1 {
        return Err(LeaderboardError::SingleOrgRequiresOneOrg(env.orgs.len()));
    }
    let resolved_window = resolve_window_at(&env.window, now)?;
    Ok(ResolvedLeaderboardEnvelope {
        resolved_at: now,
        resolved_window,
        scope_mode: env.scope_mode,
        subject: env.subject,
        rank_by: env.rank_by,
    })
}

// ---------------------------------------------------------------------------
// Thin SQL builder — subject=user, single-org (stage 3)
// ---------------------------------------------------------------------------

/// Parameter bind order for [`build_user_single_org_sql`].
///
/// Documented as a const so the `dp-store-pg` adapter and any
/// integration test bind in the same order — drift here is exactly
/// the §11.4 divergence trap.
pub const USER_SINGLE_ORG_BIND_ORDER: &[&str] = &[
    "$1 window.start (timestamptz)",
    "$2 window.end (timestamptz, exclusive)",
    "$3 org_id (uuid)",
    "$4 event_kind (text — from CountMetric::event_kind())",
    "$5 actor_roles (text[] — from envelope.actor_roles or CountMetric::default_actor_roles())",
    "$6 repos (uuid[]; cardinality 0 == no filter)",
];

/// SQL for the `subject = user` single-org leaderboard.
///
/// Emits the per-user aggregate columns the response shape needs:
///
/// * `subject_id`     — `ea.user_id`
/// * `primary_value`  — `count(*)` for the count-metric path
/// * `active_days`    — distinct UTC days the user was active on
/// * `repos_touched`  — distinct repos the user touched
/// * `active_orgs`    — always 1 in single-org mode, but the column
///   is selected so the row-mapper can be shared with stage 4's
///   `all_orgs_combined` SQL without a second mapper
///
/// The §6.1 tie-break (`rank_by DESC → active_days DESC →
/// subject_id ASC`) is applied in the `ORDER BY` so callers don't
/// reinvent it. `active_days` truncation is in UTC here; stage 4
/// re-truncates to the window's anchor TZ for the sparkline path.
///
/// No `LIMIT` / `OFFSET` — pagination lands in stage 6, where the
/// builder gains a cursor-derived `WHERE (primary_value, subject_id)
/// < ($cursor)` predicate.
///
/// Bot suppression is **not** in this SQL — the store applies it as a
/// row-level filter after fetch so the §6.4 `bots_suppressed_events`
/// counter is computable without a second query.
pub fn build_user_single_org_sql() -> &'static str {
    "SELECT ea.user_id                                       AS subject_id, \
            count(*)::bigint                                 AS primary_value, \
            count(DISTINCT date_trunc('day', e.ts))::bigint  AS active_days, \
            count(DISTINCT e.repo_id)::bigint                AS repos_touched, \
            1::bigint                                        AS active_orgs \
       FROM dp_event_actors ea \
       JOIN dp_activity_events e ON e.id = ea.event_id \
      WHERE e.ts   >= $1 \
        AND e.ts   <  $2 \
        AND e.org_id = $3 \
        AND e.kind   = $4 \
        AND ea.role  = ANY($5) \
        AND (cardinality($6::uuid[]) = 0 OR e.repo_id = ANY($6)) \
      GROUP BY ea.user_id \
      ORDER BY primary_value DESC, active_days DESC, subject_id ASC"
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::WindowSpec;
    use chrono::TimeZone;
    use dp_domain::event::ActorRole;
    use dp_domain::window::WindowAnchor;

    fn utc(y: i32, m: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, mi, s).single().unwrap()
    }

    fn sample_envelope() -> LeaderboardEnvelope {
        LeaderboardEnvelope {
            window: WindowSpec {
                label: crate::envelope::WindowLabel::LastWeek,
                tz: "UTC".into(),
                anchor: WindowAnchor::Utc,
                custom_start: None,
                custom_end: None,
            },
            scope_mode: ScopeMode::SingleOrg,
            orgs: vec![Uuid::nil()],
            repos: vec![],
            teams: vec![],
            actor_roles: vec![ActorRole::Author],
            subject: SubjectKind::User,
            rank_by: MetricId::Count(CountMetric::PullRequestsOpened),
            include_bots: false,
        }
    }

    #[test]
    fn envelope_round_trips_through_json() {
        let env = sample_envelope();
        let json = serde_json::to_string(&env).unwrap();
        let back: LeaderboardEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(env, back);
    }

    #[test]
    fn metric_id_wire_form_is_tagged() {
        // Internally-tagged so adding `MetricId::Duration(...)` later
        // is non-breaking — the existing `count` variant keeps its
        // exact wire shape.
        let m = MetricId::Count(CountMetric::PullRequestsMerged);
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"family\":\"count\""), "{json}");
        assert!(json.contains("\"id\":\"pull_requests_merged\""), "{json}");
    }

    #[test]
    fn subject_kind_uses_snake_case_wire_form() {
        assert_eq!(
            serde_json::to_string(&SubjectKind::HomeOrgLabel).unwrap(),
            "\"home_org_label\""
        );
    }

    #[test]
    fn include_bots_defaults_to_false_when_absent() {
        // ORG-REPORTS §6.4 — bot suppression is the default.
        let json = r#"{
            "window": { "label": "today", "tz": "UTC", "anchor": "utc" },
            "scope_mode": "single_org",
            "orgs": ["00000000-0000-0000-0000-000000000000"],
            "subject": "user",
            "rank_by": { "family": "count", "id": "pull_requests_opened" }
        }"#;
        let env: LeaderboardEnvelope = serde_json::from_str(json).unwrap();
        assert!(!env.include_bots);
    }

    #[test]
    fn resolve_rejects_non_user_subject() {
        let mut env = sample_envelope();
        env.subject = SubjectKind::Team;
        let err = resolve_leaderboard_envelope(&env, utc(2025, 6, 15, 12, 0, 0)).unwrap_err();
        assert_eq!(err, LeaderboardError::SubjectNotYetWired(SubjectKind::Team));
    }

    #[test]
    fn resolve_rejects_non_single_org_scope() {
        let mut env = sample_envelope();
        env.scope_mode = ScopeMode::AllOrgsCombined;
        let err = resolve_leaderboard_envelope(&env, utc(2025, 6, 15, 12, 0, 0)).unwrap_err();
        assert_eq!(
            err,
            LeaderboardError::ScopeModeNotYetWired(ScopeMode::AllOrgsCombined)
        );
    }

    #[test]
    fn resolve_rejects_zero_or_many_orgs_in_single_org_mode() {
        let mut env = sample_envelope();
        env.orgs = vec![];
        assert_eq!(
            resolve_leaderboard_envelope(&env, utc(2025, 6, 15, 12, 0, 0)).unwrap_err(),
            LeaderboardError::SingleOrgRequiresOneOrg(0),
        );
        env.orgs = vec![Uuid::nil(), Uuid::nil()];
        assert_eq!(
            resolve_leaderboard_envelope(&env, utc(2025, 6, 15, 12, 0, 0)).unwrap_err(),
            LeaderboardError::SingleOrgRequiresOneOrg(2),
        );
    }

    #[test]
    fn resolve_echoes_resolved_at_and_window() {
        let env = sample_envelope();
        let now = utc(2025, 6, 18, 12, 0, 0); // Wed in UTC
        let r = resolve_leaderboard_envelope(&env, now).unwrap();
        assert_eq!(r.resolved_at, now);
        // last_week in UTC at Wed = Mon..Mon of the prior week.
        assert_eq!(r.resolved_window.start, utc(2025, 6, 9, 0, 0, 0));
        assert_eq!(r.resolved_window.end, utc(2025, 6, 16, 0, 0, 0));
        assert_eq!(r.subject, SubjectKind::User);
        assert_eq!(r.scope_mode, ScopeMode::SingleOrg);
    }

    #[test]
    fn user_single_org_sql_carries_tie_break_order() {
        // §6.1: rank_by DESC → active_days DESC → subject_id ASC.
        let sql = build_user_single_org_sql();
        let idx_pv = sql.find("primary_value DESC").expect("primary_value DESC missing");
        let idx_ad = sql.find("active_days DESC").expect("active_days DESC missing");
        let idx_sid = sql.find("subject_id ASC").expect("subject_id ASC missing");
        assert!(idx_pv < idx_ad && idx_ad < idx_sid, "tie-break order: {sql}");
    }

    #[test]
    fn user_single_org_sql_projects_the_expected_columns() {
        let sql = build_user_single_org_sql();
        for col in [
            "subject_id",
            "primary_value",
            "active_days",
            "repos_touched",
            "active_orgs",
        ] {
            assert!(sql.contains(col), "missing column {col} in: {sql}");
        }
    }

    #[test]
    fn user_single_org_sql_has_no_limit_or_offset() {
        // Stage 3 scope: no pagination yet. Stage 6 introduces a
        // cursor predicate, not LIMIT/OFFSET — keep this guard so the
        // change is visible.
        let sql = build_user_single_org_sql();
        let upper = sql.to_ascii_uppercase();
        assert!(!upper.contains(" LIMIT "), "{sql}");
        assert!(!upper.contains(" OFFSET "), "{sql}");
    }

    #[test]
    fn user_single_org_bind_order_is_six_params() {
        assert_eq!(USER_SINGLE_ORG_BIND_ORDER.len(), 6);
        let sql = build_user_single_org_sql();
        for i in 1..=6 {
            assert!(sql.contains(&format!("${i}")), "missing ${i} in: {sql}");
        }
    }

    #[test]
    fn response_serialises_subject_org_only_when_present() {
        // Per ORG-REPORTS §5: `subject_org` is populated only in
        // `per_org_split`. The wire form omits it everywhere else so
        // accidental misuse is loud.
        let row_without = LeaderboardRow {
            rank: 1,
            subject_id: "u".into(),
            subject_kind: SubjectKind::User,
            subject_label: "alice".into(),
            subject_org: None,
            primary: LeaderboardPrimary {
                metric: MetricId::Count(CountMetric::PullRequestsOpened),
                value: 7,
            },
            context: LeaderboardContext::default(),
            sparkline: vec![],
            active_orgs: 1,
        };
        let json = serde_json::to_string(&row_without).unwrap();
        assert!(!json.contains("subject_org"), "{json}");

        let mut row_with = row_without.clone();
        row_with.subject_org = Some(Uuid::nil());
        let json = serde_json::to_string(&row_with).unwrap();
        assert!(json.contains("subject_org"), "{json}");
    }

    #[test]
    fn footer_zeroes_serialise_explicitly() {
        // Footer fields are part of the §6.2 contract; even when
        // zero they must appear on the wire so REST/MCP/frontend
        // can't conditionally branch on field presence.
        let f = LeaderboardFooter::default();
        let json = serde_json::to_string(&f).unwrap();
        for k in [
            "unattributed_events",
            "unattributed_events_metric",
            "insufficient_data",
            "bots_suppressed",
            "bots_suppressed_events",
        ] {
            assert!(json.contains(k), "missing {k} in: {json}");
        }
    }
}
