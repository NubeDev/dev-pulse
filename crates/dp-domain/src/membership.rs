//! [`Membership`] — the join between a [`User`](crate::User) and an
//! [`Org`](crate::Org), carrying the "home-org" label that powers
//! cross-company comparison (SCOPE §3 goal 3, §8.1).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A user's role inside an org.
///
/// GitHub exposes `Admin` and `Member`; we keep `Other(String)` open
/// because GitHub Enterprise can add custom roles and we'd rather store
/// the truth than drop the row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipRole {
    /// GitHub org admin.
    Admin,
    /// Regular org member.
    Member,
    /// Anything else GitHub returns (Enterprise custom roles, etc.).
    Other(String),
}

/// `(user, org)` join row. Home-org is set manually by an admin
/// (SCOPE §3 — manual mapping is v1; domain inference is a stretch).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Membership {
    /// User side of the join.
    pub user_id: Uuid,
    /// Org side of the join.
    pub org_id: Uuid,
    /// Role in this org.
    pub role: MembershipRole,
    /// "Home-org" label — the [`Org::id`](crate::Org::id) this user
    /// is counted under in cross-company comparisons. `None` until an
    /// admin sets it (SCOPE §3).
    pub home_org: Option<Uuid>,
    /// When dev-pulse first observed this membership.
    pub joined_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let m = Membership {
            user_id: Uuid::nil(),
            org_id: Uuid::nil(),
            role: MembershipRole::Admin,
            home_org: Some(Uuid::nil()),
            joined_at: Utc::now(),
        };
        let back: Membership = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn other_role_round_trips() {
        let m = MembershipRole::Other("billing_manager".into());
        let back: MembershipRole =
            serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(m, back);
    }
}
