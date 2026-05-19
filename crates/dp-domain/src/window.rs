//! [`Window`] — the time-range contract used by every report.
//!
//! TODO §0.4 locks the semantics: all stored timestamps are UTC, but
//! "last week" is ambiguous across audiences (manager-on-EU-time vs
//! exec-comparing-companies). The window carries an explicit IANA
//! time-zone string and an [`anchor`](WindowAnchor) so the server
//! resolves `(label, tz, anchor)` to a concrete UTC `[start, end)` —
//! never the frontend.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Which clock "last week" / "this month" is interpreted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowAnchor {
    /// The viewer's IANA TZ. Default for individual-developer
    /// reports.
    Viewer,
    /// The org's configured TZ. Default for manager reports.
    Org,
    /// Strict UTC. Default for exec / cross-company reports where a
    /// single shared frame matters more than local alignment.
    Utc,
}

/// A resolved time window. The fields are populated on the way in
/// (from `(label, tz, anchor)`) **and** echoed back on the way out so
/// the UI can label the report unambiguously.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    /// Inclusive start, in UTC.
    pub start: DateTime<Utc>,
    /// Exclusive end, in UTC.
    pub end: DateTime<Utc>,
    /// Human label that produced this window (`"last_week"`,
    /// `"this_month"`, `"custom"`, …).
    pub label: String,
    /// IANA TZ string used to resolve the label (e.g.
    /// `"Australia/Sydney"`). Kept even when `anchor` is `Utc` so the
    /// response can be re-rendered in the viewer's clock.
    pub tz: String,
    /// Which clock the label was interpreted in.
    pub anchor: WindowAnchor,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let w = Window {
            start: Utc::now(),
            end: Utc::now(),
            label: "last_week".into(),
            tz: "Australia/Sydney".into(),
            anchor: WindowAnchor::Org,
        };
        let back: Window = serde_json::from_str(&serde_json::to_string(&w).unwrap()).unwrap();
        assert_eq!(w, back);
    }

    #[test]
    fn anchor_uses_snake_case_wire_form() {
        assert_eq!(
            serde_json::to_string(&WindowAnchor::Utc).unwrap(),
            "\"utc\""
        );
    }
}
