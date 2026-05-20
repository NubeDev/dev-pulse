//! [`IssueMutation`] — audit record for a user-initiated GitHub
//! Issues write (SCOPE-PROJECTS.md §5 + §8.5).
//!
//! Every row in `dp_issue_mutations` (landed in
//! `0007_issues_optimistic_cas.sql`, a later stage of this job) is
//! one of the §8.5 verbs (`issue.create`, `issue.update`,
//! `issue.close`, `issue.reopen`, `issue.comment`). The audit row
//! covers the full lifecycle:
//!
//! * **`Pending`** — between §8.2 step 5 (local CAS applied) and
//!   step 7 (GitHub call returned).
//! * **`Committed`** — GitHub round-trip succeeded.
//! * **`Failed`** — GitHub returned an error (`error` populated).
//! * **`PendingRemoteTimeout`** — the sweeper rolled the row back
//!   because the synchronous handler crashed or its request was
//!   killed between steps 5 and 7 (§8.5 `pending_remote_timeout`).
//!
//! `version_before` / `version_after` capture the optimistic-CAS
//! token transition on `dp_issues.version` — the §13.4 + §13.7
//! decision. Together with `diff` they answer the §11 success
//! criterion: "who closed issue #1234, when, and what did they
//! change?" with one query.
//!
//! **No table for this entity ships in stage 3.** The migration
//! (`0007_issues_optimistic_cas.sql`, reserved in
//! `STAGE-1-COORDINATION.md`) lands in a later stage of the same
//! job, together with the `version` / `pending_remote*` columns on
//! `dp_issues`. The domain type lives here now so the store-trait
//! method signatures stage 3 introduces (`record_issue_mutation`,
//! `update_issue_mutation_result`) can compile against a stable
//! shape.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

/// The §8 user-initiated operation that produced this audit row.
///
/// Locked vocabulary per §8.5 — adding a verb is a code change. Bulk
/// mutations and PR / discussion / reaction verbs are non-goals
/// (§4) and are deliberately *not* representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueMutationOp {
    /// `POST /repos/{owner}/{repo}/issues` — new issue.
    Create,
    /// `PATCH /repos/{owner}/{repo}/issues/{n}` — partial update
    /// (title / body / labels / assignees / milestone).
    Update,
    /// `PATCH … {"state":"closed"}` — explicit close transition.
    Close,
    /// `PATCH … {"state":"open"}` — explicit reopen transition.
    Reopen,
    /// `POST /repos/{owner}/{repo}/issues/{n}/comments` — new
    /// comment on an existing issue.
    Comment,
}

impl IssueMutationOp {
    /// SCOPE.md §15.13 audit verb (dotted form: `"issue.create"`,
    /// `"issue.update"`, …). Used as the `dp_audit_log.action`
    /// value when this mutation is mirrored into the §15.13 log.
    pub fn audit_verb(self) -> &'static str {
        match self {
            IssueMutationOp::Create => "issue.create",
            IssueMutationOp::Update => "issue.update",
            IssueMutationOp::Close => "issue.close",
            IssueMutationOp::Reopen => "issue.reopen",
            IssueMutationOp::Comment => "issue.comment",
        }
    }
}

/// Lifecycle of an [`IssueMutation`] row. Maps directly to the §8.2
/// write path and the §8.5 audit `result` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueMutationResult {
    /// Local CAS applied (§8.2 step 5), GitHub call in flight.
    /// Cleared by the next state transition.
    Pending,
    /// GitHub returned success (§8.2 step 7).
    Committed,
    /// GitHub returned an error (§8.2 step 8). `error` populated.
    Failed,
    /// The sweeper rolled the row back after the
    /// `issues.pending_remote_timeout_secs` window expired without
    /// the synchronous handler completing (§8.5
    /// `pending_remote_timeout`).
    PendingRemoteTimeout,
}

/// One row in `dp_issue_mutations` (landing in migration 0007).
///
/// `diff` is JSON of mutated fields shaped as `{"before": …,
/// "after": …}`. For [`IssueMutationOp::Create`] the `before` side
/// is omitted (§8.5). For [`IssueMutationOp::Comment`] the diff
/// carries the new comment body, no `before`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueMutation {
    /// Primary key. Caller-assigned so the dp-rest handler can
    /// correlate the row with the inflight GitHub call without a
    /// round-trip.
    pub id: Uuid,
    /// The dev-pulse user who initiated the write. Never a
    /// fetcher / background-worker principal (§13.3).
    pub actor_user_id: Uuid,
    /// Local issue id (`dp_issues.id`). Stable across GitHub-side
    /// renumbering — there is none, but the FK is to our row id
    /// either way.
    pub issue_id: Uuid,
    /// Repo the issue lives in. Denormalised so the audit log is
    /// answerable when the issue row has been purged.
    pub repo_id: Uuid,
    /// What was done.
    pub op: IssueMutationOp,
    /// `dp_issues.version` value the form was loaded against — the
    /// `expected_version` CAS token (§8.2 step 1, §13.4).
    pub version_before: i64,
    /// `dp_issues.version` value the local row was bumped to after
    /// the CAS in §8.2 step 5. Always `version_before + 1` on the
    /// happy path; the failure path in §8.2 step 8 bumps `version`
    /// again, which is *not* reflected here — this field is the
    /// initial optimistic bump only.
    pub version_after: i64,
    /// `{ "before": ..., "after": ... }` JSON describing the
    /// mutated fields. `before` omitted on
    /// [`IssueMutationOp::Create`].
    pub diff: JsonValue,
    /// Where in the §8.2 path this row sits.
    pub result: IssueMutationResult,
    /// `X-GitHub-Delivery` id, if GitHub returned one. Lets the
    /// reconciler match the webhook for this mutation when it
    /// arrives (§13.7).
    pub github_delivery_id: Option<String>,
    /// Verbatim GitHub error text for
    /// [`IssueMutationResult::Failed`] rows. `None` otherwise.
    pub error: Option<String>,
    /// When the row was created (i.e. when the CAS applied).
    pub created_at: DateTime<Utc>,
    /// When the row transitioned out of `Pending`. `None` while
    /// still in flight.
    pub finished_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn op_audit_verbs_match_scope_section_8_5() {
        assert_eq!(IssueMutationOp::Create.audit_verb(), "issue.create");
        assert_eq!(IssueMutationOp::Update.audit_verb(), "issue.update");
        assert_eq!(IssueMutationOp::Close.audit_verb(), "issue.close");
        assert_eq!(IssueMutationOp::Reopen.audit_verb(), "issue.reopen");
        assert_eq!(IssueMutationOp::Comment.audit_verb(), "issue.comment");
    }

    #[test]
    fn round_trips_through_json() {
        let m = IssueMutation {
            id: Uuid::nil(),
            actor_user_id: Uuid::nil(),
            issue_id: Uuid::nil(),
            repo_id: Uuid::nil(),
            op: IssueMutationOp::Update,
            version_before: 7,
            version_after: 8,
            diff: json!({ "before": { "title": "old" }, "after": { "title": "new" } }),
            result: IssueMutationResult::Pending,
            github_delivery_id: None,
            error: None,
            created_at: Utc::now(),
            finished_at: None,
        };
        let back: IssueMutation =
            serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn result_variants_serialise_snake_case() {
        let j = serde_json::to_string(&IssueMutationResult::PendingRemoteTimeout).unwrap();
        assert_eq!(j, "\"pending_remote_timeout\"");
    }
}
