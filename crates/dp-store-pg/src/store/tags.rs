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

    pub(super) async fn get_tag_impl(&self, id: Uuid) -> Result<Tag, StoreError> {
        let row = sqlx::query(
            "SELECT id, scope_kind, scope_user_id, scope_team_id, scope_org_id, \
                    name, color, description, created_by, created_at, archived_at \
               FROM dp_tags WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_tag(&r),
            None => Err(not_found("tag", id)),
        }
    }

    pub(super) async fn create_tag_impl(&self, tag: &Tag) -> Result<Tag, StoreError> {
        // Derive kv-tag columns from the name per migration 0031's
        // grammar: a colon strictly between other chars = `kv` with
        // `key` = prefix and `value` = suffix (split on first `:`).
        // Without this the row defaults to `kind='single'` and the
        // bucket queries (`AND t.kind = 'kv'`) silently drop links,
        // landing issues under "Uncategorised" even when tagged.
        let (kind, key, value) = parse_tag_name_kv(&tag.name);
        let row = sqlx::query(
            "INSERT INTO dp_tags \
                 (id, scope_kind, scope_user_id, scope_team_id, scope_org_id, \
                  name, color, description, created_by, created_at, archived_at, \
                  kind, key, value) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) \
             RETURNING id, scope_kind, scope_user_id, scope_team_id, scope_org_id, \
                       name, color, description, created_by, created_at, archived_at",
        )
        .bind(tag.id)
        .bind(tag.scope_kind.as_str())
        .bind(tag.scope_user_id)
        .bind(tag.scope_team_id)
        .bind(tag.scope_org_id)
        .bind(&tag.name)
        .bind(&tag.color)
        .bind(&tag.description)
        .bind(tag.created_by)
        .bind(tag.created_at)
        .bind(tag.archived_at)
        .bind(kind)
        .bind(key)
        .bind(value)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_tag(&row)
    }

    pub(super) async fn update_tag_impl(
        &self,
        id: Uuid,
        name: Option<&str>,
        color: Option<&str>,
        description: Option<Option<&str>>,
        archived_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    ) -> Result<Tag, StoreError> {
        // COALESCE-based partial update: each $N is either the new
        // value or NULL-meaning-"unchanged". For nullable columns
        // (description / archived_at) "unchanged" vs "clear" is
        // disambiguated by an explicit `*_set` boolean. Single
        // statement keeps the operation atomic w.r.t. the unique
        // expression index on lower(name).
        let desc_set = description.is_some();
        let desc_val = description.flatten();
        let archived_set = archived_at.is_some();
        let archived_val = archived_at.flatten();
        // Recompute kv columns when the name changes so a rename
        // (`foo` → `category:bar`) doesn't leave a stale `single`
        // row that the bucket queries silently skip.
        let kv = name.map(parse_tag_name_kv);
        let new_kind = kv.as_ref().map(|(k, _, _)| *k);
        let new_key = kv.as_ref().and_then(|(_, k, _)| k.clone());
        let new_value = kv.as_ref().and_then(|(_, _, v)| v.clone());
        let rename = name.is_some();
        let row = sqlx::query(
            "UPDATE dp_tags SET \
                 name        = COALESCE($2, name), \
                 color       = COALESCE($3, color), \
                 description = CASE WHEN $4 THEN $5 ELSE description END, \
                 archived_at = CASE WHEN $6 THEN $7 ELSE archived_at END, \
                 kind        = CASE WHEN $8 THEN $9  ELSE kind  END, \
                 key         = CASE WHEN $8 THEN $10 ELSE key   END, \
                 value       = CASE WHEN $8 THEN $11 ELSE value END \
               WHERE id = $1 \
             RETURNING id, scope_kind, scope_user_id, scope_team_id, scope_org_id, \
                       name, color, description, created_by, created_at, archived_at",
        )
        .bind(id)
        .bind(name)
        .bind(color)
        .bind(desc_set)
        .bind(desc_val)
        .bind(archived_set)
        .bind(archived_val)
        .bind(rename)
        .bind(new_kind)
        .bind(new_key)
        .bind(new_value)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_tag(&r),
            None => Err(not_found("tag", id)),
        }
    }

    pub(super) async fn list_tags_visible_to_impl(
        &self,
        viewer_user_id: Uuid,
        visible_team_ids: &[Uuid],
        visible_org_ids: &[Uuid],
        include_archived: bool,
    ) -> Result<Vec<Tag>, StoreError> {
        // Union the three scope visibility predicates in one query.
        // ANY($) with an empty array is a clean no-match, so empty
        // slices collapse the corresponding branch automatically —
        // no SQL stitching needed.
        let rows = sqlx::query(
            "SELECT id, scope_kind, scope_user_id, scope_team_id, scope_org_id, \
                    name, color, description, created_by, created_at, archived_at \
               FROM dp_tags \
              WHERE ( \
                    (scope_kind = 'user' AND scope_user_id = $1) \
                 OR (scope_kind = 'team' AND scope_team_id = ANY($2)) \
                 OR (scope_kind = 'org'  AND scope_org_id  = ANY($3)) \
              ) \
                AND ($4 OR archived_at IS NULL) \
              ORDER BY lower(name) ASC",
        )
        .bind(viewer_user_id)
        .bind(visible_team_ids)
        .bind(visible_org_ids)
        .bind(include_archived)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_tag).collect()
    }

    pub(super) async fn list_tag_links_impl(
        &self,
        tag_id: Uuid,
        kinds: &[TagLinkKind],
    ) -> Result<Vec<TagLink>, StoreError> {
        // Empty `kinds` slice = "all kinds" per the trait contract.
        // We pass a text array via `ANY($2)` and short-circuit the
        // filter with a $3 boolean so the same prepared statement
        // works for both cases without SQL stitching.
        let kind_strs: Vec<&'static str> = kinds.iter().map(|k| k.as_str()).collect();
        let all_kinds = kinds.is_empty();
        let rows = sqlx::query(
            "SELECT id, tag_id, kind, target_repo_id, target_issue_id, \
                    target_user_id, target_team_id, added_by, added_at \
               FROM dp_tag_links \
              WHERE tag_id = $1 \
                AND ($3 OR kind = ANY($2)) \
              ORDER BY added_at ASC, id ASC",
        )
        .bind(tag_id)
        .bind(&kind_strs)
        .bind(all_kinds)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_tag_link).collect()
    }

    pub(super) async fn list_tag_links_for_targets_impl(
        &self,
        kind: TagLinkKind,
        target_ids: &[Uuid],
    ) -> Result<Vec<TagLink>, StoreError> {
        if target_ids.is_empty() {
            return Ok(Vec::new());
        }
        // Picks the right `target_*_id` column per kind so the
        // existing per-target indexes (`dp_tag_links_target_*_idx`)
        // are hit instead of a seq scan over the polymorphic table.
        let sql = match kind {
            TagLinkKind::Repo => {
                "SELECT id, tag_id, kind, target_repo_id, target_issue_id, \
                        target_user_id, target_team_id, added_by, added_at \
                   FROM dp_tag_links \
                  WHERE kind = 'repo' AND target_repo_id = ANY($1) \
                  ORDER BY added_at ASC, id ASC"
            }
            TagLinkKind::Issue => {
                "SELECT id, tag_id, kind, target_repo_id, target_issue_id, \
                        target_user_id, target_team_id, added_by, added_at \
                   FROM dp_tag_links \
                  WHERE kind = 'issue' AND target_issue_id = ANY($1) \
                  ORDER BY added_at ASC, id ASC"
            }
            TagLinkKind::User => {
                "SELECT id, tag_id, kind, target_repo_id, target_issue_id, \
                        target_user_id, target_team_id, added_by, added_at \
                   FROM dp_tag_links \
                  WHERE kind = 'user' AND target_user_id = ANY($1) \
                  ORDER BY added_at ASC, id ASC"
            }
            TagLinkKind::Team => {
                "SELECT id, tag_id, kind, target_repo_id, target_issue_id, \
                        target_user_id, target_team_id, added_by, added_at \
                   FROM dp_tag_links \
                  WHERE kind = 'team' AND target_team_id = ANY($1) \
                  ORDER BY added_at ASC, id ASC"
            }
        };
        let rows = sqlx::query(sql)
            .bind(target_ids)
            .fetch_all(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        rows.iter().map(row_to_tag_link).collect()
    }

    pub(super) async fn add_tag_links_impl(&self, links: &[TagLink]) -> Result<Vec<TagLink>, StoreError> {
        // Transactional batch (§7.5). The unique index
        // dp_tag_links_tag_target_uniq turns a duplicate insert
        // into SQLSTATE 23505 -> StoreError::Conflict, which the
        // REST layer translates to the per-item batch error. The
        // CHECK on `kind` + matching `target_*_id` is enforced by
        // the migration; we just bind whichever target column the
        // caller populated.
        if links.is_empty() {
            return Ok(Vec::new());
        }
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;
        let mut out = Vec::with_capacity(links.len());
        for l in links {
            let row = sqlx::query(
                "INSERT INTO dp_tag_links \
                     (id, tag_id, kind, target_repo_id, target_issue_id, \
                      target_user_id, target_team_id, added_by, added_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
                 RETURNING id, tag_id, kind, target_repo_id, target_issue_id, \
                           target_user_id, target_team_id, added_by, added_at",
            )
            .bind(l.id)
            .bind(l.tag_id)
            .bind(l.kind.as_str())
            .bind(l.target_repo_id)
            .bind(l.target_issue_id)
            .bind(l.target_user_id)
            .bind(l.target_team_id)
            .bind(l.added_by)
            .bind(l.added_at)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_sqlx)?;
            out.push(row_to_tag_link(&row)?);
        }
        tx.commit().await.map_err(map_sqlx)?;
        Ok(out)
    }

    pub(super) async fn remove_tag_links_impl(&self, link_ids: &[Uuid]) -> Result<(), StoreError> {
        // All-or-nothing per §7.5: missing ids fail the whole
        // batch with NotFound. We do the existence check inside
        // the same tx so a concurrent delete cannot race us into
        // returning success-with-fewer-rows-deleted.
        if link_ids.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;
        let found: (i64,) = sqlx::query_as(
            "SELECT COUNT(*)::BIGINT FROM dp_tag_links WHERE id = ANY($1)",
        )
        .bind(link_ids)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        if (found.0 as usize) != link_ids.len() {
            return Err(not_found("tag_link", "batch"));
        }
        sqlx::query("DELETE FROM dp_tag_links WHERE id = ANY($1)")
            .bind(link_ids)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx)?;
        tx.commit().await.map_err(map_sqlx)?;
        Ok(())
    }

    pub(super) async fn resolve_tag_targets_impl(
        &self,
        tag_ids: &[Uuid],
        visible_repo_ids: &[Uuid],
        visible_user_ids: &[Uuid],
        visible_team_ids: &[Uuid],
    ) -> Result<Vec<TagLink>, StoreError> {
        // §7.7: returns the targets the supplied tags currently
        // link, filtered by the viewer's allow-lists. Issue links
        // pass through unfiltered — issue visibility derives from
        // repo visibility, which the §15.6 report path applies in
        // a second step.
        if tag_ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(
            "SELECT id, tag_id, kind, target_repo_id, target_issue_id, \
                    target_user_id, target_team_id, added_by, added_at \
               FROM dp_tag_links \
              WHERE tag_id = ANY($1) \
                AND ( \
                    (kind = 'repo'  AND target_repo_id  = ANY($2)) \
                 OR (kind = 'user'  AND target_user_id  = ANY($3)) \
                 OR (kind = 'team'  AND target_team_id  = ANY($4)) \
                 OR (kind = 'issue') \
                )",
        )
        .bind(tag_ids)
        .bind(visible_repo_ids)
        .bind(visible_user_ids)
        .bind(visible_team_ids)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_tag_link).collect()
    }
}
