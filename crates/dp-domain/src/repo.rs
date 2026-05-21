//! [`Repo`] — a GitHub repository, plus [`RepoMetadata`] — the
//! sibling snapshot of mutable GitHub-side fields (stars, forks,
//! primary language, …) used by the repo-activity dashboard.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A GitHub repository, scoped to one [`Org`](crate::Org).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repo {
    /// Internal primary key.
    pub id: Uuid,
    /// Parent org.
    pub org_id: Uuid,
    /// GitHub's numeric repo id.
    pub github_id: i64,
    /// Repo name (no `org/` prefix).
    pub name: String,
}

/// Snapshot of mutable GitHub-side metadata for a repo: counters
/// (stars / forks / watchers), descriptive fields (description /
/// homepage / primary language / default branch), and lifecycle
/// flags (archived / fork / private).
///
/// One row per [`Repo`] in `dp_repo_metadata`, keyed by `repo_id`.
/// Kept separate from [`Repo`] because every webhook delivery
/// rewrites these values; folding them into the `Repo` row would
/// thrash `upsert_repo`'s diff and noise up identity-only callers
/// (issue write path, reconciler scopes).
///
/// Every field is optional / defaulted so a partial payload (a
/// webhook that doesn't repeat the full repo object) can still
/// upsert without clobbering known-good values — the store impl
/// uses `COALESCE(EXCLUDED.x, dp_repo_metadata.x)` for nullable
/// fields and only writes counter fields when the source carried
/// them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoMetadata {
    /// Owning repo.
    pub repo_id: Uuid,
    /// Stargazer count from GitHub.
    pub stars: i64,
    /// Fork count from GitHub.
    pub forks: i64,
    /// Subscriber / watcher count from GitHub.
    pub watchers: i64,
    /// Open-issue count GitHub itself reports (includes PRs in the
    /// REST projection — kept as-is, surfaced labeled as "GitHub's
    /// count" so consumers know it differs from our own
    /// `RepoSummary::open_issue_count`).
    pub open_issues_remote: i64,
    /// Primary language as detected by GitHub.
    pub primary_language: Option<String>,
    /// Default branch name (e.g. `main`, `master`).
    pub default_branch: Option<String>,
    /// Repo description.
    pub description: Option<String>,
    /// Repo homepage URL.
    pub homepage: Option<String>,
    /// GitHub's archived flag.
    pub is_archived: bool,
    /// GitHub's fork flag.
    pub is_fork: bool,
    /// GitHub's private flag.
    pub is_private: bool,
    /// GitHub's `pushed_at` — last push to any branch.
    pub pushed_at: Option<DateTime<Utc>>,
    /// Wall-clock the fetcher last refreshed this row.
    pub metadata_updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let r = Repo {
            id: Uuid::nil(),
            org_id: Uuid::nil(),
            github_id: 100,
            name: "dev-pulse".into(),
        };
        let back: Repo = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(r, back);
    }
}
