//! Per-user inbox state — backs the Linear-style triage UX
//! (see `linear-projects-idea.md` §3.8).
//!
//! Storage shape mirrors `dp_user_issue_state` (migration 0011):
//! one row per `(user_id, issue_id)` carrying the last issue
//! `version` the user saw plus a tri-state status with an optional
//! snooze deadline.
//!
//! Absence of a row means **default state** — implicitly "inbox",
//! `last_seen_version = 0` so the issue is unread until the user
//! opens it. That convention lives in the store impl; consumers
//! of [`UserIssueState`] only see the materialised form.

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Per-user inbox status. Persisted as the lowercase variant name
/// in the `dp_user_issue_state.status` column; the CHECK constraint
/// in migration 0011 mirrors the variants here so widening the
/// enum is a code + migration change in lockstep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InboxStatus {
    /// Default state — visible in the inbox smart view.
    Inbox,
    /// Hidden from the inbox until [`UserIssueState::snoozed_until`]
    /// passes. The application is responsible for re-promoting
    /// snoozed rows back into `Inbox` once the wake instant is past;
    /// the store does not auto-flip.
    Snoozed,
    /// Dismissed — never shows up in the inbox again unless the
    /// user explicitly re-opens it (which writes `Inbox` back).
    Done,
}

impl InboxStatus {
    /// Wire / SQL form. Matches the `dp_user_issue_state.status`
    /// CHECK in migration 0011.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Snoozed => "snoozed",
            Self::Done => "done",
        }
    }

    /// Parse the SQL form back. Returns `None` for unknown strings
    /// so the caller can fail loudly rather than silently coercing
    /// a future variant to the wrong default.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "inbox" => Some(Self::Inbox),
            "snoozed" => Some(Self::Snoozed),
            "done" => Some(Self::Done),
            _ => None,
        }
    }
}

/// One row in `dp_user_issue_state`. Returned by the inbox store
/// methods after a write; the read path returns the richer
/// [`InboxIssueRow`] which folds in the issue itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserIssueState {
    /// FK to `dp_users.id`.
    pub user_id: Uuid,
    /// FK to `dp_issues.id`.
    pub issue_id: Uuid,
    /// Highest `dp_issues.version` the user has observed. The
    /// frontend treats `dp_issues.version > last_seen_version` as
    /// "unread" — bolds the row, increments the sidebar badge.
    pub last_seen_version: i64,
    /// Tri-state status.
    pub status: InboxStatus,
    /// Wake-up instant for [`InboxStatus::Snoozed`] rows. `None` for
    /// `Inbox` / `Done`; populated by the snooze flow alongside
    /// `status = Snoozed`.
    pub snoozed_until: Option<DateTime<Utc>>,
    /// Last write to this row (any field).
    pub updated_at: DateTime<Utc>,
}

/// One row returned by [`crate::store::Store::list_inbox_issues`].
/// Pairs the issue projection with the per-user inbox metadata
/// the triage list needs to render unread dots, the `e` mark-done
/// shortcut, and snooze chips.
#[derive(Debug, Clone)]
pub struct InboxIssueRow {
    /// Full issue projection (same shape `list_issues` returns).
    pub issue: crate::issue::Issue,
    /// `true` when `issue.version > last_seen_version`. The frontend
    /// renders an unread indicator; opening the peek panel calls
    /// `mark_issues_seen` to clear it.
    pub unread: bool,
    /// Materialised inbox status. Rows absent from
    /// `dp_user_issue_state` surface as [`InboxStatus::Inbox`] with
    /// `last_seen_version = 0`.
    pub status: InboxStatus,
    /// Wake-up instant for snoozed rows. Same nullability as
    /// [`UserIssueState::snoozed_until`].
    pub snoozed_until: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_status_round_trips_through_sql_form() {
        for s in [InboxStatus::Inbox, InboxStatus::Snoozed, InboxStatus::Done] {
            assert_eq!(InboxStatus::from_str(s.as_str()), Some(s));
        }
        assert!(InboxStatus::from_str("nope").is_none());
        assert!(InboxStatus::from_str("INBOX").is_none()); // case-sensitive
    }
}
