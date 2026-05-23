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

    pub(super) async fn list_board_links_impl(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<BoardLink>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT id, project_id, github_board_node_id,
                      github_board_title, github_board_url,
                      github_board_cached_at, start_field_node_id,
                      due_field_node_id, status_field_node_id,
                      last_mirror_at, last_mirror_error,
                      created_by, created_at, updated_at
                 FROM dp_project_board_links
                WHERE project_id = $1
             ORDER BY created_at ASC, id ASC"#,
        )
        .bind(project_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_board_link).collect()
    }

    pub(super) async fn get_board_link_impl(&self, id: Uuid) -> Result<Option<BoardLink>, StoreError> {
        let row = sqlx::query(
            r#"SELECT id, project_id, github_board_node_id,
                      github_board_title, github_board_url,
                      github_board_cached_at, start_field_node_id,
                      due_field_node_id, status_field_node_id,
                      last_mirror_at, last_mirror_error,
                      created_by, created_at, updated_at
                 FROM dp_project_board_links WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.map(|r| row_to_board_link(&r)).transpose()
    }

    pub(super) async fn create_board_link_impl(
        &self,
        upsert: &BoardLinkUpsert,
    ) -> Result<BoardLink, StoreError> {
        // `github_board_cached_at` is stamped to `now()` iff the
        // caller supplied a title or url — i.e. the picker actually
        // resolved fresh display data — so the nightly refresh job
        // knows whether a row needs a backfill or is already fresh.
        let cached_now = upsert.github_board_title.is_some()
            || upsert.github_board_url.is_some();
        let row = sqlx::query(
            r#"INSERT INTO dp_project_board_links
                   (id, project_id, github_board_node_id,
                    github_board_title, github_board_url,
                    github_board_cached_at,
                    start_field_node_id, due_field_node_id,
                    status_field_node_id, created_by,
                    created_at, updated_at)
               VALUES (gen_random_uuid(), $1, $2, $3, $4,
                       CASE WHEN $5 THEN now() ELSE NULL END,
                       $6, $7, $8, $9, now(), now())
               RETURNING id, project_id, github_board_node_id,
                         github_board_title, github_board_url,
                         github_board_cached_at, start_field_node_id,
                         due_field_node_id, status_field_node_id,
                         last_mirror_at, last_mirror_error,
                         created_by, created_at, updated_at"#,
        )
        .bind(upsert.project_id)
        .bind(&upsert.github_board_node_id)
        .bind(upsert.github_board_title.as_deref())
        .bind(upsert.github_board_url.as_deref())
        .bind(cached_now)
        .bind(upsert.start_field_node_id.as_deref())
        .bind(upsert.due_field_node_id.as_deref())
        .bind(upsert.status_field_node_id.as_deref())
        .bind(upsert.created_by)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(|e| match &e {
            // The natural-key UNIQUE collision is the "already
            // linked" case the §7.3 POST handler surfaces as 409.
            sqlx::Error::Database(db)
                if db.constraint().is_some()
                    && db.message().contains("dp_project_board_links")
                    && db.message().contains("github_board_node_id") =>
            {
                StoreError::Conflict(format!(
                    "board already linked to project {}",
                    upsert.project_id
                ))
            }
            _ => map_sqlx(e),
        })?;
        row_to_board_link(&row)
    }

    pub(super) async fn delete_board_link_impl(&self, id: Uuid) -> Result<(), StoreError> {
        let res = sqlx::query("DELETE FROM dp_project_board_links WHERE id = $1")
            .bind(id)
            .execute(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(not_found("board_link", id));
        }
        Ok(())
    }

    pub(super) async fn refresh_board_link_cache_impl(
        &self,
        id: Uuid,
        title: Option<&str>,
        url: Option<&str>,
    ) -> Result<(), StoreError> {
        // COALESCE so a partial refresh (e.g. the picker only
        // resolves the title) does not clobber a previously cached
        // url. Stamping `github_board_cached_at` unconditionally
        // (so long as at least one field was supplied) lets the
        // nightly job tell stale rows apart from rows that have
        // simply never been refreshed.
        if title.is_none() && url.is_none() {
            return Ok(());
        }
        sqlx::query(
            r#"UPDATE dp_project_board_links
                  SET github_board_title     = COALESCE($2, github_board_title),
                      github_board_url       = COALESCE($3, github_board_url),
                      github_board_cached_at = now(),
                      updated_at             = now()
                WHERE id = $1"#,
        )
        .bind(id)
        .bind(title)
        .bind(url)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    pub(super) async fn list_board_items_for_issue_impl(
        &self,
        issue_id: Uuid,
    ) -> Result<Vec<BoardItem>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT link_id, issue_id, item_node_id,
                      last_synced_at, last_error,
                      created_at, updated_at
                 FROM dp_project_board_items
                WHERE issue_id = $1
             ORDER BY created_at ASC, link_id ASC"#,
        )
        .bind(issue_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_board_item).collect()
    }

    pub(super) async fn get_board_item_impl(
        &self,
        link_id: Uuid,
        issue_id: Uuid,
    ) -> Result<Option<BoardItem>, StoreError> {
        let row = sqlx::query(
            r#"SELECT link_id, issue_id, item_node_id,
                      last_synced_at, last_error,
                      created_at, updated_at
                 FROM dp_project_board_items
                WHERE link_id = $1 AND issue_id = $2"#,
        )
        .bind(link_id)
        .bind(issue_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.map(|r| row_to_board_item(&r)).transpose()
    }

    pub(super) async fn record_board_item_result_impl(
        &self,
        link_id: Uuid,
        issue_id: Uuid,
        outcome: BoardItemMirrorOutcome<'_>,
    ) -> Result<(), StoreError> {
        // Per-item upsert + aggregate roll-up in one transaction so
        // the §6.5 `SyncStatus` view can never observe a row whose
        // item state and aggregate state disagree.
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;
        match outcome {
            BoardItemMirrorOutcome::Success { item_node_id } => {
                sqlx::query(
                    r#"INSERT INTO dp_project_board_items
                           (link_id, issue_id, item_node_id,
                            last_synced_at, last_error,
                            created_at, updated_at)
                       VALUES ($1, $2, $3, now(), NULL, now(), now())
                       ON CONFLICT (link_id, issue_id) DO UPDATE SET
                           item_node_id   = EXCLUDED.item_node_id,
                           last_synced_at = now(),
                           last_error     = NULL,
                           updated_at     = now()"#,
                )
                .bind(link_id)
                .bind(issue_id)
                .bind(item_node_id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx)?;
                sqlx::query(
                    r#"UPDATE dp_project_board_links
                          SET last_mirror_at    = now(),
                              last_mirror_error = NULL,
                              updated_at        = now()
                        WHERE id = $1"#,
                )
                .bind(link_id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx)?;
            }
            BoardItemMirrorOutcome::Failure { error } => {
                // A failure-before-success leaves `item_node_id`
                // empty, which would violate the NOT NULL column.
                // Insert a sentinel placeholder so the per-item
                // failure has somewhere to land; the next
                // success-path UPSERT overwrites it with the real
                // node id. The placeholder is not a stable id —
                // `last_synced_at IS NULL` is the signal that no
                // successful mirror has run yet.
                sqlx::query(
                    r#"INSERT INTO dp_project_board_items
                           (link_id, issue_id, item_node_id,
                            last_synced_at, last_error,
                            created_at, updated_at)
                       VALUES ($1, $2, '', NULL, $3, now(), now())
                       ON CONFLICT (link_id, issue_id) DO UPDATE SET
                           last_error = EXCLUDED.last_error,
                           updated_at = now()"#,
                )
                .bind(link_id)
                .bind(issue_id)
                .bind(error)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx)?;
                sqlx::query(
                    r#"UPDATE dp_project_board_links
                          SET last_mirror_error = $2,
                              updated_at        = now()
                        WHERE id = $1"#,
                )
                .bind(link_id)
                .bind(error)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx)?;
            }
        }
        tx.commit().await.map_err(map_sqlx)?;
        Ok(())
    }
}
