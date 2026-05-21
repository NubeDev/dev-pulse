//! [`Milestone`] — GitHub repo milestone, mirrored into dev-pulse.
//!
//! See `tagging.md` §9.3 for the design rationale. A milestone is a
//! GitHub primitive (repo-scoped, due-date-bearing, 0..1 per issue)
//! that overlaps with — but is **not** the same as — a dev-pulse
//! tag. They get their own table, their own grammar, and their own
//! sync path.
//!
//! ## What this struct mirrors
//!
//! Exactly the fields the fetcher upserts from
//! `GET /repos/{owner}/{repo}/milestones`. The dev-pulse-side
//! `remote_missing_streak` is included because it's part of the
//! row identity from the fetcher worker's perspective — it's the
//! N=3 quarantine counter that turns a single missing-from-GitHub
//! observation into an eventual delete.
//!
//! ## What this struct does NOT carry
//!
//! * No per-issue list. The relationship is on the **issue** side
//!   (`dp_issues.milestone_id` once it ships; for now the existing
//!   `dp_issues.milestone` TEXT column is the wire-side projection).
//! * No project-adoption pointer. That's `dp_projects`'s problem
//!   (§9.5), not the milestone's.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Milestone state — mirrors GitHub's `open` / `closed` enum.
///
/// We deliberately do **not** mirror GitHub's `all` query-string
/// value as a variant: `all` is a filter input, not a column value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MilestoneState {
    /// Milestone is open and tracking work.
    Open,
    /// Milestone is closed. Issues that were on it keep their
    /// pointer; the milestone itself just stops accruing work.
    Closed,
}

impl MilestoneState {
    /// Lower-case wire form, matching the `dp_milestones.state`
    /// CHECK constraint (`'open' | 'closed'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }

    /// Parse the wire form. Returns `None` for anything outside
    /// the CHECK constraint so callers fail fast on a bad row
    /// rather than silently defaulting.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "open" => Some(Self::Open),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }
}

/// One mirrored milestone row, as returned by
/// [`crate::store::Store::list_milestones_for_repo`] and written by
/// [`crate::store::Store::upsert_milestone`].
///
/// Row identity is `(repo_id, github_number)` — that's the natural
/// key the fetcher upsert path conflicts on. The surrogate `id`
/// stays stable across re-fetches so any future FK from
/// `dp_issues.milestone_id` or `dp_projects.primary_milestone_id`
/// doesn't churn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Milestone {
    /// Internal id. Stable across re-fetches.
    pub id: Uuid,
    /// Parent repo.
    pub repo_id: Uuid,
    /// Repo-scoped milestone number (the integer GitHub shows in
    /// the URL — `https://github.com/{owner}/{repo}/milestone/3`).
    pub github_number: i32,
    /// Opaque GraphQL node id (`MI_kwDOABCD...`). Required for
    /// joining with Projects v2 surfaces; the fetcher only writes
    /// rows that carry one.
    pub github_node_id: String,
    /// Milestone title — what users actually see on chips and in
    /// the rail. Not guaranteed to be unique within a repo
    /// (GitHub allows duplicate titles after rename / re-create).
    pub title: String,
    /// Long-form description. May contain markdown.
    pub description: Option<String>,
    /// Open or closed.
    pub state: MilestoneState,
    /// Due date, stored as a calendar date (no timezone). `None`
    /// when the milestone has no due date set.
    ///
    /// See the migration comment in `0030_milestones.sql` for why
    /// this is `DATE` and not `TIMESTAMPTZ`.
    pub due_on: Option<NaiveDate>,
    /// Open-issue count — denormalised cache GitHub maintains.
    /// Lets the triage rail render `closed/total` progress without
    /// a per-row join. Authoritative source is GitHub; we never
    /// recompute locally.
    pub open_issues: i32,
    /// Closed-issue count — see [`Milestone::open_issues`].
    pub closed_issues: i32,
    /// GitHub-side creation timestamp.
    pub created_at: DateTime<Utc>,
    /// GitHub-side last-update timestamp (any field).
    pub updated_at: DateTime<Utc>,
    /// GitHub-side close timestamp. `None` when `state = Open`.
    pub closed_at: Option<DateTime<Utc>>,
    /// Local fetch timestamp — when the fetcher last refreshed this
    /// row from GitHub. Used by the data-as-of surface (§14.3) to
    /// answer "how stale is this milestone?".
    pub fetched_at: DateTime<Utc>,
    /// N=3 quarantine counter (`tagging.md` §5.1 / §9.4). Bumped
    /// by the fetcher when `list_milestones` confirms this
    /// milestone is absent on a complete page set; reset to 0 on
    /// any pull that re-observes it. A delete happens at streak
    /// >= 3. This slice ships the column; the streak-management
    /// helpers arrive with the fetcher integration slice.
    pub remote_missing_streak: i32,
}

/// Input shape for [`crate::store::Store::upsert_milestone`].
///
/// Mirrors [`Milestone`] minus the surrogate `id` (which the store
/// assigns on insert and preserves on conflict) and the
/// `remote_missing_streak` (which only the streak-management
/// helpers touch — an upsert always resets it to 0 because we just
/// observed the row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MilestoneUpsert {
    /// Parent repo.
    pub repo_id: Uuid,
    /// Repo-scoped milestone number.
    pub github_number: i32,
    /// GraphQL node id.
    pub github_node_id: String,
    /// Title.
    pub title: String,
    /// Description.
    pub description: Option<String>,
    /// State.
    pub state: MilestoneState,
    /// Due date.
    pub due_on: Option<NaiveDate>,
    /// Open-issue count.
    pub open_issues: i32,
    /// Closed-issue count.
    pub closed_issues: i32,
    /// Created-on-GitHub timestamp.
    pub created_at: DateTime<Utc>,
    /// Updated-on-GitHub timestamp.
    pub updated_at: DateTime<Utc>,
    /// Closed-on-GitHub timestamp (when `state = Closed`).
    pub closed_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trips() {
        for s in [MilestoneState::Open, MilestoneState::Closed] {
            assert_eq!(MilestoneState::from_str(s.as_str()), Some(s));
        }
        assert!(MilestoneState::from_str("all").is_none());
        assert!(MilestoneState::from_str("OPEN").is_none()); // case-sensitive
    }
}
