//! Per-user inbox handlers — the unread / snooze / done UX
//! (`linear-projects-idea.md` §3.8 / §5.8).
//!
//! Two routes:
//!
//! | route                              | what it does                            |
//! |------------------------------------|-----------------------------------------|
//! | `POST  /me/inbox/seen`             | bulk-mark a list of issues as read      |
//! | `PATCH /me/inbox/{issue_id}`       | set inbox status / snooze for one issue |
//!
//! Both routes are **per-caller** (`Principal::actor_user_id`)
//! and reuse the existing `(issues, read)` authz pair — this is
//! per-user UI state, not an issue mutation. There is no
//! admin-on-behalf path; the `/me/...` prefix is the only
//! addressing scheme.
//!
//! Audit: not audited. Inbox writes are personal UI state and
//! the volume (`seen` fires every time the user opens the peek)
//! would swamp the audit log without operational value — same
//! rationale as `GET /me/pins`.

use std::sync::Arc;

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::Json,
    routing::{patch, post},
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::inbox::{InboxStatus, UserIssueState};

use crate::audit::Principal;
use crate::error::ApiError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Wire DTOs
// ---------------------------------------------------------------------------

/// Wire form of [`InboxStatus`]. Lower-case to match the SQL form
/// declared in migration 0011.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum InboxStatusDto {
    /// Default — shows up in `★ My queue`.
    Inbox,
    /// Hidden from the inbox until [`UserIssueStateDto::snoozed_until`].
    Snoozed,
    /// Dismissed — hidden from the inbox permanently.
    Done,
}

impl From<InboxStatus> for InboxStatusDto {
    fn from(s: InboxStatus) -> Self {
        match s {
            InboxStatus::Inbox => Self::Inbox,
            InboxStatus::Snoozed => Self::Snoozed,
            InboxStatus::Done => Self::Done,
        }
    }
}

impl From<InboxStatusDto> for InboxStatus {
    fn from(s: InboxStatusDto) -> Self {
        match s {
            InboxStatusDto::Inbox => Self::Inbox,
            InboxStatusDto::Snoozed => Self::Snoozed,
            InboxStatusDto::Done => Self::Done,
        }
    }
}

/// One row in the inbox state table. Echoed back by the
/// PATCH handler so the UI can confirm the write without a
/// follow-up GET.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UserIssueStateDto {
    /// FK to `dp_issues.id`.
    pub issue_id: Uuid,
    /// Highest `dp_issues.version` the caller has marked seen.
    pub last_seen_version: i64,
    /// Tri-state status.
    pub status: InboxStatusDto,
    /// Wake-up instant for snoozed rows. `None` for inbox / done.
    pub snoozed_until: Option<DateTime<Utc>>,
    /// Last write to this row.
    pub updated_at: DateTime<Utc>,
}

impl From<UserIssueState> for UserIssueStateDto {
    fn from(s: UserIssueState) -> Self {
        Self {
            issue_id: s.issue_id,
            last_seen_version: s.last_seen_version,
            status: s.status.into(),
            snoozed_until: s.snoozed_until,
            updated_at: s.updated_at,
        }
    }
}

/// Body for `POST /me/inbox/seen`. The server marks every listed
/// issue as read up to its current `version`. Empty list is a
/// no-op (`204`). Capped at 200 ids per request to keep the
/// SQL bind size sane — clients with more than that should
/// page their reads via the list endpoint anyway.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct MarkSeenRequest {
    /// `dp_issues.id` rows to mark seen.
    pub issue_ids: Vec<Uuid>,
}

/// Hard cap on `issue_ids` per `POST /me/inbox/seen` request.
pub const SEEN_BATCH_CAP: usize = 200;

/// Body for `PATCH /me/inbox/{issue_id}`. Either field may be
/// absent; an empty body leaves the row untouched (still upserted
/// at default values if it did not exist).
///
/// Consistency between `status` and `snoozed_until` is the caller's
/// responsibility — the server does not refuse `status = Inbox`
/// with `snoozed_until = Some(_)` so the UI can clear the snooze
/// by setting the status alone without a second round-trip.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SetInboxStateRequest {
    /// New status (defaults to existing value when absent — for a
    /// fresh row this means [`InboxStatusDto::Inbox`]).
    #[serde(default)]
    pub status: Option<InboxStatusDto>,
    /// New snooze deadline. Use explicit `null` to clear.
    #[serde(default)]
    pub snoozed_until: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /me/inbox/seen` — bulk-mark issues read. Idempotent. The
/// store upserts one `dp_user_issue_state` row per id, setting
/// `last_seen_version` from `dp_issues.version` (so the UI never
/// has to know the version itself).
///
/// Returns `204 No Content` on success.
#[utoipa::path(
    post,
    path = "/me/inbox/seen",
    request_body = MarkSeenRequest,
    responses(
        (status = 204, description = "Issues marked seen"),
        (status = 400, description = "Too many issue ids in one batch"),
    ),
    tag = "inbox",
)]
pub async fn mark_seen(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<MarkSeenRequest>,
) -> Result<StatusCode, ApiError> {
    if body.issue_ids.len() > SEEN_BATCH_CAP {
        return Err(ApiError::BadRequest {
            code: "seen_batch_too_large",
            message: format!(
                "{} ids exceeds the per-request cap of {SEEN_BATCH_CAP}",
                body.issue_ids.len()
            ),
        });
    }
    state
        .store
        .mark_issues_seen(principal.actor_user_id, &body.issue_ids)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `PATCH /me/inbox/{issue_id}` — set inbox status / snooze.
///
/// The body's `status` and `snoozed_until` are written verbatim;
/// see [`SetInboxStateRequest`] for the consistency contract.
///
/// Returns the resulting row so the UI can confirm without a
/// second round-trip.
#[utoipa::path(
    patch,
    path = "/me/inbox/{issue_id}",
    params(("issue_id" = Uuid, Path, description = "Issue id")),
    request_body = SetInboxStateRequest,
    responses(
        (status = 200, description = "Updated inbox row", body = UserIssueStateDto),
    ),
    tag = "inbox",
)]
pub async fn set_inbox_state(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(issue_id): Path<Uuid>,
    Json(body): Json<SetInboxStateRequest>,
) -> Result<Json<UserIssueStateDto>, ApiError> {
    // When `status` is absent, the caller is only changing the
    // snooze deadline — default to `Snoozed` if they actually
    // sent a deadline, otherwise `Inbox`.
    let status: InboxStatus = match body.status {
        Some(s) => s.into(),
        None if body.snoozed_until.is_some() => InboxStatus::Snoozed,
        None => InboxStatus::Inbox,
    };
    let row = state
        .store
        .set_inbox_state(principal.actor_user_id, issue_id, status, body.snoozed_until)
        .await?;
    Ok(Json(row.into()))
}

// ---------------------------------------------------------------------------
// Bulk endpoint (slice 2)
// ---------------------------------------------------------------------------

/// Operation kind for [`BulkInboxRequest`]. One of mark-seen, snooze,
/// done, or restore-to-inbox — the four bulk actions the workbench
/// exposes from the list header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BulkInboxOp {
    /// Mark every listed issue as read up to its current version.
    /// Equivalent to a bulk `POST /me/inbox/seen` call.
    MarkAllSeen,
    /// Snooze every listed issue. `snoozed_until` is required.
    SnoozeAll,
    /// Dismiss every listed issue (`status = done`).
    DoneAll,
    /// Restore every listed issue to the inbox; clears any snooze.
    InboxAll,
}

/// Body for `POST /me/inbox/bulk`. One operation applied to a batch
/// of issue ids.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct BulkInboxRequest {
    /// `dp_issues.id` rows to touch. Capped at [`SEEN_BATCH_CAP`].
    pub issue_ids: Vec<Uuid>,
    /// Which transition to apply.
    pub op: BulkInboxOp,
    /// Required for `snooze_all`; ignored otherwise.
    #[serde(default)]
    pub snoozed_until: Option<DateTime<Utc>>,
}

/// Response from `POST /me/inbox/bulk`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BulkInboxResponse {
    /// Number of `dp_user_issue_state` rows touched (inserted +
    /// updated). For `mark_all_seen` this is the upsert count from
    /// the underlying `mark_issues_seen` call and is reported as the
    /// length of the request batch (the store does not surface the
    /// row count for that path today).
    pub touched: u64,
}

/// `POST /me/inbox/bulk` — bulk inbox transitions
/// (mark-all-seen / snooze-all / done-all / inbox-all). Used by the
/// list-header bulk action menu in the workbench. Audited under the
/// `BULK_INBOX_*` vocabulary so the audit log can answer "who did
/// the mass dismiss?" without scanning the per-row writes.
#[utoipa::path(
    post,
    path = "/me/inbox/bulk",
    request_body = BulkInboxRequest,
    responses(
        (status = 200, description = "Bulk transition applied", body = BulkInboxResponse),
        (status = 400, description = "Batch too large / snooze without deadline"),
    ),
    tag = "inbox",
)]
pub async fn bulk_inbox(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<BulkInboxRequest>,
) -> Result<Json<BulkInboxResponse>, ApiError> {
    if body.issue_ids.len() > SEEN_BATCH_CAP {
        return Err(ApiError::BadRequest {
            code: "bulk_batch_too_large",
            message: format!(
                "{} ids exceeds the per-request cap of {SEEN_BATCH_CAP}",
                body.issue_ids.len()
            ),
        });
    }
    if matches!(body.op, BulkInboxOp::SnoozeAll) && body.snoozed_until.is_none() {
        return Err(ApiError::BadRequest {
            code: "snooze_without_deadline",
            message: "snooze_all requires snoozed_until".into(),
        });
    }

    let (touched, verb) = match body.op {
        BulkInboxOp::MarkAllSeen => {
            state
                .store
                .mark_issues_seen(principal.actor_user_id, &body.issue_ids)
                .await?;
            (body.issue_ids.len() as u64, crate::audit::BULK_INBOX_SEEN)
        }
        BulkInboxOp::SnoozeAll => {
            let n = state
                .store
                .set_inbox_state_bulk(
                    principal.actor_user_id,
                    &body.issue_ids,
                    InboxStatus::Snoozed,
                    body.snoozed_until,
                )
                .await?;
            (n, crate::audit::BULK_INBOX_SNOOZE)
        }
        BulkInboxOp::DoneAll => {
            let n = state
                .store
                .set_inbox_state_bulk(
                    principal.actor_user_id,
                    &body.issue_ids,
                    InboxStatus::Done,
                    None,
                )
                .await?;
            (n, crate::audit::BULK_INBOX_DONE)
        }
        BulkInboxOp::InboxAll => {
            let n = state
                .store
                .set_inbox_state_bulk(
                    principal.actor_user_id,
                    &body.issue_ids,
                    InboxStatus::Inbox,
                    None,
                )
                .await?;
            (n, crate::audit::BULK_INBOX_INBOX)
        }
    };

    // Audit target carries the row count so the log can answer "how
    // big was the mass action?" without re-deriving from siblings.
    let target = format!("count={touched}");
    crate::audit::record(state.store.as_ref(), principal.actor_user_id, verb, target)
        .await
        .ok(); // audit failures never block the inbox response

    Ok(Json(BulkInboxResponse { touched }))
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the inbox router. Gated on `(issues, read)` — inbox
/// state is per-user UI metadata, not an issue mutation, and
/// every caller who can list their own issues can manage their
/// own inbox state.
pub fn inbox_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new()
                .route("/me/inbox/seen", post(mark_seen))
                .route("/me/inbox/bulk", post(bulk_inbox))
                .route("/me/inbox/{issue_id}", patch(set_inbox_state)),
            "issues",
            "read",
        ))
        .with_state(inner)
}
