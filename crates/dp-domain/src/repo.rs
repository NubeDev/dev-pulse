//! [`Repo`] — a GitHub repository.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A GitHub repository, scoped to one [`Org`](crate::Org).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repo {
    /// Internal primary key.
    pub id: Uuid,
    /// Parent org.
    pub org_id: Uuid,
    /// GitHub's numeric repo id.
    pub github_id: i64,
    /// Repo name (no `org/` prefix).
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let r = Repo {
            id: Uuid::nil(),
            org_id: Uuid::nil(),
            github_id: 100,
            name: "dev-pulse".into(),
        };
        let back: Repo = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(r, back);
    }
}
