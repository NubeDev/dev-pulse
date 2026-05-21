//! GitHub identity tracking — the multi-identity model from
//! `users.md` §1 / `linear-projects-idea.md` §3.0.
//!
//! A [`UserIdentity`] is a single GitHub login a system user has
//! proven they control. One [`crate::user::User`] can have many.
//! The set has exactly one row with [`UserIdentity::is_primary`]
//! set; the partial unique index `dp_user_identities_primary_idx`
//! enforces this at the schema level.
//!
//! Provenance for `dp_memberships` lives in
//! [`MembershipIdentity`] — given a `(user, org)` membership it
//! records *which* of the user's identities saw that org. Unlink /
//! transfer use it to decide whether the membership survives the
//! identity going away.
//!
//! OAuth link round-trips reserve a [`IdentityLinkPending`] row
//! with the `state` nonce; the callback validates and consumes it
//! atomically. See `users.md` §2.1.1.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// How the link between a dp-user and a GitHub identity was
/// established. The schema enforces the same set via a CHECK
/// constraint on `dp_user_identities.verified_via`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifiedVia {
    /// The user proved control via an OAuth round-trip from their
    /// own session. The everyday case.
    Oauth,
    /// An admin attached the identity on the user's behalf
    /// (break-glass; high-visibility audit row also surfaces in
    /// the target user's own audit log).
    AdminLink,
    /// Reserved for scheduled token rotation. Not emitted today
    /// but pinned in the schema's CHECK so a future rotator can
    /// land without a migration.
    Rotation,
}

impl VerifiedVia {
    /// Wire-form string matching the SQL CHECK constraint.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Oauth => "oauth",
            Self::AdminLink => "admin_link",
            Self::Rotation => "rotation",
        }
    }

    /// Parse the SQL-side string. Returns `None` for unknown
    /// values so the decoder can fail loudly instead of silently
    /// mapping to a default.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "oauth" => Some(Self::Oauth),
            "admin_link" => Some(Self::AdminLink),
            "rotation" => Some(Self::Rotation),
            _ => None,
        }
    }
}

/// One row of `dp_user_identities`: a GitHub identity claimed by
/// a dp-user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserIdentity {
    /// Owning dp-user.
    pub user_id: Uuid,
    /// GitHub's numeric user id. Stable across login renames; the
    /// authoritative join key for membership provenance.
    pub github_user_id: i64,
    /// GitHub login at link time / last refresh. Denormalized for
    /// directory search; not unique because GitHub allows logins
    /// to be renamed (and reused).
    pub github_login: String,
    /// True iff this is the user's primary identity. Exactly one
    /// per user, enforced by `dp_user_identities_primary_idx`.
    pub is_primary: bool,
    /// When the link was established.
    pub linked_at: DateTime<Utc>,
    /// Provenance of the link.
    pub verified_via: VerifiedVia,
}

/// One row of `dp_membership_identities`: a record that identity
/// `github_user_id` (belonging to `user_id`) reaches `org_id`.
/// Memberships in `dp_memberships` exist iff at least one of
/// these rows exists for the same `(user_id, org_id)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipIdentity {
    /// Owning dp-user.
    pub user_id: Uuid,
    /// Org the identity reaches.
    pub org_id: Uuid,
    /// GitHub identity providing the access.
    pub github_user_id: i64,
    /// When the stamper last observed this identity inside the
    /// org.
    pub observed_at: DateTime<Utc>,
}

/// In-flight OAuth link round-trip. The `nonce` is the opaque
/// `state` value handed to GitHub; the row is consumed (deleted)
/// in the same transaction as the resulting `dp_user_identities`
/// insert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityLinkPending {
    /// Opaque OAuth `state` value. Never the session id.
    pub nonce: Uuid,
    /// dp-user the link will attach to on success.
    pub dp_user_id: Uuid,
    /// Session that started the link. The callback must run in
    /// the same session, or it is rejected.
    pub session_id: String,
    /// When the row was created.
    pub created_at: DateTime<Utc>,
    /// Hard deadline. Callbacks past this point are rejected and
    /// the row is GC'd.
    pub expires_at: DateTime<Utc>,
}

/// Why an identity-link attempt was rejected. Mirrors the
/// `IDENTITY_LINK_REJECTED { reason }` audit vocabulary so the
/// REST layer can write the audit and return a 4xx in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityLinkRejection {
    /// The `state` nonce was unknown, already consumed, or
    /// expired.
    NonceInvalid,
    /// The session at callback time differs from the session that
    /// started the link.
    SessionMismatch,
    /// The dp-user at callback time differs from the dp-user that
    /// started the link.
    UserMismatch,
    /// The GitHub account is already claimed by a different
    /// dp-user. Surfaces to the caller as HTTP 409.
    ClaimConflict,
}

impl IdentityLinkRejection {
    /// Wire-form reason string for audit / API responses.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NonceInvalid => "nonce_invalid",
            Self::SessionMismatch => "session_mismatch",
            Self::UserMismatch => "user_mismatch",
            Self::ClaimConflict => "claim_conflict",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_via_round_trip() {
        for v in [VerifiedVia::Oauth, VerifiedVia::AdminLink, VerifiedVia::Rotation] {
            assert_eq!(VerifiedVia::from_str(v.as_str()), Some(v));
        }
        assert_eq!(VerifiedVia::from_str("nope"), None);
    }

    #[test]
    fn identity_round_trips_through_json() {
        let row = UserIdentity {
            user_id: Uuid::nil(),
            github_user_id: 42,
            github_login: "octocat".into(),
            is_primary: true,
            linked_at: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            verified_via: VerifiedVia::Oauth,
        };
        let j = serde_json::to_string(&row).unwrap();
        let back: UserIdentity = serde_json::from_str(&j).unwrap();
        assert_eq!(row, back);
    }
}
