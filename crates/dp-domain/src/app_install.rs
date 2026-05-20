//! [`OrgAppInstall`] — a row capturing the GitHub App's installation
//! state on one org, including the *permissions* the org admin
//! consented to (SCOPE-PROJECTS §8.4, §13.6).
//!
//! Stage 8 introduces this type as the read-side input the §8 issue
//! write surface (stages 9+) consults before letting a mutation
//! through. The reconciler / install-callback handler keeps the row
//! fresh; the row exists per org and is overwritten on every fresh
//! observation.
//!
//! A row whose [`AppInstallPermissions::issues_write`] is `false` —
//! or, equivalently, the *absence* of a row for an org — means
//! "writes not available for this org": the §8.4 affordance applies
//! and any caller that bypasses the UI gets
//! `403 writes_not_available_for_org` (see `dp_rest::ApiError`).
//!
//! Nothing in this module performs an HTTP call. It is a pure value
//! type; the fetcher / install callback populates rows via the
//! [`Store`](crate::Store) trait.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The subset of GitHub App permissions dev-pulse pays attention to.
///
/// v1 only cares about `issues: write` — the other resources the App
/// requests (metadata, contents, pull_requests, members) are
/// read-only across the board and are not gated per-call. Add more
/// fields here when a future surface needs a finer gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppInstallPermissions {
    /// `true` iff the per-org installation has been granted
    /// `issues: write`. The §8 issue-mutation handlers check this
    /// before calling GitHub; `false` triggers the §8.4
    /// `writes_not_available_for_org` 403.
    pub issues_write: bool,
}

impl AppInstallPermissions {
    /// Fail-closed default: nothing is allowed. Used when no
    /// `OrgAppInstall` row exists for an org yet — the §8.4 path
    /// applies until the install-callback / reconciler fills the
    /// row in.
    pub const READ_ONLY: Self = Self {
        issues_write: false,
    };
}

impl Default for AppInstallPermissions {
    fn default() -> Self {
        Self::READ_ONLY
    }
}

/// One per-org GitHub App installation record.
///
/// `org_id` is the dev-pulse-local org primary key; `installation_id`
/// is the GitHub-side numeric id (kept as `i64` to match the
/// representation in `dp-fetcher::client::credentials`). `observed_at`
/// is the wall-clock time the permission snapshot was taken so an
/// admin can tell stale rows from fresh ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgAppInstall {
    /// dev-pulse-local org primary key.
    pub org_id: Uuid,
    /// GitHub-side numeric installation id.
    pub installation_id: i64,
    /// Permissions snapshot at `observed_at`.
    pub permissions: AppInstallPermissions,
    /// When the snapshot was taken.
    pub observed_at: DateTime<Utc>,
}

impl OrgAppInstall {
    /// Convenience: does this install have the `issues: write`
    /// permission? Mirrors
    /// [`AppInstallPermissions::issues_write`].
    pub fn allows_issues_write(&self) -> bool {
        self.permissions.issues_write
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_default() {
        assert!(!AppInstallPermissions::default().issues_write);
        assert_eq!(
            AppInstallPermissions::default(),
            AppInstallPermissions::READ_ONLY
        );
    }

    #[test]
    fn allows_issues_write_mirrors_permission() {
        let install = OrgAppInstall {
            org_id: Uuid::nil(),
            installation_id: 42,
            permissions: AppInstallPermissions { issues_write: true },
            observed_at: Utc::now(),
        };
        assert!(install.allows_issues_write());

        let ro = OrgAppInstall {
            permissions: AppInstallPermissions::READ_ONLY,
            ..install
        };
        assert!(!ro.allows_issues_write());
    }

    #[test]
    fn round_trips_through_json() {
        let install = OrgAppInstall {
            org_id: Uuid::nil(),
            installation_id: 99,
            permissions: AppInstallPermissions { issues_write: true },
            observed_at: Utc::now(),
        };
        let back: OrgAppInstall =
            serde_json::from_str(&serde_json::to_string(&install).unwrap()).unwrap();
        assert_eq!(install, back);
    }
}
