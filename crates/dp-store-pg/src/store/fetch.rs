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

    pub(super) async fn get_cursor_impl(
        &self,
        org_id: Uuid,
        repo_id: Option<Uuid>,
        resource_kind: ResourceKind,
    ) -> Result<FetchCursor, StoreError> {
        // `IS NOT DISTINCT FROM` so the NULL repo_id (org-scoped
        // resources) matches the way the unique index does
        // (NULLS NOT DISTINCT).
        let rk_text = resource_kind_to_text(resource_kind);
        let row = sqlx::query(
            "SELECT org_id, repo_id, resource_kind, since, etag, last_event_id, updated_at \
             FROM dp_fetch_cursors \
             WHERE org_id = $1 \
               AND repo_id IS NOT DISTINCT FROM $2 \
               AND resource_kind = $3",
        )
        .bind(org_id)
        .bind(repo_id)
        .bind(rk_text)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_fetch_cursor(&r),
            None => Err(not_found(
                "cursor",
                format!("({org_id}, {repo_id:?}, {rk_text})"),
            )),
        }
    }

    pub(super) async fn put_cursor_impl(&self, cursor: &FetchCursor) -> Result<(), StoreError> {
        // `ON CONFLICT` references the unique constraint columns
        // directly — the runner created it with NULLS NOT DISTINCT
        // so two cursors with the same (org, NULL, kind) collide.
        let rk_text = resource_kind_to_text(cursor.resource_kind);
        sqlx::query(
            "INSERT INTO dp_fetch_cursors \
                 (org_id, repo_id, resource_kind, since, etag, last_event_id, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT (org_id, repo_id, resource_kind) DO UPDATE SET \
                 since         = EXCLUDED.since, \
                 etag          = EXCLUDED.etag, \
                 last_event_id = EXCLUDED.last_event_id, \
                 updated_at    = EXCLUDED.updated_at",
        )
        .bind(cursor.org_id)
        .bind(cursor.repo_id)
        .bind(rk_text)
        .bind(cursor.since)
        .bind(&cursor.etag)
        .bind(&cursor.last_event_id)
        .bind(cursor.updated_at)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    pub(super) async fn start_fetch_run_impl(&self, kind: FetchRunKind) -> Result<Uuid, StoreError> {
        let id = Uuid::new_v4();
        let kind_text = fetch_run_kind_to_text(kind);
        sqlx::query(
            "INSERT INTO dp_fetch_runs (id, kind, started, items, errors, partial) \
             VALUES ($1, $2, NOW(), 0, 0, FALSE)",
        )
        .bind(id)
        .bind(kind_text)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(id)
    }

    pub(super) async fn finish_fetch_run_impl(
        &self,
        id: Uuid,
        items: i64,
        errors: i64,
        partial: bool,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(
            "UPDATE dp_fetch_runs SET \
                 finished = NOW(), items = $2, errors = $3, partial = $4 \
             WHERE id = $1",
        )
        .bind(id)
        .bind(items)
        .bind(errors)
        .bind(partial)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        if result.rows_affected() == 0 {
            return Err(not_found("fetch_run", id));
        }
        Ok(())
    }

    pub(super) async fn record_fetch_run_errors_impl(
        &self,
        id: Uuid,
        samples: &[FetchRunErrorSample],
    ) -> Result<(), StoreError> {
        // Empty input clears the column — callers that find
        // themselves with no samples after a retry get a clean slate
        // rather than a stale partial sample.
        let value: Option<JsonValue> = if samples.is_empty() {
            None
        } else {
            Some(serde_json::to_value(samples).map_err(|e| invalid(e.to_string()))?)
        };
        let result = sqlx::query(
            "UPDATE dp_fetch_runs SET error_sample = $2 WHERE id = $1",
        )
        .bind(id)
        .bind(value)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        if result.rows_affected() == 0 {
            return Err(not_found("fetch_run", id));
        }
        Ok(())
    }

    pub(super) async fn list_recent_fetch_runs_impl(&self, limit: i64) -> Result<Vec<FetchRun>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, kind, started, finished, items, errors, partial, error_sample \
             FROM dp_fetch_runs ORDER BY started DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_fetch_run).collect()
    }

    pub(super) async fn list_fetch_runs_impl(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<FetchRun>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, kind, started, finished, items, errors, partial, error_sample \
             FROM dp_fetch_runs ORDER BY started DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit.max(0))
        .bind(offset.max(0))
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_fetch_run).collect()
    }

    pub(super) async fn data_as_of_impl(&self) -> Result<DataAsOf, StoreError> {
        // Three indexed aggregates dispatched as three small queries
        // rather than one CTE so the row decoders stay obvious. The
        // dp_fetch_runs_started_idx covers the headline `MAX(finished)`
        // probes; the per-org group-by on dp_fetch_cursors is small
        // (one row per (org, repo, resource_kind)) so a seq-scan +
        // hash-agg is fine at the scales TODO §0.1 sizes for.
        let webhook_latest: Option<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT MAX(finished) FROM dp_fetch_runs \
             WHERE kind = $1 AND finished IS NOT NULL",
        )
        .bind(fetch_run_kind_to_text(FetchRunKind::WebhookWorker))
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        let reconciler_latest: Option<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT MAX(finished) FROM dp_fetch_runs \
             WHERE kind = $1 AND finished IS NOT NULL",
        )
        .bind(fetch_run_kind_to_text(FetchRunKind::Reconciler))
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        let cursor_rows = sqlx::query(
            "SELECT org_id, MAX(updated_at) AS latest \
             FROM dp_fetch_cursors \
             GROUP BY org_id",
        )
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        let mut per_org = std::collections::HashMap::with_capacity(cursor_rows.len());
        for r in &cursor_rows {
            let org_id: Uuid = r.try_get("org_id").map_err(map_sqlx)?;
            let latest: DateTime<Utc> = r.try_get("latest").map_err(map_sqlx)?;
            per_org.insert(org_id, latest);
        }

        Ok(DataAsOf {
            webhook_latest,
            reconciler_latest,
            per_org,
        })
    }
}
