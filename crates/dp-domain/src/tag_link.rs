//! [`TagLink`] — polymorphic edge from a [`Tag`](crate::tag::Tag) to
//! one of: repo, issue, user, team (SCOPE-PROJECTS.md §7.2 + §7.3).
//!
//! Polymorphism is the point of the type. A single tag with
//! `(repo, issue, user, team)` links covers the four §7.3
//! use-cases: cross-org repo grouping, follow-up issues, "the
//! squad", and team-level membership in one project. The §13.2
//! decision locks the kind set at exactly these four for v1.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What a [`TagLink`] points at. Pairs with the matching
/// `target_*_id` column on the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TagLinkKind {
    /// `target_repo_id` set. Contributes to every report metric
    /// (filters on `activity_events.repo_id` — see §7.7 table).
    Repo,
    /// `target_issue_id` set. Contributes to issue-centric metrics
    /// only; ignored for commit / PR / review / workflow metrics.
    Issue,
    /// `target_user_id` set. Contributes to every metric (filters on
    /// `event_actors.user_id`).
    User,
    /// `target_team_id` set. Contributes to every metric (expands to
    /// team members at query time).
    Team,
    /// `target_project_id` set. The cross-org grouping concept the
    /// portfolio surface tags. Not an activity-attribution target —
    /// project tags drive the portfolio column + filter, not the
    /// §7.7 report metrics (see [`resolve_tag_targets`] passthrough).
    ///
    /// [`resolve_tag_targets`]: crate::store::Store::resolve_tag_targets
    Project,
}

impl TagLinkKind {
    /// Lower-case wire form, matching the `dp_tag_links.kind` CHECK
    /// constraint (`'repo' | 'issue' | 'user' | 'team' | 'project'`).
    pub fn as_str(self) -> &'static str {
        match self {
            TagLinkKind::Repo => "repo",
            TagLinkKind::Issue => "issue",
            TagLinkKind::User => "user",
            TagLinkKind::Team => "team",
            TagLinkKind::Project => "project",
        }
    }

    /// Whether a link of this kind contributes to non-issue report
    /// metrics. Encodes the §7.7 "metric × link-kind" mapping:
    /// `issue`-only tags produce an empty result with
    /// `empty_reason = "tag links do not match metric attribution"`
    /// when queried against commit / PR / workflow metrics.
    pub fn applies_to_non_issue_metrics(self) -> bool {
        !matches!(self, TagLinkKind::Issue)
    }
}

/// One row in `dp_tag_links`. The four `target_*_id` columns are
/// nullable; exactly one is non-NULL, matching `kind`. The
/// migration's CHECK enforces the invariant; this struct mirrors it
/// so the store can round-trip rows without a second query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagLink {
    /// Primary key.
    pub id: Uuid,
    /// Parent tag.
    pub tag_id: Uuid,
    /// Discriminator for `target_*_id`.
    pub kind: TagLinkKind,
    /// Set iff `kind == Repo`.
    pub target_repo_id: Option<Uuid>,
    /// Set iff `kind == Issue`.
    pub target_issue_id: Option<Uuid>,
    /// Set iff `kind == User`.
    pub target_user_id: Option<Uuid>,
    /// Set iff `kind == Team`.
    pub target_team_id: Option<Uuid>,
    /// Set iff `kind == Project`.
    pub target_project_id: Option<Uuid>,
    /// Who attached the link. Audited; survives pseudonymisation
    /// (the `dp_users.id` stays stable per §0.5).
    pub added_by: Uuid,
    /// When the link was attached.
    pub added_at: DateTime<Utc>,
}

impl TagLink {
    /// The single non-NULL target id. The DB CHECK constraint
    /// guarantees exactly one of the four `target_*_id` columns is
    /// set; this helper picks it without forcing every caller to
    /// repeat the match.
    pub fn target_id(&self) -> Option<Uuid> {
        match self.kind {
            TagLinkKind::Repo => self.target_repo_id,
            TagLinkKind::Issue => self.target_issue_id,
            TagLinkKind::User => self.target_user_id,
            TagLinkKind::Team => self.target_team_id,
            TagLinkKind::Project => self.target_project_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(kind: TagLinkKind) -> TagLink {
        let id = Uuid::nil();
        let mut l = TagLink {
            id,
            tag_id: id,
            kind,
            target_repo_id: None,
            target_issue_id: None,
            target_user_id: None,
            target_team_id: None,
            target_project_id: None,
            added_by: id,
            added_at: Utc::now(),
        };
        match kind {
            TagLinkKind::Repo => l.target_repo_id = Some(id),
            TagLinkKind::Issue => l.target_issue_id = Some(id),
            TagLinkKind::User => l.target_user_id = Some(id),
            TagLinkKind::Team => l.target_team_id = Some(id),
            TagLinkKind::Project => l.target_project_id = Some(id),
        }
        l
    }

    #[test]
    fn target_id_picks_the_matching_column() {
        for k in [
            TagLinkKind::Repo,
            TagLinkKind::Issue,
            TagLinkKind::User,
            TagLinkKind::Team,
            TagLinkKind::Project,
        ] {
            assert_eq!(sample(k).target_id(), Some(Uuid::nil()));
        }
    }

    #[test]
    fn tag_link_round_trips_through_json() {
        let l = sample(TagLinkKind::Repo);
        let back: TagLink = serde_json::from_str(&serde_json::to_string(&l).unwrap()).unwrap();
        assert_eq!(l, back);
    }

    #[test]
    fn kind_as_str_matches_check_constraint() {
        assert_eq!(TagLinkKind::Repo.as_str(), "repo");
        assert_eq!(TagLinkKind::Issue.as_str(), "issue");
        assert_eq!(TagLinkKind::User.as_str(), "user");
        assert_eq!(TagLinkKind::Team.as_str(), "team");
        assert_eq!(TagLinkKind::Project.as_str(), "project");
    }

    #[test]
    fn issue_kind_does_not_apply_to_commit_metrics() {
        assert!(!TagLinkKind::Issue.applies_to_non_issue_metrics());
        for k in [TagLinkKind::Repo, TagLinkKind::User, TagLinkKind::Team] {
            assert!(k.applies_to_non_issue_metrics());
        }
    }
}
