//! Text <-> enum helpers.
//!
//! The schema stores every closed enum as `TEXT` (TODO §0.4 — keeps
//! the migration story straightforward and avoids PG enums that need
//! a migration to extend). `MembershipRole::Other(s)` round-trips
//! verbatim; anything else uses its `snake_case` wire form so the
//! database column matches the JSON wire form on the read path.

use dp_domain::event::{ActorRole, EventKind};
use dp_domain::fetch::{FetchRunKind, ResourceKind};
use dp_domain::membership::MembershipRole;

// ---- ActorRole ------------------------------------------------------

pub(crate) fn actor_role_to_text(r: ActorRole) -> &'static str {
    match r {
        ActorRole::Author => "author",
        ActorRole::CoAuthor => "co_author",
        ActorRole::Committer => "committer",
        ActorRole::Merger => "merger",
        ActorRole::Reviewer => "reviewer",
        ActorRole::Commenter => "commenter",
        ActorRole::Assignee => "assignee",
        ActorRole::Requester => "requester",
        ActorRole::Closer => "closer",
    }
}

pub(crate) fn actor_role_from_text(s: &str) -> Result<ActorRole, String> {
    Ok(match s {
        "author" => ActorRole::Author,
        "co_author" => ActorRole::CoAuthor,
        "committer" => ActorRole::Committer,
        "merger" => ActorRole::Merger,
        "reviewer" => ActorRole::Reviewer,
        "commenter" => ActorRole::Commenter,
        "assignee" => ActorRole::Assignee,
        "requester" => ActorRole::Requester,
        "closer" => ActorRole::Closer,
        other => return Err(format!("unknown actor role: {other}")),
    })
}

// ---- EventKind ------------------------------------------------------

pub(crate) fn event_kind_to_text(k: EventKind) -> &'static str {
    match k {
        EventKind::Commit => "commit",
        EventKind::PullRequestOpened => "pull_request_opened",
        EventKind::PullRequestMerged => "pull_request_merged",
        EventKind::PullRequestClosed => "pull_request_closed",
        EventKind::Review => "review",
        EventKind::ReviewComment => "review_comment",
        EventKind::IssueOpened => "issue_opened",
        EventKind::IssueClosed => "issue_closed",
        EventKind::IssueComment => "issue_comment",
        EventKind::WorkflowRun => "workflow_run",
        EventKind::Deployment => "deployment",
        EventKind::Release => "release",
    }
}

pub(crate) fn event_kind_from_text(s: &str) -> Result<EventKind, String> {
    Ok(match s {
        "commit" => EventKind::Commit,
        "pull_request_opened" => EventKind::PullRequestOpened,
        "pull_request_merged" => EventKind::PullRequestMerged,
        "pull_request_closed" => EventKind::PullRequestClosed,
        "review" => EventKind::Review,
        "review_comment" => EventKind::ReviewComment,
        "issue_opened" => EventKind::IssueOpened,
        "issue_closed" => EventKind::IssueClosed,
        "issue_comment" => EventKind::IssueComment,
        "workflow_run" => EventKind::WorkflowRun,
        "deployment" => EventKind::Deployment,
        "release" => EventKind::Release,
        other => return Err(format!("unknown event kind: {other}")),
    })
}

// ---- MembershipRole -------------------------------------------------
//
// Open vocab: `Other(String)` stores the raw GitHub role verbatim
// (Enterprise can ship custom roles — we'd rather keep the truth than
// drop the row). On the read path anything that isn't `admin` or
// `member` is decoded as `Other(s)`.

pub(crate) fn membership_role_to_text(r: &MembershipRole) -> &str {
    match r {
        MembershipRole::Admin => "admin",
        MembershipRole::Member => "member",
        MembershipRole::Other(s) => s.as_str(),
    }
}

pub(crate) fn membership_role_from_text(s: &str) -> MembershipRole {
    match s {
        "admin" => MembershipRole::Admin,
        "member" => MembershipRole::Member,
        other => MembershipRole::Other(other.to_string()),
    }
}

// ---- ResourceKind ---------------------------------------------------

pub(crate) fn resource_kind_to_text(k: ResourceKind) -> &'static str {
    match k {
        ResourceKind::Commits => "commits",
        ResourceKind::PullRequests => "pull_requests",
        ResourceKind::Reviews => "reviews",
        ResourceKind::ReviewComments => "review_comments",
        ResourceKind::Issues => "issues",
        ResourceKind::IssueComments => "issue_comments",
        ResourceKind::WorkflowRuns => "workflow_runs",
        ResourceKind::Deployments => "deployments",
        ResourceKind::Releases => "releases",
        ResourceKind::Members => "members",
        ResourceKind::Teams => "teams",
    }
}

pub(crate) fn resource_kind_from_text(s: &str) -> Result<ResourceKind, String> {
    Ok(match s {
        "commits" => ResourceKind::Commits,
        "pull_requests" => ResourceKind::PullRequests,
        "reviews" => ResourceKind::Reviews,
        "review_comments" => ResourceKind::ReviewComments,
        "issues" => ResourceKind::Issues,
        "issue_comments" => ResourceKind::IssueComments,
        "workflow_runs" => ResourceKind::WorkflowRuns,
        "deployments" => ResourceKind::Deployments,
        "releases" => ResourceKind::Releases,
        "members" => ResourceKind::Members,
        "teams" => ResourceKind::Teams,
        other => return Err(format!("unknown resource kind: {other}")),
    })
}

// ---- FetchRunKind ---------------------------------------------------

pub(crate) fn fetch_run_kind_to_text(k: FetchRunKind) -> &'static str {
    match k {
        FetchRunKind::WebhookWorker => "webhook_worker",
        FetchRunKind::Reconciler => "reconciler",
        FetchRunKind::Backfill => "backfill",
    }
}

pub(crate) fn fetch_run_kind_from_text(s: &str) -> Result<FetchRunKind, String> {
    Ok(match s {
        "webhook_worker" => FetchRunKind::WebhookWorker,
        "reconciler" => FetchRunKind::Reconciler,
        "backfill" => FetchRunKind::Backfill,
        other => return Err(format!("unknown fetch run kind: {other}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_role_round_trips_for_every_variant() {
        for r in [
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
            assert_eq!(actor_role_from_text(actor_role_to_text(r)).unwrap(), r);
        }
    }

    #[test]
    fn event_kind_round_trips_for_every_variant() {
        for k in [
            EventKind::Commit,
            EventKind::PullRequestOpened,
            EventKind::PullRequestMerged,
            EventKind::PullRequestClosed,
            EventKind::Review,
            EventKind::ReviewComment,
            EventKind::IssueOpened,
            EventKind::IssueClosed,
            EventKind::IssueComment,
            EventKind::WorkflowRun,
            EventKind::Deployment,
            EventKind::Release,
        ] {
            assert_eq!(event_kind_from_text(event_kind_to_text(k)).unwrap(), k);
        }
    }

    #[test]
    fn membership_role_open_vocab_round_trips() {
        assert_eq!(
            membership_role_from_text(membership_role_to_text(&MembershipRole::Admin)),
            MembershipRole::Admin
        );
        let custom = MembershipRole::Other("billing_manager".into());
        assert_eq!(
            membership_role_from_text(membership_role_to_text(&custom)),
            custom
        );
    }

    #[test]
    fn resource_and_fetch_kind_round_trip() {
        for k in [
            ResourceKind::Commits,
            ResourceKind::Members,
            ResourceKind::WorkflowRuns,
        ] {
            assert_eq!(
                resource_kind_from_text(resource_kind_to_text(k)).unwrap(),
                k
            );
        }
        for k in [
            FetchRunKind::Backfill,
            FetchRunKind::Reconciler,
            FetchRunKind::WebhookWorker,
        ] {
            assert_eq!(
                fetch_run_kind_from_text(fetch_run_kind_to_text(k)).unwrap(),
                k
            );
        }
    }
}
