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

    pub(super) async fn list_user_settings_impl(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<UserSetting>, StoreError> {
        let rows = sqlx::query(
            "SELECT user_id, key, value, is_secret, updated_at \
             FROM dp_user_settings WHERE user_id = $1 ORDER BY key ASC",
        )
        .bind(user_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_user_setting).collect()
    }

    pub(super) async fn get_user_setting_impl(
        &self,
        user_id: Uuid,
        key: &str,
    ) -> Result<Option<UserSetting>, StoreError> {
        let row = sqlx::query(
            "SELECT user_id, key, value, is_secret, updated_at \
             FROM dp_user_settings WHERE user_id = $1 AND key = $2",
        )
        .bind(user_id)
        .bind(key)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.as_ref().map(row_to_user_setting).transpose()
    }

    pub(super) async fn upsert_user_setting_impl(
        &self,
        setting: &UserSetting,
    ) -> Result<UserSetting, StoreError> {
        // Upsert: same (user_id, key) replaces value + flips
        // is_secret + stamps updated_at. updated_at is bumped
        // server-side so the caller can't backdate writes.
        let row = sqlx::query(
            "INSERT INTO dp_user_settings \
                 (user_id, key, value, is_secret, updated_at) \
             VALUES ($1, $2, $3, $4, now()) \
             ON CONFLICT (user_id, key) DO UPDATE \
             SET value = EXCLUDED.value, \
                 is_secret = EXCLUDED.is_secret, \
                 updated_at = now() \
             RETURNING user_id, key, value, is_secret, updated_at",
        )
        .bind(setting.user_id)
        .bind(&setting.key)
        .bind(&setting.value)
        .bind(setting.is_secret)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_user_setting(&row)
    }

    pub(super) async fn delete_user_setting_impl(
        &self,
        user_id: Uuid,
        key: &str,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(
            "DELETE FROM dp_user_settings \
             WHERE user_id = $1 AND key = $2",
        )
        .bind(user_id)
        .bind(key)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        if result.rows_affected() == 0 {
            return Err(not_found("user_setting", key));
        }
        Ok(())
    }
}
