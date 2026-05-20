//! `my_standing` — IC self-view companion to the leaderboard
//! (ORG-REPORTS §6.9).
//!
//! The leaderboard endpoint requires manager/admin scope (SCOPE.md
//! §15.12). An IC asking "where do I sit?" calls this separate
//! `my_standing` endpoint instead, which returns:
//!
//! * the viewer's own row in full,
//! * an anonymised neighbour window (±N ranks, labels replaced with
//!   `"—"`),
//! * a headline computed **over the visible set only**.
//!
//! Same SQL primitives underneath — [`build_my_standing_sql`] wraps
//! the per-`(subject, scope_mode)` SQL string emitted by
//! [`crate::leaderboard::build_leaderboard_sql`] in a `RANK()` window
//! and slices to the viewer's neighbourhood — but a distinct
//! endpoint and envelope so totals, pagination, and tie-break
//! boundaries cannot leak distributional information about
//! colleagues. The §6.9 framing rejected "same SQL, projection
//! only" precisely because `total_subjects` and page boundaries are
//! themselves information leaks.
//!
//! ## Permission boundary
//!
//! [`validate_my_standing_permission`] enforces `principal ==
//! viewer_subject_id`. A manager who wants to see another user's
//! standing has the leaderboard endpoint; the IC self-view is not
//! an "anyone can ask about anyone" hole. The check lives here so
//! the REST and MCP surfaces share one rule (SCOPE.md §15.12).
//!
//! ## Boundary
//!
//! Pure types + pure SQL string builder, mirroring
//! `crate::leaderboard`. No `sqlx`, no `dp-store-pg` import.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use dp_domain::event::ActorRole;
use dp_domain::window::Window;

use crate::envelope::{resolve_window_at, ResolveError, ScopeMode, WindowSpec};
use crate::leaderboard::{
    build_leaderboard_sql, validate_also_compute, validate_subject_scope_combo, LeaderboardError,
    LeaderboardHeadline, LeaderboardRow, MetricId, SubjectKind,
    LEADERBOARD_TIE_BREAK_ORDER_BY_CLAUSE,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default ±radius around the viewer's rank when `neighbor_radius`
/// is omitted or `0`. ORG-REPORTS §6.9 specifies ±3 as the
/// product-design default — small enough that neighbour identity
/// can't be reconstructed by repeated requests, large enough to
/// convey "where do I sit".
pub const MY_STANDING_NEIGHBOR_RADIUS_DEFAULT: u32 = 3;

/// Hard cap on `neighbor_radius`. A larger window starts
/// approximating a leaderboard slice — and the leaderboard
/// endpoint is the right tool when a manager wants that view.
/// Enforced at envelope resolution so the SQL layer never sees an
/// unbounded `BETWEEN` predicate.
pub const MY_STANDING_NEIGHBOR_RADIUS_MAX: u32 = 10;

/// Replacement label used for every neighbour row when serialised
/// back to the client. The em-dash is the §6.9 sentinel — short,
/// not confusable with a real login/slug.
pub const MY_STANDING_NEIGHBOUR_ANONYMISED_LABEL: &str = "—";

/// Replacement `subject_id` for neighbour rows. The string is
/// intentionally non-UUID and prefixed/suffixed with underscores so
/// it cannot collide with a real user/team/org id, and is the same
/// across every neighbour row (a per-row salt would help an
/// attacker correlate two consecutive requests; a fixed token
/// cannot).
pub const MY_STANDING_NEIGHBOUR_ANONYMISED_SUBJECT_ID: &str = "__anonymised__";

// ---------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------

/// Default for `include_bots`. ORG-REPORTS §6.4 — bot suppression
/// is the default for the leaderboard *and* for `my_standing`; bot
/// neighbours would be a non-sequitur in a self-view.
fn default_include_bots() -> bool {
    false
}

/// Inputs to the `my_standing` endpoint.
///
/// Distinct from [`crate::leaderboard::LeaderboardEnvelope`] on
/// purpose (§6.9): adding a field to one envelope must not silently
/// expand the other's permission surface. Fields that mirror the
/// leaderboard envelope keep the same names and wire form so the
/// frontend can reuse the same form-state shape, but
/// `viewer_subject_id` and `neighbor_radius` are unique to this
/// endpoint and `page` / `subject_ids` are intentionally absent
/// (pagination is meaningless for a single-viewer slice; the
/// neighbourhood radius is the bound).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MyStandingEnvelope {
    /// Window spec, resolved server-side via [`resolve_window_at`].
    pub window: WindowSpec,
    /// Org-scope lens (SCOPE §8.1). All three modes are accepted;
    /// invalid pairings with [`Self::subject`] are rejected by the
    /// reused [`validate_subject_scope_combo`].
    pub scope_mode: ScopeMode,
    /// Orgs in scope. Same cardinality contract as
    /// [`crate::leaderboard::LeaderboardEnvelope::orgs`].
    #[serde(default)]
    pub orgs: Vec<Uuid>,
    /// Repo filter. Empty == no filter.
    #[serde(default)]
    pub repos: Vec<Uuid>,
    /// Team filter. Empty == no filter.
    #[serde(default)]
    pub teams: Vec<Uuid>,
    /// `actor_roles` override (SCOPE §15.7). Empty == use the
    /// `rank_by` metric's default-role set.
    #[serde(default)]
    pub actor_roles: Vec<ActorRole>,
    /// Subject axis. Only [`SubjectKind::User`] and
    /// [`SubjectKind::Team`] are meaningful for a self-view; the
    /// `org` / `home_org_label` aggregations are not "where do I
    /// sit" questions and are rejected with
    /// [`MyStandingError::SubjectKindUnsupported`].
    pub subject: SubjectKind,
    /// The one §15.7 metric used to rank the population.
    pub rank_by: MetricId,
    /// Bot suppression. Defaults `false`.
    #[serde(default = "default_include_bots")]
    pub include_bots: bool,
    /// The viewer's own `subject_id`. The principal layer
    /// (SCOPE.md §15.12) is the authoritative source — the request
    /// echo lets the client confirm which identity the server is
    /// answering on behalf of, and
    /// [`validate_my_standing_permission`] enforces that
    /// `principal == viewer_subject_id`.
    pub viewer_subject_id: String,
    /// ±N ranks around the viewer to return as anonymised
    /// neighbours. `0` means "use the default"
    /// ([`MY_STANDING_NEIGHBOR_RADIUS_DEFAULT`]). Capped at
    /// [`MY_STANDING_NEIGHBOR_RADIUS_MAX`].
    #[serde(default)]
    pub neighbor_radius: u32,
    /// Extra §15.7 metrics carried into each row's
    /// `LeaderboardContext::extras` (ORG-REPORTS §6.3). Capped at
    /// [`crate::leaderboard::LEADERBOARD_ALSO_COMPUTE_CAP`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_compute: Vec<MetricId>,
}

// ---------------------------------------------------------------------------
// Resolved envelope + response
// ---------------------------------------------------------------------------

/// Echo of the resolved request that travels back on every
/// response. Mirrors the role of
/// [`crate::leaderboard::ResolvedLeaderboardEnvelope`] but does
/// not echo `page` (the slice is a fixed radius, not a paginated
/// scan) or `subject_ids` (the small-N filter is leaderboard-only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedMyStandingEnvelope {
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
    /// Viewer `subject_id` echoed back. Identical to the request
    /// field — included so a cache/proxy seeing only the response
    /// can pin "who this view is for".
    pub viewer_subject_id: String,
    /// Effective neighbour radius after default substitution and
    /// the [`MY_STANDING_NEIGHBOR_RADIUS_MAX`] clamp. Differs from
    /// `envelope.neighbor_radius` precisely when the client sent
    /// `0` (meaning "use the default").
    pub neighbor_radius: u32,
    /// `also_compute` echoed back (ORG-REPORTS §6.3). Omitted from
    /// the wire form when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_compute: Vec<MetricId>,
}

/// The `my_standing` response.
///
/// `headline` is computed **over the visible set only** (viewer +
/// neighbours), not over the full population. This is the §6.9
/// information-leak boundary: a manager who could see
/// `total_subjects` over the full org would be able to fingerprint
/// activity changes from one self-view request to the next. The
/// IC self-view's headline is a "what does this slice look like"
/// summary, not a population statistic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MyStandingResponse {
    /// Resolved request echo.
    pub envelope: ResolvedMyStandingEnvelope,
    /// Headline counters — visible set only (§6.9).
    pub headline: LeaderboardHeadline,
    /// The viewer's own row in full (label, id, primary value,
    /// context, sparkline). `None` when the viewer has no events
    /// in the resolved window — the wire-form shape is preserved
    /// so the frontend can render "no activity" without inspecting
    /// neighbour rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewer_row: Option<LeaderboardRow>,
    /// Anonymised neighbours, sorted by rank ASC. Length is
    /// bounded by `2 * envelope.neighbor_radius` (the radius
    /// truncates at the population edges, so a viewer near rank 1
    /// gets fewer rows above). Every row's `subject_id` and
    /// `subject_label` are replaced with the sentinels in
    /// [`MY_STANDING_NEIGHBOUR_ANONYMISED_SUBJECT_ID`] /
    /// [`MY_STANDING_NEIGHBOUR_ANONYMISED_LABEL`].
    #[serde(default)]
    pub neighbors: Vec<LeaderboardRow>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Failure modes the `my_standing` envelope rejects.
///
/// Distinct from [`LeaderboardError`] so the REST/MCP layer can
/// match on "this is a self-view error" without conflating it with
/// leaderboard errors — but bubbles through it via
/// [`Self::Leaderboard`] for the §6.3 / §2 / `(subject, scope)`
/// rules that are genuinely shared. Splitting permission and radius
/// errors out keeps the surface honest: the §6.9 boundaries are
/// not leaderboard boundaries.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum MyStandingError {
    /// `principal != envelope.viewer_subject_id`. The IC self-view
    /// answers only for the requesting principal; a manager wanting
    /// another user's standing uses the leaderboard endpoint
    /// (SCOPE.md §15.12).
    #[error(
        "my_standing_permission_denied: principal={principal} cannot request \
         viewer_subject_id={requested}"
    )]
    PermissionDenied {
        /// Principal id from the authenticated session.
        principal: String,
        /// `viewer_subject_id` from the request envelope.
        requested: String,
    },
    /// The subject axis is not a self-view axis. Only `user` and
    /// `team` are meaningful for "where do I sit". `org` and
    /// `home_org_label` are aggregations, not self-views, and the
    /// leaderboard endpoint covers them.
    #[error("my_standing_subject_unsupported: subject={subject:?} is not a self-view axis")]
    SubjectKindUnsupported {
        /// The unsupported subject kind that was requested.
        subject: SubjectKind,
    },
    /// `neighbor_radius` exceeded [`MY_STANDING_NEIGHBOR_RADIUS_MAX`].
    #[error("neighbor_radius_out_of_range: requested={requested} max={max}")]
    NeighborRadiusOutOfRange {
        /// Requested radius after default substitution.
        requested: u32,
        /// Server-side cap.
        max: u32,
    },
    /// Pass-through for the shared (subject, scope), `orgs`
    /// cardinality, and `also_compute` cap rules that
    /// [`LeaderboardError`] already encodes.
    #[error(transparent)]
    Leaderboard(#[from] LeaderboardError),
    /// Window spec failed to resolve.
    #[error(transparent)]
    Resolve(#[from] ResolveError),
}

// ---------------------------------------------------------------------------
// Permission boundary
// ---------------------------------------------------------------------------

/// Enforce `principal == envelope.viewer_subject_id` (§6.9).
///
/// Called once by [`resolve_my_standing_envelope`] so the REST and
/// MCP surfaces share the rule; calling directly is supported for
/// pre-flight checks (e.g. an audit-log shim that wants the
/// permission decision before the full envelope resolves).
pub fn validate_my_standing_permission(
    principal_subject_id: &str,
    env: &MyStandingEnvelope,
) -> Result<(), MyStandingError> {
    if principal_subject_id == env.viewer_subject_id {
        Ok(())
    } else {
        Err(MyStandingError::PermissionDenied {
            principal: principal_subject_id.to_owned(),
            requested: env.viewer_subject_id.clone(),
        })
    }
}

/// Resolve a [`MyStandingEnvelope::neighbor_radius`] to its
/// effective value, applying the `0 → default` substitution and
/// rejecting values over [`MY_STANDING_NEIGHBOR_RADIUS_MAX`].
///
/// Lives as a free function (not on the envelope) so REST query
/// strings and MCP structured args hit the exact same bound — the
/// same pattern [`crate::leaderboard::effective_page_size`] uses.
pub fn effective_neighbor_radius(requested: u32) -> Result<u32, MyStandingError> {
    let radius = if requested == 0 {
        MY_STANDING_NEIGHBOR_RADIUS_DEFAULT
    } else {
        requested
    };
    if radius > MY_STANDING_NEIGHBOR_RADIUS_MAX {
        return Err(MyStandingError::NeighborRadiusOutOfRange {
            requested: radius,
            max: MY_STANDING_NEIGHBOR_RADIUS_MAX,
        });
    }
    Ok(radius)
}

// ---------------------------------------------------------------------------
// Envelope resolution
// ---------------------------------------------------------------------------

/// Resolve a [`MyStandingEnvelope`] at `now`, returning the
/// [`ResolvedMyStandingEnvelope`] echoed in the response.
///
/// Order of operations (each is a typed `Err` so REST/MCP surface a
/// precise 4xx):
///
/// 1. Permission check (`principal == viewer_subject_id`).
/// 2. Subject-kind support (`user` / `team` only).
/// 3. (subject, scope_mode) §2 validity — shared with the
///    leaderboard.
/// 4. `also_compute` cap — shared with the leaderboard.
/// 5. `orgs` cardinality per `scope_mode`.
/// 6. `neighbor_radius` clamp.
/// 7. Window resolution.
pub fn resolve_my_standing_envelope(
    env: &MyStandingEnvelope,
    principal_subject_id: &str,
    now: DateTime<Utc>,
) -> Result<ResolvedMyStandingEnvelope, MyStandingError> {
    validate_my_standing_permission(principal_subject_id, env)?;
    match env.subject {
        SubjectKind::User | SubjectKind::Team => {}
        other => return Err(MyStandingError::SubjectKindUnsupported { subject: other }),
    }
    validate_subject_scope_combo(env.subject, env.scope_mode)?;
    validate_also_compute(&env.also_compute)?;
    match env.scope_mode {
        ScopeMode::SingleOrg => {
            if env.orgs.len() != 1 {
                return Err(LeaderboardError::SingleOrgRequiresOneOrg(env.orgs.len()).into());
            }
        }
        ScopeMode::AllOrgsCombined | ScopeMode::PerOrgSplit => {
            // Same "empty == defer to auth layer" policy as the
            // leaderboard envelope. The principal layer narrows
            // the set in Phase 4.
        }
    }
    let radius = effective_neighbor_radius(env.neighbor_radius)?;
    let resolved_window = resolve_window_at(&env.window, now)?;
    Ok(ResolvedMyStandingEnvelope {
        resolved_at: now,
        resolved_window,
        scope_mode: env.scope_mode,
        subject: env.subject,
        rank_by: env.rank_by,
        viewer_subject_id: env.viewer_subject_id.clone(),
        neighbor_radius: radius,
        also_compute: env.also_compute.clone(),
    })
}

// ---------------------------------------------------------------------------
// SQL builder
// ---------------------------------------------------------------------------

/// Parameter bind order for [`build_my_standing_sql`].
///
/// The base [`crate::leaderboard::LEADERBOARD_BIND_ORDER`] is
/// extended with two trailing slots: the viewer's id (for the
/// `me` CTE row lookup) and the neighbour radius (for the
/// `BETWEEN` slice). No `LIMIT` — the radius bounds the response
/// size and pagination is meaningless for a single-viewer slice.
pub const MY_STANDING_BIND_ORDER: &[&str] = &[
    "$1 window.start (timestamptz)",
    "$2 window.end (timestamptz, exclusive)",
    "$3 org_ids (uuid[]; cardinality >= 1)",
    "$4 event_kind (text — from CountMetric::event_kind())",
    "$5 actor_roles (text[] — from envelope.actor_roles or CountMetric::default_actor_roles())",
    "$6 repos (uuid[]; cardinality 0 == no filter)",
    "$7 viewer_subject_id (text)",
    "$8 neighbor_radius (int; effective_neighbor_radius)",
];

/// Wrap the base [`build_leaderboard_sql`] in a `RANK()` window
/// + viewer-centred slice for the §6.9 self-view path.
///
/// The emitted SQL is:
///
/// ```text
/// WITH ranked AS (
///   SELECT sub.*,
///          rank() OVER (ORDER BY sub.primary_value DESC,
///                                sub.active_days  DESC,
///                                sub.subject_id   ASC) AS row_rank
///     FROM ( <base SQL with its ORDER BY> ) AS sub
/// ),
/// me AS (
///   SELECT row_rank AS me_rank FROM ranked WHERE subject_id = $7
/// )
/// SELECT ranked.*
///   FROM ranked, me
///  WHERE ranked.row_rank BETWEEN me.me_rank - $8 AND me.me_rank + $8
///  ORDER BY ranked.row_rank ASC
/// ```
///
/// The `RANK()` `OVER` clause re-uses the §6.1 tie-break order
/// verbatim — emitting it here rather than referencing
/// [`LEADERBOARD_TIE_BREAK_ORDER_BY_CLAUSE`] inline is intentional:
/// the window function spelling differs from the trailing
/// `ORDER BY` (no comma, no `ORDER BY` keyword inside the
/// parentheses' over-clause prefix), and copy-pasting the clause
/// across the two spellings is the kind of drift the §11.4 trust
/// contract is meant to catch.
///
/// The viewer's own row is included in `ranked` (rank 0 offset
/// from itself), so the store layer can split the result into
/// "viewer row + neighbours" on `subject_id == viewer`. The
/// neighbour anonymisation lives in [`anonymise_neighbour_row`].
///
/// Returns the same [`LeaderboardError::InvalidSubjectScopeCombo`]
/// the base builder does — the wrap can't rescue a meaningless
/// aggregate.
pub fn build_my_standing_sql(
    subject: SubjectKind,
    scope_mode: ScopeMode,
) -> Result<String, LeaderboardError> {
    let base = build_leaderboard_sql(subject, scope_mode)?;
    Ok(format!(
        "WITH ranked AS (\
           SELECT sub.*, \
                  rank() OVER (ORDER BY sub.primary_value DESC, sub.active_days DESC, sub.subject_id ASC) AS row_rank \
             FROM ({base}) AS sub\
         ), me AS (\
           SELECT row_rank AS me_rank FROM ranked WHERE subject_id = $7\
         ) \
         SELECT ranked.* FROM ranked, me \
          WHERE ranked.row_rank BETWEEN me.me_rank - $8 AND me.me_rank + $8 \
          ORDER BY ranked.row_rank ASC",
        base = base,
    ))
}

// Touch the tie-break constant so a future rewrite that removes it
// is caught by the build, not by a runtime divergence between the
// two leaderboards. `RANK()`'s `OVER` clause above must mirror the
// trailing-`ORDER BY` form of this constant for §6.1 to hold.
const _LEADERBOARD_TIE_BREAK_REFERENCED: &str = LEADERBOARD_TIE_BREAK_ORDER_BY_CLAUSE;

// ---------------------------------------------------------------------------
// Neighbour anonymisation
// ---------------------------------------------------------------------------

/// Return a copy of `row` with `subject_id` and `subject_label`
/// replaced by the §6.9 anonymisation sentinels.
///
/// The viewer's own row is never anonymised — callers compare
/// `row.subject_id == viewer_subject_id` and route to
/// [`MyStandingResponse::viewer_row`] in that case. This helper
/// asserts on the mismatch (debug builds) so a caller can't
/// accidentally anonymise the viewer.
///
/// `rank`, `primary`, `context`, `sparkline`, and `active_orgs`
/// are preserved — they convey "what the neighbour position looks
/// like" without identifying the neighbour. `subject_org` is
/// preserved when present because it's a property of the rank
/// slot (per-org-split mode) and not of the person.
pub fn anonymise_neighbour_row(row: LeaderboardRow) -> LeaderboardRow {
    LeaderboardRow {
        subject_id: MY_STANDING_NEIGHBOUR_ANONYMISED_SUBJECT_ID.to_owned(),
        subject_label: MY_STANDING_NEIGHBOUR_ANONYMISED_LABEL.to_owned(),
        ..row
    }
}

// ---------------------------------------------------------------------------
// Visible-set headline
// ---------------------------------------------------------------------------

/// Compute the §6.9 "visible set only" headline.
///
/// `total_subjects` is the count of returned rows (viewer +
/// neighbours), not the population. `events_total` is the
/// saturating-sum of those rows' primary values. The duration
/// metric exemption in [`crate::leaderboard::MetricId::is_count`]
/// does not apply here — the headline summarises the slice and is
/// rendered identically by the UI for both families.
///
/// Negative primary values (which the count-metric path never
/// produces) clamp to 0 when summing — same policy as
/// [`crate::leaderboard::check_reconciliation_identity`].
pub fn compute_visible_headline(
    viewer_row: Option<&LeaderboardRow>,
    neighbours: &[LeaderboardRow],
) -> LeaderboardHeadline {
    let viewer_count = u64::from(viewer_row.is_some());
    let total_subjects = viewer_count + neighbours.len() as u64;
    let sum_iter = viewer_row
        .into_iter()
        .chain(neighbours.iter())
        .map(|r| u64::try_from(r.primary.value).unwrap_or(0));
    let events_total = sum_iter.fold(0u64, |acc, v| acc.saturating_add(v));
    LeaderboardHeadline {
        total_subjects,
        events_total,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::CountMetric;
    use crate::envelope::WindowLabel;
    use crate::leaderboard::{LeaderboardContext, LeaderboardPrimary};
    use chrono::TimeZone;
    use dp_domain::window::WindowAnchor;

    fn utc(y: i32, m: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, mi, s).single().unwrap()
    }

    fn sample_env(viewer: &str) -> MyStandingEnvelope {
        MyStandingEnvelope {
            window: WindowSpec {
                label: WindowLabel::LastWeek,
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
            viewer_subject_id: viewer.to_owned(),
            neighbor_radius: 0,
            also_compute: vec![],
        }
    }

    fn row(rank: u32, subject_id: &str, primary: i64) -> LeaderboardRow {
        LeaderboardRow {
            rank,
            subject_id: subject_id.to_owned(),
            subject_kind: SubjectKind::User,
            subject_label: format!("label-{subject_id}"),
            subject_org: None,
            primary: LeaderboardPrimary {
                metric: MetricId::Count(CountMetric::PullRequestsOpened),
                value: primary,
            },
            context: LeaderboardContext::default(),
            sparkline: vec![],
            active_orgs: 1,
        }
    }

    // ---- envelope round-trip ----------------------------------------------

    #[test]
    fn envelope_round_trips_through_json() {
        let env = sample_env("viewer-1");
        let json = serde_json::to_string(&env).unwrap();
        let back: MyStandingEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(env, back);
    }

    #[test]
    fn envelope_omits_empty_also_compute() {
        // Wire-shape stability: an envelope without extras must not
        // serialise an empty `also_compute` array — the field is
        // optional so older clients pre-§6.3 keep working.
        let env = sample_env("viewer-1");
        let json = serde_json::to_string(&env).unwrap();
        assert!(!json.contains("also_compute"), "{json}");
    }

    #[test]
    fn envelope_default_neighbor_radius_is_zero_when_absent() {
        // A missing `neighbor_radius` deserialises as `0` (== "use
        // the default"), so a client that pre-dates the radius knob
        // keeps the §6.9 default behaviour.
        let json = r#"{
            "window": { "label": "today", "tz": "UTC", "anchor": "utc" },
            "scope_mode": "single_org",
            "orgs": ["00000000-0000-0000-0000-000000000000"],
            "subject": "user",
            "rank_by": { "family": "count", "id": "pull_requests_opened" },
            "viewer_subject_id": "v"
        }"#;
        let env: MyStandingEnvelope = serde_json::from_str(json).unwrap();
        assert_eq!(env.neighbor_radius, 0);
    }

    // ---- permission check -------------------------------------------------

    #[test]
    fn permission_check_accepts_matching_principal() {
        let env = sample_env("viewer-1");
        assert!(validate_my_standing_permission("viewer-1", &env).is_ok());
    }

    #[test]
    fn permission_check_rejects_mismatched_principal() {
        // §6.9 / §15.12: the IC self-view only answers for the
        // requesting principal. A manager wanting another user's
        // standing must use the leaderboard endpoint.
        let env = sample_env("viewer-1");
        let err = validate_my_standing_permission("attacker", &env).unwrap_err();
        match err {
            MyStandingError::PermissionDenied { principal, requested } => {
                assert_eq!(principal, "attacker");
                assert_eq!(requested, "viewer-1");
            }
            other => panic!("expected PermissionDenied, got {other:?}"),
        }
    }

    #[test]
    fn resolve_rejects_principal_mismatch_before_anything_else() {
        // Order-of-checks: permission denial must beat every other
        // 4xx so a probe that varies `subject` / `window` / `orgs`
        // never gets a differential response that leaks "is this
        // viewer_id real".
        let mut env = sample_env("real-viewer");
        env.subject = SubjectKind::Org; // also invalid for self-view
        env.orgs = vec![]; // also invalid for single_org
        let err = resolve_my_standing_envelope(&env, "attacker", utc(2025, 6, 18, 12, 0, 0))
            .unwrap_err();
        assert!(matches!(err, MyStandingError::PermissionDenied { .. }));
    }

    // ---- subject-kind support --------------------------------------------

    #[test]
    fn resolve_rejects_org_subject() {
        let mut env = sample_env("v");
        env.subject = SubjectKind::Org;
        env.scope_mode = ScopeMode::AllOrgsCombined; // valid combo otherwise
        env.orgs = vec![Uuid::nil()];
        let err = resolve_my_standing_envelope(&env, "v", utc(2025, 6, 18, 12, 0, 0)).unwrap_err();
        assert!(matches!(
            err,
            MyStandingError::SubjectKindUnsupported { subject: SubjectKind::Org }
        ));
    }

    #[test]
    fn resolve_rejects_home_org_label_subject() {
        let mut env = sample_env("v");
        env.subject = SubjectKind::HomeOrgLabel;
        let err = resolve_my_standing_envelope(&env, "v", utc(2025, 6, 18, 12, 0, 0)).unwrap_err();
        assert!(matches!(
            err,
            MyStandingError::SubjectKindUnsupported {
                subject: SubjectKind::HomeOrgLabel
            }
        ));
    }

    #[test]
    fn resolve_accepts_team_subject() {
        // A team-lead asking "where does my team sit" is a
        // legitimate self-view per §6.9; the IC framing in
        // ORG-REPORTS is the motivating example but the boundary
        // is "viewer-scoped", not "user-only".
        let mut env = sample_env("team-uuid");
        env.subject = SubjectKind::Team;
        env.scope_mode = ScopeMode::SingleOrg;
        assert!(resolve_my_standing_envelope(&env, "team-uuid", utc(2025, 6, 18, 12, 0, 0)).is_ok());
    }

    // ---- neighbour radius -------------------------------------------------

    #[test]
    fn neighbor_radius_zero_substitutes_default() {
        assert_eq!(
            effective_neighbor_radius(0).unwrap(),
            MY_STANDING_NEIGHBOR_RADIUS_DEFAULT,
        );
    }

    #[test]
    fn neighbor_radius_respects_explicit_value() {
        assert_eq!(effective_neighbor_radius(5).unwrap(), 5);
    }

    #[test]
    fn neighbor_radius_rejects_over_cap() {
        let err = effective_neighbor_radius(MY_STANDING_NEIGHBOR_RADIUS_MAX + 1).unwrap_err();
        assert!(matches!(
            err,
            MyStandingError::NeighborRadiusOutOfRange {
                requested,
                max,
            } if requested == MY_STANDING_NEIGHBOR_RADIUS_MAX + 1
              && max == MY_STANDING_NEIGHBOR_RADIUS_MAX
        ));
    }

    #[test]
    fn resolve_echoes_effective_radius() {
        let mut env = sample_env("v");
        env.neighbor_radius = 0;
        let resolved =
            resolve_my_standing_envelope(&env, "v", utc(2025, 6, 18, 12, 0, 0)).unwrap();
        assert_eq!(
            resolved.neighbor_radius,
            MY_STANDING_NEIGHBOR_RADIUS_DEFAULT,
            "neighbor_radius=0 must echo back as the default"
        );

        env.neighbor_radius = 5;
        let resolved =
            resolve_my_standing_envelope(&env, "v", utc(2025, 6, 18, 12, 0, 0)).unwrap();
        assert_eq!(resolved.neighbor_radius, 5);
    }

    // ---- shared leaderboard rules pass through ---------------------------

    #[test]
    fn resolve_bubbles_leaderboard_orgs_cardinality_error() {
        // Single-org with 2 orgs must fail with the same
        // `LeaderboardError::SingleOrgRequiresOneOrg` the
        // leaderboard endpoint reports — surfaces consistently
        // between the two endpoints (§11.4).
        let mut env = sample_env("v");
        env.orgs = vec![Uuid::nil(), Uuid::nil()];
        let err = resolve_my_standing_envelope(&env, "v", utc(2025, 6, 18, 12, 0, 0)).unwrap_err();
        assert!(matches!(
            err,
            MyStandingError::Leaderboard(LeaderboardError::SingleOrgRequiresOneOrg(2))
        ));
    }

    #[test]
    fn resolve_bubbles_also_compute_cap() {
        let mut env = sample_env("v");
        env.also_compute = (0..7)
            .map(|_| MetricId::Count(CountMetric::PullRequestsMerged))
            .collect();
        let err = resolve_my_standing_envelope(&env, "v", utc(2025, 6, 18, 12, 0, 0)).unwrap_err();
        assert!(matches!(
            err,
            MyStandingError::Leaderboard(LeaderboardError::AlsoComputeTooLarge { .. })
        ));
    }

    // ---- SQL builder ------------------------------------------------------

    #[test]
    fn sql_wraps_base_in_rank_cte_and_slice() {
        let sql = build_my_standing_sql(SubjectKind::User, ScopeMode::SingleOrg).unwrap();
        let lower = sql.to_ascii_lowercase();
        assert!(lower.contains("with ranked as"), "{sql}");
        assert!(lower.contains("rank() over"), "{sql}");
        assert!(
            lower.contains("order by sub.primary_value desc, sub.active_days desc, sub.subject_id asc"),
            "tie-break in over-clause: {sql}",
        );
        assert!(lower.contains("where subject_id = $7"), "{sql}");
        assert!(
            lower.contains("between me.me_rank - $8 and me.me_rank + $8"),
            "{sql}",
        );
        assert!(
            lower.contains("order by ranked.row_rank asc"),
            "outer order missing: {sql}",
        );
        assert!(!lower.contains("limit"), "no LIMIT in my_standing SQL: {sql}");
    }

    #[test]
    fn sql_works_for_every_self_view_subject_scope_pair() {
        for (subject, scope_mode) in [
            (SubjectKind::User, ScopeMode::SingleOrg),
            (SubjectKind::User, ScopeMode::AllOrgsCombined),
            (SubjectKind::User, ScopeMode::PerOrgSplit),
            (SubjectKind::Team, ScopeMode::SingleOrg),
            (SubjectKind::Team, ScopeMode::PerOrgSplit),
        ] {
            let sql = build_my_standing_sql(subject, scope_mode).unwrap_or_else(|e| {
                panic!("my_standing SQL failed for ({subject:?}, {scope_mode:?}): {e}")
            });
            assert!(sql.to_ascii_lowercase().contains("rank() over"), "{sql}");
        }
    }

    #[test]
    fn sql_rejects_invalid_subject_scope_combo() {
        assert!(matches!(
            build_my_standing_sql(SubjectKind::Team, ScopeMode::AllOrgsCombined),
            Err(LeaderboardError::InvalidSubjectScopeCombo { .. })
        ));
    }

    #[test]
    fn sql_bind_order_has_eight_slots() {
        // 6 from the base + viewer + radius. Locked here so a
        // future change to either constant is a single, reviewable
        // diff (mirrors the §6.10 lock-test).
        assert_eq!(MY_STANDING_BIND_ORDER.len(), 8);
        assert!(MY_STANDING_BIND_ORDER[6].contains("viewer_subject_id"));
        assert!(MY_STANDING_BIND_ORDER[7].contains("neighbor_radius"));
    }

    // ---- anonymisation ----------------------------------------------------

    #[test]
    fn anonymise_replaces_subject_id_and_label() {
        let neighbour = row(2, "neighbour-uuid", 42);
        let anon = anonymise_neighbour_row(neighbour.clone());
        assert_eq!(anon.subject_id, MY_STANDING_NEIGHBOUR_ANONYMISED_SUBJECT_ID);
        assert_eq!(anon.subject_label, MY_STANDING_NEIGHBOUR_ANONYMISED_LABEL);
        // Everything that conveys *position* is preserved.
        assert_eq!(anon.rank, neighbour.rank);
        assert_eq!(anon.primary, neighbour.primary);
        assert_eq!(anon.active_orgs, neighbour.active_orgs);
    }

    #[test]
    fn anonymise_sentinel_is_constant_across_rows() {
        // §6.9: a per-row salt would let an attacker correlate two
        // self-view requests; the sentinel is intentionally fixed.
        let a = anonymise_neighbour_row(row(1, "n1", 1));
        let b = anonymise_neighbour_row(row(2, "n2", 1));
        assert_eq!(a.subject_id, b.subject_id);
        assert_eq!(a.subject_label, b.subject_label);
    }

    // ---- visible-set headline --------------------------------------------

    #[test]
    fn visible_headline_counts_viewer_plus_neighbours() {
        let viewer = row(5, "me", 10);
        let neighbours = vec![
            row(4, "n1", 12),
            row(6, "n2", 9),
            row(7, "n3", 8),
        ];
        let h = compute_visible_headline(Some(&viewer), &neighbours);
        assert_eq!(h.total_subjects, 4);
        assert_eq!(h.events_total, 10 + 12 + 9 + 8);
    }

    #[test]
    fn visible_headline_omits_missing_viewer() {
        // §6.9: when the viewer has no events in the window the
        // response carries `viewer_row = None`; the headline still
        // summarises whatever rows the slice contains so the UI
        // can render "the neighbourhood you'd be near".
        let neighbours = vec![row(1, "n1", 3), row(2, "n2", 2)];
        let h = compute_visible_headline(None, &neighbours);
        assert_eq!(h.total_subjects, 2);
        assert_eq!(h.events_total, 5);
    }

    #[test]
    fn visible_headline_is_zero_for_empty_slice() {
        let h = compute_visible_headline(None, &[]);
        assert_eq!(h.total_subjects, 0);
        assert_eq!(h.events_total, 0);
    }

    #[test]
    fn visible_headline_clamps_negative_primary_to_zero() {
        // Mirrors `check_reconciliation_identity`'s policy — the
        // count path never produces negatives but the saturating
        // add keeps the function infallible against malformed
        // inputs.
        let viewer = row(1, "me", -5);
        let h = compute_visible_headline(Some(&viewer), &[]);
        assert_eq!(h.events_total, 0);
    }

    // ---- response round-trip ---------------------------------------------

    #[test]
    fn response_round_trips_through_json() {
        let resolved = resolve_my_standing_envelope(
            &sample_env("me"),
            "me",
            utc(2025, 6, 18, 12, 0, 0),
        )
        .unwrap();
        let viewer = row(3, "me", 7);
        let neighbours = vec![anonymise_neighbour_row(row(2, "n1", 8))];
        let headline = compute_visible_headline(Some(&viewer), &neighbours);
        let resp = MyStandingResponse {
            envelope: resolved,
            headline,
            viewer_row: Some(viewer),
            neighbors: neighbours,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: MyStandingResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }
}
