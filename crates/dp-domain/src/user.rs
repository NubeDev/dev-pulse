//! [`User`] — a GitHub user as known to dev-pulse.
//!
//! Soft-delete + pseudonymisation per TODO §0.5: deletion sets
//! [`User::deleted_at`] and rewrites `login`/`email`/`name` to a
//! `deleted-user-<hash>` form. The row's `id` is never reused so
//! historical reports keep referential integrity.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A GitHub user tracked by dev-pulse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    /// Internal primary key. Stable across pseudonymisation.
    pub id: Uuid,
    /// GitHub's numeric user id (stable across login changes).
    pub github_id: i64,
    /// GitHub login (handle). Rewritten on pseudonymisation.
    pub login: String,
    /// Optional public email. Rewritten on pseudonymisation.
    pub email: Option<String>,
    /// Display name. Rewritten on pseudonymisation.
    pub name: Option<String>,
    /// `Some(t)` after a GDPR erasure (TODO §0.5); `None` otherwise.
    pub deleted_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let user = User {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
            github_id: 42,
            login: "octocat".into(),
            email: Some("octocat@example.com".into()),
            name: Some("The Octocat".into()),
            deleted_at: None,
        };
        let j = serde_json::to_string(&user).unwrap();
        let back: User = serde_json::from_str(&j).unwrap();
        assert_eq!(user, back);
    }

    #[test]
    fn pseudonymised_user_still_round_trips() {
        let user = User {
            id: Uuid::nil(),
            github_id: 1,
            login: "deleted-user-abc".into(),
            email: None,
            name: None,
            deleted_at: Some(Utc::now()),
        };
        let j = serde_json::to_string(&user).unwrap();
        let back: User = serde_json::from_str(&j).unwrap();
        assert_eq!(user, back);
    }
}
