//! Issue start / due dates (`linear-projects-idea.md` §3.10).
//!
//! Dates are local-first: `PATCH /issues/{id}/dates` does a
//! synchronous upsert into [`IssueDates`]. If the parent repo has a
//! [`RepoProjectLink`] row, the handler additionally enqueues a
//! best-effort GraphQL mirror task (a [`ProjectV2MirrorTask`] of
//! kind [`ProjectV2MirrorTaskKind::MirrorDates`]) that the worker
//! drains; failures are written back to [`IssueDates::mirror_error`]
//! and never block the local save.

use chrono::{DateTime, Utc};
use serde_json::Value as JsonValue;
use uuid::Uuid;

/// Per-issue start / due window. Both bounds are optional and the
/// schema CHECK guards `start_at <= due_at` whenever both are
/// present. The three `mirror_*` fields carry the most recent
/// Projects v2 mirror outcome — advisory only; the local row is
/// authoritative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueDates {
    /// `dp_issues.id` this row belongs to.
    pub issue_id: Uuid,
    /// Start of the window (inclusive), UTC. `None` when unset.
    pub start_at: Option<DateTime<Utc>>,
    /// Due instant (inclusive), UTC. `None` when unset.
    pub due_at: Option<DateTime<Utc>>,
    /// The Projects v2 *item* node id GitHub returned from
    /// `addProjectV2ItemById` the first time we mirrored this row.
    /// Reused on subsequent edits to keep the mirror a single
    /// card instead of accreting duplicates.
    pub mirror_node_id: Option<String>,
    /// Wall-clock the mirror worker last succeeded against
    /// GitHub. `None` until the first successful mirror, or after
    /// the row has only ever failed.
    pub mirror_synced_at: Option<DateTime<Utc>>,
    /// Verbatim GraphQL error from the most recent *failed*
    /// mirror attempt; cleared on success. `None` when the latest
    /// attempt succeeded or no attempt has run yet.
    pub mirror_error: Option<String>,
    /// `updated_at` on the local row — bumped by every upsert.
    pub updated_at: DateTime<Utc>,
}

/// Optional 1:1 mapping from a repo to the Projects v2 project the
/// mirror task targets. Absence means "no mirroring; the local
/// upsert is the entire story for this repo".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoProjectLink {
    /// `dp_repos.id` this link belongs to.
    pub repo_id: Uuid,
    /// Projects v2 project node id (the `projectId` arg to
    /// `addProjectV2ItemById`).
    pub project_node_id: String,
    /// Field node id for the Projects v2 start-date field, or
    /// `None` when the project does not define one. The mirror
    /// task skips the start lane when this is unset.
    pub start_field_node_id: Option<String>,
    /// Field node id for the Projects v2 due-date field, or
    /// `None` when the project does not define one. The mirror
    /// task skips the due lane when this is unset.
    pub due_field_node_id: Option<String>,
}

/// Closed enum guarding the `dp_projectv2_mirror_tasks.kind`
/// column. Adding a variant is a code + migration change in
/// lockstep — never a config typo away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectV2MirrorTaskKind {
    /// `addProjectV2ItemById` then `updateProjectV2ItemFieldValue`
    /// for both date fields. The §3.10 best-effort outbox.
    MirrorDates,
    /// Reserved — Projects v2 pull-back (read GitHub Projects
    /// state into dev-pulse). Stub only this slice; the slice-3
    /// worker fills in the producer side.
    PullBack,
}

impl ProjectV2MirrorTaskKind {
    /// Wire string used in the `kind` column and on the
    /// `enqueue_projectv2_mirror_task` API.
    pub const fn as_str(self) -> &'static str {
        match self {
            ProjectV2MirrorTaskKind::MirrorDates => "mirror_dates",
            ProjectV2MirrorTaskKind::PullBack => "pull_back",
        }
    }
}

/// One row in `dp_projectv2_mirror_tasks`. Returned by the worker
/// drain; the handler only ever *constructs* one to enqueue.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectV2MirrorTask {
    /// `dp_projectv2_mirror_tasks.id`.
    pub id: Uuid,
    /// Target issue.
    pub issue_id: Uuid,
    /// Target repo (denormalised so the worker can resolve the
    /// board link without joining back through `dp_issues`).
    pub repo_id: Uuid,
    /// Closed-vocabulary task kind.
    pub kind: ProjectV2MirrorTaskKind,
    /// Free-form JSON payload — the worker reads what it needs
    /// per `kind`. For `MirrorDates` this carries the new
    /// `{ start_at, due_at }` pair.
    pub payload: JsonValue,
    /// Retry counter.
    pub attempts: i32,
    /// Verbatim error from the most recent failed drain attempt;
    /// cleared on success.
    pub last_error: Option<String>,
    /// When the handler enqueued the row.
    pub enqueued_at: DateTime<Utc>,
    /// When the worker last drained the row (success or terminal
    /// failure). `None` while still pending.
    pub processed_at: Option<DateTime<Utc>>,
}
