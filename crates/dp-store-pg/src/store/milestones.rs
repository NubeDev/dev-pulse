#![allow(unused_imports)]
use std::error::Error as StdError;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dp_domain::audit::AuditEntry;
use dp_domain::event::{ActivityEvent, ActorRole, EventActor};
use dp_domain::fetch::{FetchCursor, FetchRun, FetchRunErrorSample, FetchRunKind, ResourceKind};
use dp_domain::freshness::DataAsOf;
use dp_domain::identity::{IdentityLinkPending, UserIdentity, VerifiedVia};
use dp_domain::inbox::{InboxIssueRow, InboxStatus, UserIssueState};
use dp_domain::membership::Membership;
use dp_domain::milestone::{Milestone, MilestoneState, MilestoneUpsert};
use dp_domain::org::Org;
use dp_domain::pin::{Pin, PinKind};
use dp_domain::repo::Repo;
use dp_domain::setting::UserSetting;
use dp_domain::issue::{Issue, IssueState, IssueUpsert, IssueUpsertOutcome, RepoSummary};
use dp_domain::issue_mutation::{IssueMutation, IssueMutationOp, IssueMutationResult};
use dp_domain::event::EventKind;
use dp_domain::tag::Tag;
use dp_domain::tag_link::{TagLink, TagLinkKind};
use dp_domain::board_link::{
    BoardItem, BoardItemMirrorOutcome, BoardLink, BoardLinkUpsert,
};
use dp_domain::issue_dates::{IssueDates, ProjectV2MirrorTask, ProjectV2MirrorTaskKind};
use dp_domain::project::{
    PortfolioQueryFilter, PortfolioRawRow, Project, ProjectIssueAddOutcome, ProjectIssueAddSkip,
    ProjectListFilter, ProjectRepo, ProjectStatus, ProjectUpsert,
};
use dp_domain::project_view::{
    ProjectView, ProjectViewFilterClause, ProjectViewUpsert, ProjectViewVisibility,
};
use dp_domain::store::{
    EventActorRow, IssueDatesMirrorOutcome, IssueListFilter, IssueMetric, IssueMetricGroupBy,
    IssueMetricRow, IssueMetricsFilter, IssueTimelineRow, PendingRemoteIssue, RepoListFilter,
    RepoSyncStatus, Store, StoreError,
};
use dp_domain::team::Team;
use dp_domain::user::User;
use dp_domain::webhook::WebhookDelivery;
use dp_domain::window::Window;
use serde_json::Value as JsonValue;
use sqlx::Row;
use starter_store_postgres::Pool;
use uuid::Uuid;

use crate::encode::{
    actor_role_from_text, actor_role_to_text, event_kind_from_text, event_kind_to_text,
    tag_link_kind_from_text, tag_scope_kind_from_text,
    fetch_run_kind_from_text, fetch_run_kind_to_text, membership_role_from_text,
    membership_role_to_text, resource_kind_from_text, resource_kind_to_text,
};

use super::{invalid, map_sqlx, not_found, parse_tag_name_kv, PgStore};
use super::rows::*;


impl PgStore {

    pub(super) async fn upsert_milestone_impl(
        &self,
        upsert: &MilestoneUpsert,
    ) -> Result<Milestone, StoreError> {
        // Natural-key upsert on `(repo_id, github_number)`. The
        // surrogate `id` is preserved on conflict so any future FK
        // from `dp_issues.milestone_id` stays stable. Observing the
        // milestone is the strongest evidence it's not missing on
        // the remote, so we always reset `remote_missing_streak`
        // to 0 on upsert.
        let row = sqlx::query(
            "INSERT INTO dp_milestones ( \
                 repo_id, github_number, github_node_id, title, description, \
                 state, due_on, open_issues, closed_issues, \
                 created_at, updated_at, closed_at, \
                 fetched_at, remote_missing_streak \
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, now(), 0) \
             ON CONFLICT (repo_id, github_number) DO UPDATE SET \
                 github_node_id        = EXCLUDED.github_node_id, \
                 title                 = EXCLUDED.title, \
                 description           = EXCLUDED.description, \
                 state                 = EXCLUDED.state, \
                 due_on                = EXCLUDED.due_on, \
                 open_issues           = EXCLUDED.open_issues, \
                 closed_issues         = EXCLUDED.closed_issues, \
                 created_at            = EXCLUDED.created_at, \
                 updated_at            = EXCLUDED.updated_at, \
                 closed_at             = EXCLUDED.closed_at, \
                 fetched_at            = now(), \
                 remote_missing_streak = 0 \
             RETURNING id, repo_id, github_number, github_node_id, title, \
                       description, state, due_on, open_issues, closed_issues, \
                       created_at, updated_at, closed_at, fetched_at, \
                       remote_missing_streak",
        )
        .bind(upsert.repo_id)
        .bind(upsert.github_number)
        .bind(&upsert.github_node_id)
        .bind(&upsert.title)
        .bind(upsert.description.as_deref())
        .bind(upsert.state.as_str())
        .bind(upsert.due_on)
        .bind(upsert.open_issues)
        .bind(upsert.closed_issues)
        .bind(upsert.created_at)
        .bind(upsert.updated_at)
        .bind(upsert.closed_at)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_milestone(&row)
    }

    pub(super) async fn list_milestones_for_repo_impl(
        &self,
        repo_id: Uuid,
        include_closed: bool,
    ) -> Result<Vec<Milestone>, StoreError> {
        // `due_on NULLS LAST` so undated milestones drop to the
        // bottom of the open list (operators care about dated
        // ones first). `github_number ASC` as a stable tie-break.
        let rows = if include_closed {
            sqlx::query(
                "SELECT id, repo_id, github_number, github_node_id, title, \
                        description, state, due_on, open_issues, closed_issues, \
                        created_at, updated_at, closed_at, fetched_at, \
                        remote_missing_streak \
                   FROM dp_milestones \
                  WHERE repo_id = $1 \
                  ORDER BY state ASC, due_on ASC NULLS LAST, github_number ASC",
            )
            .bind(repo_id)
            .fetch_all(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?
        } else {
            sqlx::query(
                "SELECT id, repo_id, github_number, github_node_id, title, \
                        description, state, due_on, open_issues, closed_issues, \
                        created_at, updated_at, closed_at, fetched_at, \
                        remote_missing_streak \
                   FROM dp_milestones \
                  WHERE repo_id = $1 AND state = 'open' \
                  ORDER BY due_on ASC NULLS LAST, github_number ASC",
            )
            .bind(repo_id)
            .fetch_all(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?
        };
        rows.iter().map(row_to_milestone).collect()
    }

    pub(super) async fn list_project_milestones_impl(
        &self,
        project_id: Uuid,
        include_closed: bool,
    ) -> Result<Vec<Milestone>, StoreError> {
        // Join via dp_project_repos so the strip covers every linked
        // repo, then DISTINCT — `(repo_id, github_number)` is the
        // milestone PK already, but the join itself is unique on
        // `(project_id, repo_id)` so this is a defensive no-op.
        // Sort: open first when including closed; due_on ASC NULLS
        // LAST so soonest-due bubbles to the front; title ASC as a
        // stable tie-break across repos that share a milestone name.
        let rows = if include_closed {
            sqlx::query(
                "SELECT m.id, m.repo_id, m.github_number, m.github_node_id, m.title, \
                        m.description, m.state, m.due_on, m.open_issues, m.closed_issues, \
                        m.created_at, m.updated_at, m.closed_at, m.fetched_at, \
                        m.remote_missing_streak \
                   FROM dp_milestones m \
                   JOIN dp_project_repos pr ON pr.repo_id = m.repo_id \
                  WHERE pr.project_id = $1 \
                  ORDER BY m.state ASC, m.due_on ASC NULLS LAST, m.title ASC, \
                           m.github_number ASC",
            )
            .bind(project_id)
            .fetch_all(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?
        } else {
            sqlx::query(
                "SELECT m.id, m.repo_id, m.github_number, m.github_node_id, m.title, \
                        m.description, m.state, m.due_on, m.open_issues, m.closed_issues, \
                        m.created_at, m.updated_at, m.closed_at, m.fetched_at, \
                        m.remote_missing_streak \
                   FROM dp_milestones m \
                   JOIN dp_project_repos pr ON pr.repo_id = m.repo_id \
                  WHERE pr.project_id = $1 AND m.state = 'open' \
                  ORDER BY m.due_on ASC NULLS LAST, m.title ASC, m.github_number ASC",
            )
            .bind(project_id)
            .fetch_all(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?
        };
        rows.iter().map(row_to_milestone).collect()
    }

    pub(super) async fn set_project_primary_milestone_impl(
        &self,
        project_id: Uuid,
        milestone_id: Option<Uuid>,
    ) -> Result<Project, StoreError> {
        // When adopting, validate the milestone belongs to a repo
        // linked to the project. The UI only surfaces eligible
        // milestones; this is the server-side enforcement that
        // resists a stale strip or a hand-rolled API call.
        if let Some(mid) = milestone_id {
            let row: Option<(Uuid,)> = sqlx::query_as(
                r#"SELECT m.id
                     FROM dp_milestones m
                     JOIN dp_project_repos pr ON pr.repo_id = m.repo_id
                    WHERE m.id = $1 AND pr.project_id = $2"#,
            )
            .bind(mid)
            .bind(project_id)
            .fetch_optional(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
            if row.is_none() {
                return Err(invalid(
                    "milestone does not belong to any repo linked to this project",
                ));
            }
        }
        // Bumping `version` keeps any concurrent PATCH callers
        // honest — a stale `expected_version` on the next edit will
        // now 409 instead of silently overwriting.
        let row = sqlx::query(
            r#"UPDATE dp_projects
                  SET primary_milestone_id = $2,
                      version              = version + 1,
                      updated_at           = now()
                WHERE id = $1
               RETURNING id, org_id, name, description, lead_user_id, status,
                         start_at, due_at, issue_count, closed_issue_count,
                         created_by, created_at, updated_at, version,
                         primary_milestone_id"#,
        )
        .bind(project_id)
        .bind(milestone_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_project(&r),
            None => Err(not_found("project", project_id)),
        }
    }

    pub(super) async fn delete_milestone_impl(
        &self,
        milestone_id: Uuid,
    ) -> Result<(), StoreError> {
        // FK `dp_projects.primary_milestone_id` is `ON DELETE SET
        // NULL` (migration 0035), so adopters of this milestone
        // automatically clear without a follow-up UPDATE.
        let result = sqlx::query("DELETE FROM dp_milestones WHERE id = $1")
            .bind(milestone_id)
            .execute(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        if result.rows_affected() == 0 {
            return Err(not_found("milestone", milestone_id));
        }
        Ok(())
    }
}
