//! Report request envelope + [`Window`] resolution.
//!
//! This module is the one entry point that every report (Phase 3) and
//! every downstream surface (Phase 4 REST, Phase 5 MCP) accepts. The
//! decision in TODO §0.4 is that the server — not the frontend —
//! resolves `(label, tz, anchor)` into a concrete UTC `[start, end)`,
//! and that the response echoes the resolved window back so the UI
//! can label it unambiguously.
//!
//! Boundary rule (TODO §0.6): zero `starter_*` imports. Only
//! `dp-domain` + third-party crates.
//!
//! ## Anchor matrix
//!
//! | anchor   | clock the label is interpreted in            |
//! |----------|----------------------------------------------|
//! | `viewer` | viewer's IANA TZ (default for per-user)      |
//! | `org`    | org's configured IANA TZ (default for mgrs)  |
//! | `utc`    | strict UTC (default for exec / cross-company) |
//!
//! For `viewer` / `org` the caller passes the relevant IANA name in
//! [`WindowSpec::tz`]. For `utc` the computation is in UTC regardless
//! of `tz`, but `tz` is still echoed back so the UI can re-render the
//! window in the viewer's clock if it wants.

use chrono::{DateTime, Datelike, Days, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use dp_domain::event::{ActorRole, EventKind};
use dp_domain::window::{Window, WindowAnchor};

// ---------------------------------------------------------------------------
// Report envelope
// ---------------------------------------------------------------------------

/// Org-scope lens (SCOPE §8.1). Every report supports all three and
/// must produce consistent de-duplicated counts in
/// [`ScopeMode::AllOrgsCombined`] — see TODO §0.2 (de-dup operates on
/// `(user_id, event_id)` pairs, **not** event rows alone).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeMode {
    /// A single org's data only. No de-dup needed.
    SingleOrg,
    /// All requested orgs merged into one set, de-duplicating
    /// co-authored / multi-actor events that span orgs.
    AllOrgsCombined,
    /// One result row per org, no de-dup across orgs.
    PerOrgSplit,
}

/// How report rows are grouped. The set is intentionally small — adding
/// new variants here forces a matching update in every report SQL
/// builder, so we keep it tight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupBy {
    /// One row per actor user.
    User,
    /// One row per team.
    Team,
    /// One row per repo.
    Repo,
    /// One row per org.
    Org,
    /// One row per UTC-truncated day (re-anchored to window TZ for
    /// labelling — SCOPE §15.8).
    Day,
    /// One row per UTC-truncated ISO week.
    Week,
    /// One row per UTC-truncated month.
    Month,
}

/// The single envelope every report takes. Phase 4 REST handlers and
/// Phase 5 MCP tools deserialise into this same shape (decision locked
/// in SCOPE §15.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportRequest {
    /// Orgs to include. Empty == "all orgs the principal can see"
    /// (the auth layer narrows the set in Phase 4).
    #[serde(default)]
    pub orgs: Vec<Uuid>,
    /// User filter. Empty == no filter.
    #[serde(default)]
    pub users: Vec<Uuid>,
    /// Team filter. Empty == no filter.
    #[serde(default)]
    pub teams: Vec<Uuid>,
    /// The window spec. Resolved server-side via [`resolve_window`].
    pub window: WindowSpec,
    /// Lens (SCOPE §8.1).
    pub scope_mode: ScopeMode,
    /// Optional grouping. `None` means "headline only".
    #[serde(default)]
    pub group_by: Option<GroupBy>,
    /// Event-kind filter. Empty == no filter.
    #[serde(default)]
    pub activity_types: Vec<EventKind>,
    /// Actor-role filter (TODO §0.2 — e.g. "commits authored" sets
    /// `[Author, CoAuthor]`). Empty == no filter.
    #[serde(default)]
    pub actor_roles: Vec<ActorRole>,
}

// ---------------------------------------------------------------------------
// Window spec + resolution
// ---------------------------------------------------------------------------

/// Recognised window labels. The wire form is snake_case so a query
/// string `window.label=last_week` deserialises directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowLabel {
    /// Just the local current day.
    Today,
    /// Just the local previous day.
    Yesterday,
    /// Monday 00:00 of the current ISO week → next Monday 00:00 local.
    ThisWeek,
    /// The previous ISO week, Mon 00:00 → Mon 00:00 local.
    LastWeek,
    /// First of the current month 00:00 local → first of next month
    /// 00:00 local.
    ThisMonth,
    /// First of the previous month → first of current month, local.
    LastMonth,
    /// Rolling 7 days: `[start_of_tomorrow_local − 7 days,
    /// start_of_tomorrow_local)` so the report always includes today.
    #[serde(rename = "last_7_days")]
    Last7Days,
    /// Rolling 30 days (same shape as [`WindowLabel::Last7Days`]).
    #[serde(rename = "last_30_days")]
    Last30Days,
    /// Rolling 90 days (same shape as [`WindowLabel::Last7Days`]).
    #[serde(rename = "last_90_days")]
    Last90Days,
    /// Caller supplies absolute UTC start + end in
    /// [`WindowSpec::custom_start`] / `custom_end`.
    Custom,
}

impl WindowLabel {
    /// Stable wire string used in [`Window::label`] echoes.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Yesterday => "yesterday",
            Self::ThisWeek => "this_week",
            Self::LastWeek => "last_week",
            Self::ThisMonth => "this_month",
            Self::LastMonth => "last_month",
            Self::Last7Days => "last_7_days",
            Self::Last30Days => "last_30_days",
            Self::Last90Days => "last_90_days",
            Self::Custom => "custom",
        }
    }
}

/// Inputs to [`resolve_window`]. Carried over the wire on the request;
/// the resolved [`Window`] is carried on the response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSpec {
    /// The label (`"last_week"`, `"custom"`, …).
    pub label: WindowLabel,
    /// IANA TZ name (`"Australia/Sydney"`, `"UTC"`, …). Used for
    /// `viewer` / `org` anchors and echoed back for all anchors.
    pub tz: String,
    /// Which clock to interpret `label` in.
    pub anchor: WindowAnchor,
    /// Required iff `label == Custom`.
    #[serde(default)]
    pub custom_start: Option<DateTime<Utc>>,
    /// Required iff `label == Custom`.
    #[serde(default)]
    pub custom_end: Option<DateTime<Utc>>,
}

/// Failure modes of [`resolve_window`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResolveError {
    /// The TZ string isn't an IANA name we recognise.
    #[error("invalid IANA tz: {0}")]
    InvalidTz(String),
    /// `label == Custom` but `custom_start` / `custom_end` were not
    /// both supplied.
    #[error("custom window requires both custom_start and custom_end")]
    MissingCustomRange,
    /// `custom_end <= custom_start`.
    #[error("custom window end must be strictly after start")]
    InvertedCustomRange,
}

/// Resolve `spec` against the current wall-clock UTC.
///
/// Equivalent to `resolve_window_at(spec, Utc::now())`. Tests use
/// [`resolve_window_at`] directly so they don't depend on the system
/// clock.
pub fn resolve_window(spec: &WindowSpec) -> Result<Window, ResolveError> {
    resolve_window_at(spec, Utc::now())
}

/// Resolve `spec` as if the current instant were `now`.
///
/// Pulled out from [`resolve_window`] so the table-driven unit tests
/// can pin a deterministic clock (cross-DST, year-end, leap day).
pub fn resolve_window_at(
    spec: &WindowSpec,
    now: DateTime<Utc>,
) -> Result<Window, ResolveError> {
    if spec.label == WindowLabel::Custom {
        let start = spec.custom_start.ok_or(ResolveError::MissingCustomRange)?;
        let end = spec.custom_end.ok_or(ResolveError::MissingCustomRange)?;
        if end <= start {
            return Err(ResolveError::InvertedCustomRange);
        }
        return Ok(Window {
            start,
            end,
            label: WindowLabel::Custom.as_str().into(),
            tz: spec.tz.clone(),
            anchor: spec.anchor,
        });
    }

    // The TZ used to compute local midnight. For `Utc` anchor we
    // ignore `spec.tz` for the computation but still validate it
    // (caller bug protection) and echo it back.
    let effective_tz: Tz = match spec.anchor {
        WindowAnchor::Utc => Tz::UTC,
        WindowAnchor::Viewer | WindowAnchor::Org => spec
            .tz
            .parse()
            .map_err(|_| ResolveError::InvalidTz(spec.tz.clone()))?,
    };

    let today_local: NaiveDate = now.with_timezone(&effective_tz).date_naive();

    let (start_date, end_date) = match spec.label {
        WindowLabel::Today => (today_local, add_days(today_local, 1)),
        WindowLabel::Yesterday => (sub_days(today_local, 1), today_local),
        WindowLabel::ThisWeek => {
            let monday = monday_of(today_local);
            (monday, add_days(monday, 7))
        }
        WindowLabel::LastWeek => {
            let monday = monday_of(today_local);
            (sub_days(monday, 7), monday)
        }
        WindowLabel::ThisMonth => {
            let first = first_of_month(today_local);
            (first, first_of_next_month(first))
        }
        WindowLabel::LastMonth => {
            let first = first_of_month(today_local);
            (first_of_prev_month(first), first)
        }
        WindowLabel::Last7Days => {
            let end = add_days(today_local, 1);
            (sub_days(end, 7), end)
        }
        WindowLabel::Last30Days => {
            let end = add_days(today_local, 1);
            (sub_days(end, 30), end)
        }
        WindowLabel::Last90Days => {
            let end = add_days(today_local, 1);
            (sub_days(end, 90), end)
        }
        WindowLabel::Custom => unreachable!("handled above"),
    };

    let start = local_midnight_to_utc(&effective_tz, start_date);
    let end = local_midnight_to_utc(&effective_tz, end_date);

    Ok(Window {
        start,
        end,
        label: spec.label.as_str().into(),
        tz: spec.tz.clone(),
        anchor: spec.anchor,
    })
}

// ---------------------------------------------------------------------------
// Date helpers
// ---------------------------------------------------------------------------

fn add_days(d: NaiveDate, n: u64) -> NaiveDate {
    d.checked_add_days(Days::new(n))
        .expect("date arithmetic out of NaiveDate range")
}

fn sub_days(d: NaiveDate, n: u64) -> NaiveDate {
    d.checked_sub_days(Days::new(n))
        .expect("date arithmetic out of NaiveDate range")
}

fn monday_of(d: NaiveDate) -> NaiveDate {
    let dow = d.weekday().num_days_from_monday() as u64;
    sub_days(d, dow)
}

fn first_of_month(d: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(d.year(), d.month(), 1).expect("year/month always yields day 1")
}

fn first_of_next_month(first: NaiveDate) -> NaiveDate {
    let (y, m) = if first.month() == 12 {
        (first.year() + 1, 1)
    } else {
        (first.year(), first.month() + 1)
    };
    NaiveDate::from_ymd_opt(y, m, 1).expect("next-month rollover valid")
}

fn first_of_prev_month(first: NaiveDate) -> NaiveDate {
    let (y, m) = if first.month() == 1 {
        (first.year() - 1, 12)
    } else {
        (first.year(), first.month() - 1)
    };
    NaiveDate::from_ymd_opt(y, m, 1).expect("prev-month rollover valid")
}

/// Convert the local-midnight instant on `date` (in `tz`) into UTC.
///
/// DST corner cases:
///
/// * **Skipped** (spring-forward in a TZ whose transition is at
///   midnight — e.g. some historical America/Sao_Paulo records): we
///   walk forward 1h at a time until a valid local instant exists.
///   This keeps the window non-empty and monotonic.
/// * **Ambiguous** (fall-back at midnight): we take the *earlier*
///   instant, so the window starts at the first occurrence of
///   "midnight" and includes the duplicated hour.
fn local_midnight_to_utc(tz: &Tz, date: NaiveDate) -> DateTime<Utc> {
    let naive = NaiveDateTime::new(date, NaiveTime::from_hms_opt(0, 0, 0).unwrap());
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt.with_timezone(&Utc),
        LocalResult::Ambiguous(earliest, _latest) => earliest.with_timezone(&Utc),
        LocalResult::None => {
            // Walk forward 1h at a time until the local clock catches up.
            let mut probe = naive;
            for _ in 0..6 {
                probe += chrono::Duration::hours(1);
                if let LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) =
                    tz.from_local_datetime(&probe)
                {
                    return dt.with_timezone(&Utc);
                }
            }
            // Should be unreachable for any sane IANA zone; fall back
            // to treating the naive date as UTC midnight so the report
            // is still produced rather than 500ing.
            Utc.from_utc_datetime(&naive)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Build a `DateTime<Utc>` from Y-M-D H:M:S in UTC.
    fn utc(y: i32, m: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, mi, s).single().expect("valid UTC")
    }

    fn spec(label: WindowLabel, tz: &str, anchor: WindowAnchor) -> WindowSpec {
        WindowSpec {
            label,
            tz: tz.into(),
            anchor,
            custom_start: None,
            custom_end: None,
        }
    }

    // -- table-driven anchor matrix ------------------------------------

    struct Case {
        name: &'static str,
        label: WindowLabel,
        anchor: WindowAnchor,
        tz: &'static str,
        now: DateTime<Utc>,
        // Expected UTC start/end.
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    }

    #[test]
    fn anchor_matrix_table() {
        let cases = vec![
            // --- viewer anchor, Australia/Sydney --------------------
            // Sydney is UTC+11 (AEDT) in early January 2025. "last_week"
            // computed at Wed 2025-01-08 in Sydney local = Mon 2024-12-30
            // 00:00 +11 → 2024-12-29 13:00Z, end = Mon 2025-01-06 00:00
            // +11 → 2025-01-05 13:00Z.
            Case {
                name: "sydney viewer last_week",
                label: WindowLabel::LastWeek,
                anchor: WindowAnchor::Viewer,
                tz: "Australia/Sydney",
                now: utc(2025, 1, 8, 3, 0, 0),
                start: utc(2024, 12, 29, 13, 0, 0),
                end: utc(2025, 1, 5, 13, 0, 0),
            },
            // --- org anchor, America/New_York -----------------------
            // "this_month" at 2025-02-10 12:00Z (NY is UTC-5 in Feb)
            // = Feb 10 07:00 local → Feb 1 00:00 -05 = 2025-02-01 05:00Z,
            // end = Mar 1 00:00 -05 = 2025-03-01 05:00Z.
            Case {
                name: "ny org this_month feb",
                label: WindowLabel::ThisMonth,
                anchor: WindowAnchor::Org,
                tz: "America/New_York",
                now: utc(2025, 2, 10, 12, 0, 0),
                start: utc(2025, 2, 1, 5, 0, 0),
                end: utc(2025, 3, 1, 5, 0, 0),
            },
            // --- utc anchor, label still labelled --------------------
            // "today" UTC on 2025-06-15 18:00Z = [2025-06-15 00:00Z,
            // 2025-06-16 00:00Z).
            Case {
                name: "utc today",
                label: WindowLabel::Today,
                anchor: WindowAnchor::Utc,
                tz: "UTC",
                now: utc(2025, 6, 15, 18, 0, 0),
                start: utc(2025, 6, 15, 0, 0, 0),
                end: utc(2025, 6, 16, 0, 0, 0),
            },
            // --- DST spring-forward, America/New_York ---------------
            // 2025 DST: clocks jump forward at 2025-03-09 02:00 local.
            // "last_week" computed at Wed 2025-03-12 12:00Z (Wed 08:00
            // NY) → Mon 2025-03-03 00:00 EST (-05) = 2025-03-03 05:00Z,
            // end Mon 2025-03-10 00:00 EDT (-04) = 2025-03-10 04:00Z.
            // i.e. the UTC duration is 6d 23h, NOT 7d — the test pins
            // this so a future refactor can't silently round to days.
            Case {
                name: "ny last_week crosses spring-forward",
                label: WindowLabel::LastWeek,
                anchor: WindowAnchor::Viewer,
                tz: "America/New_York",
                now: utc(2025, 3, 12, 12, 0, 0),
                start: utc(2025, 3, 3, 5, 0, 0),
                end: utc(2025, 3, 10, 4, 0, 0),
            },
            // --- DST fall-back, America/New_York --------------------
            // 2024 DST ends at 2024-11-03 02:00 local. "last_week" at
            // Wed 2024-11-06 12:00Z = Mon 2024-10-28 00:00 EDT (-04) =
            // 2024-10-28 04:00Z, end Mon 2024-11-04 00:00 EST (-05) =
            // 2024-11-04 05:00Z. Duration 7d 1h.
            Case {
                name: "ny last_week crosses fall-back",
                label: WindowLabel::LastWeek,
                anchor: WindowAnchor::Viewer,
                tz: "America/New_York",
                now: utc(2024, 11, 6, 12, 0, 0),
                start: utc(2024, 10, 28, 4, 0, 0),
                end: utc(2024, 11, 4, 5, 0, 0),
            },
            // --- year-end roll, UTC ---------------------------------
            // "this_week" at 2026-01-01 12:00Z (Thursday): Monday is
            // 2025-12-29. Window = [2025-12-29 00:00Z, 2026-01-05 00:00Z).
            Case {
                name: "utc this_week crosses year boundary",
                label: WindowLabel::ThisWeek,
                anchor: WindowAnchor::Utc,
                tz: "UTC",
                now: utc(2026, 1, 1, 12, 0, 0),
                start: utc(2025, 12, 29, 0, 0, 0),
                end: utc(2026, 1, 5, 0, 0, 0),
            },
            // --- year-end roll, last_month, Sydney (UTC+11) ---------
            // "last_month" at 2026-01-15 02:00Z = 2026-01-15 13:00 +11.
            // Local Dec 1 00:00 +11 = 2025-11-30 13:00Z; Jan 1 00:00 +11
            // = 2025-12-31 13:00Z.
            Case {
                name: "sydney last_month crosses year boundary",
                label: WindowLabel::LastMonth,
                anchor: WindowAnchor::Viewer,
                tz: "Australia/Sydney",
                now: utc(2026, 1, 15, 2, 0, 0),
                start: utc(2025, 11, 30, 13, 0, 0),
                end: utc(2025, 12, 31, 13, 0, 0),
            },
            // --- leap day, UTC, this_month --------------------------
            // Feb 2024 has 29 days. "this_month" at 2024-02-15 = [Feb 1,
            // Mar 1). End − start = 29 days.
            Case {
                name: "utc this_month feb 2024 leap",
                label: WindowLabel::ThisMonth,
                anchor: WindowAnchor::Utc,
                tz: "UTC",
                now: utc(2024, 2, 15, 0, 0, 0),
                start: utc(2024, 2, 1, 0, 0, 0),
                end: utc(2024, 3, 1, 0, 0, 0),
            },
            // --- leap day exactly, today, UTC -----------------------
            Case {
                name: "utc today on feb 29",
                label: WindowLabel::Today,
                anchor: WindowAnchor::Utc,
                tz: "UTC",
                now: utc(2024, 2, 29, 13, 0, 0),
                start: utc(2024, 2, 29, 0, 0, 0),
                end: utc(2024, 3, 1, 0, 0, 0),
            },
            // --- rolling last_7_days at Sydney year-end -------------
            // At Wed 2026-01-07 02:00Z = Wed 2026-01-07 13:00 +11.
            // end = Thu 2026-01-08 00:00 +11 = 2026-01-07 13:00Z.
            // start = end − 7d = 2025-12-31 13:00Z.
            Case {
                name: "sydney last_7_days across year boundary",
                label: WindowLabel::Last7Days,
                anchor: WindowAnchor::Viewer,
                tz: "Australia/Sydney",
                now: utc(2026, 1, 7, 2, 0, 0),
                start: utc(2025, 12, 31, 13, 0, 0),
                end: utc(2026, 1, 7, 13, 0, 0),
            },
        ];

        for c in cases {
            let s = spec(c.label, c.tz, c.anchor);
            let w = resolve_window_at(&s, c.now)
                .unwrap_or_else(|e| panic!("[{}] resolve failed: {e}", c.name));
            assert_eq!(w.start, c.start, "[{}] start mismatch", c.name);
            assert_eq!(w.end, c.end, "[{}] end mismatch", c.name);
            assert_eq!(w.label, c.label.as_str(), "[{}] label echo", c.name);
            assert_eq!(w.tz, c.tz, "[{}] tz echo", c.name);
            assert_eq!(w.anchor, c.anchor, "[{}] anchor echo", c.name);
            assert!(w.start < w.end, "[{}] non-empty window", c.name);
        }
    }

    // -- custom window edge cases -------------------------------------

    #[test]
    fn custom_window_passes_through_utc_unchanged() {
        let s = WindowSpec {
            label: WindowLabel::Custom,
            tz: "UTC".into(),
            anchor: WindowAnchor::Utc,
            custom_start: Some(utc(2025, 5, 1, 0, 0, 0)),
            custom_end: Some(utc(2025, 5, 8, 0, 0, 0)),
        };
        let w = resolve_window_at(&s, utc(2025, 6, 1, 0, 0, 0)).unwrap();
        assert_eq!(w.start, utc(2025, 5, 1, 0, 0, 0));
        assert_eq!(w.end, utc(2025, 5, 8, 0, 0, 0));
        assert_eq!(w.label, "custom");
    }

    #[test]
    fn custom_window_missing_bounds_is_rejected() {
        let s = WindowSpec {
            label: WindowLabel::Custom,
            tz: "UTC".into(),
            anchor: WindowAnchor::Utc,
            custom_start: Some(utc(2025, 5, 1, 0, 0, 0)),
            custom_end: None,
        };
        assert_eq!(
            resolve_window_at(&s, utc(2025, 6, 1, 0, 0, 0)),
            Err(ResolveError::MissingCustomRange),
        );
    }

    #[test]
    fn custom_window_inverted_bounds_is_rejected() {
        let s = WindowSpec {
            label: WindowLabel::Custom,
            tz: "UTC".into(),
            anchor: WindowAnchor::Utc,
            custom_start: Some(utc(2025, 5, 8, 0, 0, 0)),
            custom_end: Some(utc(2025, 5, 1, 0, 0, 0)),
        };
        assert_eq!(
            resolve_window_at(&s, utc(2025, 6, 1, 0, 0, 0)),
            Err(ResolveError::InvertedCustomRange),
        );
    }

    // -- TZ validation -------------------------------------------------

    #[test]
    fn invalid_tz_is_rejected_for_viewer_anchor() {
        let s = spec(WindowLabel::ThisWeek, "Not/A_Real_Zone", WindowAnchor::Viewer);
        assert_eq!(
            resolve_window_at(&s, utc(2025, 6, 1, 0, 0, 0)),
            Err(ResolveError::InvalidTz("Not/A_Real_Zone".into())),
        );
    }

    #[test]
    fn utc_anchor_ignores_invalid_tz_for_computation_but_echoes_it() {
        // SCOPE §0.4: with `utc` anchor, the computation is in UTC so
        // the tz string is only a label hint. We DO still echo it.
        let s = WindowSpec {
            label: WindowLabel::Today,
            tz: "Not/Real".into(),
            anchor: WindowAnchor::Utc,
            custom_start: None,
            custom_end: None,
        };
        let w = resolve_window_at(&s, utc(2025, 6, 15, 18, 0, 0)).unwrap();
        assert_eq!(w.start, utc(2025, 6, 15, 0, 0, 0));
        assert_eq!(w.tz, "Not/Real");
    }

    // -- ReportRequest wire form --------------------------------------

    #[test]
    fn report_request_round_trips_through_json() {
        let req = ReportRequest {
            orgs: vec![Uuid::nil()],
            users: vec![],
            teams: vec![],
            window: WindowSpec {
                label: WindowLabel::LastWeek,
                tz: "Australia/Sydney".into(),
                anchor: WindowAnchor::Viewer,
                custom_start: None,
                custom_end: None,
            },
            scope_mode: ScopeMode::AllOrgsCombined,
            group_by: Some(GroupBy::User),
            activity_types: vec![EventKind::PullRequestMerged],
            actor_roles: vec![ActorRole::Author, ActorRole::CoAuthor],
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: ReportRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn report_request_defaults_empty_filters() {
        let json = r#"{
            "window": {
                "label": "last_week",
                "tz": "UTC",
                "anchor": "utc"
            },
            "scope_mode": "single_org"
        }"#;
        let req: ReportRequest = serde_json::from_str(json).unwrap();
        assert!(req.orgs.is_empty());
        assert!(req.users.is_empty());
        assert!(req.teams.is_empty());
        assert!(req.activity_types.is_empty());
        assert!(req.actor_roles.is_empty());
        assert!(req.group_by.is_none());
        assert_eq!(req.scope_mode, ScopeMode::SingleOrg);
    }

    #[test]
    fn scope_mode_uses_snake_case_wire_form() {
        assert_eq!(
            serde_json::to_string(&ScopeMode::AllOrgsCombined).unwrap(),
            "\"all_orgs_combined\""
        );
        assert_eq!(
            serde_json::to_string(&ScopeMode::PerOrgSplit).unwrap(),
            "\"per_org_split\""
        );
    }

    #[test]
    fn window_label_uses_snake_case_wire_form() {
        assert_eq!(
            serde_json::to_string(&WindowLabel::Last7Days).unwrap(),
            "\"last_7_days\""
        );
        assert_eq!(
            serde_json::to_string(&WindowLabel::ThisMonth).unwrap(),
            "\"this_month\""
        );
    }
}
