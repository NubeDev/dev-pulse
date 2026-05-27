//! [`User`] — a GitHub user as known to dev-pulse.
//!
//! Soft-delete + pseudonymisation per TODO §0.5: deletion sets
//! [`User::deleted_at`] and rewrites `login`/`email`/`name` to a
//! `deleted-user-<hash>` form. The row's `id` is never reused so
//! historical reports keep referential integrity.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Operator-controlled authorisation tier
/// (DOCS/SCOPE-AUTHZ-USERS.md §2.2).
///
/// Persisted lowercase on `dp_users.role`; mapped to
/// `starter_spi::auth::Role` at the principal-mint seam in
/// `dp-server` so the wider policy engine sees one role per
/// request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Read-only access (default for new users).
    Reader,
    /// Writer can mutate triage / workflow surfaces.
    Writer,
    /// Full admin — manages users, runs the GDPR cascade.
    Admin,
}

impl Role {
    /// Stable lowercase wire form (matches the DB CHECK and the
    /// `serde(rename_all = "lowercase")` derive).
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Reader => "reader",
            Role::Writer => "writer",
            Role::Admin => "admin",
        }
    }

    /// Inverse of [`Role::as_str`]. Returns `None` for unknown
    /// strings so the store layer surfaces a clear error rather
    /// than silently degrading.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "reader" => Some(Role::Reader),
            "writer" => Some(Role::Writer),
            "admin" => Some(Role::Admin),
            _ => None,
        }
    }
}

impl Default for Role {
    fn default() -> Self {
        Role::Reader
    }
}

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
    /// Operator-controlled authorisation tier
    /// (DOCS/SCOPE-AUTHZ-USERS.md §2). Defaults to `Reader`.
    #[serde(default)]
    pub role: Role,
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
            role: Role::Writer,
            deleted_at: None,
        };
        let j = serde_json::to_string(&user).unwrap();
        let back: User = serde_json::from_str(&j).unwrap();
        assert_eq!(user, back);
        // Lowercase wire form per SCOPE §2.2.
        assert!(j.contains("\"role\":\"writer\""));
    }

    #[test]
    fn pseudonymised_user_still_round_trips() {
        let user = User {
            id: Uuid::nil(),
            github_id: 1,
            login: "deleted-user-abc".into(),
            email: None,
            name: None,
            role: Role::Reader,
            deleted_at: Some(Utc::now()),
        };
        let j = serde_json::to_string(&user).unwrap();
        let back: User = serde_json::from_str(&j).unwrap();
        assert_eq!(user, back);
    }

    #[test]
    fn role_wire_form_is_lowercase() {
        assert_eq!(Role::Reader.as_str(), "reader");
        assert_eq!(Role::Writer.as_str(), "writer");
        assert_eq!(Role::Admin.as_str(), "admin");
        assert_eq!(Role::from_str("admin"), Some(Role::Admin));
        assert_eq!(Role::from_str("nope"), None);
    }
}
