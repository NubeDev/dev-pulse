//! [`Org`] — a GitHub organisation tracked by dev-pulse.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A GitHub organisation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Org {
    /// Internal primary key.
    pub id: Uuid,
    /// GitHub's numeric org id.
    pub github_id: i64,
    /// GitHub login (slug) of the org.
    pub login: String,
    /// Display name, if set.
    pub name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let org = Org {
            id: Uuid::nil(),
            github_id: 7,
            login: "nube-io".into(),
            name: Some("Nube".into()),
        };
        let back: Org = serde_json::from_str(&serde_json::to_string(&org).unwrap()).unwrap();
        assert_eq!(org, back);
    }
}
