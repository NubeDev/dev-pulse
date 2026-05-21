//! [`Project`] — first-class planning surface
//! (`linear-projects-v2.md` §5).
//!
//! A project is a dev-pulse-owned container of issues across repos
//! in one org, with a goal, optional start / due dates, a lead, and
//! a derived status. Owned by dev-pulse; not derived from anything
//! on GitHub. Optional GitHub Projects v2 board mirroring lands in
//! slice B; this module covers the local-only object (slice A).
//!
//! The CAS contract matches `dp_issues` (TODO §8.2): every PATCH /
//! archive / bulk-add carries an `expected_version`; the SQL clause
//! `WHERE id = ? AND version = ?` emits 0 rows on a stale write.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Project lifecycle state. Mirrors the `dp_projects.status` text
/// column constrained by the migration's CHECK to one of these four
/// values. Adding a variant is a code + migration change in
/// lockstep — never a config typo away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectStatus {
    /// Actively planned against. Default for new projects.
    Active,
    /// On the books but not currently in-flight. Surfaces under
    /// `Projects ▸ Backlog` in the §6.1 sidebar.
    Backlog,
    /// Work is finished. Surfaces under `Projects ▸ Done`.
    Done,
    /// Hidden from default views; one-click expand under
    /// `Projects ▸ Archived`. Excluded from the partial-unique
    /// name index so the name can be reused.
    Archived,
}

impl ProjectStatus {
    /// Wire form used by the SQL column and the JSON envelope.
    pub const fn as_str(self) -> &'static str {
        match self {
            ProjectStatus::Active => "active",
            ProjectStatus::Backlog => "backlog",
            ProjectStatus::Done => "done",
            ProjectStatus::Archived => "archived",
        }
    }

    /// Parse the SQL / JSON form. Unknown values map to `None` so
    /// the caller (typically the store layer) can surface a
    /// `StoreError::Invalid` with the offending value attached.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "active" => Some(Self::Active),
            "backlog" => Some(Self::Backlog),
            "done" => Some(Self::Done),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

/// One row in `dp_projects`. Read shape for the §6.2 list / §6.3
/// detail surfaces. `issue_count` / `closed_issue_count` are the
/// denormalised counters maintained by the §7.2 membership writes
/// and the issue-close webhook — surfaces read them directly off
/// the row instead of joining through `dp_project_issues`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    /// Primary key.
    pub id: Uuid,
    /// Parent org (v1: a project belongs to exactly one org).
    pub org_id: Uuid,
    /// Human-readable name. Case-insensitively unique within
    /// `(org_id, status <> 'archived')`.
    pub name: String,
    /// Optional markdown description shown on the §6.3 detail page.
    pub description: Option<String>,
    /// Lead drives default visibility and is the default `Mentioned`
    /// filter target. Mutating this field is an elevated op (§9.2).
    pub lead_user_id: Option<Uuid>,
    /// Lifecycle state. See [`ProjectStatus`].
    pub status: ProjectStatus,
    /// Planned start instant, UTC. `None` when unset.
    pub start_at: Option<DateTime<Utc>>,
    /// Planned due instant, UTC. `None` when unset.
    pub due_at: Option<DateTime<Utc>>,
    /// Denormalised count of `dp_project_issues` rows for this
    /// project. Maintained by §7.2 add / remove paths.
    pub issue_count: i32,
    /// Denormalised count of `dp_project_issues` rows whose linked
    /// `dp_issues.state = 'closed'`. Maintained by §7.2 plus the
    /// issue-close webhook. `<= issue_count` is a schema CHECK.
    pub closed_issue_count: i32,
    /// User who created the project. Immutable per §9.2. Nullable
    /// only because `ON DELETE SET NULL` keeps history when a user
    /// is pseudonymised.
    pub created_by: Option<Uuid>,
    /// When the row was first written.
    pub created_at: DateTime<Utc>,
    /// When the row last mutated. Bumped by every accepted write.
    pub updated_at: DateTime<Utc>,
    /// §8.2 CAS counter. Bumped by every accepted write; PATCH /
    /// archive callers send `expected_version`.
    pub version: i64,
}

/// Mutable payload for create / update. Carries only the
/// caller-supplied fields; the store fills in `id`, `version`,
/// `created_at`, `updated_at`, and the denormalised counts. The
/// REST layer collapses POST and PATCH onto the same shape; PATCH
/// callers additionally carry `expected_version` outside this
/// struct.
///
/// `status` is mandatory on create (defaults to
/// [`ProjectStatus::Active`] in the handler before this struct is
/// built) so the storage layer never has to guess. Update calls
/// pass the new desired status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectUpsert {
    /// Parent org.
    pub org_id: Uuid,
    /// Project name.
    pub name: String,
    /// Optional description.
    pub description: Option<String>,
    /// Optional lead user.
    pub lead_user_id: Option<Uuid>,
    /// Lifecycle state.
    pub status: ProjectStatus,
    /// Optional planned start.
    pub start_at: Option<DateTime<Utc>>,
    /// Optional planned due.
    pub due_at: Option<DateTime<Utc>>,
    /// Author (the caller). Stored in `created_by` on create;
    /// ignored on update (the column is immutable per §9.2).
    pub created_by: Option<Uuid>,
}

/// Outcome of a single issue add via
/// [`crate::store::Store::add_issues_to_project`]. Mirrors the
/// `BulkAddResult` shape `linear-projects-v2.md` §7.2 wires through
/// the REST layer so the UI can render per-row outcomes from one
/// round-trip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectIssueAddOutcome {
    /// Issue ids that were successfully attached to the project.
    pub added: Vec<Uuid>,
    /// Issue ids that the store refused, with the reason. The v1
    /// `UNIQUE (issue_id)` constraint surfaces collisions here with
    /// `reason = "already_in_project"` and the offending
    /// `existing_project_id` filled in so the UI can render a
    /// one-click `Move here?` affordance.
    pub skipped: Vec<ProjectIssueAddSkip>,
}

/// One skipped row in a bulk add (§7.2). The `reason` is a
/// closed vocabulary so the UI can branch deterministically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectIssueAddSkip {
    /// The issue id that was rejected.
    pub issue_id: Uuid,
    /// Closed-vocabulary reason. v1 values:
    ///
    /// * `"already_in_project"` — issue is already attached to
    ///   another project (or this one); `existing_project_id` is
    ///   set.
    /// * `"unknown_issue"` — the FK to `dp_issues` did not resolve.
    /// * `"cross_org"` — the issue's `org_id` differs from the
    ///   project's `org_id` (v1: a project belongs to exactly one
    ///   org, §4).
    pub reason: String,
    /// Set when `reason == "already_in_project"`. Lets the UI link
    /// directly to the existing project.
    pub existing_project_id: Option<Uuid>,
}

/// Filter for [`crate::store::Store::list_projects`] /
/// [`crate::store::Store::count_projects`].
///
/// All fields are conjunctive. `limit` is capped at
/// [`crate::store::MAX_LIST_LIMIT`] by the dp-rest layer before it
/// reaches the store; the store treats it as a hard upper bound.
#[derive(Debug, Clone, Default)]
pub struct ProjectListFilter {
    /// Restrict to one org. `None` ⇒ every org the §15 access gate
    /// already filtered to.
    pub org_id: Option<Uuid>,
    /// Restrict to one status. `None` ⇒ every status (the sidebar
    /// queries one status at a time per §6.1).
    pub status: Option<ProjectStatus>,
    /// Case-insensitive substring search on `dp_projects.name`.
    /// `None` or empty ⇒ no search.
    pub q: Option<String>,
    /// Page size. 1..=[`crate::store::MAX_LIST_LIMIT`].
    pub limit: i64,
    /// Page offset.
    pub offset: i64,
}

/// One row from `dp_project_repos` — a soft association between a
/// project and a repo. Used by the §6.3 "Add issues" dialog to
/// narrow the issue picker to repos the operator has explicitly
/// associated with the project. Does **not** gate membership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRepo {
    /// The project this row associates the repo with.
    pub project_id: Uuid,
    /// The repo associated with the project.
    pub repo_id: Uuid,
    /// User who created the association, if known.
    pub added_by: Option<Uuid>,
    /// When the association was created.
    pub added_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_status_wire_round_trip() {
        for s in [
            ProjectStatus::Active,
            ProjectStatus::Backlog,
            ProjectStatus::Done,
            ProjectStatus::Archived,
        ] {
            assert_eq!(ProjectStatus::from_str(s.as_str()), Some(s));
        }
        assert_eq!(ProjectStatus::from_str("nope"), None);
    }

    #[test]
    fn project_status_serde_lowercase() {
        // JSON envelope matches the SQL CHECK vocabulary.
        let j = serde_json::to_string(&ProjectStatus::Backlog).unwrap();
        assert_eq!(j, "\"backlog\"");
        let back: ProjectStatus = serde_json::from_str("\"archived\"").unwrap();
        assert_eq!(back, ProjectStatus::Archived);
    }
}
