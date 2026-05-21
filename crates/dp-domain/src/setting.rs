//! [`UserSetting`] — one row in `dp_user_settings`, the per-user
//! open-ended key/value store behind the frontend "Account →
//! Settings" page (migration `0029_user_settings.sql`).
//!
//! Designed so new settings ship as a pinned key constant +
//! frontend field, *not* a schema migration. The first consumer
//! is the per-user GitHub PAT (`github.pat`). The REST layer
//! enforces a small pinned key catalogue so a typo can't grow
//! the schema silently — see `dp_rest::settings::KEYS`.
//!
//! Secrets handling: the REST layer redacts `value` on every
//! GET when [`UserSetting::is_secret`] is `true`. The bit lives
//! on the domain row (not only at the REST layer) so a future
//! direct consumer (CLI export, MCP) still sees it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One row in `dp_user_settings`. The composite primary key is
/// `(user_id, key)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserSetting {
    /// Owner. Settings are strictly per-user.
    pub user_id: Uuid,
    /// Dotted-namespace key (e.g. `"github.pat"`, `"ui.theme"`).
    /// Validated against `dp_rest::settings::KEYS` at the edge.
    pub key: String,
    /// Opaque value. Interpretation per key (string, JSON, …).
    /// Never returned verbatim for [`is_secret`](Self::is_secret) rows.
    pub value: String,
    /// When `true`, the REST layer redacts [`value`](Self::value)
    /// on read.
    pub is_secret: bool,
    /// Last write.
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let s = UserSetting {
            user_id: Uuid::nil(),
            key: "github.pat".into(),
            value: "ghp_xxxx".into(),
            is_secret: true,
            updated_at: Utc::now(),
        };
        let j = serde_json::to_string(&s).unwrap();
        let back: UserSetting = serde_json::from_str(&j).unwrap();
        assert_eq!(s, back);
    }
}
