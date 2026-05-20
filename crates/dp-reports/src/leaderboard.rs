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
    /// Pagination request (§6.5). Default is "page 1, default size".
    #[serde(default)]
    pub page: PageRequest,
    /// Extra §15.7 metrics carried into each row's
    /// [`LeaderboardContext::extras`] (ORG-REPORTS §6.3).
    ///
    /// Capped at [`LEADERBOARD_ALSO_COMPUTE_CAP`]; over-cap requests
    /// fail at envelope resolution with
    /// [`LeaderboardError::AlsoComputeTooLarge`]. Server-side sort and
    /// pagination stay single-metric — `rank_by` is authoritative for
    /// page boundaries and the §6.5 cursor — so changing
    /// `also_compute` between requests must never drift which rows
    /// land on which page. Empty by default; omitted from the wire
    /// form when empty so the stage-3 shape stays stable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_compute: Vec<MetricId>,
    /// Small-N "compare these subjects" filter (ORG-REPORTS §6.10).
    ///
    /// When non-empty, ranking is restricted to subjects whose
    /// `subject_id` is in this set; the §6.1 tie-break order still
    /// applies within the filtered population. Capped at
    /// [`LEADERBOARD_SUBJECT_IDS_CAP`]; over-cap requests fail at
    /// envelope resolution with
    /// [`LeaderboardError::SubjectIdsTooLarge`].
    ///
    /// **Pagination is disabled in this mode** — the server returns
    /// every matching row in one response, so the compare-users UI
    /// can pair `subject_ids` with `also_compute` and never deal with
    /// cursors. Sending a cursor or a non-zero `page.size` alongside
    /// a non-empty `subject_ids` is a typed
    /// [`LeaderboardError::PaginationDisabledForSubjectIds`].
    ///
    /// Values are opaque strings (UUIDs for `user`/`team`/`org`,
    /// labels for `home_org_label` — possibly
    /// [`HOME_ORG_LABEL_UNLABELED_BUCKET`]). The store binds the list
    /// as a single `text[]` predicate that lives *outside* the
    /// GROUP BY (§6.10), so the §6.1 tie-break order is unaffected.
    /// Empty by default; omitted from the wire form when empty so
    /// the stage-3 shape stays stable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subject_ids: Vec<String>,
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
    /// `also_compute` echoed back (ORG-REPORTS §6.3). The wire-form
    /// echo lets clients confirm which extra metrics they should
    /// expect under each row's `context.extras`, and lets caches
    /// key on the full (rank_by + extras) tuple. Omitted from the
    /// wire form when empty so responses without extras keep their
    /// stage-3 shape.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_compute: Vec<MetricId>,
    /// `subject_ids` echoed back (ORG-REPORTS §6.10). The wire-form
    /// echo lets clients confirm which subjects the server ranked
    /// against and lets caches key on the full
    /// (rank_by + extras + subject_ids) tuple. Omitted from the wire
    /// form when empty so responses without the small-N filter keep
    /// their stage-3 shape.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subject_ids: Vec<String>,
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
    /// Pagination state (ORG-REPORTS §6.5). Wired in stage 6.
    pub page: LeaderboardPage,
}

// ---------------------------------------------------------------------------
// §6.5 — pinned-cursor pagination
// ---------------------------------------------------------------------------

/// Cap on `also_compute` cardinality. ORG-REPORTS §6.3.
///
/// "Up to 5 §15.7 metrics per row" — beyond that the row's
/// `context.extras` payload grows without bound on a hot path. The
/// cap is enforced at envelope resolution via
/// [`validate_also_compute`] so the SQL layer never has to defend
/// against an unbounded fan-out.
pub const LEADERBOARD_ALSO_COMPUTE_CAP: usize = 5;

/// Cap on `subject_ids` cardinality. ORG-REPORTS §6.10.
///
/// `subject_ids` is the "compare these subjects" small-N path —
/// 50 is the inflection point above which the UI affordance
/// (chips, a side-by-side matrix) stops making sense and the
/// general leaderboard is the right tool. Enforced at envelope
/// resolution via [`validate_subject_ids`] so the SQL layer never
/// has to defend against an unbounded `ANY(...)` predicate.
pub const LEADERBOARD_SUBJECT_IDS_CAP: usize = 50;

/// Default page size when the client omits `page.size`. ORG-REPORTS §6.5.
pub const LEADERBOARD_PAGE_SIZE_DEFAULT: u32 = 25;

/// Maximum page size the server honours. Requests above this are
/// rejected with [`LeaderboardError::PageSizeOutOfRange`]. ORG-REPORTS §6.5.
pub const LEADERBOARD_PAGE_SIZE_MAX: u32 = 200;

/// Wire-form request for one page of a leaderboard.
///
/// `cursor` is opaque to clients — they echo back whatever
/// `LeaderboardResponse.page.next_cursor` carried. `size` is bounded
/// by [`LEADERBOARD_PAGE_SIZE_MAX`]; `0` and absent both mean "use
/// [`LEADERBOARD_PAGE_SIZE_DEFAULT`]" so a missing query param can't
/// silently degrade to an empty page.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PageRequest {
    /// Page size; clamped at construction time by
    /// [`validate_page_request`]. Wire form treats `0` as "use the
    /// default".
    #[serde(default)]
    pub size: u32,
    /// Opaque cursor from a prior response's `next_cursor`. `None`
    /// requests page 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// Decoded cursor body — never serialised directly on the wire (the
/// public form is the opaque [`PageRequest::cursor`] string produced
/// by [`PageCursor::encode`]).
///
/// The triple `(resolved_window_end, rank_by_value, subject_id)` is
/// exactly the §6.5 cursor definition. Pinning `resolved_window_end`
/// here is what makes page 1 → page 2 a single consistent snapshot
/// even when new events land between requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageCursor {
    /// The `[start, end)` end the cursor was minted against. Pinned
    /// into the cursor so a subsequent page is fetched against the
    /// same window even if the envelope would now resolve to a new
    /// one.
    pub resolved_window_end: DateTime<Utc>,
    /// `rank_by` value of the last row on the previous page — the
    /// primary axis of the §6.1 tie-break.
    pub rank_by_value: i64,
    /// `subject_id` of the last row on the previous page — the final
    /// tie-break key. Opaque string (UUID for user/team/org, label
    /// string for `home_org_label`, possibly `__unlabeled__`).
    pub subject_id: String,
}

impl PageCursor {
    /// Encode the cursor as the opaque string a client echoes back.
    ///
    /// Today the wire form is plain JSON — opaque to clients, easy to
    /// diagnose in logs. The encoding is deliberately reversible
    /// without a side channel so a stale cursor in a bug report can
    /// be re-played against a fixture. Changing the encoding is a
    /// breaking change for any cached cursor still on the wire; treat
    /// the function pair as the format boundary.
    pub fn encode(&self) -> String {
        serde_json::to_string(self).expect("PageCursor serialises infallibly")
    }

    /// Decode a wire-form cursor back into its triple, returning a
    /// typed error so the REST/MCP layer can return a precise 400
    /// (the bare error string carries the parse cause).
    pub fn decode(s: &str) -> Result<Self, LeaderboardError> {
        serde_json::from_str(s).map_err(|e| LeaderboardError::CursorDecode(e.to_string()))
    }
}

/// Pagination state on a response. ORG-REPORTS §6.5.
///
/// `has_more` is intentionally independent of `next_cursor.is_some()`
/// so a server can communicate "no more pages" without the client
/// having to introspect the cursor string. They should agree, but the
/// flag is the authoritative signal — clients must check it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaderboardPage {
    /// Opaque cursor to pass back as `PageRequest.cursor` for the
    /// next page. `None` when there are no more pages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// `true` while more pages remain. `false` on the final page,
    /// regardless of whether `next_cursor` is `Some`.
    pub has_more: bool,
}

/// Resolve a [`PageRequest`]'s effective page size, clamping the
/// default-substitution rule (`0` → default) and rejecting values
/// over [`LEADERBOARD_PAGE_SIZE_MAX`].
///
/// The clamp lives here rather than the `Deserialize` impl so REST
/// (which converts `?size=` query strings) and MCP (which passes
/// structured args) hit the exact same bound.
pub fn effective_page_size(req: &PageRequest) -> Result<u32, LeaderboardError> {
    let size = if req.size == 0 {
        LEADERBOARD_PAGE_SIZE_DEFAULT
    } else {
        req.size
    };
    if size > LEADERBOARD_PAGE_SIZE_MAX {
        return Err(LeaderboardError::PageSizeOutOfRange {
            size,
            max: LEADERBOARD_PAGE_SIZE_MAX,
        });
    }
    Ok(size)
}

/// Validate a [`PageRequest`] against a freshly resolved envelope
/// (ORG-REPORTS §6.5).
///
/// Two checks:
///
/// 1. **Size bound** — see [`effective_page_size`].
/// 2. **Cursor-window match** — if a cursor is present, its
///    `resolved_window_end` must equal the freshly-resolved
///    envelope's `resolved_window.end`. Drift is a §11.4 trust
///    violation: a client that changed `window.label` mid-paginate
///    would otherwise silently mix two snapshots. The check rejects
///    *any* drift, not just "forward" drift — going backwards is
///    equally a misuse.
///
/// Returns the effective page size on success so callers don't
/// re-compute it.
pub fn validate_page_request(
    req: &PageRequest,
    resolved: &ResolvedLeaderboardEnvelope,
) -> Result<u32, LeaderboardError> {
    // §6.10: pagination is disabled in `subject_ids` mode. A cursor
    // or an explicit non-default `size` is a client bug, not a
    // request to quietly fall back. We check this *before* the size
    // bound so an over-cap size in subject_ids mode reports the more
    // actionable error.
    if !resolved.subject_ids.is_empty() && (req.cursor.is_some() || req.size != 0) {
        return Err(LeaderboardError::PaginationDisabledForSubjectIds {
            subject_ids_len: resolved.subject_ids.len(),
        });
    }
    let size = effective_page_size(req)?;
    if let Some(raw) = req.cursor.as_deref() {
        let cursor = PageCursor::decode(raw)?;
        if cursor.resolved_window_end != resolved.resolved_window.end {
            return Err(LeaderboardError::CursorWindowMismatch {
                cursor_window_end: cursor.resolved_window_end,
                resolved_window_end: resolved.resolved_window.end,
            });
        }
    }
    Ok(size)
}

/// Build the next-page cursor from the last row on the current page.
///
/// Returns `None` when `rows` is empty (no cursor to mint).
/// Callers compare `rows.len()` against the requested page size to
/// decide `has_more`; this helper only mints the cursor itself.
pub fn build_next_cursor(
    resolved: &ResolvedLeaderboardEnvelope,
    rows: &[LeaderboardRow],
) -> Option<String> {
    rows.last().map(|r| {
        PageCursor {
            resolved_window_end: resolved.resolved_window.end,
            rank_by_value: r.primary.value,
            subject_id: r.subject_id.clone(),
        }
        .encode()
    })
}

/// Bind order for [`build_paginated_leaderboard_sql`] when called
/// **without** a cursor. The base order from
/// [`LEADERBOARD_BIND_ORDER`] is extended with one trailing slot for
/// the `LIMIT`.
pub const LEADERBOARD_BIND_ORDER_PAGED: &[&str] = &[
    "$1 window.start (timestamptz)",
    "$2 window.end (timestamptz, exclusive)",
    "$3 org_ids (uuid[]; cardinality >= 1)",
    "$4 event_kind (text — from CountMetric::event_kind())",
    "$5 actor_roles (text[] — from envelope.actor_roles or CountMetric::default_actor_roles())",
    "$6 repos (uuid[]; cardinality 0 == no filter)",
    "$7 page_size (int; effective_page_size)",
];

/// Bind order for [`build_paginated_leaderboard_sql`] when called
/// **with** a cursor. Two trailing slots are appended to the base
/// order: the cursor's `(rank_by_value, subject_id)` tuple, then the
/// `LIMIT`.
pub const LEADERBOARD_BIND_ORDER_PAGED_WITH_CURSOR: &[&str] = &[
    "$1 window.start (timestamptz)",
    "$2 window.end (timestamptz, exclusive)",
    "$3 org_ids (uuid[]; cardinality >= 1)",
    "$4 event_kind (text — from CountMetric::event_kind())",
    "$5 actor_roles (text[] — from envelope.actor_roles or CountMetric::default_actor_roles())",
    "$6 repos (uuid[]; cardinality 0 == no filter)",
    "$7 cursor.rank_by_value (bigint)",
    "$8 cursor.subject_id (text)",
    "$9 page_size (int; effective_page_size)",
];

/// Wrap the base [`build_leaderboard_sql`] in a paginated subquery.
///
/// Pagination is a *predicate over the aggregate result*, not a
/// `WHERE` on the raw events — so the cursor `(primary_value,
/// subject_id) < ($cursor_value, $cursor_id)` lives on the *outer*
/// query around the GROUP BY. Wrapping is the cheapest way to keep
/// the §6.1 tie-break order from each per-variant SQL string
/// authoritative without re-implementing it here.
///
/// When `has_cursor == false`, the SQL appends `LIMIT $7` only —
/// page 1 returns the head of the §6.1 order.
///
/// When `has_cursor == true`, the SQL becomes:
///
/// ```text
/// SELECT * FROM ( <base SQL with its ORDER BY> ) AS sub
///  WHERE (sub.primary_value, sub.subject_id) < ($7::bigint, $8::text)
///  LIMIT $9
/// ```
///
/// The tuple comparison is strict (`<`, not `<=`) so the row that
/// was the cursor on the previous page is not re-emitted. PostgreSQL
/// preserves the inner `ORDER BY` through the outer `SELECT *` here
/// in practice, but we re-emit the §6.1 clause on the outer query
/// too so any subsequent rewrite cannot drift.
/// Bind order for [`build_subject_ids_leaderboard_sql`].
///
/// The base [`LEADERBOARD_BIND_ORDER`] is extended with one
/// trailing slot for the `subject_ids` predicate. Pagination is
/// disabled in this mode (§6.10), so there is no `LIMIT` slot and
/// no cursor slot — the server returns every matching row in one
/// response.
pub const LEADERBOARD_BIND_ORDER_SUBJECT_IDS: &[&str] = &[
    "$1 window.start (timestamptz)",
    "$2 window.end (timestamptz, exclusive)",
    "$3 org_ids (uuid[]; cardinality >= 1)",
    "$4 event_kind (text — from CountMetric::event_kind())",
    "$5 actor_roles (text[] — from envelope.actor_roles or CountMetric::default_actor_roles())",
    "$6 repos (uuid[]; cardinality 0 == no filter)",
    "$7 subject_ids (text[]; cardinality 1..=LEADERBOARD_SUBJECT_IDS_CAP)",
];

/// Wrap the base [`build_leaderboard_sql`] in a `subject_ids` filter
/// for the §6.10 small-N "compare these subjects" path.
///
/// The filter lives on the *outer* query — `WHERE sub.subject_id =
/// ANY($7::text[])` — so the inner GROUP BY and §6.1 tie-break
/// `ORDER BY` are untouched. This is the same wrapping trick
/// [`build_paginated_leaderboard_sql`] uses, for the same reason:
/// it keeps each per-variant SQL string authoritative for the
/// aggregate shape.
///
/// No `LIMIT` is emitted — pagination is disabled in this mode
/// (§6.10), and the cap on `subject_ids` cardinality
/// ([`LEADERBOARD_SUBJECT_IDS_CAP`] = 50) is what bounds the
/// response size. The outer `ORDER BY` is re-emitted so callers
/// can rely on §6.1 ordering even if a future PostgreSQL rewrite
/// drops the inner sort.
pub fn build_subject_ids_leaderboard_sql(
    subject: SubjectKind,
    scope_mode: ScopeMode,
) -> Result<String, LeaderboardError> {
    let base = build_leaderboard_sql(subject, scope_mode)?;
    Ok(format!(
        "SELECT * FROM ({base}) AS sub \
          WHERE sub.subject_id = ANY($7::text[]) \
          {tie_break}",
        base = base,
        tie_break = LEADERBOARD_TIE_BREAK_ORDER_BY_CLAUSE,
    ))
}

pub fn build_paginated_leaderboard_sql(
    subject: SubjectKind,
    scope_mode: ScopeMode,
    has_cursor: bool,
) -> Result<String, LeaderboardError> {
    let base = build_leaderboard_sql(subject, scope_mode)?;
    Ok(if has_cursor {
        format!(
            "SELECT * FROM ({base}) AS sub \
              WHERE (sub.primary_value, sub.subject_id) < ($7::bigint, $8::text) \
              {tie_break} \
              LIMIT $9",
            base = base,
            tie_break = LEADERBOARD_TIE_BREAK_ORDER_BY_CLAUSE,
        )
    } else {
        format!(
            "SELECT * FROM ({base}) AS sub {tie_break} LIMIT $7",
            base = base,
            tie_break = LEADERBOARD_TIE_BREAK_ORDER_BY_CLAUSE,
        )
    })
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
    /// The cursor pinned to a different `resolved_window_end` than
    /// the freshly-resolved envelope produces. ORG-REPORTS §6.5: the
    /// server refuses to silently mix two snapshots — the client
    /// must re-fetch page 1 against the new envelope.
    #[error(
        "cursor_window_mismatch: cursor pinned resolved_window_end={cursor_window_end} \
         but envelope now resolves to {resolved_window_end}"
    )]
    CursorWindowMismatch {
        /// Window end the cursor was minted against.
        cursor_window_end: DateTime<Utc>,
        /// Window end the envelope would resolve to right now.
        resolved_window_end: DateTime<Utc>,
    },
    /// The cursor string failed to parse. The wire form is opaque
    /// but reversible (see [`PageCursor::encode`] / [`PageCursor::decode`]);
    /// a malformed cursor is a client bug, surfaced here so REST and
    /// MCP can return a precise 400.
    #[error("cursor_invalid: {0}")]
    CursorDecode(String),
    /// `page.size` exceeded [`LEADERBOARD_PAGE_SIZE_MAX`]. §6.5.
    #[error("page_size_out_of_range: size={size} max={max}")]
    PageSizeOutOfRange {
        /// Requested size after default substitution.
        size: u32,
        /// Server-side cap ([`LEADERBOARD_PAGE_SIZE_MAX`]).
        max: u32,
    },
    /// `also_compute` exceeded [`LEADERBOARD_ALSO_COMPUTE_CAP`]. §6.3.
    #[error("also_compute_too_large: len={len} cap={cap}")]
    AlsoComputeTooLarge {
        /// Requested `also_compute` length.
        len: usize,
        /// Server-side cap ([`LEADERBOARD_ALSO_COMPUTE_CAP`], = 5).
        cap: usize,
    },
    /// `subject_ids` exceeded [`LEADERBOARD_SUBJECT_IDS_CAP`]. §6.10.
    ///
    /// The wire form is `400 subject_ids_too_large`; the typed
    /// payload echoes the requested length and the server-side cap
    /// (= 50) so a UI that paginated a chip-picker over the cap can
    /// render a precise message without re-grepping the spec.
    #[error("subject_ids_too_large: len={len} cap={cap}")]
    SubjectIdsTooLarge {
        /// Requested `subject_ids` length.
        len: usize,
        /// Server-side cap ([`LEADERBOARD_SUBJECT_IDS_CAP`], = 50).
        cap: usize,
    },
    /// Pagination was requested alongside a non-empty `subject_ids`.
    /// §6.10: in `subject_ids` mode the server returns every
    /// matching row in one response, so a cursor or non-zero
    /// `page.size` is a client bug rather than a quietly-degraded
    /// query. Surfaced as a typed error so REST and MCP can return
    /// a precise 400.
    #[error("pagination_disabled_for_subject_ids: subject_ids mode returns all rows in one response (subject_ids_len={subject_ids_len})")]
    PaginationDisabledForSubjectIds {
        /// Cardinality of the requested `subject_ids` set, echoed
        /// back so the client message can be specific.
        subject_ids_len: usize,
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

/// Validate the `also_compute` list against the §6.3 cap.
///
/// The cap is enforced at envelope resolution so the SQL layer
/// never sees an over-sized extras payload — a future "compare
/// these users" flow (§6.9) that passes `also_compute` from a UI
/// dropdown gets a precise 400 instead of a quietly degraded query.
/// Returns `Ok(())` on the empty list (the stage-3 shape).
pub fn validate_also_compute(also_compute: &[MetricId]) -> Result<(), LeaderboardError> {
    if also_compute.len() > LEADERBOARD_ALSO_COMPUTE_CAP {
        return Err(LeaderboardError::AlsoComputeTooLarge {
            len: also_compute.len(),
            cap: LEADERBOARD_ALSO_COMPUTE_CAP,
        });
    }
    Ok(())
}

/// Validate the `subject_ids` list against the §6.10 cap.
///
/// Returns `Ok(())` on the empty list (the default "no small-N
/// filter" shape). Over-cap requests surface
/// [`LeaderboardError::SubjectIdsTooLarge`] at envelope resolution
/// so the SQL layer never sees an unbounded `ANY(...)` predicate.
pub fn validate_subject_ids(subject_ids: &[String]) -> Result<(), LeaderboardError> {
    if subject_ids.len() > LEADERBOARD_SUBJECT_IDS_CAP {
        return Err(LeaderboardError::SubjectIdsTooLarge {
            len: subject_ids.len(),
            cap: LEADERBOARD_SUBJECT_IDS_CAP,
        });
    }
    Ok(())
}

/// Resolve a [`LeaderboardEnvelope`] at `now`, returning the
/// [`ResolvedLeaderboardEnvelope`] echoed in the response.
///
/// Stage 4 accepts every valid `(subject, scope_mode)` pair per
/// ORG-REPORTS §2 and validates the `orgs` cardinality contract per
/// scope mode. Stage 7 additionally enforces the §6.3 `also_compute`
/// cap; over-cap requests fail with
/// [`LeaderboardError::AlsoComputeTooLarge`].
pub fn resolve_leaderboard_envelope(
    env: &LeaderboardEnvelope,
    now: DateTime<Utc>,
) -> Result<ResolvedLeaderboardEnvelope, LeaderboardError> {
    validate_subject_scope_combo(env.subject, env.scope_mode)?;
    validate_also_compute(&env.also_compute)?;
    validate_subject_ids(&env.subject_ids)?;
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
        also_compute: env.also_compute.clone(),
        subject_ids: env.subject_ids.clone(),
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
            page: PageRequest::default(),
            also_compute: vec![],
            subject_ids: vec![],
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

    // ----- Stage 6: §6.5 pinned-cursor pagination ----------------------

    fn resolved_env_with_window_end(end: DateTime<Utc>) -> ResolvedLeaderboardEnvelope {
        ResolvedLeaderboardEnvelope {
            resolved_at: end,
            resolved_window: Window {
                start: end - chrono::Duration::days(7),
                end,
                label: "last_week".into(),
                tz: "UTC".into(),
                anchor: WindowAnchor::Utc,
            },
            scope_mode: ScopeMode::SingleOrg,
            subject: SubjectKind::User,
            rank_by: MetricId::Count(CountMetric::PullRequestsOpened),
            also_compute: vec![],
            subject_ids: vec![],
        }
    }

    #[test]
    fn page_size_defaults_apply_when_zero() {
        // §6.5: `size = 0` (or absent) means "use the default". A
        // missing query param must not collapse to an empty page.
        let req = PageRequest::default();
        assert_eq!(effective_page_size(&req).unwrap(), LEADERBOARD_PAGE_SIZE_DEFAULT);
    }

    #[test]
    fn page_size_rejects_values_above_the_cap() {
        // §6.5: 200 is the cap; 201 must trip
        // PageSizeOutOfRange. Locking this here means a server
        // operator can audit the cap without grepping for the
        // constant.
        let req = PageRequest {
            size: LEADERBOARD_PAGE_SIZE_MAX + 1,
            cursor: None,
        };
        let err = effective_page_size(&req).unwrap_err();
        assert_eq!(
            err,
            LeaderboardError::PageSizeOutOfRange {
                size: LEADERBOARD_PAGE_SIZE_MAX + 1,
                max: LEADERBOARD_PAGE_SIZE_MAX,
            }
        );
    }

    #[test]
    fn page_request_round_trips_through_json_with_optional_cursor() {
        // Wire form: absent cursor is omitted; present cursor is a
        // plain string. Locking the wire form here keeps REST and MCP
        // in lock-step without each one reimplementing the shape.
        let no_cursor = PageRequest { size: 50, cursor: None };
        let json = serde_json::to_string(&no_cursor).unwrap();
        assert!(!json.contains("cursor"), "absent cursor must not serialise: {json}");
        let back: PageRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, no_cursor);

        let with_cursor = PageRequest {
            size: 25,
            cursor: Some("opaque".into()),
        };
        let json = serde_json::to_string(&with_cursor).unwrap();
        assert!(json.contains("\"cursor\":\"opaque\""), "{json}");
    }

    #[test]
    fn page_cursor_round_trips_through_encode_decode() {
        // §6.5 cursor triple: (resolved_window_end, rank_by_value,
        // subject_id). The encode/decode pair is the format
        // boundary; changing the encoding is a breaking change.
        let c = PageCursor {
            resolved_window_end: utc(2025, 6, 16, 0, 0, 0),
            rank_by_value: 42,
            subject_id: "user-abc".into(),
        };
        let encoded = c.encode();
        let back = PageCursor::decode(&encoded).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn page_cursor_decode_surfaces_a_parse_error() {
        // Garbage in → CursorDecode out, not a panic. The REST/MCP
        // layer maps this to a precise 400.
        let err = PageCursor::decode("not json").unwrap_err();
        match err {
            LeaderboardError::CursorDecode(msg) => assert!(!msg.is_empty()),
            other => panic!("expected CursorDecode, got {other:?}"),
        }
    }

    #[test]
    fn validate_page_request_honours_a_stale_but_consistent_cursor() {
        // §6.5: "a subsequent page request with a stale
        // resolved_window_end is honoured (server re-uses the pinned
        // window)." Operationally: if the cursor's pinned window-end
        // matches the freshly-resolved envelope's window-end, the
        // cursor is honoured — even though the request arrived after
        // page 1 was minted. A real clock tick between page 1 and
        // page 2 within the same week resolves last_week to the same
        // [start, end), so the cursor is still valid.
        let window_end = utc(2025, 6, 16, 0, 0, 0);
        let resolved = resolved_env_with_window_end(window_end);
        let cursor = PageCursor {
            resolved_window_end: window_end,
            rank_by_value: 10,
            subject_id: "u1".into(),
        };
        let req = PageRequest {
            size: 25,
            cursor: Some(cursor.encode()),
        };
        let size = validate_page_request(&req, &resolved)
            .expect("stale-but-consistent cursor must be honoured");
        assert_eq!(size, 25);
    }

    #[test]
    fn validate_page_request_rejects_re_resolved_cursor_with_400_mismatch() {
        // §6.5: "a request whose envelope window has moved forward
        // returns a 400 cursor_window_mismatch rather than silently
        // mixing two snapshots." Operationally: the cursor was minted
        // against page 1's window; the new request (e.g. arriving
        // after a week boundary, or with a changed window.label)
        // resolves to a *different* window-end. The server refuses to
        // mix snapshots.
        let prev_window_end = utc(2025, 6, 16, 0, 0, 0);
        let new_window_end = utc(2025, 6, 23, 0, 0, 0); // a week later
        let resolved = resolved_env_with_window_end(new_window_end);
        let cursor = PageCursor {
            resolved_window_end: prev_window_end,
            rank_by_value: 10,
            subject_id: "u1".into(),
        };
        let req = PageRequest {
            size: 25,
            cursor: Some(cursor.encode()),
        };
        let err = validate_page_request(&req, &resolved).unwrap_err();
        assert_eq!(
            err,
            LeaderboardError::CursorWindowMismatch {
                cursor_window_end: prev_window_end,
                resolved_window_end: new_window_end,
            }
        );
    }

    #[test]
    fn validate_page_request_rejects_cursor_window_drift_in_either_direction() {
        // Going *backwards* (cursor newer than envelope) is equally a
        // misuse — a client shouldn't be able to splice page 2 of a
        // future snapshot into page 1 of an older one. The error
        // surfaces with the actual mismatch so operators can diagnose
        // which side drifted.
        let cursor_end = utc(2025, 6, 23, 0, 0, 0);
        let envelope_end = utc(2025, 6, 16, 0, 0, 0);
        let resolved = resolved_env_with_window_end(envelope_end);
        let cursor = PageCursor {
            resolved_window_end: cursor_end,
            rank_by_value: 1,
            subject_id: "u1".into(),
        };
        let req = PageRequest {
            size: 0, // exercise default substitution at the same time
            cursor: Some(cursor.encode()),
        };
        let err = validate_page_request(&req, &resolved).unwrap_err();
        assert!(matches!(err, LeaderboardError::CursorWindowMismatch { .. }));
    }

    #[test]
    fn validate_page_request_surfaces_decode_errors() {
        // A garbage cursor is a 400-class error too — but it's
        // CursorDecode, not CursorWindowMismatch, so the REST/MCP
        // layer can distinguish "client sent malformed cursor" from
        // "client changed the window between calls".
        let resolved = resolved_env_with_window_end(utc(2025, 6, 16, 0, 0, 0));
        let req = PageRequest {
            size: 25,
            cursor: Some("not-json".into()),
        };
        let err = validate_page_request(&req, &resolved).unwrap_err();
        assert!(matches!(err, LeaderboardError::CursorDecode(_)));
    }

    #[test]
    fn build_next_cursor_returns_none_for_empty_rows() {
        // No rows → no cursor to mint. has_more should be `false` in
        // that case, which the caller decides independently.
        let resolved = resolved_env_with_window_end(utc(2025, 6, 16, 0, 0, 0));
        assert!(build_next_cursor(&resolved, &[]).is_none());
    }

    #[test]
    fn build_next_cursor_uses_the_last_row_and_pins_the_window() {
        // Page 2's cursor is page 1's last row — that's how the §6.1
        // tie-break stays stable across pages. The window-end is
        // pinned from the resolved envelope so the snapshot survives
        // event arrivals between requests.
        let window_end = utc(2025, 6, 16, 0, 0, 0);
        let resolved = resolved_env_with_window_end(window_end);
        let rows = vec![
            count_row(1, "u1", 12, 5),
            count_row(2, "u2", 7, 4),
            count_row(3, "u3", 3, 2),
        ];
        let raw = build_next_cursor(&resolved, &rows).expect("non-empty rows must mint a cursor");
        let c = PageCursor::decode(&raw).unwrap();
        assert_eq!(c.resolved_window_end, window_end);
        assert_eq!(c.rank_by_value, 3);
        assert_eq!(c.subject_id, "u3");
    }

    #[test]
    fn paginated_sql_without_cursor_appends_limit_only() {
        // Page 1 has no cursor predicate — only a LIMIT. The §6.1
        // tie-break clause is re-emitted on the outer query so any
        // future rewrite of the inner SQL cannot drift the order.
        let sql = build_paginated_leaderboard_sql(
            SubjectKind::User,
            ScopeMode::SingleOrg,
            false,
        )
        .unwrap();
        let upper = sql.to_ascii_uppercase();
        assert!(upper.contains(" LIMIT $7"), "{sql}");
        assert!(!sql.contains("(sub.primary_value"), "page 1 must not carry a cursor predicate: {sql}");
        assert!(sql.contains(LEADERBOARD_TIE_BREAK_ORDER_BY_CLAUSE), "{sql}");
    }

    #[test]
    fn paginated_sql_with_cursor_appends_tuple_predicate_and_limit() {
        // §6.5: `(primary_value, subject_id) < ($cursor)` — a strict
        // tuple comparison so the cursor row itself is not re-emitted
        // on the next page. The LIMIT slot moves to $9 with the
        // cursor's two slots ($7, $8) in between.
        let sql = build_paginated_leaderboard_sql(
            SubjectKind::User,
            ScopeMode::SingleOrg,
            true,
        )
        .unwrap();
        assert!(
            sql.contains("(sub.primary_value, sub.subject_id) < ($7::bigint, $8::text)"),
            "missing tuple predicate: {sql}",
        );
        assert!(sql.contains(" LIMIT $9"), "{sql}");
        assert!(sql.contains(LEADERBOARD_TIE_BREAK_ORDER_BY_CLAUSE), "{sql}");
    }

    #[test]
    fn paginated_sql_rejects_invalid_subject_scope_combo() {
        // The pagination wrapper inherits the §2 validation: invalid
        // (subject, scope) pairs must still fail with
        // InvalidSubjectScopeCombo, not produce a meaningless paged
        // string.
        let err = build_paginated_leaderboard_sql(
            SubjectKind::Org,
            ScopeMode::SingleOrg,
            true,
        )
        .unwrap_err();
        assert!(matches!(err, LeaderboardError::InvalidSubjectScopeCombo { .. }));
    }

    #[test]
    fn paginated_bind_orders_match_the_documented_slots() {
        // The bind-order constants are the contract the
        // dp-store-pg adapter binds against; drift here is the §11.4
        // divergence trap.
        assert_eq!(LEADERBOARD_BIND_ORDER_PAGED.len(), 7);
        assert_eq!(LEADERBOARD_BIND_ORDER_PAGED_WITH_CURSOR.len(), 9);

        let no_cursor = build_paginated_leaderboard_sql(
            SubjectKind::User,
            ScopeMode::SingleOrg,
            false,
        )
        .unwrap();
        for i in 1..=7 {
            assert!(no_cursor.contains(&format!("${i}")), "missing ${i} in: {no_cursor}");
        }

        let with_cursor = build_paginated_leaderboard_sql(
            SubjectKind::User,
            ScopeMode::SingleOrg,
            true,
        )
        .unwrap();
        for i in 1..=9 {
            assert!(with_cursor.contains(&format!("${i}")), "missing ${i} in: {with_cursor}");
        }
    }

    #[test]
    fn response_serialises_page_block_with_explicit_has_more() {
        // `has_more` is on the wire even when false so a client never
        // has to introspect the cursor string to decide whether to
        // request another page.
        let page = LeaderboardPage::default();
        let json = serde_json::to_string(&page).unwrap();
        assert!(json.contains("\"has_more\":false"), "{json}");
        assert!(!json.contains("next_cursor"), "absent cursor must omit: {json}");

        let page = LeaderboardPage {
            next_cursor: Some("c".into()),
            has_more: true,
        };
        let json = serde_json::to_string(&page).unwrap();
        assert!(json.contains("\"next_cursor\":\"c\""), "{json}");
        assert!(json.contains("\"has_more\":true"), "{json}");
    }

    #[test]
    fn envelope_default_page_round_trips_with_no_cursor_field() {
        // The envelope's `page` defaults so an existing report URL
        // can be pivoted into a leaderboard without adding paging
        // params. Backwards compat: a missing `page` in the request
        // JSON must deserialise as the default.
        let json = r#"{
            "window": { "label": "today", "tz": "UTC", "anchor": "utc" },
            "scope_mode": "single_org",
            "orgs": ["00000000-0000-0000-0000-000000000000"],
            "subject": "user",
            "rank_by": { "family": "count", "id": "pull_requests_opened" }
        }"#;
        let env: LeaderboardEnvelope = serde_json::from_str(json).unwrap();
        assert_eq!(env.page, PageRequest::default());
        assert!(env.page.cursor.is_none());
        assert_eq!(env.page.size, 0); // → effective_page_size = default
    }

    // ----- Stage 7: §6.3 `also_compute` --------------------------------

    /// Build an `also_compute` list of `n` distinct count metrics for
    /// cap-boundary tests. We only need stable identity, not real
    /// semantic distinctness — the validator is purely cardinality.
    fn also_compute_of(n: usize) -> Vec<MetricId> {
        let pool = [
            MetricId::Count(CountMetric::PullRequestsOpened),
            MetricId::Count(CountMetric::PullRequestsMerged),
            MetricId::Count(CountMetric::PullRequestsReviewed),
            MetricId::Count(CountMetric::IssuesOpened),
            MetricId::Count(CountMetric::IssuesClosed),
            MetricId::Count(CountMetric::CommitsAuthored),
            MetricId::Count(CountMetric::ReviewComments),
        ];
        (0..n).map(|i| pool[i % pool.len()]).collect()
    }

    #[test]
    fn also_compute_cap_is_five() {
        // §6.3 nails the cap at 5 — surface it as a const so a future
        // bump is a single, reviewable change rather than scattered
        // magic numbers across REST / MCP / frontend.
        assert_eq!(LEADERBOARD_ALSO_COMPUTE_CAP, 5);
    }

    #[test]
    fn validate_also_compute_accepts_empty_and_up_to_cap() {
        assert!(validate_also_compute(&[]).is_ok());
        assert!(validate_also_compute(&also_compute_of(1)).is_ok());
        assert!(validate_also_compute(&also_compute_of(LEADERBOARD_ALSO_COMPUTE_CAP)).is_ok());
    }

    #[test]
    fn validate_also_compute_rejects_over_cap() {
        // §6.3: anything beyond 5 must fail with the typed error so
        // REST/MCP can return a precise 400.
        let too_many = also_compute_of(LEADERBOARD_ALSO_COMPUTE_CAP + 1);
        let err = validate_also_compute(&too_many).unwrap_err();
        assert_eq!(
            err,
            LeaderboardError::AlsoComputeTooLarge {
                len: LEADERBOARD_ALSO_COMPUTE_CAP + 1,
                cap: LEADERBOARD_ALSO_COMPUTE_CAP,
            }
        );
    }

    #[test]
    fn resolve_envelope_enforces_also_compute_cap() {
        // The cap must be checked at envelope resolution — not at the
        // SQL layer — so an over-sized payload never reaches the store.
        let mut env = sample_envelope();
        env.also_compute = also_compute_of(LEADERBOARD_ALSO_COMPUTE_CAP + 1);
        let err = resolve_leaderboard_envelope(&env, utc(2025, 6, 18, 12, 0, 0)).unwrap_err();
        assert!(matches!(err, LeaderboardError::AlsoComputeTooLarge { .. }));
    }

    #[test]
    fn resolve_envelope_echoes_also_compute_in_resolved_form() {
        // §4 echo rule: the resolved envelope carries every input
        // axis the response shape can depend on. Clients keying their
        // cache on (rank_by + extras) need the echo.
        let mut env = sample_envelope();
        env.also_compute = also_compute_of(3);
        let resolved = resolve_leaderboard_envelope(&env, utc(2025, 6, 18, 12, 0, 0)).unwrap();
        assert_eq!(resolved.also_compute, env.also_compute);
    }

    #[test]
    fn envelope_also_compute_round_trips_through_json() {
        // Wire form: present when non-empty, omitted when empty so
        // the stage-3 shape stays stable for clients that don't use
        // the §6.3 escape hatch.
        let mut env = sample_envelope();
        assert!(env.also_compute.is_empty());
        let json = serde_json::to_string(&env).unwrap();
        assert!(
            !json.contains("also_compute"),
            "empty also_compute must be omitted: {json}",
        );

        env.also_compute = also_compute_of(2);
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("\"also_compute\""), "{json}");
        let back: LeaderboardEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(back.also_compute, env.also_compute);
    }

    #[test]
    fn envelope_default_also_compute_is_empty_when_field_absent() {
        // Backwards compat: a missing `also_compute` in the request
        // JSON must deserialise as an empty list. A previously-shipped
        // client that pre-dates §6.3 keeps working unchanged.
        let json = r#"{
            "window": { "label": "today", "tz": "UTC", "anchor": "utc" },
            "scope_mode": "single_org",
            "orgs": ["00000000-0000-0000-0000-000000000000"],
            "subject": "user",
            "rank_by": { "family": "count", "id": "pull_requests_opened" }
        }"#;
        let env: LeaderboardEnvelope = serde_json::from_str(json).unwrap();
        assert!(env.also_compute.is_empty());
    }

    #[test]
    fn context_extras_serialise_under_row_context() {
        // §6.3: extras live under `row.context`, keyed by metric. A
        // dropped key here would silently degrade the multi-metric UI
        // to single-metric — locked in a test so a future
        // restructuring of `LeaderboardContext` is loud.
        let mut row = count_row(1, "u1", 10, 5);
        row.context.extras.insert(
            "reviews_given".into(),
            serde_json::json!({ "value": 41 }),
        );
        let json = serde_json::to_string(&row).unwrap();
        assert!(json.contains("\"context\""), "{json}");
        assert!(
            json.contains("\"reviews_given\":{\"value\":41}"),
            "extras must serialise under context: {json}",
        );
    }

    #[test]
    fn paginated_sql_signature_is_independent_of_also_compute() {
        // The pagination wrapper is purely a function of (subject,
        // scope_mode, has_cursor) — `also_compute` cannot reach
        // [`build_paginated_leaderboard_sql`] because pagination is
        // single-metric by design. Locked here: the function has no
        // also_compute parameter, the SQL string it emits has no
        // reference to extras, and the bind-order constants stay at
        // 7 / 9 slots.
        let sql = build_paginated_leaderboard_sql(
            SubjectKind::User,
            ScopeMode::SingleOrg,
            true,
        )
        .unwrap();
        assert!(!sql.to_ascii_lowercase().contains("also_compute"), "{sql}");
        assert!(!sql.to_ascii_lowercase().contains("extras"), "{sql}");
        assert_eq!(LEADERBOARD_BIND_ORDER_PAGED.len(), 7);
        assert_eq!(LEADERBOARD_BIND_ORDER_PAGED_WITH_CURSOR.len(), 9);
    }

    #[test]
    fn page_boundary_cursor_is_invariant_under_also_compute_changes() {
        // **The stage-7 invariant.** §6.3: "server-side sort and
        // pagination are always on `rank_by`." Concretely: the next
        // cursor is `(resolved_window_end, rank_by_value,
        // subject_id)`. Adding, removing, or reordering `also_compute`
        // metrics — and the `context.extras` payload they materialise
        // into — must not change the cursor for the same page.
        //
        // A drift here would mean a UI that toggled a "show reviews
        // alongside" affordance would silently see *different rows*
        // on page 2, even though the rank order is unchanged. That's
        // exactly the §11.4 trust violation the §6.3 single-metric
        // pagination rule exists to prevent.
        let window_end = utc(2025, 6, 16, 0, 0, 0);
        let resolved = resolved_env_with_window_end(window_end);

        // Page 1 with no extras.
        let mut rows_a = vec![
            count_row(1, "u1", 12, 5),
            count_row(2, "u2", 7, 4),
            count_row(3, "u3", 3, 2),
        ];
        let cursor_a = build_next_cursor(&resolved, &rows_a).unwrap();

        // Same page, identical rank_by ordering, but every row now
        // carries five §15.7 extras under `context.extras`. The
        // rank_by value and subject_id of the last row are unchanged
        // — and that's all the cursor consults.
        for r in &mut rows_a {
            for k in ["reviews_given", "issues_opened", "issues_closed", "commits", "comments"] {
                r.context.extras.insert(
                    k.into(),
                    serde_json::json!({ "value": 999 }),
                );
            }
        }
        let cursor_b = build_next_cursor(&resolved, &rows_a).unwrap();
        assert_eq!(
            cursor_a, cursor_b,
            "cursor must be invariant under also_compute / extras changes",
        );

        // And: the decoded triple really is the rank_by-and-id-only
        // triple from §6.5, not a hash over the full row.
        let decoded = PageCursor::decode(&cursor_a).unwrap();
        assert_eq!(decoded.resolved_window_end, window_end);
        assert_eq!(decoded.rank_by_value, 3);
        assert_eq!(decoded.subject_id, "u3");
    }

    #[test]
    fn page_boundary_validation_ignores_also_compute_on_envelope() {
        // Concrete companion to the cursor-invariance test:
        // [`validate_page_request`] consults only the resolved
        // envelope's `resolved_window.end`. The cursor minted by a
        // request *without* `also_compute` must still be honoured on
        // a follow-up request *with* `also_compute` — and vice versa
        // — because §6.3 promises page boundaries don't shift.
        let window_end = utc(2025, 6, 16, 0, 0, 0);
        let mut env = sample_envelope();
        let resolved_no_extras =
            resolve_leaderboard_envelope(&env, utc(2025, 6, 18, 12, 0, 0)).unwrap();

        // Mint a cursor against the no-extras resolved envelope.
        let cursor = PageCursor {
            resolved_window_end: resolved_no_extras.resolved_window.end,
            rank_by_value: 10,
            subject_id: "u1".into(),
        };
        let req = PageRequest { size: 25, cursor: Some(cursor.encode()) };

        // Now flip on `also_compute` and re-resolve at the same wall
        // clock. The resolved window-end is the same, so the cursor
        // must still validate cleanly.
        env.also_compute = also_compute_of(5);
        let resolved_with_extras =
            resolve_leaderboard_envelope(&env, utc(2025, 6, 18, 12, 0, 0)).unwrap();
        assert_eq!(
            resolved_with_extras.resolved_window.end,
            resolved_no_extras.resolved_window.end,
        );
        // For the assertion below we need the fixture-style resolved
        // envelope shape (page-validation only looks at the window
        // end); use the explicit constructor so the assertion is
        // independent of any future fields.
        let fixture = resolved_env_with_window_end(window_end);
        assert_eq!(fixture.resolved_window.end, window_end);
        let _ = validate_page_request(&req, &resolved_with_extras)
            .expect("cursor minted without extras must still validate with extras present");
    }

    // ----- Stage 8: §6.10 `subject_ids` small-N path -------------------

    fn subject_ids_of(n: usize) -> Vec<String> {
        // Stable, deterministic ids — the validator is purely
        // cardinality-based, so identity content doesn't matter.
        (0..n).map(|i| format!("u{i:04}")).collect()
    }

    #[test]
    fn subject_ids_cap_is_fifty() {
        // §6.10 nails the cap at 50 — surface it as a const so a
        // future bump is a single, reviewable change rather than
        // scattered magic numbers across REST / MCP / frontend.
        assert_eq!(LEADERBOARD_SUBJECT_IDS_CAP, 50);
    }

    #[test]
    fn validate_subject_ids_accepts_empty_and_up_to_cap() {
        assert!(validate_subject_ids(&[]).is_ok());
        assert!(validate_subject_ids(&subject_ids_of(1)).is_ok());
        assert!(validate_subject_ids(&subject_ids_of(LEADERBOARD_SUBJECT_IDS_CAP)).is_ok());
    }

    #[test]
    fn validate_subject_ids_rejects_over_cap() {
        // §6.10: anything beyond 50 must fail with the typed
        // `subject_ids_too_large` error so REST/MCP can return a
        // precise 400.
        let too_many = subject_ids_of(LEADERBOARD_SUBJECT_IDS_CAP + 1);
        let err = validate_subject_ids(&too_many).unwrap_err();
        assert_eq!(
            err,
            LeaderboardError::SubjectIdsTooLarge {
                len: LEADERBOARD_SUBJECT_IDS_CAP + 1,
                cap: LEADERBOARD_SUBJECT_IDS_CAP,
            }
        );
    }

    #[test]
    fn resolve_envelope_enforces_subject_ids_cap() {
        // The cap must be checked at envelope resolution — not at
        // the SQL layer — so an over-sized payload never reaches the
        // store.
        let mut env = sample_envelope();
        env.subject_ids = subject_ids_of(LEADERBOARD_SUBJECT_IDS_CAP + 1);
        let err = resolve_leaderboard_envelope(&env, utc(2025, 6, 18, 12, 0, 0)).unwrap_err();
        assert!(matches!(err, LeaderboardError::SubjectIdsTooLarge { .. }));
    }

    #[test]
    fn resolve_envelope_echoes_subject_ids() {
        // §4 echo rule: the resolved envelope carries every input
        // axis the response shape can depend on. Caches keying on
        // (rank_by + extras + subject_ids) need the echo.
        let mut env = sample_envelope();
        env.subject_ids = subject_ids_of(3);
        let resolved = resolve_leaderboard_envelope(&env, utc(2025, 6, 18, 12, 0, 0)).unwrap();
        assert_eq!(resolved.subject_ids, env.subject_ids);
    }

    #[test]
    fn envelope_subject_ids_round_trips_through_json() {
        // Wire form: present when non-empty, omitted when empty so
        // the stage-3 shape stays stable for clients that don't use
        // the §6.10 small-N path.
        let mut env = sample_envelope();
        assert!(env.subject_ids.is_empty());
        let json = serde_json::to_string(&env).unwrap();
        assert!(
            !json.contains("subject_ids"),
            "empty subject_ids must be omitted: {json}",
        );

        env.subject_ids = subject_ids_of(2);
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("\"subject_ids\""), "{json}");
        let back: LeaderboardEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(back.subject_ids, env.subject_ids);
    }

    #[test]
    fn envelope_default_subject_ids_is_empty_when_field_absent() {
        // Backwards compat: a missing `subject_ids` in the request
        // JSON must deserialise as an empty list. A previously-shipped
        // client that pre-dates §6.10 keeps working unchanged.
        let json = r#"{
            "window": { "label": "today", "tz": "UTC", "anchor": "utc" },
            "scope_mode": "single_org",
            "orgs": ["00000000-0000-0000-0000-000000000000"],
            "subject": "user",
            "rank_by": { "family": "count", "id": "pull_requests_opened" }
        }"#;
        let env: LeaderboardEnvelope = serde_json::from_str(json).unwrap();
        assert!(env.subject_ids.is_empty());
    }

    #[test]
    fn validate_page_request_rejects_cursor_in_subject_ids_mode() {
        // §6.10: pagination is disabled when `subject_ids` is
        // non-empty. A cursor alongside the small-N filter is a
        // client bug, surfaced as a precise typed error so REST/MCP
        // return 400 rather than silently mixing two modes.
        let mut env = sample_envelope();
        env.subject_ids = subject_ids_of(3);
        let resolved = resolve_leaderboard_envelope(&env, utc(2025, 6, 18, 12, 0, 0)).unwrap();
        let cursor = PageCursor {
            resolved_window_end: resolved.resolved_window.end,
            rank_by_value: 10,
            subject_id: "u1".into(),
        };
        let req = PageRequest { size: 0, cursor: Some(cursor.encode()) };
        let err = validate_page_request(&req, &resolved).unwrap_err();
        assert_eq!(
            err,
            LeaderboardError::PaginationDisabledForSubjectIds { subject_ids_len: 3 },
        );
    }

    #[test]
    fn validate_page_request_rejects_explicit_size_in_subject_ids_mode() {
        // A non-zero `size` is also a client bug in this mode — the
        // server returns every matching row in one response, so
        // asking for "page size = 10" is meaningless. Default
        // (`size == 0`, no cursor) is accepted.
        let mut env = sample_envelope();
        env.subject_ids = subject_ids_of(5);
        let resolved = resolve_leaderboard_envelope(&env, utc(2025, 6, 18, 12, 0, 0)).unwrap();

        // Explicit size — rejected.
        let req = PageRequest { size: 10, cursor: None };
        let err = validate_page_request(&req, &resolved).unwrap_err();
        assert!(matches!(
            err,
            LeaderboardError::PaginationDisabledForSubjectIds { subject_ids_len: 5 },
        ));

        // Default request shape — accepted (no cursor, size == 0).
        let req = PageRequest::default();
        assert!(validate_page_request(&req, &resolved).is_ok());
    }

    #[test]
    fn build_subject_ids_sql_appends_outer_filter() {
        // §6.10: the predicate lives on the *outer* query so the
        // inner GROUP BY and §6.1 tie-break ORDER BY are
        // untouched. A drift here (e.g. predicate inlined into the
        // base SQL) would shift active_days / repos_touched
        // aggregates because filtered-out subjects would no longer
        // contribute to the per-subject counts.
        let sql = build_subject_ids_leaderboard_sql(
            SubjectKind::User,
            ScopeMode::SingleOrg,
        )
        .unwrap();
        let lower = sql.to_ascii_lowercase();
        assert!(lower.contains("select * from ("), "outer wrap missing: {sql}");
        assert!(
            lower.contains("where sub.subject_id = any($7::text[])"),
            "outer predicate missing: {sql}",
        );
        // No LIMIT — pagination is disabled in this mode (§6.10).
        assert!(!lower.contains("limit"), "unexpected LIMIT in subject_ids SQL: {sql}");
        // Tie-break re-emitted on the outer query for §6.1.
        assert!(
            lower.contains("order by primary_value desc, active_days desc, subject_id asc"),
            "outer tie-break missing: {sql}",
        );
    }

    #[test]
    fn subject_ids_sql_bind_order_has_seven_slots() {
        // The base bind order is 6 slots; subject_ids adds exactly
        // one trailing `text[]`. No cursor / limit slot — pagination
        // is disabled in this mode (§6.10). Locked here so a future
        // change to either constant is a single, reviewable diff.
        assert_eq!(LEADERBOARD_BIND_ORDER.len(), 6);
        assert_eq!(LEADERBOARD_BIND_ORDER_SUBJECT_IDS.len(), 7);
        assert!(
            LEADERBOARD_BIND_ORDER_SUBJECT_IDS[6].contains("subject_ids"),
            "trailing slot must document subject_ids: {:?}",
            LEADERBOARD_BIND_ORDER_SUBJECT_IDS[6],
        );
    }

    #[test]
    fn subject_ids_sql_works_for_every_valid_subject_scope_pair() {
        // §6.10 applies to every (subject, scope_mode) pair the
        // base builder honours — the outer wrap is purely a
        // predicate addition. Locked here so a future refactor of
        // `build_leaderboard_sql`'s per-variant strings can't
        // silently break the small-N path for one combo.
        for (subject, scope_mode) in [
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
        ] {
            let sql = build_subject_ids_leaderboard_sql(subject, scope_mode).unwrap_or_else(|e| {
                panic!("subject_ids SQL failed for ({subject:?}, {scope_mode:?}): {e}")
            });
            assert!(
                sql.to_ascii_lowercase().contains("any($7::text[])"),
                "outer predicate missing for ({subject:?}, {scope_mode:?}): {sql}",
            );
        }
    }

    #[test]
    fn subject_ids_sql_rejects_invalid_subject_scope_combo() {
        // §2 invalid pairings (team×all_orgs_combined, org×single_org)
        // are rejected at the SQL layer too — the outer wrap can't
        // rescue a meaningless aggregate.
        assert!(matches!(
            build_subject_ids_leaderboard_sql(SubjectKind::Team, ScopeMode::AllOrgsCombined),
            Err(LeaderboardError::InvalidSubjectScopeCombo { .. }),
        ));
        assert!(matches!(
            build_subject_ids_leaderboard_sql(SubjectKind::Org, ScopeMode::SingleOrg),
            Err(LeaderboardError::InvalidSubjectScopeCombo { .. }),
        ));
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
