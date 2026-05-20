//! [`Tag`] — home-grown cross-org grouping primitive
//! (SCOPE-PROJECTS.md §5 + §7).
//!
//! A tag is an opaque bucket: meaning comes from what's linked to it
//! via [`TagLink`](crate::tag_link::TagLink). Tags are **cross-org by
//! construction** — a single tag can link repos / issues / users /
//! teams from several orgs at once, which is the whole reason
//! dev-pulse rolls its own primitive instead of using GitHub
//! Projects v2 (§7.1, §13.1).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Where a tag is visible. SCOPE-PROJECTS.md §7.4 ties this to the
/// SCOPE.md §15.11 access gate:
///
/// * `User`  — only the owner can see the tag.
/// * `Team`  — anyone the access gate lets see the team.
/// * `Org`   — anyone the access gate lets see the org.
///
/// The discriminator pairs with the matching `scope_*_id` field on
/// [`Tag`]; the migration's CHECK constraint guarantees exactly one
/// such field is non-NULL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TagScopeKind {
    /// Personal tag — only the owner sees it. `scope_user_id` set.
    User,
    /// Team-shared. `scope_team_id` set.
    Team,
    /// Org-shared. `scope_org_id` set. Default for new tags when the
    /// creator is in exactly one visible org (§7.4).
    Org,
}

impl TagScopeKind {
    /// Lower-case wire form, matching the `dp_tags.scope_kind` CHECK
    /// constraint (`'user' | 'team' | 'org'`).
    pub fn as_str(self) -> &'static str {
        match self {
            TagScopeKind::User => "user",
            TagScopeKind::Team => "team",
            TagScopeKind::Org => "org",
        }
    }
}

/// One row in `dp_tags`. The polymorphic scope is modelled as a
/// discriminator plus three nullable id columns — exactly one is
/// non-NULL, matching `scope_kind`. The migration's CHECK enforces
/// the invariant at the DB level; this struct mirrors the shape so
/// the store can round-trip rows without a second query.
///
/// `color` is a **semantic palette name** (`"indigo"`, `"red"`, …),
/// **not** a frontend design-token id. The frontend maps the
/// semantic name to its current token at render time — that
/// decouples stored rows from design-token churn (§7.2 notes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    /// Primary key.
    pub id: Uuid,
    /// Visibility scope discriminator.
    pub scope_kind: TagScopeKind,
    /// Set iff `scope_kind == User`.
    pub scope_user_id: Option<Uuid>,
    /// Set iff `scope_kind == Team`.
    pub scope_team_id: Option<Uuid>,
    /// Set iff `scope_kind == Org`.
    pub scope_org_id: Option<Uuid>,
    /// Human-readable name. Case-insensitively unique within scope
    /// (expression index on `lower(name)` in migration 0005).
    pub name: String,
    /// Semantic palette name (`"indigo"`, `"red"`, …) — not a
    /// design-token id.
    pub color: String,
    /// Optional free-form description.
    pub description: Option<String>,
    /// Author. References `dp_users.id` (no cascade — we want the
    /// audit trail of who created what to survive pseudonymisation).
    pub created_by: Uuid,
    /// When the tag was created.
    pub created_at: DateTime<Utc>,
    /// Soft-delete (§7.2). Archived tags survive for historical
    /// reports filtered by the tag, but are filtered out of pickers
    /// at query time.
    pub archived_at: Option<DateTime<Utc>>,
}

impl Tag {
    /// The single non-NULL scope id for this tag. The DB CHECK
    /// constraint guarantees exactly one of the three `scope_*_id`
    /// columns is set; this helper picks it without forcing every
    /// caller to repeat the match.
    pub fn scope_id(&self) -> Option<Uuid> {
        match self.scope_kind {
            TagScopeKind::User => self.scope_user_id,
            TagScopeKind::Team => self.scope_team_id,
            TagScopeKind::Org => self.scope_org_id,
        }
    }

    /// True if `archived_at` is set — convenience for pickers that
    /// hide archived tags (§7.2).
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(kind: TagScopeKind) -> Tag {
        let id = Uuid::nil();
        let mut t = Tag {
            id,
            scope_kind: kind,
            scope_user_id: None,
            scope_team_id: None,
            scope_org_id: None,
            name: "Phoenix".into(),
            color: "indigo".into(),
            description: None,
            created_by: id,
            created_at: Utc::now(),
            archived_at: None,
        };
        match kind {
            TagScopeKind::User => t.scope_user_id = Some(id),
            TagScopeKind::Team => t.scope_team_id = Some(id),
            TagScopeKind::Org => t.scope_org_id = Some(id),
        }
        t
    }

    #[test]
    fn scope_id_picks_the_matching_column() {
        for k in [TagScopeKind::User, TagScopeKind::Team, TagScopeKind::Org] {
            let t = sample(k);
            assert_eq!(t.scope_id(), Some(Uuid::nil()));
        }
    }

    #[test]
    fn tag_round_trips_through_json() {
        let t = sample(TagScopeKind::Org);
        let back: Tag = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn scope_kind_as_str_matches_check_constraint() {
        assert_eq!(TagScopeKind::User.as_str(), "user");
        assert_eq!(TagScopeKind::Team.as_str(), "team");
        assert_eq!(TagScopeKind::Org.as_str(), "org");
    }

    #[test]
    fn archived_helper() {
        let mut t = sample(TagScopeKind::Org);
        assert!(!t.is_archived());
        t.archived_at = Some(Utc::now());
        assert!(t.is_archived());
    }
}
