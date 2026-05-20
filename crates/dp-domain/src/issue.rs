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
