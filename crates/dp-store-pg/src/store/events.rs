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

    pub(super) async fn record_audit_log_impl(&self, entry: &AuditEntry) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO dp_audit_log (id, actor_user_id, action, target, at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(entry.id)
        .bind(entry.actor_user_id)
        .bind(&entry.action)
        .bind(&entry.target)
        .bind(entry.at)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    pub(super) async fn record_event_impl(&self, event: &ActivityEvent) -> Result<ActivityEvent, StoreError> {
        let kind_text = event_kind_to_text(event.kind);
        let row = sqlx::query(
            "INSERT INTO dp_activity_events (id, org_id, repo_id, kind, ts, external_id, payload) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT (kind, external_id) DO UPDATE SET \
                 ts      = EXCLUDED.ts, \
                 payload = EXCLUDED.payload \
             RETURNING id, org_id, repo_id, kind, ts, external_id, payload",
        )
        .bind(event.id)
        .bind(event.org_id)
        .bind(event.repo_id)
        .bind(kind_text)
        .bind(event.ts)
        .bind(&event.external_id)
        .bind(&event.payload)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_activity_event(&row)
    }

    pub(super) async fn add_event_actors_impl(&self, actors: &[EventActor]) -> Result<(), StoreError> {
        if actors.is_empty() {
            return Ok(());
        }
        // Batch via UNNEST so the call is one round-trip regardless
        // of fan-out. ON CONFLICT DO NOTHING because the composite
        // PK is the dedupe key — retries are safe.
        let event_ids: Vec<Uuid> = actors.iter().map(|a| a.event_id).collect();
        let user_ids: Vec<Uuid> = actors.iter().map(|a| a.user_id).collect();
        let roles: Vec<String> = actors
            .iter()
            .map(|a| actor_role_to_text(a.role).to_string())
            .collect();
        sqlx::query(
            "INSERT INTO dp_event_actors (event_id, user_id, role) \
             SELECT * FROM UNNEST($1::uuid[], $2::uuid[], $3::text[]) \
             ON CONFLICT (event_id, user_id, role) DO NOTHING",
        )
        .bind(&event_ids)
        .bind(&user_ids)
        .bind(&roles)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    pub(super) async fn list_event_actor_rows_in_window_impl(
        &self,
        window: &Window,
        orgs: &[Uuid],
        repos: &[Uuid],
        users: &[Uuid],
        roles: &[ActorRole],
    ) -> Result<Vec<EventActorRow>, StoreError> {
        // Empty array = "no filter on this dimension"; each predicate
        // short-circuits with `cardinality($N) = 0`. Avoids dynamic
        // SQL building and keeps the prepared-statement cache happy.
        let role_texts: Vec<String> = roles
            .iter()
            .map(|r| actor_role_to_text(*r).to_string())
            .collect();
        let rows = sqlx::query(
            "SELECT ea.event_id, ea.user_id, ea.role, \
                    e.org_id, e.repo_id, e.kind, e.ts \
             FROM dp_event_actors ea \
             JOIN dp_activity_events e ON e.id = ea.event_id \
             WHERE e.ts >= $1 AND e.ts < $2 \
               AND (cardinality($3::uuid[]) = 0 OR e.org_id  = ANY($3)) \
               AND (cardinality($4::uuid[]) = 0 OR e.repo_id = ANY($4)) \
               AND (cardinality($5::uuid[]) = 0 OR ea.user_id = ANY($5)) \
               AND (cardinality($6::text[]) = 0 OR ea.role   = ANY($6)) \
             ORDER BY e.ts",
        )
        .bind(window.start)
        .bind(window.end)
        .bind(orgs)
        .bind(repos)
        .bind(users)
        .bind(&role_texts)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_event_actor_row).collect()
    }

    pub(super) async fn list_event_actor_rows_for_user_page_impl(
        &self,
        user_id: Uuid,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<EventActorRow>, StoreError> {
        // Stable order across pages so the streaming export emits
        // events in deterministic chronological order even when two
        // events share a `ts` (squash-merge + commit at the same
        // instant) — break ties on the event id.
        let rows = sqlx::query(
            "SELECT ea.event_id, ea.user_id, ea.role, \
                    e.org_id, e.repo_id, e.kind, e.ts \
             FROM dp_event_actors ea \
             JOIN dp_activity_events e ON e.id = ea.event_id \
             WHERE ea.user_id = $1 \
             ORDER BY e.ts ASC, ea.event_id ASC \
             LIMIT $2 OFFSET $3",
        )
        .bind(user_id)
        .bind(limit.max(0))
        .bind(offset.max(0))
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_event_actor_row).collect()
    }
}
