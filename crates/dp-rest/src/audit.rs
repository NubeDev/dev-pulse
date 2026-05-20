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
