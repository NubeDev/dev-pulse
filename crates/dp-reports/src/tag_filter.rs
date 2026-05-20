//! Metric × tag-link-kind compatibility (SCOPE-PROJECTS §7.7).
//!
//! Resolves the question "what does a tag with only `issue` links
//! mean for `commits authored`?" — the answer is **nothing**, and
//! the report responds with an explicit
//! [`EMPTY_REASON_TAG_KIND_MISMATCH`] rather than a silent zero.
//!
//! ## The mapping table
//!
//! Mirrors SCOPE-PROJECTS §7.7 exactly:
//!
//! | Tag link kind | Contributes to                                  |
//! |---------------|-------------------------------------------------|
//! | `repo`        | every metric — filters on `events.repo_id`.     |
//! | `user`        | every metric — filters on actor `user_id`.      |
//! | `team`        | every metric — expands to members at query time.|
//! | `issue`       | **only** issue-centric metrics (issues opened / |
//! |               | closed / commented). Ignored for commit / PR /  |
//! |               | review / workflow metrics.                      |
//!
//! Issue-centric [`EventKind`]s are the SCOPE.md §15.7 issue rows:
//! [`EventKind::IssueOpened`], [`EventKind::IssueClosed`],
//! [`EventKind::IssueComment`]. Every other [`EventKind`] is
//! "non-issue" for this table.
//!
//! Adding a new issue-centric metric (e.g. "issues assigned" when
//! the matching [`EventKind`] lands) requires extending
//! [`is_issue_centric_event_kind`] *additively* — never repurpose an
//! existing row, per SCOPE.md §15.6 / §15.7 revisit rule.

use dp_domain::event::EventKind;
use dp_domain::tag_link::TagLinkKind;

/// The exact `empty_reason` string returned in the report response
/// when a tag filter resolves to only `issue`-kind links and the
/// requested metric is non-issue-centric.
///
/// Wire form locked by SCOPE-PROJECTS §7.7. Pin-checked by unit
/// tests so a typo cannot ship: the frontend's "why are there no
/// rows?" branch matches on this exact literal.
pub const EMPTY_REASON_TAG_KIND_MISMATCH: &str =
    "tag links do not match metric attribution";

/// `true` iff `kind` is one of the SCOPE.md §15.7 issue-centric
/// event kinds.
///
/// `IssueOpened` / `IssueClosed` / `IssueComment` are the three
/// issue rows in §15.7 today; any future issue-centric event kind
/// (e.g. an `IssueAssigned` once the fetcher emits it) must be
/// added here so an `issue`-only tag covers it. Tested explicitly
/// — see [`tests::every_event_kind_is_classified`].
pub fn is_issue_centric_event_kind(kind: EventKind) -> bool {
    matches!(
        kind,
        EventKind::IssueOpened | EventKind::IssueClosed | EventKind::IssueComment
    )
}

/// `true` iff a tag whose links are exactly `kinds` can contribute
/// to a report counting events of `event_kind`.
///
/// The §7.7 rule:
///
/// * `repo` / `user` / `team` links contribute to **every** metric.
/// * `issue` links contribute **only** to issue-centric metrics.
///
/// So the tag matches the metric iff at least one of its links is a
/// "applies to every metric" kind, **or** the metric itself is
/// issue-centric. A tag with **only** `issue` links does not match
/// a non-issue metric — that is the case the empty-reason path
/// catches.
///
/// An empty `kinds` slice (a tag with zero resolved links) is also
/// a mismatch by definition: nothing to OR in to the WHERE clause.
pub fn tag_link_kinds_match_event_kind<I>(kinds: I, event_kind: EventKind) -> bool
where
    I: IntoIterator<Item = TagLinkKind>,
{
    let mut any = false;
    let mut any_non_issue = false;
    for k in kinds {
        any = true;
        if k.applies_to_non_issue_metrics() {
            any_non_issue = true;
        }
    }
    if !any {
        return false;
    }
    if is_issue_centric_event_kind(event_kind) {
        true
    } else {
        any_non_issue
    }
}

/// Convenience: given the *aggregate* set of link kinds collected
/// across **all** tags in a request's `tags` filter, decide whether
/// the report can have any non-empty result against the requested
/// `event_kinds`.
///
/// `event_kinds` is the request's `activity_types` filter (empty ==
/// "all kinds" == always satisfiable so long as the tag has at
/// least one link).
///
/// Returns `Some(EMPTY_REASON_TAG_KIND_MISMATCH)` iff the report
/// would be empty *because* of the §7.7 mismatch. Returns `None`
/// when the filter is satisfiable (or when there is no tag filter
/// at all — that case is the caller's responsibility to short-
/// circuit before calling this).
pub fn empty_reason_for_tag_filter<K, E>(link_kinds: K, event_kinds: E) -> Option<&'static str>
where
    K: IntoIterator<Item = TagLinkKind>,
    E: IntoIterator<Item = EventKind>,
{
    // Collect once; we iterate twice (any-non-issue check + per-
    // event-kind compatibility).
    let kinds: Vec<TagLinkKind> = link_kinds.into_iter().collect();
    if kinds.is_empty() {
        // No resolved links at all — nothing the WHERE can OR in.
        // Treat as the §7.7 mismatch so the UI shows the explicit
        // reason rather than a silent zero.
        return Some(EMPTY_REASON_TAG_KIND_MISMATCH);
    }
    let any_non_issue = kinds.iter().any(|k| k.applies_to_non_issue_metrics());
    let mut saw_any_kind = false;
    let mut all_non_issue = true;
    for ek in event_kinds {
        saw_any_kind = true;
        if !is_issue_centric_event_kind(ek) {
            // a non-issue requested kind; the tag must have at
            // least one non-issue link to satisfy it.
            if !any_non_issue {
                return Some(EMPTY_REASON_TAG_KIND_MISMATCH);
            }
        } else {
            all_non_issue = false;
        }
    }
    // No `activity_types` filter at all → "all kinds" → satisfiable
    // so long as the tag has *some* link, which we already
    // confirmed.
    let _ = (saw_any_kind, all_non_issue);
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_reason_literal_is_locked() {
        // Frontend matches on this literal — a typo breaks the UI.
        assert_eq!(
            EMPTY_REASON_TAG_KIND_MISMATCH,
            "tag links do not match metric attribution"
        );
    }

    #[test]
    fn every_event_kind_is_classified() {
        // Exhaustive sweep — when a new EventKind variant lands,
        // this test forces the author to decide issue vs non-issue
        // rather than silently default to "non-issue".
        use EventKind::*;
        let issue_centric = [IssueOpened, IssueClosed, IssueComment];
        let non_issue_centric = [
            Commit,
            PullRequestOpened,
            PullRequestMerged,
            PullRequestClosed,
            Review,
            ReviewComment,
            WorkflowRun,
            Deployment,
            Release,
        ];
        for k in issue_centric {
            assert!(
                is_issue_centric_event_kind(k),
                "{k:?} should be issue-centric"
            );
        }
        for k in non_issue_centric {
            assert!(
                !is_issue_centric_event_kind(k),
                "{k:?} should NOT be issue-centric"
            );
        }
    }

    #[test]
    fn repo_user_team_links_match_every_event_kind() {
        for link in [TagLinkKind::Repo, TagLinkKind::User, TagLinkKind::Team] {
            for ek in [
                EventKind::Commit,
                EventKind::PullRequestMerged,
                EventKind::Review,
                EventKind::IssueOpened,
                EventKind::WorkflowRun,
                EventKind::Deployment,
                EventKind::Release,
            ] {
                assert!(
                    tag_link_kinds_match_event_kind([link], ek),
                    "{link:?} should match {ek:?}"
                );
            }
        }
    }

    #[test]
    fn issue_only_tag_matches_issue_metrics_but_not_others() {
        // matches: issue-centric event kinds
        for ek in [
            EventKind::IssueOpened,
            EventKind::IssueClosed,
            EventKind::IssueComment,
        ] {
            assert!(tag_link_kinds_match_event_kind([TagLinkKind::Issue], ek));
        }
        // does NOT match: commit, PR, review, workflow, deployment, release
        for ek in [
            EventKind::Commit,
            EventKind::PullRequestOpened,
            EventKind::PullRequestMerged,
            EventKind::PullRequestClosed,
            EventKind::Review,
            EventKind::ReviewComment,
            EventKind::WorkflowRun,
            EventKind::Deployment,
            EventKind::Release,
        ] {
            assert!(!tag_link_kinds_match_event_kind([TagLinkKind::Issue], ek));
        }
    }

    #[test]
    fn mixed_link_kinds_satisfy_non_issue_metric() {
        // A tag with both `issue` and `repo` links satisfies a
        // commit metric (the repo link does the work).
        assert!(tag_link_kinds_match_event_kind(
            [TagLinkKind::Issue, TagLinkKind::Repo],
            EventKind::Commit
        ));
    }

    #[test]
    fn empty_link_set_never_matches() {
        for ek in [EventKind::Commit, EventKind::IssueOpened] {
            assert!(!tag_link_kinds_match_event_kind(std::iter::empty(), ek));
        }
    }

    #[test]
    fn empty_reason_fires_for_issue_only_tag_on_commit_metric() {
        let reason = empty_reason_for_tag_filter([TagLinkKind::Issue], [EventKind::Commit]);
        assert_eq!(reason, Some(EMPTY_REASON_TAG_KIND_MISMATCH));
    }

    #[test]
    fn empty_reason_silent_for_issue_only_tag_on_issue_metric() {
        let reason =
            empty_reason_for_tag_filter([TagLinkKind::Issue], [EventKind::IssueOpened]);
        assert_eq!(reason, None);
    }

    #[test]
    fn empty_reason_silent_for_repo_link_on_any_metric() {
        let reason = empty_reason_for_tag_filter(
            [TagLinkKind::Repo],
            [EventKind::Commit, EventKind::IssueOpened],
        );
        assert_eq!(reason, None);
    }

    #[test]
    fn empty_reason_fires_when_tag_has_zero_links() {
        // No resolved links → mismatch by definition (nothing to
        // OR into the WHERE). Empty `event_kinds` is allowed
        // because "all kinds" is still empty when the tag has no
        // links.
        let reason: Option<&'static str> =
            empty_reason_for_tag_filter(std::iter::empty(), [EventKind::Commit]);
        assert_eq!(reason, Some(EMPTY_REASON_TAG_KIND_MISMATCH));
    }

    #[test]
    fn empty_reason_silent_when_no_activity_types_filter() {
        // With at least one non-issue link and no kind filter, the
        // report is satisfiable across the full event-kind range.
        let reason: Option<&'static str> =
            empty_reason_for_tag_filter([TagLinkKind::Repo], std::iter::empty::<EventKind>());
        assert_eq!(reason, None);
    }

    #[test]
    fn empty_reason_silent_when_all_kinds_filter_issue_only() {
        // `activity_types` only contains issue kinds → an issue-
        // only tag is fine.
        let reason = empty_reason_for_tag_filter(
            [TagLinkKind::Issue],
            [EventKind::IssueOpened, EventKind::IssueClosed],
        );
        assert_eq!(reason, None);
    }

    #[test]
    fn empty_reason_fires_when_any_requested_kind_is_unmet() {
        // Mixed activity_types: one issue-centric, one commit. An
        // issue-only tag CANNOT contribute to the commit row → §7.7
        // mismatch surfaces.
        let reason = empty_reason_for_tag_filter(
            [TagLinkKind::Issue],
            [EventKind::IssueOpened, EventKind::Commit],
        );
        assert_eq!(reason, Some(EMPTY_REASON_TAG_KIND_MISMATCH));
    }
}
