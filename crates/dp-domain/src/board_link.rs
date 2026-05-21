//! [`BoardLink`] / [`BoardItem`] — project → GitHub Projects v2
//! board mirror plumbing (`linear-projects-v2.md` §5, slice B).
//!
//! A [`BoardLink`] is one (project, GitHub board) attachment. A
//! project can carry many board links; mirror writes fan out across
//! them and per-link outcomes surface back in the §7.4
//! `207 Multi-Status` response. A [`BoardItem`] is the per
//! (link, issue) projection state — the GitHub Projects v2 *item*
//! node id the mirror reuses on subsequent edits, plus the most
//! recent sync outcome for that pair.
//!
//! The §3.10 per-repo [`crate::issue_dates::RepoProjectLink`] type
//! is retained for source-compat while the admin-pane REST surface
//! is wound down in slice B; new code should reach for [`BoardLink`]
//! instead.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One row in `dp_project_board_links`. Read shape for the §6.3
/// "Linked GitHub boards" block and the §7.3
/// `GET /projects/{id}/board-links` response.
///
/// `github_board_title` / `github_board_url` are *cached* —
/// refreshed opportunistically by the §7.3 picker and by the
/// nightly safety-net job. Surfaces render whatever the store has
/// and let the next picker call backfill misses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardLink {
    /// Primary key (opaque to clients).
    pub id: Uuid,
    /// Parent project.
    pub project_id: Uuid,
    /// GitHub Projects v2 board node id (`PVT_…`). The mirror
    /// passes this to `addProjectV2ItemById` as `projectId`.
    pub github_board_node_id: String,
    /// Cached human-readable board title. `None` when the picker
    /// has not refreshed yet.
    pub github_board_title: Option<String>,
    /// Cached deep link to github.com for this board. `None` when
    /// the picker has not refreshed yet.
    pub github_board_url: Option<String>,
    /// Wall-clock the picker last refreshed the cached title / url
    /// fields. `None` until the first refresh.
    pub github_board_cached_at: Option<DateTime<Utc>>,
    /// Start-date field node id on the board, or `None` when the
    /// board does not configure one. The mirror skips the start
    /// lane whenever this is unset.
    pub start_field_node_id: Option<String>,
    /// Due-date field node id on the board, or `None` when the
    /// board does not configure one. The mirror skips the due
    /// lane whenever this is unset.
    pub due_field_node_id: Option<String>,
    /// Reserved — status field node id. Not written by the v1
    /// mirror; carried so a v2 expansion lands without another
    /// migration (§5 entity table).
    pub status_field_node_id: Option<String>,
    /// Aggregate timestamp of the most recent successful mirror
    /// for any item under this link. `None` until the first
    /// success. Updated transactionally by
    /// [`crate::store::Store::record_board_item_result`].
    pub last_mirror_at: Option<DateTime<Utc>>,
    /// Aggregate error from the most recent *failed* mirror for
    /// any item under this link. `None` when the most recent
    /// outcome (across any item) was a success.
    pub last_mirror_error: Option<String>,
    /// User who created the link. Immutable. Nullable so the
    /// `ON DELETE SET NULL` on the FK preserves history when a
    /// user is pseudonymised.
    pub created_by: Option<Uuid>,
    /// When the link was first written.
    pub created_at: DateTime<Utc>,
    /// When any field on the link row last mutated.
    pub updated_at: DateTime<Utc>,
}

/// Mutable payload for [`crate::store::Store::create_board_link`].
/// Carries only the caller-supplied fields; the store fills in
/// `id`, `created_at`, `updated_at`, the cached `github_board_*`
/// fields (from the picker DTO), and the per-link mirror status.
///
/// A POST that pre-fills the cached `github_board_title` /
/// `github_board_url` (because the picker just returned them)
/// passes them through here so the §6.4 dialog can roundtrip the
/// chosen board's display data without a second picker call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardLinkUpsert {
    /// Parent project the link attaches to.
    pub project_id: Uuid,
    /// GitHub board node id chosen from the picker.
    pub github_board_node_id: String,
    /// Cached board title from the picker, or `None` if the caller
    /// has not resolved one. The nightly refresh will fill it in.
    pub github_board_title: Option<String>,
    /// Cached board URL from the picker.
    pub github_board_url: Option<String>,
    /// Mapped start-date field, or `None` when the board does not
    /// define one. The user picks this in the §6.4 dialog.
    pub start_field_node_id: Option<String>,
    /// Mapped due-date field, or `None` when the board does not
    /// define one.
    pub due_field_node_id: Option<String>,
    /// Reserved — status field (not written by the v1 mirror).
    pub status_field_node_id: Option<String>,
    /// Caller (stored in `created_by`).
    pub created_by: Option<Uuid>,
}

/// One row in `dp_project_board_items`. Per (link, issue) projection
/// state — the GitHub Projects v2 item node id the mirror reuses on
/// subsequent edits, plus the most recent sync outcome for that
/// pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardItem {
    /// Parent link.
    pub link_id: Uuid,
    /// Target issue.
    pub issue_id: Uuid,
    /// GitHub Projects v2 item node id (`PVTI_…`) — the card on
    /// the board. Returned by `addProjectV2ItemById` on first
    /// projection; reused on every subsequent
    /// `updateProjectV2ItemFieldValue` so the mirror doesn't
    /// accrete duplicate cards.
    pub item_node_id: String,
    /// Wall-clock the worker last successfully synced this pair.
    /// `None` until the first success.
    pub last_synced_at: Option<DateTime<Utc>>,
    /// Verbatim GraphQL error from the most recent failed sync;
    /// cleared on success so the UI does not keep showing a stale
    /// failure after the operator fixes it.
    pub last_error: Option<String>,
    /// When the row was first written.
    pub created_at: DateTime<Utc>,
    /// When `last_synced_at` / `last_error` / `item_node_id` last
    /// mutated.
    pub updated_at: DateTime<Utc>,
}

/// Outcome of one mirror attempt for a (link, issue) pair, fed back
/// into [`crate::store::Store::record_board_item_result`]. Borrowed
/// strings so the worker can pass GraphQL error text straight
/// through without an intermediate allocation. The store layer
/// rolls the per-item outcome up to the aggregate
/// `dp_project_board_links.last_mirror_*` columns transactionally.
#[derive(Debug, Clone, Copy)]
pub enum BoardItemMirrorOutcome<'a> {
    /// Mirror succeeded. `item_node_id` is the (possibly newly
    /// minted) `PVTI_…` returned by GitHub.
    Success {
        /// Persist as `dp_project_board_items.item_node_id`.
        item_node_id: &'a str,
    },
    /// Mirror failed. `error` is the verbatim GraphQL error text.
    Failure {
        /// Persist as `dp_project_board_items.last_error` (and the
        /// aggregate `dp_project_board_links.last_mirror_error`).
        error: &'a str,
    },
}
