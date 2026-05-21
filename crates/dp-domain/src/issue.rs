//! [`Issue`] — a GitHub issue mirrored into `dp_issues`.
//!
//! Read-side projection of the row the §8 write path mutates. The
//! storage layer always carries the full row (title, body, labels,
//! assignees, state, milestone, plus the §8 CAS `version`) — the
//! same shape every surface (REST, MCP, frontend) renders.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Issue state — mirrors the `state` text column on `dp_issues`,
/// constrained to the two values GitHub itself uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueState {
    /// Issue is open.
    Open,
    /// Issue is closed.
    Closed,
}

impl IssueState {
    /// Wire form used by both the SQL column and the JSON envelope.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }

    /// Parse the SQL / JSON form. Unknown values map to `None` so
    /// the caller (typically the store layer) can surface a
    /// `StoreError::Invalid` with the offending value attached.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "open" => Some(Self::Open),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }
}

/// One issue row. Carries everything the read surfaces need; the
/// §8 write path mutates the same shape and bumps [`Issue::version`]
/// on every accepted update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    /// Internal primary key.
    pub id: Uuid,
    /// Parent org id (denormalised on the row to keep org-scope
    /// queries from joining through `dp_repos`).
    pub org_id: Uuid,
    /// Parent repo id.
    pub repo_id: Uuid,
    /// GitHub's numeric issue id (stable across transfers).
    pub github_id: i64,
    /// Repo-relative issue number.
    pub number: i64,
    /// Title.
    pub title: String,
    /// Body (nullable on the GitHub side).
    pub body: Option<String>,
    /// Open / closed.
    pub state: IssueState,
    /// Labels as a JSONB array of strings.
    pub labels: Vec<String>,
    /// Assignee logins as a JSONB array of strings.
    pub assignees: Vec<String>,
    /// Milestone title, if assigned.
    pub milestone: Option<String>,
    /// §8 optimistic-CAS token. Bumped on every accepted update
    /// *and* on every webhook-applied refresh.
    pub version: i64,
    /// GitHub GraphQL node id for this issue (e.g.
    /// `I_kwDOABC...`). Captured from the webhook /
    /// backfill payload via `issue.node_id` and persisted so the
    /// §3.10 Projects v2 mirror can pass it verbatim as the
    /// `addProjectV2ItemById` `contentId` argument without an
    /// extra `repository.issue(number)` round-trip per first
    /// mirror call. `None` only for rows ingested before the
    /// 0021 migration shipped; the mirror adapter resolves the
    /// id lazily in that case and caches it back here.
    pub github_node_id: Option<String>,
    /// Last time the row changed (GitHub `updated_at`, or local
    /// mutation time for optimistic writes).
    pub updated_at: DateTime<Utc>,
}

/// Aggregated summary for the `GET /repos` list pane. Carries the
/// counts the workflow UI needs to drive its "100s of repos"
/// drill-down without forcing the caller to re-issue per-repo
/// queries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSummary {
    /// Internal repo id.
    pub id: Uuid,
    /// Parent org id.
    pub org_id: Uuid,
    /// Parent org login (joined from `dp_orgs`).
    pub org_login: String,
    /// Repo name (no `owner/` prefix).
    pub name: String,
    /// Count of `state = 'open'` issues in this repo.
    pub open_issue_count: i64,
    /// `MAX(updated_at)` across the repo's issues; `None` when
    /// the repo has no issues yet.
    pub last_activity_at: Option<DateTime<Utc>>,
}

/// Ingest-side projection of a GitHub issue payload, ready to be
/// upserted into `dp_issues`. Constructed by the fetcher (webhook
/// handler or REST backfill) and consumed by
/// [`crate::store::Store::upsert_issue_from_github`].
///
/// The shape mirrors the columns the §13.7 reconciler guard +
/// slice-2 read endpoints care about. Fields the fetcher does not
/// authoritatively know (the local `id`, the `version` bump, the
/// `pending_remote_*` triad) stay on the store side; the upsert
/// allocates `id` on first sighting, bumps `version`, and leaves
/// the pending-remote columns alone.
///
/// `org_id` / `repo_id` are resolved *before* this struct is
/// built — typically by the caller's `upsert_repo_from_payload`
/// helper — so the upsert never touches `dp_orgs` / `dp_repos`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueUpsert {
    /// Parent org id (`dp_orgs.id`).
    pub org_id: Uuid,
    /// Parent repo id (`dp_repos.id`).
    pub repo_id: Uuid,
    /// GitHub's numeric issue id (stable across transfers).
    pub github_id: i64,
    /// GitHub GraphQL node id (`issue.node_id` on the payload).
    /// Forwarded by the fetcher so the dp-store-pg upsert can
    /// persist it on `dp_issues.github_node_id` — the §3.10
    /// Projects v2 mirror needs this as the `contentId` argument
    /// to `addProjectV2ItemById`. Optional because some test
    /// fixtures may not supply it; production payloads always
    /// do.
    pub github_node_id: Option<String>,
    /// Repo-relative issue number.
    pub number: i64,
    /// Title.
    pub title: String,
    /// Body (nullable on the GitHub side).
    pub body: Option<String>,
    /// Open / closed.
    pub state: IssueState,
    /// Labels as a vector of label names (GitHub's `label.name`).
    pub labels: Vec<String>,
    /// Assignee logins as a vector of GitHub `user.login` values.
    pub assignees: Vec<String>,
    /// Milestone title, if assigned.
    pub milestone: Option<String>,
    /// Author login (GitHub `user.login`). Stored on
    /// `dp_issues.author` for the per-author filter pill (§5.5).
    pub author: Option<String>,
    /// GitHub's `state_reason` (`completed` / `not_planned` /
    /// `reopened` / NULL). Stored on `dp_issues.state_reason`
    /// for the throughput / lead-time reports (slice 3).
    pub state_reason: Option<String>,
    /// Wall-clock GitHub `created_at`.
    pub created_at: DateTime<Utc>,
    /// Wall-clock GitHub `updated_at`. Bumped by the upsert iff
    /// the value moved forward.
    pub updated_at: DateTime<Utc>,
    /// Wall-clock GitHub `closed_at`; `None` while the issue is
    /// open.
    pub closed_at: Option<DateTime<Utc>>,
}

/// Outcome reported by [`crate::store::Store::upsert_issue_from_github`].
/// Useful to the caller (webhook handler / CLI backfill) for
/// metrics — "how many rows did I actually insert vs update vs
/// skip because §13.7 deferred the write?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueUpsertOutcome {
    /// Row did not exist; one was inserted with `version = 1`.
    Inserted,
    /// Row existed and was updated; `version` bumped by 1 and the
    /// projected columns refreshed.
    Updated,
    /// Row existed but [`updated_at`](IssueUpsert::updated_at) was
    /// not newer than the local copy. No write, no version bump —
    /// the local copy is at least as fresh as the inbound payload.
    /// This is the common case during a re-backfill.
    Skipped,
    /// Row existed and is in `pending_remote = TRUE` state inside
    /// the §13.7 timeout window. The upsert refused to clobber it
    /// so the in-flight optimistic write can land first. The
    /// caller should buffer (the webhook drain loop already does)
    /// or simply skip (the CLI backfill does — the next sweep
    /// will pick the row up after the timeout clears).
    Deferred,
}
