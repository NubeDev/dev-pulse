//! [`ActivityEvent`] + [`EventActor`] — the multi-actor activity log.
//!
//! TODO §0.2 is explicit: events do **not** carry a `user_id` column.
//! Attribution lives in [`EventActor`] rows keyed by
//! `(event_id, user_id, role)`. This is the only way to model
//! co-authored commits, multi-reviewer PRs, and squash-merge
//! author/committer splits without either losing co-authors or
//! double-counting events.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

/// Kinds of activity tracked. Mirrors SCOPE §6 categories, expanded
/// to the granularity the fetcher actually receives from GitHub
/// webhooks / REST.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// A commit landed on a branch.
    Commit,
    /// A pull request was opened.
    PullRequestOpened,
    /// A pull request was merged.
    PullRequestMerged,
    /// A pull request was closed without merge.
    PullRequestClosed,
    /// A pull request review was submitted (approve / request changes
    /// / comment).
    Review,
    /// A review comment was left on a PR.
    ReviewComment,
    /// An issue was opened.
    IssueOpened,
    /// An issue was closed.
    IssueClosed,
    /// A comment was added to an issue.
    IssueComment,
    /// A workflow run completed.
    WorkflowRun,
    /// A deployment was created.
    Deployment,
    /// A release was published.
    Release,
}

/// A user's role in a specific [`ActivityEvent`]. TODO §0.2 lists the
/// canonical set; reports filter on subsets per metric (e.g. "commits
/// authored" filters `role IN (author, co_author)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorRole {
    /// Primary author (commit author, PR opener, issue opener, …).
    Author,
    /// Listed via `Co-authored-by:` trailer.
    CoAuthor,
    /// Recorded as committer (distinct from author for squash-merges).
    Committer,
    /// Pressed the merge button.
    Merger,
    /// Submitted a PR review.
    Reviewer,
    /// Left a comment.
    Commenter,
    /// Assigned to an issue or PR.
    Assignee,
    /// Requested a review.
    Requester,
    /// Closed an issue or PR.
    Closer,
}

/// One row in `activity_events`. **No `user_id`** — see
/// [`EventActor`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityEvent {
    /// Internal primary key.
    pub id: Uuid,
    /// Org the event happened in.
    pub org_id: Uuid,
    /// Repo the event happened in.
    pub repo_id: Uuid,
    /// What kind of event this is.
    pub kind: EventKind,
    /// When the event occurred at the source (GitHub's timestamp,
    /// UTC). Reports key off this, not `received_at`.
    pub ts: DateTime<Utc>,
    /// GitHub's stable identifier (e.g. node_id) for idempotent
    /// upsert. Unique together with `kind` is enough to dedup
    /// replays.
    pub external_id: String,
    /// Trimmed projection of the source payload (SCOPE §0 REVIEW
    /// decision — keep the minimum needed for reports + the
    /// `external_id` to refetch the raw form from GitHub).
    pub payload: JsonValue,
}

/// One actor's involvement in one event. Composite PK on
/// `(event_id, user_id, role)` so the same user can appear in two
/// roles on the same event (e.g. author + committer on a squash
/// merge) without conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventActor {
    /// Event this attribution belongs to.
    pub event_id: Uuid,
    /// User credited.
    pub user_id: Uuid,
    /// Role the user played in this event.
    pub role: ActorRole,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn activity_event_round_trips() {
        let e = ActivityEvent {
            id: Uuid::nil(),
            org_id: Uuid::nil(),
            repo_id: Uuid::nil(),
            kind: EventKind::PullRequestMerged,
            ts: Utc::now(),
            external_id: "PR_kwDOAAAA".into(),
            payload: json!({ "number": 42, "merged": true }),
        };
        let back: ActivityEvent =
            serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn event_actor_round_trips_for_every_role() {
        for role in [
            ActorRole::Author,
            ActorRole::CoAuthor,
            ActorRole::Committer,
            ActorRole::Merger,
            ActorRole::Reviewer,
            ActorRole::Commenter,
            ActorRole::Assignee,
            ActorRole::Requester,
            ActorRole::Closer,
        ] {
            let a = EventActor {
                event_id: Uuid::nil(),
                user_id: Uuid::nil(),
                role,
            };
            let back: EventActor =
                serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
            assert_eq!(a, back);
        }
    }

    #[test]
    fn event_kind_uses_snake_case_wire_form() {
        // Guard the wire format — reports + fixtures depend on it.
        assert_eq!(
            serde_json::to_string(&EventKind::PullRequestMerged).unwrap(),
            "\"pull_request_merged\""
        );
        assert_eq!(
            serde_json::to_string(&ActorRole::CoAuthor).unwrap(),
            "\"co_author\""
        );
    }
}
