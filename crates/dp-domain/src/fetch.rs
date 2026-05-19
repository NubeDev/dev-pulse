//! [`FetchRun`] (run log) + [`FetchCursor`] (per-resource resume
//! point). TODO §0.3 is explicit: there is **no** single global
//! cursor — cursors are per-`(org_id, repo_id, resource_kind)`, and
//! `fetch_runs` is a run log only, never the resume point.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What kind of work a [`FetchRun`] performed. The three ingestion
/// paths from TODO §0.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FetchRunKind {
    /// Drained N rows off `webhook_inbox`.
    WebhookWorker,
    /// 4h reconciliation tick — diff local store vs GitHub, fill
    /// gaps via the cursor pagination path.
    Reconciler,
    /// One-shot historical backfill at install time.
    Backfill,
}

/// Resources fetched via cursor pagination. The set of resource
/// kinds the reconciler / backfill can hold a cursor for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    /// Commits on the default branch (or a configured set of
    /// branches).
    Commits,
    /// Pull requests.
    PullRequests,
    /// PR reviews.
    Reviews,
    /// PR review comments.
    ReviewComments,
    /// Issues.
    Issues,
    /// Issue comments.
    IssueComments,
    /// `workflow_run` events.
    WorkflowRuns,
    /// Deployments.
    Deployments,
    /// Releases.
    Releases,
    /// Members of an org (drift detection).
    Members,
    /// Team memberships (drift detection).
    Teams,
}

/// A row in `fetch_runs`. **Run log only** — never used as the
/// resume point for the next pull.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchRun {
    /// Internal primary key.
    pub id: Uuid,
    /// Which ingestion path this run belonged to.
    pub kind: FetchRunKind,
    /// Wall-clock start. UTC.
    pub started: DateTime<Utc>,
    /// Wall-clock end, if the run terminated.
    pub finished: Option<DateTime<Utc>>,
    /// Items fetched / processed in this run.
    pub items: i64,
    /// Items that errored.
    pub errors: i64,
    /// `true` if the run finished but some items failed (not a hard
    /// failure — the next tick retries).
    pub partial: bool,
}

/// A per-`(org_id, repo_id, resource_kind)` resume point.
///
/// `repo_id = None` is meaningful for org-scoped resources (members,
/// teams) where there is no repo to pin to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchCursor {
    /// Org scope.
    pub org_id: Uuid,
    /// Repo scope. `None` for org-level resources (members, teams).
    pub repo_id: Option<Uuid>,
    /// What this cursor advances through.
    pub resource_kind: ResourceKind,
    /// "Fetched everything before this point in source-time" — the
    /// `since=` GitHub parameter.
    pub since: Option<DateTime<Utc>>,
    /// ETag from the last conditional GET. Lets the reconciler skip
    /// no-change polls.
    pub etag: Option<String>,
    /// Last event id seen. Used for tie-breaking when timestamps
    /// collide.
    pub last_event_id: Option<String>,
    /// Local wall clock for the last successful advance.
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_run_round_trips() {
        let r = FetchRun {
            id: Uuid::nil(),
            kind: FetchRunKind::Reconciler,
            started: Utc::now(),
            finished: None,
            items: 0,
            errors: 0,
            partial: false,
        };
        let back: FetchRun = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn fetch_cursor_round_trips_with_and_without_repo() {
        let with_repo = FetchCursor {
            org_id: Uuid::nil(),
            repo_id: Some(Uuid::nil()),
            resource_kind: ResourceKind::PullRequests,
            since: Some(Utc::now()),
            etag: Some("W/\"abc\"".into()),
            last_event_id: Some("evt_42".into()),
            updated_at: Utc::now(),
        };
        let back: FetchCursor =
            serde_json::from_str(&serde_json::to_string(&with_repo).unwrap()).unwrap();
        assert_eq!(with_repo, back);

        let org_level = FetchCursor {
            org_id: Uuid::nil(),
            repo_id: None,
            resource_kind: ResourceKind::Members,
            since: None,
            etag: None,
            last_event_id: None,
            updated_at: Utc::now(),
        };
        let back: FetchCursor =
            serde_json::from_str(&serde_json::to_string(&org_level).unwrap()).unwrap();
        assert_eq!(org_level, back);
    }
}
