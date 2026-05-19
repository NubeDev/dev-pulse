//! Org-scope lenses (SCOPE §8.1).
//!
//! Three pure functions over the [`EventActorRow`] projection from
//! the Phase 1 store. The store has already filtered to the window
//! and (optionally) to the requested orgs / repos / users / roles via
//! [`Store::list_event_actor_rows_in_window`][store-list]; these
//! lenses then shape the rows according to the caller-selected
//! [`ScopeMode`][crate::ScopeMode]:
//!
//! * [`single_org`] — narrow to one org. Trivial filter.
//! * [`all_orgs_combined`] — union across orgs, de-duplicating on
//!   the **`(user_id, event_id)` pair** rather than on events alone.
//!   This is the TODO §0.2 rule: a co-authored commit that lands in
//!   our store with the same actor credited under more than one role
//!   (author + committer on a squash-merge, author of the original
//!   commit + author of the merge in a fork-merge straddling two
//!   orgs) collapses to a single contribution per user, not double-
//!   counted.
//! * [`per_org_split`] — bucket by `(user_id, org_id)` so the same
//!   person's work in two orgs surfaces as two buckets, exposing
//!   context-switching (SCOPE §8.1 third lens).
//!
//! All three are pure (no I/O, no clock reads) so the SCOPE §11.4
//! spot-check harness can hand them recorded fixture rows and
//! compare counts against the GitHub reference response exactly.
//!
//! [store-list]: dp_domain::store::Store::list_event_actor_rows_in_window

use std::collections::{BTreeMap, HashSet};

use uuid::Uuid;

use dp_domain::store::EventActorRow;

/// Single-org lens (SCOPE §8.1, first lens).
///
/// Returns the subset of `rows` whose `org_id` matches `org_id`,
/// preserving input order. The store usually already narrows the
/// query to one org when this lens is in play, but applying the
/// filter here as well keeps the lens a single source of truth — the
/// caller never has to "remember" whether the store-side filter ran.
pub fn single_org(rows: &[EventActorRow], org_id: Uuid) -> Vec<EventActorRow> {
    rows.iter()
        .filter(|r| r.org_id == org_id)
        .cloned()
        .collect()
}

/// All-orgs-combined lens (SCOPE §8.1, second lens).
///
/// Returns one row per distinct `(user_id, event_id)` pair, with
/// first-seen wins so the result is deterministic given the input
/// order the store returns. The TODO §0.2 rule is explicit: de-dup
/// operates on the **pair**, not on `event_id` alone — that's what
/// makes "PRs reviewed" stay correct when the same user is both
/// `reviewer` and `commenter` on the same PR, and what makes the
/// co-authored-commit case across two orgs (same external commit,
/// recorded under both org_ids via shared external_id) count once
/// per author rather than twice.
pub fn all_orgs_combined(rows: &[EventActorRow]) -> Vec<EventActorRow> {
    let mut seen: HashSet<(Uuid, Uuid)> = HashSet::with_capacity(rows.len());
    rows.iter()
        .filter(|r| seen.insert((r.user_id, r.event_id)))
        .cloned()
        .collect()
}

/// Per-org-split lens (SCOPE §8.1, third lens).
///
/// Groups `rows` by `(user_id, org_id)`. The same user appearing in
/// two orgs lands in two buckets — that's the whole point of this
/// lens, which is to make context-switching visible to managers
/// looking across a multi-org tenant.
///
/// Returns a [`BTreeMap`] so iteration order is stable for
/// snapshot-style fixture tests.
pub fn per_org_split(
    rows: &[EventActorRow],
) -> BTreeMap<(Uuid, Uuid), Vec<EventActorRow>> {
    let mut out: BTreeMap<(Uuid, Uuid), Vec<EventActorRow>> = BTreeMap::new();
    for r in rows {
        out.entry((r.user_id, r.org_id)).or_default().push(r.clone());
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, TimeZone, Utc};
    use dp_domain::event::{ActorRole, EventKind};

    fn uid(b: u8) -> Uuid {
        Uuid::from_bytes([b; 16])
    }

    fn ts(min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2025, 1, 6, 12, min, 0).single().unwrap()
    }

    fn row(
        event_id: Uuid,
        user_id: Uuid,
        role: ActorRole,
        org_id: Uuid,
        repo_id: Uuid,
        kind: EventKind,
        t: DateTime<Utc>,
    ) -> EventActorRow {
        EventActorRow {
            event_id,
            user_id,
            role,
            org_id,
            repo_id,
            kind,
            ts: t,
        }
    }

    // -- single_org ---------------------------------------------------

    #[test]
    fn single_org_keeps_only_matching_org_rows_in_order() {
        let org_a = uid(0xA);
        let org_b = uid(0xB);
        let e1 = uid(1);
        let e2 = uid(2);
        let e3 = uid(3);
        let u1 = uid(0x11);
        let u2 = uid(0x22);

        let rows = vec![
            row(e1, u1, ActorRole::Author, org_a, uid(0x33), EventKind::Commit, ts(1)),
            row(e2, u2, ActorRole::Reviewer, org_b, uid(0x44), EventKind::Review, ts(2)),
            row(e3, u1, ActorRole::Author, org_a, uid(0x33), EventKind::Commit, ts(3)),
        ];

        let out = single_org(&rows, org_a);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].event_id, e1);
        assert_eq!(out[1].event_id, e3);
        assert!(out.iter().all(|r| r.org_id == org_a));
    }

    #[test]
    fn single_org_returns_empty_when_no_match() {
        let rows = vec![row(
            uid(1),
            uid(2),
            ActorRole::Author,
            uid(0xA),
            uid(3),
            EventKind::Commit,
            ts(0),
        )];
        assert!(single_org(&rows, uid(0xB)).is_empty());
    }

    // -- all_orgs_combined --------------------------------------------

    #[test]
    fn all_orgs_combined_collapses_multi_role_same_event() {
        // Squash-merge: one user appears as both Author and Committer
        // on the same commit event. The pair (user_id, event_id) is
        // identical, so all_orgs_combined keeps exactly one row.
        let org = uid(0xA);
        let event = uid(1);
        let user = uid(0x11);
        let repo = uid(0x22);

        let rows = vec![
            row(event, user, ActorRole::Author, org, repo, EventKind::Commit, ts(1)),
            row(event, user, ActorRole::Committer, org, repo, EventKind::Commit, ts(1)),
        ];

        let out = all_orgs_combined(&rows);
        assert_eq!(out.len(), 1, "multi-role same user same event must collapse");
        // First-seen wins.
        assert_eq!(out[0].role, ActorRole::Author);
    }

    #[test]
    fn all_orgs_combined_keeps_distinct_users_on_one_event() {
        // Co-authored commit: two distinct users on one event. Both
        // pairs are unique, so both rows survive.
        let org = uid(0xA);
        let event = uid(1);
        let u1 = uid(0x11);
        let u2 = uid(0x22);
        let repo = uid(0x33);

        let rows = vec![
            row(event, u1, ActorRole::Author, org, repo, EventKind::Commit, ts(1)),
            row(event, u2, ActorRole::CoAuthor, org, repo, EventKind::Commit, ts(1)),
        ];

        let out = all_orgs_combined(&rows);
        assert_eq!(out.len(), 2);
        let users: HashSet<Uuid> = out.iter().map(|r| r.user_id).collect();
        assert!(users.contains(&u1));
        assert!(users.contains(&u2));
    }

    /// The SCOPE §11.4 trust regression test, called out in this stage:
    /// a co-authored commit that lands in our store under two orgs
    /// (same external commit recorded against both orgs via the shared
    /// `external_id`, so the upsert resolves to one `event_id`) MUST
    /// count once per user in the all-orgs-combined lens. If a future
    /// refactor regresses to deduping on `event_id` alone, the
    /// co-author row gets silently dropped; if it regresses to no
    /// dedup, the primary author gets double-counted. Both are wrong
    /// and this test catches both.
    #[test]
    fn all_orgs_combined_cross_org_co_author_counts_once_per_user() {
        // event E: commit with author=u1 and co_author=u2. The same
        // event_id (post-external_id dedup) is reachable under org_a
        // AND org_b — e.g. fork-of-fork relationships where the
        // reconciler picks it up via both repo cursors. We model this
        // as four rows: (u1, author) under each org and (u2, co_author)
        // under each org.
        let org_a = uid(0xA);
        let org_b = uid(0xB);
        let event = uid(1);
        let u1 = uid(0x11);
        let u2 = uid(0x22);
        let repo = uid(0x33);

        let rows = vec![
            row(event, u1, ActorRole::Author,   org_a, repo, EventKind::Commit, ts(1)),
            row(event, u2, ActorRole::CoAuthor, org_a, repo, EventKind::Commit, ts(1)),
            row(event, u1, ActorRole::Author,   org_b, repo, EventKind::Commit, ts(1)),
            row(event, u2, ActorRole::CoAuthor, org_b, repo, EventKind::Commit, ts(1)),
        ];

        let out = all_orgs_combined(&rows);

        // Exactly two surviving rows: one per user, both pinned to
        // event E. NEITHER user is double-counted, NEITHER is dropped.
        assert_eq!(out.len(), 2, "must count once per user across orgs");
        let pairs: HashSet<(Uuid, Uuid)> =
            out.iter().map(|r| (r.user_id, r.event_id)).collect();
        assert!(pairs.contains(&(u1, event)));
        assert!(pairs.contains(&(u2, event)));

        // First-seen org wins for each user — deterministic.
        let by_user: BTreeMap<Uuid, Uuid> =
            out.iter().map(|r| (r.user_id, r.org_id)).collect();
        assert_eq!(by_user[&u1], org_a);
        assert_eq!(by_user[&u2], org_a);
    }

    #[test]
    fn all_orgs_combined_keeps_distinct_events_for_same_user() {
        // Same user, two different events (different commits in two
        // orgs). Pairs (user, event) differ, both kept. This is the
        // "they actually did work in two orgs" case, NOT a dedup
        // candidate.
        let user = uid(0x11);
        let e1 = uid(1);
        let e2 = uid(2);

        let rows = vec![
            row(e1, user, ActorRole::Author, uid(0xA), uid(0x33), EventKind::Commit, ts(1)),
            row(e2, user, ActorRole::Author, uid(0xB), uid(0x44), EventKind::Commit, ts(2)),
        ];

        let out = all_orgs_combined(&rows);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn all_orgs_combined_preserves_input_order() {
        let org = uid(0xA);
        let user = uid(0x11);
        let repo = uid(0x22);
        let e1 = uid(1);
        let e2 = uid(2);
        let e3 = uid(3);

        let rows = vec![
            row(e2, user, ActorRole::Author, org, repo, EventKind::Commit, ts(2)),
            row(e1, user, ActorRole::Author, org, repo, EventKind::Commit, ts(1)),
            row(e3, user, ActorRole::Author, org, repo, EventKind::Commit, ts(3)),
        ];

        let out = all_orgs_combined(&rows);
        let ids: Vec<Uuid> = out.iter().map(|r| r.event_id).collect();
        assert_eq!(ids, vec![e2, e1, e3]);
    }

    #[test]
    fn all_orgs_combined_on_empty_input_returns_empty() {
        let out = all_orgs_combined(&[]);
        assert!(out.is_empty());
    }

    // -- per_org_split ------------------------------------------------

    #[test]
    fn per_org_split_buckets_by_user_and_org() {
        let org_a = uid(0xA);
        let org_b = uid(0xB);
        let u1 = uid(0x11);
        let u2 = uid(0x22);
        let repo = uid(0x33);

        let rows = vec![
            row(uid(1), u1, ActorRole::Author, org_a, repo, EventKind::Commit, ts(1)),
            row(uid(2), u1, ActorRole::Author, org_a, repo, EventKind::Commit, ts(2)),
            row(uid(3), u1, ActorRole::Author, org_b, repo, EventKind::Commit, ts(3)),
            row(uid(4), u2, ActorRole::Author, org_b, repo, EventKind::Commit, ts(4)),
        ];

        let buckets = per_org_split(&rows);

        assert_eq!(buckets.len(), 3, "(u1,a), (u1,b), (u2,b) — 3 buckets");
        assert_eq!(buckets[&(u1, org_a)].len(), 2);
        assert_eq!(buckets[&(u1, org_b)].len(), 1);
        assert_eq!(buckets[&(u2, org_b)].len(), 1);
    }

    #[test]
    fn per_org_split_does_not_dedup_multi_role_rows() {
        // Per-org-split is intentionally NOT the place where dedup
        // happens — that's all_orgs_combined's job. If a downstream
        // metric wants per-(user, org) counts of unique events, it
        // applies the (user_id, event_id) dedup itself; this lens
        // just bucketises so context-switching is visible.
        let org = uid(0xA);
        let user = uid(0x11);
        let event = uid(1);
        let repo = uid(0x22);

        let rows = vec![
            row(event, user, ActorRole::Author, org, repo, EventKind::Commit, ts(1)),
            row(event, user, ActorRole::Committer, org, repo, EventKind::Commit, ts(1)),
        ];

        let buckets = per_org_split(&rows);
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[&(user, org)].len(), 2);
    }

    #[test]
    fn per_org_split_on_empty_input_returns_empty_map() {
        let buckets = per_org_split(&[]);
        assert!(buckets.is_empty());
    }
}
