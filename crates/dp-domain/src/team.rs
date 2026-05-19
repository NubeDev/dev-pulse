//! [`Team`] — a named group inside an organisation.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A GitHub team, scoped to one [`Org`](crate::Org).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Team {
    /// Internal primary key.
    pub id: Uuid,
    /// Parent org.
    pub org_id: Uuid,
    /// GitHub's numeric team id.
    pub github_id: i64,
    /// URL slug as it appears on github.com (e.g. `backend`).
    pub slug: String,
    /// Display name.
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let t = Team {
            id: Uuid::nil(),
            org_id: Uuid::nil(),
            github_id: 9,
            slug: "backend".into(),
            name: "Backend".into(),
        };
        let back: Team = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(t, back);
    }
}
