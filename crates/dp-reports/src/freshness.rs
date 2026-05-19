//! Per-response data-freshness envelope (TODO §0.3 / SCOPE §11.7).
//!
//! Every report response carries a [`DataAsOf`] snapshot. The store
//! produces it via [`Store::data_as_of`] (a single cheap call); this
//! module wraps it with the per-lens picker the UI uses to render the
//! "data as of <timestamp>" line.
//!
//! Lens picker per SCOPE §11.7:
//!
//! | lens                  | render                                         |
//! |-----------------------|------------------------------------------------|
//! | single-org            | [`DataAsOfExt::for_single_org`]                |
//! | all-orgs-combined     | [`DataAsOfExt::for_all_orgs_combined`] (min)   |
//! | per-org-split         | per row — caller iterates [`DataAsOf::per_org`]|
//!
//! All helpers are pure: hand them a recorded `DataAsOf` and the
//! Phase 3 spot-check harness (SCOPE §11.4) can pin the lens output
//! deterministically.
//!
//! [`Store::data_as_of`]: dp_domain::store::Store::data_as_of

use chrono::{DateTime, Utc};
use uuid::Uuid;

pub use dp_domain::freshness::DataAsOf;

use crate::ScopeMode;

/// Lens-specific freshness helpers, exposed as an extension trait so
/// callers can write `data_as_of.for_single_org(org)` without
/// importing extra helpers.
pub trait DataAsOfExt {
    /// Single-org lens: freshness of `org_id`, or `None` if no
    /// reconciler tick has touched that org yet.
    fn for_single_org(&self, org_id: Uuid) -> Option<DateTime<Utc>>;

    /// All-orgs-combined lens: `MIN(per_org[o])` across the requested
    /// `orgs`. `None` if `orgs` is empty, or if none of the requested
    /// orgs have a freshness entry (e.g. brand-new install). The
    /// rationale (SCOPE §11.7) is "the combined view is only as fresh
    /// as its laggiest constituent" — so we pick the min over the
    /// orgs we *have* and treat unknowns as unknowns rather than as a
    /// 1970-sentinel that would yank the headline to zero.
    fn for_all_orgs_combined(&self, orgs: &[Uuid]) -> Option<DateTime<Utc>>;
}

impl DataAsOfExt for DataAsOf {
    fn for_single_org(&self, org_id: Uuid) -> Option<DateTime<Utc>> {
        self.per_org.get(&org_id).copied()
    }

    fn for_all_orgs_combined(&self, orgs: &[Uuid]) -> Option<DateTime<Utc>> {
        if orgs.is_empty() {
            return None;
        }
        orgs.iter()
            .filter_map(|o| self.per_org.get(o).copied())
            .min()
    }
}

/// Pick the rendered freshness for a non-grouped report. Convenience
/// wrapper for the headline case where the UI shows a single
/// "data as of …" line.
///
/// * [`ScopeMode::SingleOrg`] picks the per-org entry for `orgs[0]`
///   (the auth layer has already narrowed `orgs` to one entry —
///   passing more is a caller bug).
/// * [`ScopeMode::AllOrgsCombined`] picks the min across `orgs`.
/// * [`ScopeMode::PerOrgSplit`] returns `None`: the per-row case is
///   the caller's responsibility (one freshness per row) and there
///   is no single value to render.
pub fn pick_headline(
    data_as_of: &DataAsOf,
    scope_mode: ScopeMode,
    orgs: &[Uuid],
) -> Option<DateTime<Utc>> {
    match scope_mode {
        ScopeMode::SingleOrg => orgs.first().and_then(|o| data_as_of.for_single_org(*o)),
        ScopeMode::AllOrgsCombined => data_as_of.for_all_orgs_combined(orgs),
        ScopeMode::PerOrgSplit => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::collections::HashMap;

    fn utc(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, 0, 0).single().unwrap()
    }

    fn sample() -> (Uuid, Uuid, Uuid, DataAsOf) {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let d = DataAsOf {
            webhook_latest: Some(utc(2025, 5, 19, 10)),
            reconciler_latest: Some(utc(2025, 5, 19, 8)),
            per_org: HashMap::from([
                (a, utc(2025, 5, 19, 9)),
                (b, utc(2025, 5, 19, 7)),
                // c absent on purpose — "never reconciled"
            ]),
        };
        (a, b, c, d)
    }

    #[test]
    fn single_org_returns_per_org_entry_or_none() {
        let (a, _b, c, d) = sample();
        assert_eq!(d.for_single_org(a), Some(utc(2025, 5, 19, 9)));
        assert_eq!(d.for_single_org(c), None);
    }

    #[test]
    fn all_orgs_combined_takes_min_across_requested() {
        let (a, b, _c, d) = sample();
        // min(09:00, 07:00) = 07:00
        assert_eq!(
            d.for_all_orgs_combined(&[a, b]),
            Some(utc(2025, 5, 19, 7))
        );
    }

    #[test]
    fn all_orgs_combined_ignores_missing_orgs_rather_than_sentinel() {
        let (a, _b, c, d) = sample();
        // c has no entry — must NOT pull min down to zero; falls back
        // to a only. Treating absence as min-sentinel would make a
        // brand-new org spuriously age the combined view.
        assert_eq!(
            d.for_all_orgs_combined(&[a, c]),
            Some(utc(2025, 5, 19, 9))
        );
    }

    #[test]
    fn all_orgs_combined_returns_none_when_no_entries_match() {
        let d = DataAsOf::default();
        assert_eq!(d.for_all_orgs_combined(&[Uuid::new_v4()]), None);
    }

    #[test]
    fn all_orgs_combined_empty_orgs_is_none() {
        let (.., d) = sample();
        assert_eq!(d.for_all_orgs_combined(&[]), None);
    }

    #[test]
    fn pick_headline_routes_by_scope_mode() {
        let (a, b, _c, d) = sample();
        assert_eq!(
            pick_headline(&d, ScopeMode::SingleOrg, &[a]),
            Some(utc(2025, 5, 19, 9))
        );
        assert_eq!(
            pick_headline(&d, ScopeMode::AllOrgsCombined, &[a, b]),
            Some(utc(2025, 5, 19, 7))
        );
        // Per-org-split has no headline value; UI renders per row.
        assert_eq!(pick_headline(&d, ScopeMode::PerOrgSplit, &[a, b]), None);
    }

    #[test]
    fn pick_headline_single_org_empty_is_none() {
        let (.., d) = sample();
        assert_eq!(pick_headline(&d, ScopeMode::SingleOrg, &[]), None);
    }
}
