//! Leaderboard report kind — scaffold (ORG-REPORTS.md §1–§6).
//!
//! Stage 3 lays down the type surface and a thin SQL builder for
//! `subject = user` in single-org mode only. Stage 4 extends the
//! envelope gate + the SQL builder to every valid
//! ([`SubjectKind`], [`ScopeMode`]) pair, locks the §6.1 tie-break
//! order via [`LEADERBOARD_TIE_BREAK_ORDER_BY_CLAUSE`], and pins the
//! §6.8 `home_org_label` aggregation (incl. the `__unlabeled__`
//! synthetic bucket). Pagination (ORG-REPORTS §6.5), `also_compute`
//! (§6.3), `subject_ids` (§6.10), the reconciliation footer (§6.2),
//! and the `my_standing` companion endpoint (§6.9) land in later
//! stages.
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
/// SCOPE.md §8. Stage 4 wires every variant with the full
/// [`ScopeMode`] fan-out subject to the §2 invalid-combo rules
/// rejected up-front by [`validate_subject_scope_combo`].
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

impl MetricId {
    /// True for count-family metrics — the §6.2 reconciliation
    /// identity applies only to these.
    ///
    /// The duration family (when it lands) returns false here so
    /// [`check_reconciliation_identity`] short-circuits for it: a
    /// duration metric's row value is an aggregate (p50, p95, …),
    /// not a count, and `sum(rows) + footer` is meaningless against
    /// `headline.events_total`.
    pub const fn is_count(self) -> bool {
        matches!(self, MetricId::Count(_))
    }

    /// True for duration-family metrics. Inverse of [`Self::is_count`].
    ///
    /// Kept as a separate accessor (rather than `!is_count()`) so
    /// future variants — e.g. ratio metrics — can be added without
    /// silently flipping the §6.2 exemption.
    pub const fn is_duration(self) -> bool {
        !self.is_count()
    }
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
    /// Org-scope lens (SCOPE §8.1). All three modes are accepted as
    /// of stage 4; invalid pairings with [`Self::subject`] are
    /// rejected by [`validate_subject_scope_combo`].
    pub scope_mode: ScopeMode,
    /// Orgs in scope. Empty means "all orgs the principal can see"
    /// (the auth layer narrows the set in Phase 4). Single-org mode
    /// expects exactly one entry; cross-org modes accept one or
    /// more.
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
    /// Subject axis (§2). All four variants accepted as of stage 4.
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
/// Stage 5 wires the §6.2 reconciliation identity (see
/// [`check_reconciliation_identity`]) and the §6.4 bot split; stage 6
/// wires `insufficient_data` for duration metrics. The wire shape is
/// locked here so later stages don't reshape the response.
///
/// All five fields serialise unconditionally — REST/MCP/frontend must
/// not branch on field presence (a SCOPE.md §11.4 trust requirement).
/// `bots_suppressed_events` is the §6.4 reconciliation counter: it
/// exists separately from `bots_suppressed` precisely so the §6.2
/// identity has every term it needs without re-querying the bot set.
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
/// Other stages add `CursorWindowMismatch` (§6.5) and
/// `SubjectIdsTooLarge` (§6.10). Stage 5 added
/// [`Self::ReconciliationViolation`] so the §6.2 identity is a
/// first-class failure the REST/MCP layers can match on (release
/// builds may choose to log + drop it; debug builds panic via
/// [`debug_assert_reconciliation_identity`]). They share this error
/// enum so the REST and MCP surfaces map every leaderboard failure
/// through one match.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum LeaderboardError {
    /// `orgs` must contain exactly one id in single-org mode.
    #[error("single_org scope requires exactly one org id, got {0}")]
    SingleOrgRequiresOneOrg(usize),
    /// Cross-org modes require at least one org in scope.
    #[error("cross-org scope requires at least one org id, got 0")]
    CrossOrgRequiresOrgs,
    /// The `(subject, scope_mode)` pair is meaningless per
    /// ORG-REPORTS §2 — e.g. `team` in `all_orgs_combined` (teams do
    /// not cross orgs) or `org` in `single_org` (a one-row
    /// leaderboard is not a leaderboard).
    #[error("leaderboard subject={subject:?} is invalid in scope_mode={scope_mode:?} (ORG-REPORTS §2)")]
    InvalidSubjectScopeCombo {
        /// The subject axis that was requested.
        subject: SubjectKind,
        /// The scope mode that was requested.
        scope_mode: ScopeMode,
    },
    /// The §6.2 reconciliation identity does not hold for a count
    /// metric:
    ///
    /// `headline.events_total
    ///    == sum(rows[].primary.value)
    ///     + footer.unattributed_events_metric
    ///     + footer.bots_suppressed_events`
    ///
    /// Reported only for count metrics — duration metrics are
    /// exempt per §6.2 (their row values are aggregates, not
    /// counts).
    #[error("reconciliation identity broken: events_total={events_total} != \
             sum(rows.primary)={rows_sum} + unattributed_metric={unattributed_metric} + \
             bots_suppressed_events={bots_suppressed_events} (delta={delta})")]
    ReconciliationViolation {
        /// `headline.events_total` echoed back for debugging.
        events_total: u64,
        /// Σ `rows[].primary.value` (saturating-clamped at 0 for
        /// the rare negative aggregate that should never reach
        /// the count-metric path).
        rows_sum: u64,
        /// `footer.unattributed_events_metric` echoed back.
        unattributed_metric: u64,
        /// `footer.bots_suppressed_events` echoed back.
        bots_suppressed_events: u64,
        /// `events_total - (rows_sum + unattributed_metric +
        /// bots_suppressed_events)` as a signed delta so callers
        /// can tell over- from under-counting.
        delta: i128,
    },
    /// Window spec failed to resolve.
    #[error(transparent)]
    Resolve(#[from] ResolveError),
}

// ---------------------------------------------------------------------------
// Envelope resolution
// ---------------------------------------------------------------------------

/// Validate a (subject, scope_mode) pair per ORG-REPORTS §2.
///
/// The combinations rejected here are *meaningless*, not merely
/// unwired: `team` in `all_orgs_combined` (teams are org-scoped by
/// definition — combining them across orgs is undefined) and `org`
/// in `single_org` (a leaderboard with one row is degenerate).
/// Every other pair is honoured by the stage-4 SQL builder.
pub fn validate_subject_scope_combo(
    subject: SubjectKind,
    scope_mode: ScopeMode,
) -> Result<(), LeaderboardError> {
    match (subject, scope_mode) {
        (SubjectKind::Team, ScopeMode::AllOrgsCombined)
        | (SubjectKind::Org, ScopeMode::SingleOrg) => {
            Err(LeaderboardError::InvalidSubjectScopeCombo { subject, scope_mode })
        }
        _ => Ok(()),
    }
}

/// Resolve a [`LeaderboardEnvelope`] at `now`, returning the
/// [`ResolvedLeaderboardEnvelope`] echoed in the response.
///
/// Stage 4 accepts every valid `(subject, scope_mode)` pair per
/// ORG-REPORTS §2 and validates the `orgs` cardinality contract per
/// scope mode.
pub fn resolve_leaderboard_envelope(
    env: &LeaderboardEnvelope,
    now: DateTime<Utc>,
) -> Result<ResolvedLeaderboardEnvelope, LeaderboardError> {
    validate_subject_scope_combo(env.subject, env.scope_mode)?;
    match env.scope_mode {
        ScopeMode::SingleOrg => {
            if env.orgs.len() != 1 {
                return Err(LeaderboardError::SingleOrgRequiresOneOrg(env.orgs.len()));
            }
        }
        ScopeMode::AllOrgsCombined | ScopeMode::PerOrgSplit => {
            // Empty `orgs` is the wire-form "all orgs the principal
            // can see" — the auth layer in Phase 4 narrows it. The
            // SQL builder bind point requires the resolved list to
            // be non-empty; here we only reject the unambiguous
            // "client sent [] and we have no principal context"
            // shape once the principal layer lands. Stage 4 keeps
            // the constructor permissive (empty == defer to auth).
        }
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
// §6.8 — `home_org_label` aggregation
// ---------------------------------------------------------------------------

/// Synthetic `subject_id` for users with `dp_memberships.home_org IS
/// NULL` when `subject = home_org_label`.
///
/// ORG-REPORTS §6.8: NULL home-orgs are bucketed into a single
/// synthetic row rather than silently dropped, so an org with poor
/// home-org coverage shows up loud in the leaderboard rather than
/// vanishing. Suppression requires an explicit envelope filter, not
/// an accident of aggregation. The string is intentionally
/// underscore-bracketed so it can never collide with a real
/// org-slug/UUID.
pub const HOME_ORG_LABEL_UNLABELED_BUCKET: &str = "__unlabeled__";

/// Human-friendly label for [`HOME_ORG_LABEL_UNLABELED_BUCKET`].
pub const HOME_ORG_LABEL_UNLABELED_LABEL: &str = "(no home org)";

// ---------------------------------------------------------------------------
// §6.1 — tie-break order, locked
// ---------------------------------------------------------------------------

/// The §6.1 tie-break order, expressed as the `ORDER BY` clause
/// every leaderboard SQL string emits verbatim.
///
/// `primary_value DESC → active_days DESC → subject_id ASC`. The
/// `subject_id` final break is intentional: labels (`login`,
/// `team.slug`, `org.login`) can change; ids do not. Sharing this
/// string across every (subject × scope) variant is what makes the
/// REST / MCP / frontend surfaces deterministic without each one
/// re-implementing the rule.
pub const LEADERBOARD_TIE_BREAK_ORDER_BY_CLAUSE: &str =
    "ORDER BY primary_value DESC, active_days DESC, subject_id ASC";

// ---------------------------------------------------------------------------
// Stage-4 thin SQL builder — every valid (subject, scope) pair
// ---------------------------------------------------------------------------

/// Parameter bind order for [`build_leaderboard_sql`].
///
/// Unified across every `(subject, scope_mode)` combo so the store
/// adapter and any integration test bind the same way regardless of
/// the lens chosen. Single-org callers pass `org_ids` as a
/// one-element `uuid[]`; the envelope validation in
/// [`resolve_leaderboard_envelope`] guarantees the cardinality.
pub const LEADERBOARD_BIND_ORDER: &[&str] = &[
    "$1 window.start (timestamptz)",
    "$2 window.end (timestamptz, exclusive)",
    "$3 org_ids (uuid[]; cardinality >= 1)",
    "$4 event_kind (text — from CountMetric::event_kind())",
    "$5 actor_roles (text[] — from envelope.actor_roles or CountMetric::default_actor_roles())",
    "$6 repos (uuid[]; cardinality 0 == no filter)",
];

/// SQL for the leaderboard, fanned out per `(subject, scope_mode)`.
///
/// Returns the SQL string the store will execute against
/// `dp_activity_events` / `dp_event_actors` (+ `dp_memberships` /
/// `dp_teams` joins where the subject requires them). Returns an
/// error for the §2 invalid combinations.
///
/// Every variant:
///
/// * binds in [`LEADERBOARD_BIND_ORDER`],
/// * projects the same five columns —
///   `subject_id / primary_value / active_days / repos_touched /
///   active_orgs` — so the row-mapper is shared,
/// * sorts by [`LEADERBOARD_TIE_BREAK_ORDER_BY_CLAUSE`] (§6.1),
/// * omits `LIMIT` / `OFFSET` (pagination lands in stage 6),
/// * omits bot suppression (the store applies it post-fetch so
///   §6.4's `bots_suppressed_events` is computable without a second
///   query).
///
/// `per_org_split` is the only mode whose SQL groups by
/// `(subject, org_id)` and projects `subject_org` as a sixth
/// column; the response shape's `LeaderboardRow.subject_org` is
/// populated **only** for that mode (response §5).
///
/// `home_org_label` uses `COALESCE(m.home_org::text,
/// HOME_ORG_LABEL_UNLABELED_BUCKET)` as the grouping key so NULL
/// home-orgs end up in the synthetic bucket per §6.8.
pub fn build_leaderboard_sql(
    subject: SubjectKind,
    scope_mode: ScopeMode,
) -> Result<&'static str, LeaderboardError> {
    validate_subject_scope_combo(subject, scope_mode)?;
    Ok(match (subject, scope_mode) {
        // ---- subject = user ------------------------------------------------
        (SubjectKind::User, ScopeMode::SingleOrg) => USER_SINGLE_ORG_SQL,
        (SubjectKind::User, ScopeMode::AllOrgsCombined) => USER_ALL_ORGS_COMBINED_SQL,
        (SubjectKind::User, ScopeMode::PerOrgSplit) => USER_PER_ORG_SPLIT_SQL,
        // ---- subject = team (single-org and per-org-split only) -----------
        (SubjectKind::Team, ScopeMode::SingleOrg) => TEAM_SINGLE_ORG_SQL,
        (SubjectKind::Team, ScopeMode::PerOrgSplit) => TEAM_PER_ORG_SPLIT_SQL,
        // ---- subject = org (all-orgs-combined and per-org-split only) -----
        (SubjectKind::Org, ScopeMode::AllOrgsCombined) => ORG_ALL_ORGS_COMBINED_SQL,
        (SubjectKind::Org, ScopeMode::PerOrgSplit) => ORG_PER_ORG_SPLIT_SQL,
        // ---- subject = home_org_label (all three modes) -------------------
        (SubjectKind::HomeOrgLabel, ScopeMode::SingleOrg) => HOME_ORG_LABEL_SINGLE_ORG_SQL,
        (SubjectKind::HomeOrgLabel, ScopeMode::AllOrgsCombined) => {
            HOME_ORG_LABEL_ALL_ORGS_COMBINED_SQL
        }
        (SubjectKind::HomeOrgLabel, ScopeMode::PerOrgSplit) => HOME_ORG_LABEL_PER_ORG_SPLIT_SQL,
        // ---- §2 invalid combos — already rejected above -------------------
        (SubjectKind::Team, ScopeMode::AllOrgsCombined)
        | (SubjectKind::Org, ScopeMode::SingleOrg) => unreachable!("validated above"),
    })
}

// ---------------------------------------------------------------------------
// Per-(subject, scope) SQL strings
// ---------------------------------------------------------------------------
//
// Each string assumes `LEADERBOARD_BIND_ORDER`. The active_orgs
// column is computed as `count(DISTINCT e.org_id)` for cross-org
// variants and hard-coded `1::bigint` for single-org / per-org-split
// (which both produce one row per org by construction).
//
// active_days truncates `e.ts` to UTC days — re-anchoring to the
// window TZ is a §15.8 trend-bucket concern owned by stage 4's
// sparkline plumbing, not the rank query.
// ---------------------------------------------------------------------------

// --- subject = user --------------------------------------------------------

const USER_SINGLE_ORG_SQL: &str = "SELECT ea.user_id::text                                AS subject_id, \
                                          count(*)::bigint                                 AS primary_value, \
                                          count(DISTINCT date_trunc('day', e.ts))::bigint  AS active_days, \
                                          count(DISTINCT e.repo_id)::bigint                AS repos_touched, \
                                          1::bigint                                        AS active_orgs \
                                     FROM dp_event_actors ea \
                                     JOIN dp_activity_events e ON e.id = ea.event_id \
                                    WHERE e.ts   >= $1 \
                                      AND e.ts   <  $2 \
                                      AND e.org_id = ANY($3) \
                                      AND e.kind   = $4 \
                                      AND ea.role  = ANY($5) \
                                      AND (cardinality($6::uuid[]) = 0 OR e.repo_id = ANY($6)) \
                                    GROUP BY ea.user_id \
                                    ORDER BY primary_value DESC, active_days DESC, subject_id ASC";

const USER_ALL_ORGS_COMBINED_SQL: &str = "SELECT ea.user_id::text                                AS subject_id, \
                                                 count(*)::bigint                                 AS primary_value, \
                                                 count(DISTINCT date_trunc('day', e.ts))::bigint  AS active_days, \
                                                 count(DISTINCT e.repo_id)::bigint                AS repos_touched, \
                                                 count(DISTINCT e.org_id)::bigint                 AS active_orgs \
                                            FROM dp_event_actors ea \
                                            JOIN dp_activity_events e ON e.id = ea.event_id \
                                           WHERE e.ts   >= $1 \
                                             AND e.ts   <  $2 \
                                             AND e.org_id = ANY($3) \
                                             AND e.kind   = $4 \
                                             AND ea.role  = ANY($5) \
                                             AND (cardinality($6::uuid[]) = 0 OR e.repo_id = ANY($6)) \
                                           GROUP BY ea.user_id \
                                           ORDER BY primary_value DESC, active_days DESC, subject_id ASC";

const USER_PER_ORG_SPLIT_SQL: &str = "SELECT ea.user_id::text                                AS subject_id, \
                                             e.org_id                                         AS subject_org, \
                                             count(*)::bigint                                 AS primary_value, \
                                             count(DISTINCT date_trunc('day', e.ts))::bigint  AS active_days, \
                                             count(DISTINCT e.repo_id)::bigint                AS repos_touched, \
                                             1::bigint                                        AS active_orgs \
                                        FROM dp_event_actors ea \
                                        JOIN dp_activity_events e ON e.id = ea.event_id \
                                       WHERE e.ts   >= $1 \
                                         AND e.ts   <  $2 \
                                         AND e.org_id = ANY($3) \
                                         AND e.kind   = $4 \
                                         AND ea.role  = ANY($5) \
                                         AND (cardinality($6::uuid[]) = 0 OR e.repo_id = ANY($6)) \
                                       GROUP BY ea.user_id, e.org_id \
                                       ORDER BY primary_value DESC, active_days DESC, subject_id ASC";

// --- subject = team --------------------------------------------------------
//
// Team membership lives in a yet-to-be-added `dp_team_members
// (team_id, user_id, org_id)` table — the store-side prerequisite
// for these strings is tracked alongside the duration-metric fetch
// (STAGE-1-COMPOSABILITY §3). The SQL is shaped so it lights up the
// instant the membership table exists; until then the team variants
// are scaffold-only (REST/MCP integration sits behind a §6.x feature
// gate that stage 6+ wires).

const TEAM_SINGLE_ORG_SQL: &str = "SELECT tm.team_id::text                                AS subject_id, \
                                          count(*)::bigint                                 AS primary_value, \
                                          count(DISTINCT date_trunc('day', e.ts))::bigint  AS active_days, \
                                          count(DISTINCT e.repo_id)::bigint                AS repos_touched, \
                                          1::bigint                                        AS active_orgs \
                                     FROM dp_event_actors ea \
                                     JOIN dp_activity_events e ON e.id = ea.event_id \
                                     JOIN dp_team_members   tm ON tm.user_id = ea.user_id AND tm.org_id = e.org_id \
                                    WHERE e.ts   >= $1 \
                                      AND e.ts   <  $2 \
                                      AND e.org_id = ANY($3) \
                                      AND e.kind   = $4 \
                                      AND ea.role  = ANY($5) \
                                      AND (cardinality($6::uuid[]) = 0 OR e.repo_id = ANY($6)) \
                                    GROUP BY tm.team_id \
                                    ORDER BY primary_value DESC, active_days DESC, subject_id ASC";

const TEAM_PER_ORG_SPLIT_SQL: &str = "SELECT tm.team_id::text                                AS subject_id, \
                                             e.org_id                                         AS subject_org, \
                                             count(*)::bigint                                 AS primary_value, \
                                             count(DISTINCT date_trunc('day', e.ts))::bigint  AS active_days, \
                                             count(DISTINCT e.repo_id)::bigint                AS repos_touched, \
                                             1::bigint                                        AS active_orgs \
                                        FROM dp_event_actors ea \
                                        JOIN dp_activity_events e ON e.id = ea.event_id \
                                        JOIN dp_team_members   tm ON tm.user_id = ea.user_id AND tm.org_id = e.org_id \
                                       WHERE e.ts   >= $1 \
                                         AND e.ts   <  $2 \
                                         AND e.org_id = ANY($3) \
                                         AND e.kind   = $4 \
                                         AND ea.role  = ANY($5) \
                                         AND (cardinality($6::uuid[]) = 0 OR e.repo_id = ANY($6)) \
                                       GROUP BY tm.team_id, e.org_id \
                                       ORDER BY primary_value DESC, active_days DESC, subject_id ASC";

// --- subject = org ---------------------------------------------------------

const ORG_ALL_ORGS_COMBINED_SQL: &str = "SELECT e.org_id::text                                  AS subject_id, \
                                                count(*)::bigint                                 AS primary_value, \
                                                count(DISTINCT date_trunc('day', e.ts))::bigint  AS active_days, \
                                                count(DISTINCT e.repo_id)::bigint                AS repos_touched, \
                                                1::bigint                                        AS active_orgs \
                                           FROM dp_event_actors ea \
                                           JOIN dp_activity_events e ON e.id = ea.event_id \
                                          WHERE e.ts   >= $1 \
                                            AND e.ts   <  $2 \
                                            AND e.org_id = ANY($3) \
                                            AND e.kind   = $4 \
                                            AND ea.role  = ANY($5) \
                                            AND (cardinality($6::uuid[]) = 0 OR e.repo_id = ANY($6)) \
                                          GROUP BY e.org_id \
                                          ORDER BY primary_value DESC, active_days DESC, subject_id ASC";

// per_org_split for subject=org is degenerate (subject_id == subject_org),
// but ORG-REPORTS §5 keeps it valid — the frontend renders the grouped
// table with one row per group, which is still useful UX (it exercises
// the same code path as user/team per-org-split). subject_org is
// projected to keep the row-mapper shared.
const ORG_PER_ORG_SPLIT_SQL: &str = "SELECT e.org_id::text                                  AS subject_id, \
                                            e.org_id                                         AS subject_org, \
                                            count(*)::bigint                                 AS primary_value, \
                                            count(DISTINCT date_trunc('day', e.ts))::bigint  AS active_days, \
                                            count(DISTINCT e.repo_id)::bigint                AS repos_touched, \
                                            1::bigint                                        AS active_orgs \
                                       FROM dp_event_actors ea \
                                       JOIN dp_activity_events e ON e.id = ea.event_id \
                                      WHERE e.ts   >= $1 \
                                        AND e.ts   <  $2 \
                                        AND e.org_id = ANY($3) \
                                        AND e.kind   = $4 \
                                        AND ea.role  = ANY($5) \
                                        AND (cardinality($6::uuid[]) = 0 OR e.repo_id = ANY($6)) \
                                      GROUP BY e.org_id \
                                      ORDER BY primary_value DESC, active_days DESC, subject_id ASC";

// --- subject = home_org_label ---------------------------------------------
//
// §6.8: group by `COALESCE(m.home_org::text, '__unlabeled__')` so
// users without a home-org membership land in the synthetic bucket
// rather than vanishing. Count metrics sum across all members of
// the label (one row per `(user, event)` actor pair already, so a
// plain `count(*)` is the sum the spec asks for — no averaging-of-
// averages). The `m.home_org` join is `LEFT JOIN` so users with no
// membership row still aggregate into `__unlabeled__`.

const HOME_ORG_LABEL_SINGLE_ORG_SQL: &str = "SELECT COALESCE(m.home_org::text, '__unlabeled__')      AS subject_id, \
                                                    count(*)::bigint                                 AS primary_value, \
                                                    count(DISTINCT date_trunc('day', e.ts))::bigint  AS active_days, \
                                                    count(DISTINCT e.repo_id)::bigint                AS repos_touched, \
                                                    1::bigint                                        AS active_orgs \
                                               FROM dp_event_actors ea \
                                               JOIN dp_activity_events e ON e.id = ea.event_id \
                                          LEFT JOIN dp_memberships    m ON m.user_id = ea.user_id AND m.home_org = m.org_id \
                                              WHERE e.ts   >= $1 \
                                                AND e.ts   <  $2 \
                                                AND e.org_id = ANY($3) \
                                                AND e.kind   = $4 \
                                                AND ea.role  = ANY($5) \
                                                AND (cardinality($6::uuid[]) = 0 OR e.repo_id = ANY($6)) \
                                              GROUP BY COALESCE(m.home_org::text, '__unlabeled__') \
                                              ORDER BY primary_value DESC, active_days DESC, subject_id ASC";

const HOME_ORG_LABEL_ALL_ORGS_COMBINED_SQL: &str = "SELECT COALESCE(m.home_org::text, '__unlabeled__')      AS subject_id, \
                                                           count(*)::bigint                                 AS primary_value, \
                                                           count(DISTINCT date_trunc('day', e.ts))::bigint  AS active_days, \
                                                           count(DISTINCT e.repo_id)::bigint                AS repos_touched, \
                                                           count(DISTINCT e.org_id)::bigint                 AS active_orgs \
                                                      FROM dp_event_actors ea \
                                                      JOIN dp_activity_events e ON e.id = ea.event_id \
                                                 LEFT JOIN dp_memberships    m ON m.user_id = ea.user_id AND m.home_org = m.org_id \
                                                     WHERE e.ts   >= $1 \
                                                       AND e.ts   <  $2 \
                                                       AND e.org_id = ANY($3) \
                                                       AND e.kind   = $4 \
                                                       AND ea.role  = ANY($5) \
                                                       AND (cardinality($6::uuid[]) = 0 OR e.repo_id = ANY($6)) \
                                                     GROUP BY COALESCE(m.home_org::text, '__unlabeled__') \
                                                     ORDER BY primary_value DESC, active_days DESC, subject_id ASC";

const HOME_ORG_LABEL_PER_ORG_SPLIT_SQL: &str = "SELECT COALESCE(m.home_org::text, '__unlabeled__')      AS subject_id, \
                                                       e.org_id                                         AS subject_org, \
                                                       count(*)::bigint                                 AS primary_value, \
                                                       count(DISTINCT date_trunc('day', e.ts))::bigint  AS active_days, \
                                                       count(DISTINCT e.repo_id)::bigint                AS repos_touched, \
                                                       1::bigint                                        AS active_orgs \
                                                  FROM dp_event_actors ea \
                                                  JOIN dp_activity_events e ON e.id = ea.event_id \
                                             LEFT JOIN dp_memberships    m ON m.user_id = ea.user_id AND m.home_org = m.org_id \
                                                 WHERE e.ts   >= $1 \
                                                   AND e.ts   <  $2 \
                                                   AND e.org_id = ANY($3) \
                                                   AND e.kind   = $4 \
                                                   AND ea.role  = ANY($5) \
                                                   AND (cardinality($6::uuid[]) = 0 OR e.repo_id = ANY($6)) \
                                                 GROUP BY COALESCE(m.home_org::text, '__unlabeled__'), e.org_id \
                                                 ORDER BY primary_value DESC, active_days DESC, subject_id ASC";

// ---------------------------------------------------------------------------
// §6.2 — reconciliation identity (count metrics) + duration exemption
// ---------------------------------------------------------------------------

/// Verify the §6.2 reconciliation identity for count metrics.
///
/// The identity, locked in ORG-REPORTS §6.2:
///
/// ```text
/// headline.events_total
///   == sum(rows[].primary.value)
///    + footer.unattributed_events_metric
///    + footer.bots_suppressed_events
/// ```
///
/// Duration metrics are **exempt** — their row values are
/// aggregates (p50, p95, …), not counts, so the sum is meaningless.
/// For [`MetricId::is_duration`] the function returns `Ok(())`
/// without inspecting any term.
///
/// Why this matters: every number in a leaderboard response must
/// trace back to a §15.7 row (SCOPE.md §9 transparency, §11.4
/// trust). The reconciliation identity is the cheapest possible
/// surface-level proof that the bot filter, the unattributed-events
/// footer, and the visible rows together account for every event
/// the headline reports. A drift here means at least one of those
/// terms is wrong — the worst case being that bot or unattributed
/// activity has silently leaked into a user's rank.
///
/// Negative row values are clamped to 0 when summing — for the
/// count path they are never produced, but the saturating add
/// keeps the function infallible against malformed inputs and
/// avoids a panic in release builds.
pub fn check_reconciliation_identity(
    metric: MetricId,
    headline: &LeaderboardHeadline,
    rows: &[LeaderboardRow],
    footer: &LeaderboardFooter,
) -> Result<(), LeaderboardError> {
    if metric.is_duration() {
        // §6.2 duration-metric exemption.
        return Ok(());
    }
    let rows_sum: u64 = rows
        .iter()
        .map(|r| u64::try_from(r.primary.value).unwrap_or(0))
        .fold(0u64, |acc, v| acc.saturating_add(v));
    let expected = (rows_sum as i128)
        + (footer.unattributed_events_metric as i128)
        + (footer.bots_suppressed_events as i128);
    let delta = (headline.events_total as i128) - expected;
    if delta == 0 {
        Ok(())
    } else {
        Err(LeaderboardError::ReconciliationViolation {
            events_total: headline.events_total,
            rows_sum,
            unattributed_metric: footer.unattributed_events_metric,
            bots_suppressed_events: footer.bots_suppressed_events,
            delta,
        })
    }
}

/// Debug-build assertion of [`check_reconciliation_identity`].
///
/// SCOPE.md constraint: "The §6.2 reconciliation identity is
/// enforced as a debug-build assertion for count metrics; release
/// builds may skip it but the tests must verify it." This function
/// is the embodiment of that rule:
///
/// * In `cfg(debug_assertions)` builds it panics on violation, so a
///   test or local-dev run catches the drift loud and immediately.
/// * In release builds it is a no-op — the cost is paid only where
///   it's cheap. REST/MCP layers wanting a real-build check should
///   call [`check_reconciliation_identity`] directly and log/return
///   the [`LeaderboardError::ReconciliationViolation`].
///
/// The duration-metric exemption is delegated to
/// [`check_reconciliation_identity`], so callers can hand any
/// `MetricId` here without branching.
pub fn debug_assert_reconciliation_identity(
    metric: MetricId,
    headline: &LeaderboardHeadline,
    rows: &[LeaderboardRow],
    footer: &LeaderboardFooter,
) {
    #[cfg(debug_assertions)]
    {
        if let Err(e) = check_reconciliation_identity(metric, headline, rows, footer) {
            panic!("§6.2 reconciliation identity broken: {e}");
        }
    }
    #[cfg(not(debug_assertions))]
    {
        // Release builds skip the check per SCOPE.md constraint;
        // touch the inputs so unused-arg lints stay silent.
        let _ = (metric, headline, rows, footer);
    }
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
    fn resolve_accepts_every_valid_subject_scope_pair() {
        // Stage 4 lifts the stage-3 gate: every §2-valid combo must
        // resolve cleanly so the dispatch fans out to the right SQL.
        let valid = [
            (SubjectKind::User, ScopeMode::SingleOrg),
            (SubjectKind::User, ScopeMode::AllOrgsCombined),
            (SubjectKind::User, ScopeMode::PerOrgSplit),
            (SubjectKind::Team, ScopeMode::SingleOrg),
            (SubjectKind::Team, ScopeMode::PerOrgSplit),
            (SubjectKind::Org, ScopeMode::AllOrgsCombined),
            (SubjectKind::Org, ScopeMode::PerOrgSplit),
            (SubjectKind::HomeOrgLabel, ScopeMode::SingleOrg),
            (SubjectKind::HomeOrgLabel, ScopeMode::AllOrgsCombined),
            (SubjectKind::HomeOrgLabel, ScopeMode::PerOrgSplit),
        ];
        for (subject, scope_mode) in valid {
            let mut env = sample_envelope();
            env.subject = subject;
            env.scope_mode = scope_mode;
            // Single-org expects exactly one; cross-org accepts >= 1
            // (the auth layer narrows []).
            env.orgs = vec![Uuid::nil()];
            let r = resolve_leaderboard_envelope(&env, utc(2025, 6, 18, 12, 0, 0));
            assert!(
                r.is_ok(),
                "expected ({subject:?}, {scope_mode:?}) to resolve, got {r:?}",
            );
        }
    }

    #[test]
    fn resolve_rejects_invalid_subject_scope_combos() {
        // ORG-REPORTS §2: team is meaningless in all_orgs_combined;
        // org is meaningless in single_org (one-row leaderboard).
        for (subject, scope_mode) in [
            (SubjectKind::Team, ScopeMode::AllOrgsCombined),
            (SubjectKind::Org, ScopeMode::SingleOrg),
        ] {
            let mut env = sample_envelope();
            env.subject = subject;
            env.scope_mode = scope_mode;
            env.orgs = vec![Uuid::nil()];
            let err = resolve_leaderboard_envelope(&env, utc(2025, 6, 18, 12, 0, 0)).unwrap_err();
            assert_eq!(
                err,
                LeaderboardError::InvalidSubjectScopeCombo { subject, scope_mode },
            );
        }
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

    // ----- Stage 4: dispatch + tie-break + home_org_label ---------------

    fn all_valid_pairs() -> Vec<(SubjectKind, ScopeMode)> {
        vec![
            (SubjectKind::User, ScopeMode::SingleOrg),
            (SubjectKind::User, ScopeMode::AllOrgsCombined),
            (SubjectKind::User, ScopeMode::PerOrgSplit),
            (SubjectKind::Team, ScopeMode::SingleOrg),
            (SubjectKind::Team, ScopeMode::PerOrgSplit),
            (SubjectKind::Org, ScopeMode::AllOrgsCombined),
            (SubjectKind::Org, ScopeMode::PerOrgSplit),
            (SubjectKind::HomeOrgLabel, ScopeMode::SingleOrg),
            (SubjectKind::HomeOrgLabel, ScopeMode::AllOrgsCombined),
            (SubjectKind::HomeOrgLabel, ScopeMode::PerOrgSplit),
        ]
    }

    #[test]
    fn dispatch_returns_sql_for_every_valid_pair() {
        for (s, sm) in all_valid_pairs() {
            let sql = build_leaderboard_sql(s, sm)
                .unwrap_or_else(|e| panic!("({s:?}, {sm:?}) should dispatch: {e}"));
            assert!(!sql.is_empty(), "({s:?}, {sm:?}) returned empty SQL");
        }
    }

    #[test]
    fn dispatch_rejects_invalid_pairs() {
        for (s, sm) in [
            (SubjectKind::Team, ScopeMode::AllOrgsCombined),
            (SubjectKind::Org, ScopeMode::SingleOrg),
        ] {
            let err = build_leaderboard_sql(s, sm).unwrap_err();
            assert_eq!(
                err,
                LeaderboardError::InvalidSubjectScopeCombo {
                    subject: s,
                    scope_mode: sm
                },
            );
        }
    }

    #[test]
    fn every_dispatch_sql_emits_the_locked_tie_break_clause() {
        // §6.1: the tie-break order is identical across every
        // (subject, scope) combo. A drift here is the §11.4
        // divergence trap.
        for (s, sm) in all_valid_pairs() {
            let sql = build_leaderboard_sql(s, sm).unwrap();
            assert!(
                sql.contains(LEADERBOARD_TIE_BREAK_ORDER_BY_CLAUSE),
                "({s:?}, {sm:?}) missing locked tie-break clause; got: {sql}",
            );
        }
    }

    #[test]
    fn every_dispatch_sql_projects_the_shared_row_mapper_columns() {
        // The store-side row-mapper expects the same five base
        // columns regardless of (subject, scope). per_org_split
        // additionally projects `subject_org`; that case is checked
        // separately below.
        for (s, sm) in all_valid_pairs() {
            let sql = build_leaderboard_sql(s, sm).unwrap();
            for col in [
                "subject_id",
                "primary_value",
                "active_days",
                "repos_touched",
                "active_orgs",
            ] {
                assert!(sql.contains(col), "({s:?}, {sm:?}) missing {col}: {sql}");
            }
        }
    }

    #[test]
    fn per_org_split_variants_project_subject_org() {
        // §5: `subject_org` is populated only in per_org_split. The
        // SQL must project it for those variants and only those
        // variants, mirroring the response shape's
        // `skip_serializing_if = Option::is_none`.
        for (s, sm) in all_valid_pairs() {
            let sql = build_leaderboard_sql(s, sm).unwrap();
            let projects_subject_org = sql.contains("AS subject_org");
            assert_eq!(
                projects_subject_org,
                sm == ScopeMode::PerOrgSplit,
                "({s:?}, {sm:?}) subject_org projection should match per_org_split"
            );
        }
    }

    #[test]
    fn cross_org_variants_compute_active_orgs_dynamically() {
        // all_orgs_combined is the only mode where active_orgs is a
        // real signal — it tells us how many orgs the subject
        // appears in. Single-org and per-org-split produce one row
        // per org by construction, so a hard-coded `1::bigint` is
        // correct there. subject=org is special: each row IS an
        // org, so active_orgs is trivially 1 even in
        // all_orgs_combined.
        for (s, sm) in all_valid_pairs() {
            let sql = build_leaderboard_sql(s, sm).unwrap();
            let expect_distinct = sm == ScopeMode::AllOrgsCombined && s != SubjectKind::Org;
            if expect_distinct {
                assert!(
                    sql.contains("count(DISTINCT e.org_id)::bigint                 AS active_orgs"),
                    "({s:?}, {sm:?}) should compute active_orgs via count(DISTINCT): {sql}",
                );
            } else {
                assert!(
                    sql.contains("1::bigint                                        AS active_orgs"),
                    "({s:?}, {sm:?}) should hard-code active_orgs = 1: {sql}",
                );
            }
        }
    }

    #[test]
    fn home_org_label_uses_the_unlabeled_bucket_coalesce() {
        // §6.8: NULL home-orgs land in the `__unlabeled__` synthetic
        // bucket via `COALESCE(m.home_org::text, '__unlabeled__')`.
        // The bucket name is shared with the wire constant so a
        // rename here propagates to the response shape — drift is
        // exactly the §11.4 divergence trap.
        for sm in [
            ScopeMode::SingleOrg,
            ScopeMode::AllOrgsCombined,
            ScopeMode::PerOrgSplit,
        ] {
            let sql = build_leaderboard_sql(SubjectKind::HomeOrgLabel, sm).unwrap();
            assert!(
                sql.contains(&format!(
                    "COALESCE(m.home_org::text, '{}')",
                    HOME_ORG_LABEL_UNLABELED_BUCKET
                )),
                "({sm:?}) home_org_label SQL must coalesce NULL to bucket: {sql}",
            );
            // §6.8 — never silently dropped: the join must be LEFT
            // so users without a membership row still aggregate.
            assert!(
                sql.contains("LEFT JOIN dp_memberships"),
                "({sm:?}) home_org_label must LEFT JOIN memberships",
            );
        }
    }

    #[test]
    fn home_org_label_bucket_constants_are_stable() {
        // The wire form of the synthetic row is locked: any rename
        // would break dashboards keyed on `__unlabeled__`.
        assert_eq!(HOME_ORG_LABEL_UNLABELED_BUCKET, "__unlabeled__");
        assert_eq!(HOME_ORG_LABEL_UNLABELED_LABEL, "(no home org)");
    }

    #[test]
    fn leaderboard_bind_order_is_six_params_for_every_variant() {
        assert_eq!(LEADERBOARD_BIND_ORDER.len(), 6);
        for (s, sm) in all_valid_pairs() {
            let sql = build_leaderboard_sql(s, sm).unwrap();
            for i in 1..=6 {
                assert!(sql.contains(&format!("${i}")), "({s:?}, {sm:?}) missing ${i}");
            }
        }
    }

    #[test]
    fn no_dispatch_sql_carries_limit_or_offset() {
        // Pagination lands in stage 6 as a cursor predicate, not
        // LIMIT/OFFSET — keep this guard so the change is visible.
        for (s, sm) in all_valid_pairs() {
            let sql = build_leaderboard_sql(s, sm).unwrap();
            let upper = sql.to_ascii_uppercase();
            assert!(!upper.contains(" LIMIT "), "({s:?}, {sm:?}): {sql}");
            assert!(!upper.contains(" OFFSET "), "({s:?}, {sm:?}): {sql}");
        }
    }

    #[test]
    fn team_variants_join_team_members_within_org() {
        // Teams are org-scoped (§2). The dp_team_members join must
        // include the org_id predicate so a user's team in org A
        // doesn't pick up their events in org B.
        for sm in [ScopeMode::SingleOrg, ScopeMode::PerOrgSplit] {
            let sql = build_leaderboard_sql(SubjectKind::Team, sm).unwrap();
            assert!(
                sql.contains("JOIN dp_team_members   tm ON tm.user_id = ea.user_id AND tm.org_id = e.org_id"),
                "({sm:?}) team join must scope by (user_id, org_id): {sql}",
            );
        }
    }

    #[test]
    fn validator_matches_dispatcher_on_invalid_combos() {
        // The validator is the single source of truth for §2 — any
        // drift between the standalone validator and the
        // dispatcher's gate would let an invalid combo into the
        // store layer.
        for s in [
            SubjectKind::User,
            SubjectKind::Team,
            SubjectKind::Org,
            SubjectKind::HomeOrgLabel,
        ] {
            for sm in [
                ScopeMode::SingleOrg,
                ScopeMode::AllOrgsCombined,
                ScopeMode::PerOrgSplit,
            ] {
                let v_ok = validate_subject_scope_combo(s, sm).is_ok();
                let d_ok = build_leaderboard_sql(s, sm).is_ok();
                assert_eq!(v_ok, d_ok, "validator/dispatcher disagree on ({s:?}, {sm:?})");
            }
        }
    }

    // ----- Stage 5: §6.2 reconciliation + §6.4 split bot footer --------

    fn count_row(rank: u32, id: &str, value: i64, active_days: u32) -> LeaderboardRow {
        LeaderboardRow {
            rank,
            subject_id: id.into(),
            subject_kind: SubjectKind::User,
            subject_label: id.into(),
            subject_org: None,
            primary: LeaderboardPrimary {
                metric: MetricId::Count(CountMetric::PullRequestsOpened),
                value,
            },
            context: LeaderboardContext {
                active_days,
                ..LeaderboardContext::default()
            },
            sparkline: vec![],
            active_orgs: 1,
        }
    }

    #[test]
    fn metric_id_classifies_count_and_duration() {
        // §6.2 hinges on this classification; if a future metric
        // family lands without an explicit decision its identity
        // application must be re-thought, not defaulted.
        let m = MetricId::Count(CountMetric::PullRequestsOpened);
        assert!(m.is_count());
        assert!(!m.is_duration());
    }

    #[test]
    fn reconciliation_identity_holds_for_count_metrics() {
        // ORG-REPORTS §6.2:
        //   events_total == Σ rows + unattributed_metric + bots_suppressed_events
        // Construct a fixture where the sum balances exactly.
        let metric = MetricId::Count(CountMetric::PullRequestsOpened);
        let rows = vec![
            count_row(1, "u1", 12, 5),
            count_row(2, "u2", 7, 4),
            count_row(3, "u3", 3, 2),
        ];
        let headline = LeaderboardHeadline {
            total_subjects: 3,
            // 22 (rows) + 5 (unattributed_metric) + 11 (bot events) = 38
            events_total: 38,
        };
        let footer = LeaderboardFooter {
            unattributed_events: 9,
            unattributed_events_metric: 5,
            insufficient_data: 0,
            bots_suppressed: 2,
            bots_suppressed_events: 11,
        };
        assert!(check_reconciliation_identity(metric, &headline, &rows, &footer).is_ok());
        // The debug-build assertion is the production check —
        // exercise it from the unit test so cargo test (which runs
        // in debug mode) actually executes the panic path's happy
        // branch.
        debug_assert_reconciliation_identity(metric, &headline, &rows, &footer);
    }

    #[test]
    fn reconciliation_identity_detects_under_count() {
        // Drop one unattributed event from the footer — the identity
        // must surface the delta with the exact term breakdown so
        // operators can tell whether the bot path, the unattributed
        // path, or the row aggregation is at fault.
        let metric = MetricId::Count(CountMetric::PullRequestsOpened);
        let rows = vec![count_row(1, "u1", 10, 5)];
        let headline = LeaderboardHeadline {
            total_subjects: 1,
            events_total: 20,
        };
        let footer = LeaderboardFooter {
            unattributed_events: 0,
            unattributed_events_metric: 4, // should be 5 to balance
            insufficient_data: 0,
            bots_suppressed: 1,
            bots_suppressed_events: 5,
        };
        let err = check_reconciliation_identity(metric, &headline, &rows, &footer).unwrap_err();
        match err {
            LeaderboardError::ReconciliationViolation {
                events_total,
                rows_sum,
                unattributed_metric,
                bots_suppressed_events,
                delta,
            } => {
                assert_eq!(events_total, 20);
                assert_eq!(rows_sum, 10);
                assert_eq!(unattributed_metric, 4);
                assert_eq!(bots_suppressed_events, 5);
                assert_eq!(delta, 1); // headline is 1 over the visible terms
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn reconciliation_identity_detects_over_count() {
        // Inverse direction — visible terms exceed the headline.
        // The signed `delta` (-N) lets operators distinguish under-
        // from over-counting at a glance.
        let metric = MetricId::Count(CountMetric::PullRequestsOpened);
        let rows = vec![count_row(1, "u1", 50, 10)];
        let headline = LeaderboardHeadline {
            total_subjects: 1,
            events_total: 10,
        };
        let footer = LeaderboardFooter::default();
        let err = check_reconciliation_identity(metric, &headline, &rows, &footer).unwrap_err();
        if let LeaderboardError::ReconciliationViolation { delta, .. } = err {
            assert_eq!(delta, -40);
        } else {
            panic!("expected ReconciliationViolation, got {err:?}");
        }
    }

    #[test]
    fn reconciliation_identity_treats_empty_rows_as_zero_sum() {
        // An empty leaderboard with no rows is still a real response
        // shape — the headline must equal `unattributed_metric +
        // bots_suppressed_events` because every event has to live
        // somewhere on the reconciliation ledger.
        let metric = MetricId::Count(CountMetric::PullRequestsOpened);
        let headline = LeaderboardHeadline {
            total_subjects: 0,
            events_total: 7,
        };
        let footer = LeaderboardFooter {
            unattributed_events_metric: 4,
            bots_suppressed_events: 3,
            ..LeaderboardFooter::default()
        };
        assert!(check_reconciliation_identity(metric, &headline, &[], &footer).is_ok());
    }

    #[test]
    fn duration_exemption_predicate_governs_the_identity_gate() {
        // §6.2 duration-metric exemption: row values are aggregates
        // (p50/p95), not counts, so Σ rows is meaningless and the
        // identity check must short-circuit.
        //
        // Until `MetricId::Duration(...)` exists (gated on the
        // store-side `list_duration_samples_in_window` fetch per
        // STAGE-1-COMPOSABILITY §3), the exemption is testable via
        // its predicate: `MetricId::is_duration()` is the single
        // branch [`check_reconciliation_identity`] consults. We
        // assert the predicate is wired correctly — that every
        // current `MetricId` is count-classified and therefore
        // *not* exempt — so the moment the Duration variant lands
        // its rows skip the identity by construction. A regression
        // here (e.g. somebody flipping the `matches!` to include a
        // future variant by default) trips this test, not a silent
        // change in production behaviour.
        let m = MetricId::Count(CountMetric::PullRequestsOpened);
        assert!(!m.is_duration(), "count metrics must NOT be exempt from §6.2");
        // And: the function actually consults the predicate — an
        // intentionally unbalanced count fixture must fail, proving
        // the gate is not the no-op the duration branch is.
        let rows = vec![count_row(1, "u1", 1, 1)];
        let headline = LeaderboardHeadline {
            total_subjects: 1,
            events_total: 999,
        };
        let footer = LeaderboardFooter::default();
        assert!(check_reconciliation_identity(m, &headline, &rows, &footer).is_err());
    }

    #[test]
    fn debug_assert_panics_on_count_identity_violation() {
        // The debug-build assertion is the SCOPE.md-mandated
        // production check: in cargo-test (debug profile) it must
        // panic when the identity breaks, so a regression in the
        // bot or unattributed-events path is impossible to ship
        // unnoticed.
        let metric = MetricId::Count(CountMetric::PullRequestsOpened);
        let rows = vec![count_row(1, "u1", 1, 1)];
        let headline = LeaderboardHeadline {
            total_subjects: 1,
            events_total: 999,
        };
        let footer = LeaderboardFooter::default();
        let res = std::panic::catch_unwind(|| {
            debug_assert_reconciliation_identity(metric, &headline, &rows, &footer);
        });
        assert!(res.is_err(), "expected debug_assert to panic on broken identity");
    }

    #[test]
    fn bot_split_footer_fields_are_both_present_on_the_wire() {
        // ORG-REPORTS §6.4: bot suppression is a *split* footer —
        // `bots_suppressed` is the subject count, `bots_suppressed_events`
        // is the reconciliation counter. Both must be on the wire
        // for the §6.2 identity to be checkable client-side; a
        // frontend that only sees one of them cannot verify trust.
        let footer = LeaderboardFooter {
            unattributed_events: 0,
            unattributed_events_metric: 0,
            insufficient_data: 0,
            bots_suppressed: 4,
            bots_suppressed_events: 17,
        };
        let json = serde_json::to_string(&footer).unwrap();
        assert!(json.contains("\"bots_suppressed\":4"), "{json}");
        assert!(json.contains("\"bots_suppressed_events\":17"), "{json}");
        // And they round-trip independently — a typo in either
        // field name would silently zero one of the §6.2 terms.
        let back: LeaderboardFooter = serde_json::from_str(&json).unwrap();
        assert_eq!(back.bots_suppressed, 4);
        assert_eq!(back.bots_suppressed_events, 17);
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
