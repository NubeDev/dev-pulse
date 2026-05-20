//! [`Pin`] — a per-user reference to a repo or a tag, with an ordering
//! position. SCOPE-PROJECTS.md §5 + §6.
//!
//! Pins are personal UI state. They are *not* a report dimension and
//! do not appear in the §15.6 envelope.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Per-user data-model cap (SCOPE-PROJECTS §6.1 / §13.5, working
/// assumption 20). Lives in the domain crate so the REST handler
/// and the Postgres store both read the *same* number without
/// crossing a layer boundary. Eventually moves into `dp-config`.
pub const PIN_CAP: usize = 20;

/// What a pin points at. The two cases hit different target tables —
/// `repo` → `dp_repos.id`, `tag` → `dp_tags.id` — but the pin row
/// itself stores only `target_id: UUID` and this discriminator.
///
/// The `repo` arm covers "pin this repo I work in"; the `tag` arm is
/// the headline §6.1 case — pinning a tag is equivalent to pinning
/// every repo currently linked to it, expanded at render time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PinKind {
    /// `target_id` references `dp_repos.id`.
    Repo,
    /// `target_id` references `dp_tags.id`. Expands to the tag's
    /// `repo`-kind links at render time.
    Tag,
}

impl PinKind {
    /// Lower-case wire form, matching the `dp_user_pins.kind` CHECK
    /// constraint (`'repo' | 'tag'`).
    pub fn as_str(self) -> &'static str {
        match self {
            PinKind::Repo => "repo",
            PinKind::Tag => "tag",
        }
    }
}

/// One row in `dp_user_pins`. The composite primary key is
/// `(user_id, kind, target_id)`; `position` orders the user's
/// sidebar but is **not** uniqued at the DB level (atomic reorder
/// rewrites every row in one transaction — see migration 0005).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    /// Owner. Pins are strictly per-user.
    pub user_id: Uuid,
    /// Discriminator for `target_id`.
    pub kind: PinKind,
    /// Either a `dp_repos.id` or a `dp_tags.id`, per [`PinKind`].
    pub target_id: Uuid,
    /// Sidebar order. Lower comes first. Caller-assigned; the store
    /// does not renumber on insert.
    pub position: i32,
    /// When the row was created.
    pub pinned_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_kind_round_trips() {
        for k in [PinKind::Repo, PinKind::Tag] {
            let j = serde_json::to_string(&k).unwrap();
            let back: PinKind = serde_json::from_str(&j).unwrap();
            assert_eq!(k, back);
            assert!(j.contains(k.as_str()));
        }
    }

    #[test]
    fn pin_round_trips_through_json() {
        let p = Pin {
            user_id: Uuid::nil(),
            kind: PinKind::Tag,
            target_id: Uuid::nil(),
            position: 0,
            pinned_at: Utc::now(),
        };
        let back: Pin = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(p, back);
    }
}
