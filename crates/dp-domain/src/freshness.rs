//! [`DataAsOf`] — per-response data-freshness envelope.
//!
//! Every report response carries one of these (TODO §0.3 / SCOPE
//! §11.7 — "data freshness is visible"). It captures three things
//! the store can answer cheaply, all derived from rows the ingestion
//! layer (Phase 2) already writes:
//!
//! * `webhook_latest` — the most recent `finished` timestamp of a
//!   `dp_fetch_runs` row with `kind = webhook_worker`. `None` means
//!   the webhook worker has never completed a tick (typical right
//!   after install, before the first batch lands).
//! * `reconciler_latest` — the most recent `finished` timestamp of a
//!   `dp_fetch_runs` row with `kind = reconciler`. The reconciler
//!   runs every 4h (TODO §0.1) — if this trails by much more than
//!   that, the on-call should know.
//! * `per_org` — the most recent per-org reconciler tick. The
//!   reconciler advances cursors in `dp_fetch_cursors` as it pulls
//!   each `(org_id, repo_id, resource_kind)` slice; the freshness of
//!   any one org is the max `updated_at` across its cursors. Orgs
//!   with no cursor rows yet (brand-new install, no reconciler tick
//!   has touched them) are absent from the map rather than mapped to
//!   a sentinel — the UI treats absence as "unknown / pending first
//!   reconcile".
//!
//! Lens picker (SCOPE §11.7, surfaced as helpers in
//! `dp_reports::freshness`):
//!
//! | lens                  | UI renders                              |
//! |-----------------------|-----------------------------------------|
//! | single-org            | `per_org[that org]`                     |
//! | all-orgs-combined     | `min(per_org.values())` across requested orgs |
//! | per-org-split         | per row (one freshness per group)       |
//!
//! The `webhook_latest` / `reconciler_latest` headline values are
//! still surfaced on every response so an operator looking at any
//! single report can see whether the *system* is healthy even if a
//! particular org happens to be quiet.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Per-response data-freshness envelope. See module docs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DataAsOf {
    /// Most recent finished `webhook_worker` run. `None` until the
    /// first webhook tick completes.
    pub webhook_latest: Option<DateTime<Utc>>,
    /// Most recent finished `reconciler` run. `None` until the first
    /// reconciler tick completes.
    pub reconciler_latest: Option<DateTime<Utc>>,
    /// Most recent cursor advance per org. Absent orgs have never
    /// been touched by the reconciler — the UI treats that as
    /// "pending first reconcile", not as "stale".
    pub per_org: HashMap<Uuid, DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn default_is_all_none_and_empty() {
        let d = DataAsOf::default();
        assert!(d.webhook_latest.is_none());
        assert!(d.reconciler_latest.is_none());
        assert!(d.per_org.is_empty());
    }

    #[test]
    fn equality_includes_every_field() {
        let t = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).single().unwrap();
        let org = Uuid::new_v4();
        let a = DataAsOf {
            webhook_latest: Some(t),
            reconciler_latest: Some(t),
            per_org: HashMap::from([(org, t)]),
        };
        let mut b = a.clone();
        assert_eq!(a, b);
        b.per_org.insert(Uuid::new_v4(), t);
        assert_ne!(a, b);
    }
}
