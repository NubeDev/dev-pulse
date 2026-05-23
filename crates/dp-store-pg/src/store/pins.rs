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

    pub(super) async fn list_pins_for_user_impl(&self, user_id: Uuid) -> Result<Vec<Pin>, StoreError> {
        let rows = sqlx::query(
            "SELECT user_id, kind, target_id, position, pinned_at \
             FROM dp_user_pins WHERE user_id = $1 ORDER BY position ASC",
        )
        .bind(user_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_pin).collect()
    }

    pub(super) async fn add_pin_impl(&self, pin: &Pin) -> Result<Pin, StoreError> {
        // SCOPE-PROJECTS §13.5 — cap enforcement is the *store*'s
        // responsibility (the REST layer also pre-checks for a nice
        // 400, but a CLI / MCP path that bypasses REST must still
        // hit the cap). Counted inside the same transaction as the
        // insert so a concurrent add can't squeeze past.
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::BIGINT FROM dp_user_pins WHERE user_id = $1",
        )
        .bind(pin.user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        if (count as usize) >= dp_domain::PIN_CAP {
            return Err(StoreError::Invalid(format!(
                "pin cap of {} reached",
                dp_domain::PIN_CAP
            )));
        }
        let row = sqlx::query(
            "INSERT INTO dp_user_pins (user_id, kind, target_id, position, pinned_at) \
             VALUES ($1, $2, $3, $4, $5) \
             RETURNING user_id, kind, target_id, position, pinned_at",
        )
        .bind(pin.user_id)
        .bind(pin.kind.as_str())
        .bind(pin.target_id)
        .bind(pin.position)
        .bind(pin.pinned_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let saved = row_to_pin(&row)?;
        tx.commit().await.map_err(map_sqlx)?;
        Ok(saved)
    }

    pub(super) async fn remove_pin_impl(
        &self,
        user_id: Uuid,
        kind: PinKind,
        target_id: Uuid,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(
            "DELETE FROM dp_user_pins \
             WHERE user_id = $1 AND kind = $2 AND target_id = $3",
        )
        .bind(user_id)
        .bind(kind.as_str())
        .bind(target_id)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        if result.rows_affected() == 0 {
            return Err(not_found(
                "user_pin",
                format!("({user_id}, {}, {target_id})", kind.as_str()),
            ));
        }
        Ok(())
    }

    pub(super) async fn reorder_pins_impl(
        &self,
        user_id: Uuid,
        order: &[(PinKind, Uuid)],
    ) -> Result<(), StoreError> {
        // Atomic rewrite — one transaction, two statements:
        //
        //   1. Read the live `(kind, target_id)` set and verify it
        //      matches `order` exactly. We do this inside the tx so
        //      a concurrent `add_pin` / `remove_pin` can't sneak in
        //      between the check and the rewrite.
        //   2. Walk `order`, issuing per-row `UPDATE … SET position`
        //      statements. Position is NOT unique at the DB level
        //      (§6.3), so we don't have to stage through a sentinel.
        //
        // All inside ONE transaction so a reader can never observe
        // a partial reorder.
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;
        let live_rows = sqlx::query(
            "SELECT kind, target_id FROM dp_user_pins WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let mut live: Vec<(PinKind, Uuid)> = Vec::with_capacity(live_rows.len());
        for r in &live_rows {
            let kt: String = r.try_get("kind").map_err(map_sqlx)?;
            let t: Uuid = r.try_get("target_id").map_err(map_sqlx)?;
            live.push((pin_kind_from_text(&kt)?, t));
        }
        let mut a = live.clone();
        let mut b: Vec<(PinKind, Uuid)> = order.to_vec();
        a.sort_by_key(|(k, t)| (k.as_str(), *t));
        b.sort_by_key(|(k, t)| (k.as_str(), *t));
        if a != b {
            return Err(StoreError::Invalid(
                "reorder set does not match the user's live pins".into(),
            ));
        }
        for (i, (k, t)) in order.iter().enumerate() {
            sqlx::query(
                "UPDATE dp_user_pins SET position = $4 \
                 WHERE user_id = $1 AND kind = $2 AND target_id = $3",
            )
            .bind(user_id)
            .bind(k.as_str())
            .bind(*t)
            .bind(i as i32)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx)?;
        }
        tx.commit().await.map_err(map_sqlx)?;
        Ok(())
    }
}
