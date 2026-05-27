//! Audit-log writer + pinned action vocabulary (Phase 4 D4.4).
//!
//! The v1 vocabulary lives here as `pub const` strings; every
//! protected handler routes through [`record`] so the schema cannot
//! drift per-handler. Adding a new verb is a code change (a new
//! `pub const` here) — never a config knob, per SCOPE D4.4 "new
//! verbs ship as code, not config".
//!
//! ## Action vocabulary
//!
//! | const                | route / event                                       |
//! |----------------------|-----------------------------------------------------|
//! | [`REPORT_READ`]      | any `/reports/*` handler invocation                 |
//! | [`HOME_ORG_SET`]     | `POST /home-org`                                    |
//! | [`ADMIN_REFRESH`]    | `POST /admin/refresh`                               |
//! | [`USER_ANONYMISE`]   | `POST /admin/users/:id/anonymise`                   |
//! | [`USER_EXPORT`]      | `GET /admin/users/:id/export`                       |
//! | [`RUNS_LIST`]        | `GET /admin/runs`                                   |
//! | [`AUTH_SIGNED_IN`]   | successful OAuth callback (session minted)          |
//! | [`AUTH_DENIED_ORG`]  | authz denial for an out-of-org GitHub user          |
//! | [`PIN_ADD`]          | `POST /me/pins` (SCOPE-PROJECTS §6.5)               |
//! | [`PIN_REMOVE`]       | `DELETE /me/pins/{kind}/{id}` (SCOPE-PROJECTS §6.5) |
//! | [`PIN_REORDER`]      | `PUT /me/pins/order` (SCOPE-PROJECTS §6.5)          |
//! | [`TAG_CREATE`]       | `POST /tags` (SCOPE-PROJECTS §7.6)                  |
//! | [`TAG_UPDATE`]       | `PATCH /tags/{id}` rename / recolour (§7.6)         |
//! | [`TAG_ARCHIVE`]      | `PATCH /tags/{id}` setting `archived_at` (§7.6)     |
//! | [`TAG_LINK`]         | `POST /tags/{id}/links` — one row per link (§7.6)   |
//! | [`TAG_UNLINK`]       | `DELETE /tags/{id}/links` — one row per link (§7.6) |
//!
//! Stage 4 wires `HOME_ORG_SET`; the others land with their owning
//! handlers in stages 5 / 9. The three `pin.*` verbs ship with the
//! workflow-surface stage in SCOPE-PROJECTS §6; the five `tag.*`
//! verbs ship with the tags-surface stage in SCOPE-PROJECTS §7.6.

use chrono::Utc;
use uuid::Uuid;

use dp_domain::audit::AuditEntry;
use dp_domain::store::{Store, StoreError};

// ---- pinned vocabulary ---------------------------------------------------

/// `report.read` — any `/reports/*` handler invocation.
pub const REPORT_READ: &str = "report.read";
/// `home_org.set` — `POST /home-org`.
pub const HOME_ORG_SET: &str = "home_org.set";
/// `admin.refresh` — `POST /admin/refresh`.
pub const ADMIN_REFRESH: &str = "admin.refresh";
/// `user.anonymise` — `POST /admin/users/:id/anonymise`.
pub const USER_ANONYMISE: &str = "user.anonymise";
/// `user.export` — `GET /admin/users/:id/export`.
pub const USER_EXPORT: &str = "user.export";
/// `user.role_set` — `PUT /admin/users/:id/role`. Target carries
/// `user:<id>;from:<role>;to:<role>` so a single query can answer
/// "every role change this user has gone through".
/// (DOCS/SCOPE-AUTHZ-USERS.md §3.2).
pub const USER_ROLE_SET: &str = "user.role_set";
/// `user.identities_read` — `GET /admin/users/:id/identities`.
/// Read auditing is deliberate here: the operator inspecting a
/// user's identity set is a signal worth keeping in the trail
/// before any destructive action.
pub const USER_IDENTITIES_READ: &str = "user.identities_read";
/// `runs.list` — `GET /admin/runs`.
pub const RUNS_LIST: &str = "runs.list";
/// `auth.signed_in` — successful OAuth callback.
pub const AUTH_SIGNED_IN: &str = "auth.signed_in";
/// `auth.denied_org` — authz denial for an out-of-org GitHub user.
pub const AUTH_DENIED_ORG: &str = "auth.denied_org";
/// `pin.add` — `POST /me/pins` (SCOPE-PROJECTS §6.5).
pub const PIN_ADD: &str = "pin.add";
/// `pin.remove` — `DELETE /me/pins/{kind}/{id}` (SCOPE-PROJECTS §6.5).
pub const PIN_REMOVE: &str = "pin.remove";
/// `pin.reorder` — `PUT /me/pins/order` (SCOPE-PROJECTS §6.5).
pub const PIN_REORDER: &str = "pin.reorder";
/// `tag.create` — `POST /tags` (SCOPE-PROJECTS §7.6).
pub const TAG_CREATE: &str = "tag.create";
/// `tag.update` — `PATCH /tags/{id}` (rename / recolour /
/// description). Archive is its own verb so the audit log can
/// answer "when was this tag retired?" with one query.
pub const TAG_UPDATE: &str = "tag.update";
/// `tag.archive` — `PATCH /tags/{id}` with `archived_at` set.
pub const TAG_ARCHIVE: &str = "tag.archive";
/// `tag.link` — one row **per linked target** in `POST /tags/{id}/links`.
/// Target string carries the full `(tag_id, kind, target_id)` tuple
/// per §7.6 "Each `tag.link` / `tag.unlink` records the
/// `(tag_id, kind, target_id)` tuple."
pub const TAG_LINK: &str = "tag.link";
/// `tag.unlink` — one row per detached link in
/// `DELETE /tags/{id}/links`. Same tuple format as [`TAG_LINK`].
pub const TAG_UNLINK: &str = "tag.unlink";
/// `setting.set` — `PUT /me/settings/{key}`. Target is the key
/// name; the value is intentionally never recorded.
pub const SETTING_SET: &str = "setting.set";
/// `setting.delete` — `DELETE /me/settings/{key}`. Target is
/// the key name.
pub const SETTING_DELETE: &str = "setting.delete";
/// `issue.create` — `POST /repos/{owner}/{repo}/issues` mirrored
/// to GitHub via the §8.2 write path (SCOPE-PROJECTS §8.5).
pub const ISSUE_CREATE: &str = "issue.create";
/// `issue.update` — `PATCH /repos/{owner}/{repo}/issues/{n}`
/// partial update (title / body / labels / assignees / milestone).
pub const ISSUE_UPDATE: &str = "issue.update";
/// `issue.close` — explicit close transition via §8.2 write path.
pub const ISSUE_CLOSE: &str = "issue.close";
/// `issue.reopen` — explicit reopen transition via §8.2 write path.
pub const ISSUE_REOPEN: &str = "issue.reopen";
/// `issue.comment` — `POST /repos/{owner}/{repo}/issues/{n}/comments`.
pub const ISSUE_COMMENT: &str = "issue.comment";
/// `issue.dates_update` — `PATCH /issues/{id}/dates` (§3.10).
/// Local-first; the optional GraphQL mirror is best-effort and
/// emits no separate audit verb.
pub const ISSUE_DATES_UPDATE: &str = "issue.dates_update";
/// `identity.add` — operator linked an additional GitHub identity
/// (or other external login) to a `dp_users` row (slice 2 identity
/// reconciliation).
pub const IDENTITY_ADD: &str = "identity.add";
/// `identity.remove` — operator unlinked a secondary identity from
/// a `dp_users` row.
pub const IDENTITY_REMOVE: &str = "identity.remove";
/// `identity.verify` — caller verified ownership of a pending
/// identity claim (e.g. via OAuth round-trip).
pub const IDENTITY_VERIFY: &str = "identity.verify";
/// `identity.merge` — two `dp_users` rows reconciled into one
/// canonical identity record.
pub const IDENTITY_MERGE: &str = "identity.merge";
/// `date.set` — slice-2 alias-style verb for the dates surface
/// (`PATCH /issues/{id}/dates`). The original handler records
/// [`ISSUE_DATES_UPDATE`]; [`DATE_SET`] is the vocabulary entry the
/// audit-vocabulary table in the workbench docs refers to and is
/// reserved for future bulk / Projects-v2 pull-back paths that do
/// not match the per-issue verb shape.
pub const DATE_SET: &str = "date.set";
/// `repo.sync_requested` — operator-triggered per-repo reconciler
/// tick (`POST /repos/{id}/sync`).
pub const REPO_SYNC_REQUESTED: &str = "repo.sync_requested";
/// `admin.repo_import` — operator-triggered repo registration via
/// `POST /admin/repos`. Target carries `repo:<owner>/<name>` so the
/// audit log can answer "when was this repo onboarded?" with one
/// query, even before any reconciler tick fires.
pub const ADMIN_REPO_IMPORT: &str = "admin.repo_import";
/// `inbox.bulk_seen` — bulk variant of [`POST /me/inbox/seen`]
/// invoked via the slice-2 `POST /me/inbox/bulk` endpoint with
/// `op = mark_all_seen`. Audit target carries the result count.
pub const BULK_INBOX_SEEN: &str = "inbox.bulk_seen";
/// `inbox.bulk_snooze` — bulk snooze (`POST /me/inbox/bulk` with
/// `op = snooze_all`). Audit target carries `(count, until)`.
pub const BULK_INBOX_SNOOZE: &str = "inbox.bulk_snooze";
/// `inbox.bulk_done` — bulk dismiss (`POST /me/inbox/bulk` with
/// `op = done_all`).
pub const BULK_INBOX_DONE: &str = "inbox.bulk_done";
/// `inbox.bulk_inbox` — bulk restore-to-inbox (`POST /me/inbox/bulk`
/// with `op = inbox_all`). Clears any snooze deadline.
pub const BULK_INBOX_INBOX: &str = "inbox.bulk_inbox";

/// `project.create` — `POST /projects` (linear-projects-v2.md §9.3).
pub const PROJECT_CREATE: &str = "project.create";
/// `project.update` — `PATCH /projects/{id}` (§9.3). Covers name /
/// description / lead / status / start / due edits in one verb;
/// the diff is recoverable from the row's `version` plus the
/// pre-write snapshot stored elsewhere.
pub const PROJECT_UPDATE: &str = "project.update";
/// `project.archive` — `POST /projects/{id}/archive` (§9.3).
/// Distinct from `project.update` so the audit log can answer
/// "when was this project archived?" with one query.
pub const PROJECT_ARCHIVE: &str = "project.archive";
/// `project.issue.add` — one row per issue attached via
/// `POST /projects/{id}/issues` (§7.2). Reserved for the
/// stage-4 membership handler; pinned now so the vocabulary is
/// closed before that handler lands.
pub const PROJECT_ISSUE_ADD: &str = "project.issue.add";
/// `project.issue.remove` — `DELETE /projects/{id}/issues/{issue_id}`.
pub const PROJECT_ISSUE_REMOVE: &str = "project.issue.remove";
/// `project.board.link` — `POST /projects/{id}/board-links` (slice B).
pub const PROJECT_BOARD_LINK: &str = "project.board.link";
/// `project.board.unlink` — `DELETE /projects/{id}/board-links/{link_id}`.
pub const PROJECT_BOARD_UNLINK: &str = "project.board.unlink";
/// `project.repo.add` — `PUT /projects/{id}/repos/{repo_id}`.
pub const PROJECT_REPO_ADD: &str = "project.repo.add";
/// `project.repo.remove` — `DELETE /projects/{id}/repos/{repo_id}`.
pub const PROJECT_REPO_REMOVE: &str = "project.repo.remove";

/// `project.view.create` — `POST /projects/{id}/views`
/// (PROJECT-VIEW.md §7.1, Slice 4).
pub const PROJECT_VIEW_CREATE: &str = "project.view.create";
/// `project.view.update` — `PATCH /projects/{id}/views/{view_id}`.
pub const PROJECT_VIEW_UPDATE: &str = "project.view.update";
/// `project.view.delete` — `DELETE /projects/{id}/views/{view_id}`.
pub const PROJECT_VIEW_DELETE: &str = "project.view.delete";
/// `project.view.reorder` — `POST /projects/{id}/views/reorder`.
pub const PROJECT_VIEW_REORDER: &str = "project.view.reorder";

/// `project.milestone.adopt` — `POST /projects/{id}/adopt-milestone`
/// (PROJECT-VIEW.md §5.5 / §9.5, Slice 5). Target carries
/// `<project_id>:<milestone_id>` or `<project_id>:` on a clear.
pub const PROJECT_MILESTONE_ADOPT: &str = "project.milestone.adopt";

/// `project.milestone.create` — `POST /projects/{id}/milestones`.
/// Two-way sync: dev-pulse calls GitHub `POST /repos/{o}/{r}/
/// milestones`, then upserts the returned row into `dp_milestones`.
/// Target carries `<project_id>:<repo_id>#<github_number>` so the
/// audit row points at the GitHub-side row even before the local
/// id is dereferenceable.
pub const PROJECT_MILESTONE_CREATE: &str = "project.milestone.create";

/// `project.milestone.update` — `PATCH /projects/{id}/milestones/{ms_id}`.
/// Two-way sync. Target carries `<project_id>:<milestone_id>`.
pub const PROJECT_MILESTONE_UPDATE: &str = "project.milestone.update";

/// `project.milestone.close` — `PATCH …` with `state="closed"`.
/// Same target shape as [`PROJECT_MILESTONE_UPDATE`].
pub const PROJECT_MILESTONE_CLOSE: &str = "project.milestone.close";

/// `project.milestone.reopen` — `PATCH …` with `state="open"`.
/// Same target shape as [`PROJECT_MILESTONE_UPDATE`].
pub const PROJECT_MILESTONE_REOPEN: &str = "project.milestone.reopen";

/// `project.milestone.delete` — `DELETE /projects/{id}/milestones/{ms_id}`.
/// Target carries `<project_id>:<milestone_id>`.
pub const PROJECT_MILESTONE_DELETE: &str = "project.milestone.delete";

/// `project.exec_summary.patch` — `PATCH /projects/{id}/exec-summary`.
/// Target carries `<project_id>`.
pub const PROJECT_EXEC_SUMMARY_PATCH: &str = "project.exec_summary.patch";

/// `project.exec_summary.submit` — `POST /projects/{id}/exec-summary/submit`.
/// Target carries `<project_id>`.
pub const PROJECT_EXEC_SUMMARY_SUBMIT: &str = "project.exec_summary.submit";

/// `project.exec_summary.approve` — `POST /projects/{id}/exec-summary/approve`.
/// Target carries `<project_id>`.
pub const PROJECT_EXEC_SUMMARY_APPROVE: &str = "project.exec_summary.approve";

/// `project.exec_summary.revert` — `POST /projects/{id}/exec-summary/revert`.
/// Target carries `<project_id>`.
pub const PROJECT_EXEC_SUMMARY_REVERT: &str = "project.exec_summary.revert";

/// `project.exec_summary.image_add` — recorded by the upload-confirm
/// handler (lands when the starter-blob wiring is in place). Target
/// carries `<project_id>:<image_id>`.
pub const PROJECT_EXEC_SUMMARY_IMAGE_ADD: &str = "project.exec_summary.image_add";

/// `project.exec_summary.image_remove` — `DELETE /projects/{id}/exec-summary/images/{image_id}`.
/// Target carries `<project_id>:<image_id>`.
pub const PROJECT_EXEC_SUMMARY_IMAGE_REMOVE: &str = "project.exec_summary.image_remove";

/// `project.exec_summary.document_add` — recorded by the upload-confirm
/// handler. Target carries `<project_id>:<document_id>`.
pub const PROJECT_EXEC_SUMMARY_DOCUMENT_ADD: &str = "project.exec_summary.document_add";

/// `project.exec_summary.document_remove` — `DELETE /projects/{id}/exec-summary/documents/{doc_id}`.
/// Target carries `<project_id>:<document_id>`.
pub const PROJECT_EXEC_SUMMARY_DOCUMENT_REMOVE: &str = "project.exec_summary.document_remove";

/// `project.exec_summary.changelog_add` — `POST /projects/{id}/exec-summary/changelog`.
/// Target carries `<project_id>:<entry_id>`.
pub const PROJECT_EXEC_SUMMARY_CHANGELOG_ADD: &str = "project.exec_summary.changelog_add";

/// `project.exec_summary.changelog_remove` — `DELETE /projects/{id}/exec-summary/changelog/{entry_id}`.
/// Target carries `<project_id>:<entry_id>`.
pub const PROJECT_EXEC_SUMMARY_CHANGELOG_REMOVE: &str = "project.exec_summary.changelog_remove";

/// `issue.pending_remote_timeout` — emitted by the §8.5 sweeper
/// when a `dp_issues.pending_remote` flag has lingered past
/// `issues.pending_remote_timeout_secs`. The audit target carries
/// the issue id; the corresponding `dp_issue_mutations.result`
/// row (if one was recorded) is updated to
/// `pending_remote_timeout` in the same tick.
pub const ISSUE_PENDING_REMOTE_TIMEOUT: &str = "issue.pending_remote_timeout";

/// Pick the §8.5 audit verb for a given
/// [`IssueMutationOp`][dp_domain::issue_mutation::IssueMutationOp].
/// Single source of truth so handlers do not open-code the match.
pub fn issue_audit_verb(op: dp_domain::issue_mutation::IssueMutationOp) -> &'static str {
    use dp_domain::issue_mutation::IssueMutationOp as Op;
    match op {
        Op::Create => ISSUE_CREATE,
        Op::Update => ISSUE_UPDATE,
        Op::Close => ISSUE_CLOSE,
        Op::Reopen => ISSUE_REOPEN,
        Op::Comment => ISSUE_COMMENT,
    }
}

// ---- principal stub ------------------------------------------------------

/// Minimal principal carried through axum [`axum::extract::Extension`]
/// for the protected handlers in this crate. Phase 4 stage 9 swaps
/// the population path to come from `starter-auth-users` /
/// `starter-auth-oauth` via `with_principal`; until then, tests
/// inject the extension directly.
///
/// We keep this small on purpose — the audit writer only needs an
/// `actor_user_id`. Richer authz attributes (e.g. `github_orgs`)
/// stay on the full `starter_spi::auth::Principal`; this struct is
/// the slice dp-rest reads on the request hot path.
#[derive(Debug, Clone, Copy)]
pub struct Principal {
    /// Stable user id of the operator making the request. Used as
    /// `actor_user_id` on every audit row.
    pub actor_user_id: Uuid,
}

// ---- writer --------------------------------------------------------------

/// Write one row to `dp_audit_log` via [`Store::record_audit_log`].
///
/// One helper, one call site per handler. The row id and `at`
/// timestamp are filled here so per-handler code stays a single line.
pub async fn record(
    store: &dyn Store,
    actor_user_id: Uuid,
    action: &str,
    target: impl Into<String>,
) -> Result<(), StoreError> {
    let entry = AuditEntry {
        id: Uuid::new_v4(),
        actor_user_id,
        action: action.to_string(),
        target: target.into(),
        at: Utc::now(),
    };
    store.record_audit_log(&entry).await
}
