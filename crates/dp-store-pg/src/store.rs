//! [`PgStore`] — the `dp-domain::Store` implementation backed by
//! Postgres via `starter_store_postgres::Pool`.
//!
//! Every method here is a thin SQL body. Behaviour notes worth
//! knowing before changing them:
//!
//! * Upserts use `ON CONFLICT … DO UPDATE` on the GitHub-id columns
//!   so the fetcher can replay without growing duplicates.
//! * `upsert_membership` deliberately does **not** clobber
//!   `home_org`. The schema invariant (TODO §0.5) is that home-org is
//!   only ever written through `set_home_org`; the upsert path keeps
//!   the existing value via `COALESCE(EXCLUDED.home_org, dp_memberships.home_org)`.
//! * `add_event_actors` `INSERT … ON CONFLICT DO NOTHING` on the
//!   composite PK so partial batches are safe to retry.
//! * `enqueue_webhook` surfaces the unique-violation on `delivery_id`
//!   as [`StoreError::Conflict`] so the receiver can translate it to
//!   `200 OK` (idempotent replays — TODO §0.1).
//! * `claim_webhooks` uses `FOR UPDATE SKIP LOCKED` so multiple
//!   workers don't fight over the same row.
//! * Closed enums (`ActorRole`, `EventKind`, …) round-trip via the
//!   helpers in [`crate::encode`] so the column matches the JSON wire
//!   form one-for-one.

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

/// Postgres-backed [`Store`].
///
/// Cloneable: the underlying [`Pool`] is a wrapper around
/// `Arc<PgPool>` so cloning is cheap and every surface holding an
/// `Arc<dyn Store>` shares the same pool.
#[derive(Clone)]
pub struct PgStore {
    pool: Pool,
}

impl PgStore {
    /// Wrap a pre-built [`Pool`]. Construction is the consumer's
    /// problem (they pick max-connections, the URL, etc. via
    /// `starter_store_postgres::pool::connect`).
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    /// Borrow the underlying pool — handy for one-off queries from
    /// adjacent crates that need raw SQL but should not own the
    /// `PgStore`.
    pub fn pool(&self) -> &Pool {
        &self.pool
    }
}

// ---------- error mapping -------------------------------------------

/// Map a `sqlx::Error` into the most accurate `StoreError` variant.
///
/// * Unique-violation (PG SQLSTATE `23505`) becomes
///   [`StoreError::Conflict`] so the webhook receiver can recognise
///   replays and the upsert path can recognise concurrent inserts.
/// * `RowNotFound` becomes [`StoreError::NotFound`] (the caller's
///   `entity`/`id` is set by the helper, see [`not_found`]).
/// * Everything else is boxed into [`StoreError::Backend`].
/// Split a tag `name` into `(kind, key, value)` per migration
/// 0031's grammar:
///
///   * Colon strictly between other chars → `kv`, split on the
///     FIRST `:` (so `team:backend:v2` = key `team`, value
///     `backend:v2`).
///   * Otherwise → `single`, with NULL key/value.
///
/// Mirrored from the migration's backfill UPDATE so create-tag
/// produces rows that pass the `dp_tags_kind_kv_invariant` check
/// constraint.
fn parse_tag_name_kv(name: &str) -> (&'static str, Option<String>, Option<String>) {
    if let Some(pos) = name.find(':') {
        if pos > 0 && pos + 1 < name.len() {
            return (
                "kv",
                Some(name[..pos].to_owned()),
                Some(name[pos + 1..].to_owned()),
            );
        }
    }
    ("single", None, None)
}

fn map_sqlx(err: sqlx::Error) -> StoreError {
    if let sqlx::Error::Database(db) = &err {
        if db.code().as_deref() == Some("23505") {
            return StoreError::Conflict(db.message().to_string());
        }
    }
    StoreError::Backend(Box::new(err))
}

fn not_found(entity: &'static str, id: impl ToString) -> StoreError {
    StoreError::NotFound {
        entity,
        id: id.to_string(),
    }
}

fn invalid(msg: impl Into<String>) -> StoreError {
    let m: String = msg.into();
    let e: Box<dyn StdError + Send + Sync> = m.into();
    StoreError::Backend(e)
}

fn project_view_from_row(
    r: &sqlx::postgres::PgRow,
) -> Result<ProjectView, StoreError> {
    let filter_json: serde_json::Value = r.try_get("filter_json").map_err(map_sqlx)?;
    let filter_clauses: Vec<ProjectViewFilterClause> =
        serde_json::from_value(filter_json).map_err(|e| {
            StoreError::Invalid(format!("filter_json decode: {e}"))
        })?;
    let categories_json: serde_json::Value = r.try_get("categories").map_err(map_sqlx)?;
    let categories: Vec<String> =
        serde_json::from_value(categories_json).map_err(|e| {
            StoreError::Invalid(format!("categories decode: {e}"))
        })?;
    let visibility_text: String = r.try_get("visibility").map_err(map_sqlx)?;
    let visibility = ProjectViewVisibility::from_str(&visibility_text)
        .ok_or_else(|| {
            StoreError::Invalid(format!("unknown view visibility: {visibility_text}"))
        })?;
    Ok(ProjectView {
        id: r.try_get("id").map_err(map_sqlx)?,
        project_id: r.try_get("project_id").map_err(map_sqlx)?,
        owner_user_id: r.try_get("owner_user_id").map_err(map_sqlx)?,
        name: r.try_get("name").map_err(map_sqlx)?,
        group_by: r.try_get("group_by").map_err(map_sqlx)?,
        filter_clauses,
        sort: r.try_get("sort").map_err(map_sqlx)?,
        position: r.try_get("position").map_err(map_sqlx)?,
        visibility,
        start_date: r.try_get("start_date").map_err(map_sqlx)?,
        due_date: r.try_get("due_date").map_err(map_sqlx)?,
        categories,
        created_at: r.try_get("created_at").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
    })
}

// ---------- row decoders --------------------------------------------

fn row_to_user(r: &sqlx::postgres::PgRow) -> Result<User, StoreError> {
    Ok(User {
        id: r.try_get("id").map_err(map_sqlx)?,
        github_id: r.try_get("github_id").map_err(map_sqlx)?,
        login: r.try_get("login").map_err(map_sqlx)?,
        email: r.try_get("email").map_err(map_sqlx)?,
        name: r.try_get("name").map_err(map_sqlx)?,
        deleted_at: r.try_get("deleted_at").map_err(map_sqlx)?,
    })
}

fn row_to_user_identity(
    r: &sqlx::postgres::PgRow,
) -> Result<UserIdentity, StoreError> {
    let via_text: String = r.try_get("verified_via").map_err(map_sqlx)?;
    let verified_via = VerifiedVia::from_str(&via_text).ok_or_else(|| {
        StoreError::Invalid(format!("unknown verified_via: {via_text}"))
    })?;
    Ok(UserIdentity {
        user_id: r.try_get("user_id").map_err(map_sqlx)?,
        github_user_id: r.try_get("github_user_id").map_err(map_sqlx)?,
        github_login: r.try_get("github_login").map_err(map_sqlx)?,
        is_primary: r.try_get("is_primary").map_err(map_sqlx)?,
        linked_at: r.try_get("linked_at").map_err(map_sqlx)?,
        verified_via,
    })
}

fn row_to_identity_link_pending(
    r: &sqlx::postgres::PgRow,
) -> Result<IdentityLinkPending, StoreError> {
    Ok(IdentityLinkPending {
        nonce: r.try_get("nonce").map_err(map_sqlx)?,
        dp_user_id: r.try_get("dp_user_id").map_err(map_sqlx)?,
        session_id: r.try_get("session_id").map_err(map_sqlx)?,
        created_at: r.try_get("created_at").map_err(map_sqlx)?,
        expires_at: r.try_get("expires_at").map_err(map_sqlx)?,
    })
}

fn row_to_org(r: &sqlx::postgres::PgRow) -> Result<Org, StoreError> {
    Ok(Org {
        id: r.try_get("id").map_err(map_sqlx)?,
        github_id: r.try_get("github_id").map_err(map_sqlx)?,
        login: r.try_get("login").map_err(map_sqlx)?,
        name: r.try_get("name").map_err(map_sqlx)?,
    })
}

fn row_to_team(r: &sqlx::postgres::PgRow) -> Result<Team, StoreError> {
    Ok(Team {
        id: r.try_get("id").map_err(map_sqlx)?,
        org_id: r.try_get("org_id").map_err(map_sqlx)?,
        github_id: r.try_get("github_id").map_err(map_sqlx)?,
        slug: r.try_get("slug").map_err(map_sqlx)?,
        name: r.try_get("name").map_err(map_sqlx)?,
    })
}

fn row_to_repo(r: &sqlx::postgres::PgRow) -> Result<Repo, StoreError> {
    Ok(Repo {
        id: r.try_get("id").map_err(map_sqlx)?,
        org_id: r.try_get("org_id").map_err(map_sqlx)?,
        github_id: r.try_get("github_id").map_err(map_sqlx)?,
        name: r.try_get("name").map_err(map_sqlx)?,
    })
}

fn row_to_repo_metadata(
    r: &sqlx::postgres::PgRow,
) -> Result<dp_domain::RepoMetadata, StoreError> {
    Ok(dp_domain::RepoMetadata {
        repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
        stars: r.try_get("stars").map_err(map_sqlx)?,
        forks: r.try_get("forks").map_err(map_sqlx)?,
        watchers: r.try_get("watchers").map_err(map_sqlx)?,
        open_issues_remote: r.try_get("open_issues_remote").map_err(map_sqlx)?,
        primary_language: r.try_get("primary_language").map_err(map_sqlx)?,
        default_branch: r.try_get("default_branch").map_err(map_sqlx)?,
        description: r.try_get("description").map_err(map_sqlx)?,
        homepage: r.try_get("homepage").map_err(map_sqlx)?,
        is_archived: r.try_get("is_archived").map_err(map_sqlx)?,
        is_fork: r.try_get("is_fork").map_err(map_sqlx)?,
        is_private: r.try_get("is_private").map_err(map_sqlx)?,
        pushed_at: r.try_get("pushed_at").map_err(map_sqlx)?,
        metadata_updated_at: r.try_get("metadata_updated_at").map_err(map_sqlx)?,
    })
}

fn row_to_repo_summary(r: &sqlx::postgres::PgRow) -> Result<RepoSummary, StoreError> {
    Ok(RepoSummary {
        id: r.try_get("id").map_err(map_sqlx)?,
        org_id: r.try_get("org_id").map_err(map_sqlx)?,
        org_login: r.try_get("org_login").map_err(map_sqlx)?,
        name: r.try_get("name").map_err(map_sqlx)?,
        open_issue_count: r.try_get("open_issue_count").map_err(map_sqlx)?,
        last_activity_at: r.try_get("last_activity_at").map_err(map_sqlx)?,
    })
}

/// Build a JSONB containment array for the `labels` / `assignees`
/// AND filter (`IssueListFilter::labels` / `::assignees`). Returns
/// `None` for an empty input so the bind site can pass `NULL` and
/// the SQL guard (`$N::jsonb IS NULL OR …`) skips the containment
/// check. Non-empty inputs serialise to `JsonValue::Array(strings)`
/// so the PG side can use `column @> $N::jsonb`.
fn labels_or_assignees_json(values: &[String]) -> Option<JsonValue> {
    if values.is_empty() {
        return None;
    }
    Some(JsonValue::Array(
        values
            .iter()
            .map(|s| JsonValue::String(s.clone()))
            .collect(),
    ))
}

/// One-line `payload_summary` for issue-timeline rows. The text
/// is intentionally compact — the frontend prepends an icon /
/// actor list, so we just describe the change. Falls back to the
/// kind label if the payload doesn't carry an obvious summary
/// field.
fn summarise_timeline_payload(kind: EventKind, payload: &JsonValue) -> String {
    match kind {
        EventKind::IssueOpened => "opened the issue".to_string(),
        EventKind::IssueClosed => match payload.get("state_reason").and_then(|v| v.as_str()) {
            Some("not_planned") => "closed as not planned".to_string(),
            Some("completed") => "closed as completed".to_string(),
            _ => "closed the issue".to_string(),
        },
        EventKind::IssueComment => {
            let body = payload
                .get("body")
                .and_then(|v| v.as_str())
                .or_else(|| payload.get("body_excerpt").and_then(|v| v.as_str()))
                .unwrap_or("");
            let trimmed: String = body.chars().take(120).collect();
            if trimmed.is_empty() {
                "commented".to_string()
            } else {
                format!("commented: {trimmed}")
            }
        }
        _ => format!("{:?}", kind),
    }
}

fn row_to_issue(r: &sqlx::postgres::PgRow) -> Result<Issue, StoreError> {
    let state_text: String = r.try_get("state").map_err(map_sqlx)?;
    let state = IssueState::from_str(&state_text)
        .ok_or_else(|| StoreError::Invalid(format!("unknown issue state: {state_text}")))?;
    let labels_json: JsonValue = r.try_get("labels").map_err(map_sqlx)?;
    let assignees_json: JsonValue = r.try_get("assignees").map_err(map_sqlx)?;
    let labels: Vec<String> = serde_json::from_value(labels_json)
        .map_err(|e| StoreError::Invalid(format!("labels not a string array: {e}")))?;
    let assignees: Vec<String> = serde_json::from_value(assignees_json)
        .map_err(|e| StoreError::Invalid(format!("assignees not a string array: {e}")))?;
    Ok(Issue {
        id: r.try_get("id").map_err(map_sqlx)?,
        org_id: r.try_get("org_id").map_err(map_sqlx)?,
        repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
        github_id: r.try_get("github_id").map_err(map_sqlx)?,
        number: r.try_get("number").map_err(map_sqlx)?,
        title: r.try_get("title").map_err(map_sqlx)?,
        body: r.try_get("body").map_err(map_sqlx)?,
        state,
        labels,
        assignees,
        milestone: r.try_get("milestone").map_err(map_sqlx)?,
        version: r.try_get("version").map_err(map_sqlx)?,
        github_node_id: r.try_get("github_node_id").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
        // Tolerate SELECT lists that pre-date the 0041 migration —
        // they simply default to non-local, which is correct for
        // every row created before the feature shipped.
        is_local: r.try_get("is_local").unwrap_or(false),
    })
}

fn row_to_user_issue_state(
    r: &sqlx::postgres::PgRow,
) -> Result<UserIssueState, StoreError> {
    let status_text: String = r.try_get("status").map_err(map_sqlx)?;
    let status = InboxStatus::from_str(&status_text)
        .ok_or_else(|| StoreError::Invalid(format!("unknown inbox status: {status_text}")))?;
    Ok(UserIssueState {
        user_id: r.try_get("user_id").map_err(map_sqlx)?,
        issue_id: r.try_get("issue_id").map_err(map_sqlx)?,
        last_seen_version: r.try_get("last_seen_version").map_err(map_sqlx)?,
        status,
        snoozed_until: r.try_get("snoozed_until").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
    })
}

fn row_to_inbox_issue_row(
    r: &sqlx::postgres::PgRow,
) -> Result<InboxIssueRow, StoreError> {
    let issue = row_to_issue(r)?;
    let last_seen_version: i64 = r.try_get("last_seen_version").map_err(map_sqlx)?;
    let status_text: String = r.try_get("inbox_status").map_err(map_sqlx)?;
    let status = InboxStatus::from_str(&status_text)
        .ok_or_else(|| StoreError::Invalid(format!("unknown inbox status: {status_text}")))?;
    let snoozed_until = r.try_get("snoozed_until").map_err(map_sqlx)?;
    Ok(InboxIssueRow {
        unread: issue.version > last_seen_version,
        issue,
        status,
        snoozed_until,
    })
}

fn row_to_membership(r: &sqlx::postgres::PgRow) -> Result<Membership, StoreError> {
    let role_text: String = r.try_get("role").map_err(map_sqlx)?;
    Ok(Membership {
        user_id: r.try_get("user_id").map_err(map_sqlx)?,
        org_id: r.try_get("org_id").map_err(map_sqlx)?,
        role: membership_role_from_text(&role_text),
        home_org: r.try_get("home_org").map_err(map_sqlx)?,
        joined_at: r.try_get("joined_at").map_err(map_sqlx)?,
    })
}

fn row_to_activity_event(r: &sqlx::postgres::PgRow) -> Result<ActivityEvent, StoreError> {
    let kind_text: String = r.try_get("kind").map_err(map_sqlx)?;
    let kind = event_kind_from_text(&kind_text).map_err(invalid)?;
    Ok(ActivityEvent {
        id: r.try_get("id").map_err(map_sqlx)?,
        org_id: r.try_get("org_id").map_err(map_sqlx)?,
        repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
        kind,
        ts: r.try_get("ts").map_err(map_sqlx)?,
        external_id: r.try_get("external_id").map_err(map_sqlx)?,
        payload: r.try_get::<JsonValue, _>("payload").map_err(map_sqlx)?,
    })
}

fn row_to_fetch_run(r: &sqlx::postgres::PgRow) -> Result<FetchRun, StoreError> {
    let kind_text: String = r.try_get("kind").map_err(map_sqlx)?;
    let kind = fetch_run_kind_from_text(&kind_text).map_err(invalid)?;
    Ok(FetchRun {
        id: r.try_get("id").map_err(map_sqlx)?,
        kind,
        started: r.try_get("started").map_err(map_sqlx)?,
        finished: r.try_get("finished").map_err(map_sqlx)?,
        items: r.try_get("items").map_err(map_sqlx)?,
        errors: r.try_get("errors").map_err(map_sqlx)?,
        partial: r.try_get("partial").map_err(map_sqlx)?,
        error_sample: r
            .try_get::<Option<JsonValue>, _>("error_sample")
            .map_err(map_sqlx)?
            .map(|v| serde_json::from_value(v).map_err(|e| invalid(e.to_string())))
            .transpose()?,
    })
}

fn row_to_fetch_cursor(r: &sqlx::postgres::PgRow) -> Result<FetchCursor, StoreError> {
    let rk_text: String = r.try_get("resource_kind").map_err(map_sqlx)?;
    let resource_kind = resource_kind_from_text(&rk_text).map_err(invalid)?;
    Ok(FetchCursor {
        org_id: r.try_get("org_id").map_err(map_sqlx)?,
        repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
        resource_kind,
        since: r.try_get("since").map_err(map_sqlx)?,
        etag: r.try_get("etag").map_err(map_sqlx)?,
        last_event_id: r.try_get("last_event_id").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
    })
}

fn row_to_webhook_delivery(r: &sqlx::postgres::PgRow) -> Result<WebhookDelivery, StoreError> {
    Ok(WebhookDelivery {
        id: r.try_get("id").map_err(map_sqlx)?,
        delivery_id: r.try_get("delivery_id").map_err(map_sqlx)?,
        event: r.try_get("event").map_err(map_sqlx)?,
        payload: r.try_get::<JsonValue, _>("payload").map_err(map_sqlx)?,
        received_at: r.try_get("received_at").map_err(map_sqlx)?,
        processed_at: r.try_get("processed_at").map_err(map_sqlx)?,
        error: r.try_get("error").map_err(map_sqlx)?,
    })
}

fn pin_kind_from_text(s: &str) -> Result<PinKind, StoreError> {
    match s {
        "repo" => Ok(PinKind::Repo),
        "tag" => Ok(PinKind::Tag),
        other => Err(invalid(format!("unknown pin kind {other:?}"))),
    }
}

fn row_to_pin(r: &sqlx::postgres::PgRow) -> Result<Pin, StoreError> {
    let kind_text: String = r.try_get("kind").map_err(map_sqlx)?;
    Ok(Pin {
        user_id: r.try_get("user_id").map_err(map_sqlx)?,
        kind: pin_kind_from_text(&kind_text)?,
        target_id: r.try_get("target_id").map_err(map_sqlx)?,
        position: r.try_get("position").map_err(map_sqlx)?,
        pinned_at: r.try_get("pinned_at").map_err(map_sqlx)?,
    })
}

fn row_to_user_setting(r: &sqlx::postgres::PgRow) -> Result<UserSetting, StoreError> {
    Ok(UserSetting {
        user_id: r.try_get("user_id").map_err(map_sqlx)?,
        key: r.try_get("key").map_err(map_sqlx)?,
        value: r.try_get("value").map_err(map_sqlx)?,
        is_secret: r.try_get("is_secret").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
    })
}

fn row_to_event_actor_row(r: &sqlx::postgres::PgRow) -> Result<EventActorRow, StoreError> {
    let role_text: String = r.try_get("role").map_err(map_sqlx)?;
    let kind_text: String = r.try_get("kind").map_err(map_sqlx)?;
    Ok(EventActorRow {
        event_id: r.try_get("event_id").map_err(map_sqlx)?,
        user_id: r.try_get("user_id").map_err(map_sqlx)?,
        role: actor_role_from_text(&role_text).map_err(invalid)?,
        org_id: r.try_get("org_id").map_err(map_sqlx)?,
        repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
        kind: event_kind_from_text(&kind_text).map_err(invalid)?,
        ts: r.try_get("ts").map_err(map_sqlx)?,
    })
}

fn row_to_tag(r: &sqlx::postgres::PgRow) -> Result<Tag, StoreError> {
    let scope_text: String = r.try_get("scope_kind").map_err(map_sqlx)?;
    Ok(Tag {
        id: r.try_get("id").map_err(map_sqlx)?,
        scope_kind: tag_scope_kind_from_text(&scope_text).map_err(invalid)?,
        scope_user_id: r.try_get("scope_user_id").map_err(map_sqlx)?,
        scope_team_id: r.try_get("scope_team_id").map_err(map_sqlx)?,
        scope_org_id: r.try_get("scope_org_id").map_err(map_sqlx)?,
        name: r.try_get("name").map_err(map_sqlx)?,
        color: r.try_get("color").map_err(map_sqlx)?,
        description: r.try_get("description").map_err(map_sqlx)?,
        created_by: r.try_get("created_by").map_err(map_sqlx)?,
        created_at: r.try_get("created_at").map_err(map_sqlx)?,
        archived_at: r.try_get("archived_at").map_err(map_sqlx)?,
    })
}

fn row_to_tag_link(r: &sqlx::postgres::PgRow) -> Result<TagLink, StoreError> {
    let kind_text: String = r.try_get("kind").map_err(map_sqlx)?;
    Ok(TagLink {
        id: r.try_get("id").map_err(map_sqlx)?,
        tag_id: r.try_get("tag_id").map_err(map_sqlx)?,
        kind: tag_link_kind_from_text(&kind_text).map_err(invalid)?,
        target_repo_id: r.try_get("target_repo_id").map_err(map_sqlx)?,
        target_issue_id: r.try_get("target_issue_id").map_err(map_sqlx)?,
        target_user_id: r.try_get("target_user_id").map_err(map_sqlx)?,
        target_team_id: r.try_get("target_team_id").map_err(map_sqlx)?,
        added_by: r.try_get("added_by").map_err(map_sqlx)?,
        added_at: r.try_get("added_at").map_err(map_sqlx)?,
    })
}

fn row_to_milestone(r: &sqlx::postgres::PgRow) -> Result<Milestone, StoreError> {
    let state_text: String = r.try_get("state").map_err(map_sqlx)?;
    let state = MilestoneState::from_str(&state_text)
        .ok_or_else(|| StoreError::Invalid(format!("unknown milestone state {state_text:?}")))?;
    Ok(Milestone {
        id: r.try_get("id").map_err(map_sqlx)?,
        repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
        github_number: r.try_get("github_number").map_err(map_sqlx)?,
        github_node_id: r.try_get("github_node_id").map_err(map_sqlx)?,
        title: r.try_get("title").map_err(map_sqlx)?,
        description: r.try_get("description").map_err(map_sqlx)?,
        state,
        due_on: r.try_get("due_on").map_err(map_sqlx)?,
        open_issues: r.try_get("open_issues").map_err(map_sqlx)?,
        closed_issues: r.try_get("closed_issues").map_err(map_sqlx)?,
        created_at: r.try_get("created_at").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
        closed_at: r.try_get("closed_at").map_err(map_sqlx)?,
        fetched_at: r.try_get("fetched_at").map_err(map_sqlx)?,
        remote_missing_streak: r.try_get("remote_missing_streak").map_err(map_sqlx)?,
    })
}

// ---------- Store impl ----------------------------------------------

#[async_trait]
impl Store for PgStore {
    // ---- users -----------------------------------------------------

    async fn upsert_user(&self, user: &User) -> Result<User, StoreError> {
        let row = sqlx::query(
            "INSERT INTO dp_users (id, github_id, login, email, name, deleted_at) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (github_id) DO UPDATE SET \
                 login      = EXCLUDED.login, \
                 email      = EXCLUDED.email, \
                 name       = EXCLUDED.name, \
                 deleted_at = EXCLUDED.deleted_at \
             RETURNING id, github_id, login, email, name, deleted_at",
        )
        .bind(user.id)
        .bind(user.github_id)
        .bind(&user.login)
        .bind(&user.email)
        .bind(&user.name)
        .bind(user.deleted_at)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_user(&row)
    }

    async fn get_user(&self, id: Uuid) -> Result<User, StoreError> {
        let row = sqlx::query(
            "SELECT id, github_id, login, email, name, deleted_at \
             FROM dp_users WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_user(&r),
            None => Err(not_found("user", id)),
        }
    }

    async fn get_user_by_github_id(&self, github_id: i64) -> Result<User, StoreError> {
        let row = sqlx::query(
            "SELECT id, github_id, login, email, name, deleted_at \
             FROM dp_users WHERE github_id = $1",
        )
        .bind(github_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_user(&r),
            None => Err(not_found("user", github_id)),
        }
    }

    async fn list_users(&self) -> Result<Vec<User>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, github_id, login, email, name, deleted_at \
             FROM dp_users WHERE deleted_at IS NULL ORDER BY login",
        )
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_user).collect()
    }

    async fn find_user_by_login(&self, login: &str) -> Result<Option<User>, StoreError> {
        // Prefer the row with a real (positive) github_id when both
        // a synthetic (negative) trailer row and the real row exist
        // for the same login — the trailer path uses this to fold
        // future events onto the canonical row. Match case-insensitively
        // (GitHub logins are) and prefer the *lowest* positive github_id
        // (oldest real GitHub account) so this agrees with the
        // canonical-row rule in migration 0003.
        let row = sqlx::query(
            "SELECT id, github_id, login, email, name, deleted_at \
             FROM dp_users \
             WHERE lower(login) = lower($1) AND deleted_at IS NULL \
             ORDER BY (github_id >= 0) DESC, github_id ASC \
             LIMIT 1",
        )
        .bind(login)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_user(&r).map(Some),
            None => Ok(None),
        }
    }

    async fn pseudonymise_user(&self, id: Uuid) -> Result<(), StoreError> {
        // Rewrite to a stable `deleted-user-<short-id>` form. The
        // hash is derived from the row id so re-running this is a
        // no-op (idempotent) and two different users never collide.
        let short = id.simple().to_string();
        let short = &short[..16];
        let login = format!("deleted-user-{short}");
        let result = sqlx::query(
            "UPDATE dp_users SET \
                 login      = $2, \
                 email      = NULL, \
                 name       = NULL, \
                 deleted_at = COALESCE(deleted_at, NOW()) \
             WHERE id = $1",
        )
        .bind(id)
        .bind(&login)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        if result.rows_affected() == 0 {
            return Err(not_found("user", id));
        }
        Ok(())
    }

    // ---- identities (users.md §4 Slice A) --------------------------

    async fn list_identities_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<UserIdentity>, StoreError> {
        // Primary first, then newest link first. Ties on linked_at
        // break by github_user_id for a deterministic order under
        // CI fixture clock skew.
        let rows = sqlx::query(
            "SELECT user_id, github_user_id, github_login, is_primary, \
                    linked_at, verified_via \
             FROM dp_user_identities \
             WHERE user_id = $1 \
             ORDER BY is_primary DESC, linked_at DESC, github_user_id ASC",
        )
        .bind(user_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_user_identity).collect()
    }

    async fn find_user_by_github_user_id(
        &self,
        github_user_id: i64,
    ) -> Result<Option<User>, StoreError> {
        let row = sqlx::query(
            "SELECT u.id, u.github_id, u.login, u.email, u.name, u.deleted_at \
             FROM dp_user_identities i \
             JOIN dp_users u ON u.id = i.user_id \
             WHERE i.github_user_id = $1 AND u.deleted_at IS NULL",
        )
        .bind(github_user_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_user(&r).map(Some),
            None => Ok(None),
        }
    }

    async fn create_identity_link_pending(
        &self,
        pending: &IdentityLinkPending,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO dp_identity_link_pending \
                 (nonce, dp_user_id, session_id, created_at, expires_at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(pending.nonce)
        .bind(pending.dp_user_id)
        .bind(&pending.session_id)
        .bind(pending.created_at)
        .bind(pending.expires_at)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    async fn consume_identity_link_pending(
        &self,
        nonce: Uuid,
    ) -> Result<Option<IdentityLinkPending>, StoreError> {
        // RETURNING on DELETE atomically reads + removes the row
        // so a replayed callback cannot consume the same nonce twice.
        let row = sqlx::query(
            "DELETE FROM dp_identity_link_pending \
             WHERE nonce = $1 \
             RETURNING nonce, dp_user_id, session_id, created_at, expires_at",
        )
        .bind(nonce)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => Ok(Some(row_to_identity_link_pending(&r)?)),
            None => Ok(None),
        }
    }

    async fn purge_expired_identity_link_pending(
        &self,
        now: DateTime<Utc>,
    ) -> Result<u64, StoreError> {
        let result = sqlx::query(
            "DELETE FROM dp_identity_link_pending WHERE expires_at < $1",
        )
        .bind(now)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(result.rows_affected())
    }

    async fn link_identity(
        &self,
        identity: &UserIdentity,
    ) -> Result<UserIdentity, StoreError> {
        // One transaction so the "first identity is primary"
        // promotion and the insert can never tear: a concurrent
        // writer either sees zero rows (and also becomes primary)
        // or sees the new row.
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;

        // Ensure the target dp-user actually exists; the FK would
        // catch this too but the NotFound is friendlier.
        let user_exists: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM dp_users WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(identity.user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        if user_exists.is_none() {
            return Err(not_found("user", identity.user_id));
        }

        // Reject if any other dp-user already claims this
        // github_user_id. We surface a Conflict so the handler can
        // emit IDENTITY_CLAIM_CONFLICT + HTTP 409. (The UNIQUE
        // constraint also catches this on INSERT; checking here
        // makes the error path deterministic regardless of which
        // dp-user wins the race.)
        let claimed_by: Option<Uuid> = sqlx::query_scalar(
            "SELECT user_id FROM dp_user_identities WHERE github_user_id = $1",
        )
        .bind(identity.github_user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        if let Some(owner) = claimed_by {
            if owner != identity.user_id {
                return Err(StoreError::Conflict(format!(
                    "github_user_id {} is already claimed by another dp-user",
                    identity.github_user_id
                )));
            }
        }

        // The first identity for a user is always primary, even
        // if the caller passed `is_primary = false`. Otherwise we
        // honour the caller's choice; if they pass `true` we flip
        // every other row for the user to FALSE first to keep the
        // partial unique index happy.
        let existing_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM dp_user_identities WHERE user_id = $1",
        )
        .bind(identity.user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let effective_primary = identity.is_primary || existing_count == 0;
        if effective_primary && existing_count > 0 {
            sqlx::query(
                "UPDATE dp_user_identities SET is_primary = FALSE \
                 WHERE user_id = $1 AND is_primary",
            )
            .bind(identity.user_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx)?;
        }

        let row = sqlx::query(
            "INSERT INTO dp_user_identities \
                 (user_id, github_user_id, github_login, is_primary, \
                  linked_at, verified_via) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (user_id, github_user_id) DO UPDATE SET \
                 github_login = EXCLUDED.github_login, \
                 verified_via = EXCLUDED.verified_via \
             RETURNING user_id, github_user_id, github_login, is_primary, \
                       linked_at, verified_via",
        )
        .bind(identity.user_id)
        .bind(identity.github_user_id)
        .bind(&identity.github_login)
        .bind(effective_primary)
        .bind(identity.linked_at)
        .bind(identity.verified_via.as_str())
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;

        let out = row_to_user_identity(&row)?;
        tx.commit().await.map_err(map_sqlx)?;
        Ok(out)
    }

    async fn unlink_identity(
        &self,
        user_id: Uuid,
        github_user_id: i64,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;

        // Snapshot the row so we can return a useful error and so
        // we know whether it was primary before the delete.
        let row: Option<(bool,)> = sqlx::query_as(
            "SELECT is_primary FROM dp_user_identities \
             WHERE user_id = $1 AND github_user_id = $2 FOR UPDATE",
        )
        .bind(user_id)
        .bind(github_user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let Some((is_primary,)) = row else {
            return Err(not_found("identity", github_user_id));
        };

        // Last identity rule: refuse to leave the user with zero
        // rows. The principal stamper would 401 them on the next
        // request otherwise, which is worse than a clean 4xx here.
        let remaining: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM dp_user_identities WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        if remaining <= 1 {
            return Err(StoreError::Invalid(
                "cannot unlink the last identity for a user".into(),
            ));
        }
        if is_primary {
            return Err(StoreError::Invalid(
                "cannot unlink the primary identity; set another primary first"
                    .into(),
            ));
        }

        sqlx::query(
            "DELETE FROM dp_user_identities \
             WHERE user_id = $1 AND github_user_id = $2",
        )
        .bind(user_id)
        .bind(github_user_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;

        // CASCADE has dropped `dp_membership_identities` rows for
        // this `github_user_id`. Collapse any `dp_memberships`
        // rows the user can no longer reach via *any* remaining
        // identity, so the §3.0.2.b invariant holds at commit time.
        sqlx::query(
            "DELETE FROM dp_memberships m \
             WHERE m.user_id = $1 \
               AND NOT EXISTS ( \
                   SELECT 1 FROM dp_membership_identities mi \
                   WHERE mi.user_id = m.user_id AND mi.org_id = m.org_id \
               )",
        )
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;

        tx.commit().await.map_err(map_sqlx)?;
        Ok(())
    }

    async fn set_primary_identity(
        &self,
        user_id: Uuid,
        github_user_id: i64,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;

        let exists: Option<Uuid> = sqlx::query_scalar(
            "SELECT user_id FROM dp_user_identities \
             WHERE user_id = $1 AND github_user_id = $2",
        )
        .bind(user_id)
        .bind(github_user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        if exists.is_none() {
            return Err(not_found("identity", github_user_id));
        }

        // Demote the current primary, then promote the target. PG
        // would briefly see two `is_primary = TRUE` rows for the
        // same user inside the transaction except the partial
        // unique index is checked at statement end; doing the
        // demote first keeps the index happy on both deferred and
        // immediate constraint modes.
        sqlx::query(
            "UPDATE dp_user_identities SET is_primary = FALSE \
             WHERE user_id = $1 AND is_primary",
        )
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        sqlx::query(
            "UPDATE dp_user_identities SET is_primary = TRUE \
             WHERE user_id = $1 AND github_user_id = $2",
        )
        .bind(user_id)
        .bind(github_user_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;

        tx.commit().await.map_err(map_sqlx)?;
        Ok(())
    }

    // ---- orgs / teams / repos --------------------------------------

    async fn upsert_org(&self, org: &Org) -> Result<Org, StoreError> {
        let row = sqlx::query(
            "INSERT INTO dp_orgs (id, github_id, login, name) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (github_id) DO UPDATE SET \
                 login = EXCLUDED.login, \
                 name  = EXCLUDED.name \
             RETURNING id, github_id, login, name",
        )
        .bind(org.id)
        .bind(org.github_id)
        .bind(&org.login)
        .bind(&org.name)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_org(&row)
    }

    async fn upsert_team(&self, team: &Team) -> Result<Team, StoreError> {
        let row = sqlx::query(
            "INSERT INTO dp_teams (id, org_id, github_id, slug, name) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (org_id, github_id) DO UPDATE SET \
                 slug = EXCLUDED.slug, \
                 name = EXCLUDED.name \
             RETURNING id, org_id, github_id, slug, name",
        )
        .bind(team.id)
        .bind(team.org_id)
        .bind(team.github_id)
        .bind(&team.slug)
        .bind(&team.name)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_team(&row)
    }

    async fn upsert_repo(&self, repo: &Repo) -> Result<Repo, StoreError> {
        let row = sqlx::query(
            "INSERT INTO dp_repos (id, org_id, github_id, name) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (org_id, github_id) DO UPDATE SET \
                 name = EXCLUDED.name \
             RETURNING id, org_id, github_id, name",
        )
        .bind(repo.id)
        .bind(repo.org_id)
        .bind(repo.github_id)
        .bind(&repo.name)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_repo(&row)
    }

    async fn upsert_repo_metadata(
        &self,
        m: &dp_domain::RepoMetadata,
    ) -> Result<(), StoreError> {
        // COALESCE on nullable text/timestamp fields so a webhook
        // delivery that doesn't carry e.g. `description` doesn't
        // wipe a previously-recorded value. Counter fields are
        // written as supplied — the caller upserts metadata only
        // when the payload included a fresh repo object.
        sqlx::query(
            "INSERT INTO dp_repo_metadata ( \
                 repo_id, stars, forks, watchers, open_issues_remote, \
                 primary_language, default_branch, description, homepage, \
                 is_archived, is_fork, is_private, pushed_at, metadata_updated_at \
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) \
             ON CONFLICT (repo_id) DO UPDATE SET \
                 stars               = EXCLUDED.stars, \
                 forks               = EXCLUDED.forks, \
                 watchers            = EXCLUDED.watchers, \
                 open_issues_remote  = EXCLUDED.open_issues_remote, \
                 primary_language    = COALESCE(EXCLUDED.primary_language, dp_repo_metadata.primary_language), \
                 default_branch      = COALESCE(EXCLUDED.default_branch,   dp_repo_metadata.default_branch), \
                 description         = COALESCE(EXCLUDED.description,      dp_repo_metadata.description), \
                 homepage            = COALESCE(EXCLUDED.homepage,         dp_repo_metadata.homepage), \
                 is_archived         = EXCLUDED.is_archived, \
                 is_fork             = EXCLUDED.is_fork, \
                 is_private          = EXCLUDED.is_private, \
                 pushed_at           = COALESCE(EXCLUDED.pushed_at,        dp_repo_metadata.pushed_at), \
                 metadata_updated_at = EXCLUDED.metadata_updated_at",
        )
        .bind(m.repo_id)
        .bind(m.stars)
        .bind(m.forks)
        .bind(m.watchers)
        .bind(m.open_issues_remote)
        .bind(m.primary_language.as_deref())
        .bind(m.default_branch.as_deref())
        .bind(m.description.as_deref())
        .bind(m.homepage.as_deref())
        .bind(m.is_archived)
        .bind(m.is_fork)
        .bind(m.is_private)
        .bind(m.pushed_at)
        .bind(m.metadata_updated_at)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    async fn get_repo_metadata(
        &self,
        repo_id: Uuid,
    ) -> Result<Option<dp_domain::RepoMetadata>, StoreError> {
        let row = sqlx::query(
            "SELECT repo_id, stars, forks, watchers, open_issues_remote, \
                    primary_language, default_branch, description, homepage, \
                    is_archived, is_fork, is_private, pushed_at, metadata_updated_at \
             FROM dp_repo_metadata WHERE repo_id = $1",
        )
        .bind(repo_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.as_ref().map(row_to_repo_metadata).transpose()
    }

    async fn pr_size_stats_for_repo(
        &self,
        repo_id: Uuid,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
    ) -> Result<dp_domain::RepoPrSizeStats, StoreError> {
        // PR-size distribution from the JSONB payloads of merged
        // PR events. `payload->>'additions'` etc. yield text;
        // ::numeric coerces and silently rejects rows that lack
        // the field (the cast errors out), so we filter with
        // `payload ? 'additions'` first to keep the cast safe.
        //
        // Sample-size guard (SCOPE §15.9): if `n < 5`, every
        // percentile field is returned as NULL; the caller maps
        // that to `Option::None`. We surface the *real* `n` so the
        // UI can communicate "n too small" instead of "no data".
        let row = sqlx::query(
            "WITH sized AS ( \
                 SELECT \
                     (payload->>'additions')::numeric     AS additions, \
                     (payload->>'deletions')::numeric     AS deletions, \
                     (payload->>'changed_files')::numeric AS changed_files, \
                     (payload->>'commits')::numeric       AS commits \
                 FROM dp_activity_events \
                 WHERE repo_id = $1 \
                   AND kind = 'pull_request_merged' \
                   AND ts >= $2 AND ts < $3 \
                   AND payload ? 'additions' \
                   AND payload ? 'deletions' \
                   AND payload ? 'changed_files' \
             ) \
             SELECT \
                 COUNT(*)::bigint AS n, \
                 percentile_cont(0.5)  WITHIN GROUP (ORDER BY additions)                                   AS add_p50, \
                 percentile_cont(0.9)  WITHIN GROUP (ORDER BY additions)                                   AS add_p90, \
                 percentile_cont(0.95) WITHIN GROUP (ORDER BY additions)                                   AS add_p95, \
                 percentile_cont(0.5)  WITHIN GROUP (ORDER BY deletions)                                   AS del_p50, \
                 percentile_cont(0.9)  WITHIN GROUP (ORDER BY deletions)                                   AS del_p90, \
                 percentile_cont(0.95) WITHIN GROUP (ORDER BY deletions)                                   AS del_p95, \
                 percentile_cont(0.5)  WITHIN GROUP (ORDER BY additions + deletions)                       AS tot_p50, \
                 percentile_cont(0.9)  WITHIN GROUP (ORDER BY additions + deletions)                       AS tot_p90, \
                 percentile_cont(0.95) WITHIN GROUP (ORDER BY additions + deletions)                       AS tot_p95, \
                 percentile_cont(0.5)  WITHIN GROUP (ORDER BY changed_files)                               AS cf_p50, \
                 percentile_cont(0.9)  WITHIN GROUP (ORDER BY changed_files)                               AS cf_p90, \
                 percentile_cont(0.95) WITHIN GROUP (ORDER BY changed_files)                               AS cf_p95, \
                 percentile_cont(0.5)  WITHIN GROUP (ORDER BY commits)                                     AS co_p50, \
                 percentile_cont(0.9)  WITHIN GROUP (ORDER BY commits)                                     AS co_p90, \
                 percentile_cont(0.95) WITHIN GROUP (ORDER BY commits)                                     AS co_p95 \
             FROM sized",
        )
        .bind(repo_id)
        .bind(since)
        .bind(until)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        let n: i64 = row.try_get("n").map_err(map_sqlx)?;
        // Sample-size guard — § 15.9: with n < 5 percentile_cont
        // is mathematically defined but actionably noisy. Force
        // every triple to None and let the wire layer / UI render
        // a placeholder.
        let triple = |prefix: &str| -> Result<dp_domain::PercentileTriple, StoreError> {
            if n < 5 {
                return Ok(dp_domain::PercentileTriple::default());
            }
            Ok(dp_domain::PercentileTriple {
                p50: row.try_get(format!("{prefix}_p50").as_str()).map_err(map_sqlx)?,
                p90: row.try_get(format!("{prefix}_p90").as_str()).map_err(map_sqlx)?,
                p95: row.try_get(format!("{prefix}_p95").as_str()).map_err(map_sqlx)?,
            })
        };

        Ok(dp_domain::RepoPrSizeStats {
            sample_n: n,
            additions: triple("add")?,
            deletions: triple("del")?,
            total_lines: triple("tot")?,
            changed_files: triple("cf")?,
            commits: triple("co")?,
        })
    }

    async fn ci_stats_for_repo(
        &self,
        repo_id: Uuid,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
    ) -> Result<dp_domain::RepoCiStats, StoreError> {
        // CI workflow-run stats from the JSONB payload of
        // `workflow_run` events. Counts split by `conclusion`;
        // duration percentiles over `updated_at - run_started_at`
        // for rows where both timestamps parse and the delta is
        // strictly positive (negative / zero deltas would
        // otherwise distort the median for very fast cached
        // runs).
        //
        // Two filters keep the SQL safe against payloads missing
        // the keys (older fixtures, synthetic deliveries):
        //   * `payload ? 'conclusion'` for counts
        //   * `payload ? 'run_started_at' AND ? 'updated_at'` for
        //     duration percentiles
        //
        // Sample-size guard (SCOPE §15.9) applies to durations
        // only — counts are exact and useful even at small n.
        let row = sqlx::query(
            "WITH base AS ( \
                 SELECT \
                     payload->>'conclusion' AS conclusion, \
                     CASE \
                         WHEN payload ? 'run_started_at' AND payload ? 'updated_at' \
                         THEN EXTRACT(EPOCH FROM ( \
                                 (payload->>'updated_at')::timestamptz \
                                 - (payload->>'run_started_at')::timestamptz \
                             )) \
                         ELSE NULL \
                     END AS duration_s \
                 FROM dp_activity_events \
                 WHERE repo_id = $1 \
                   AND kind = 'workflow_run' \
                   AND ts >= $2 AND ts < $3 \
                   AND payload ? 'conclusion' \
             ) \
             SELECT \
                 COUNT(*)::bigint                                                 AS total_runs, \
                 COUNT(*) FILTER (WHERE conclusion = 'success')::bigint          AS success, \
                 COUNT(*) FILTER (WHERE conclusion = 'failure')::bigint          AS failure, \
                 COUNT(*) FILTER (WHERE conclusion = 'cancelled')::bigint        AS cancelled, \
                 COUNT(*) FILTER ( \
                     WHERE conclusion NOT IN ('success', 'failure', 'cancelled') \
                 )::bigint                                                        AS other, \
                 COUNT(*) FILTER (WHERE duration_s IS NOT NULL AND duration_s > 0)::bigint AS dur_n, \
                 percentile_cont(0.5)  WITHIN GROUP (ORDER BY duration_s) \
                     FILTER (WHERE duration_s IS NOT NULL AND duration_s > 0)    AS dur_p50, \
                 percentile_cont(0.9)  WITHIN GROUP (ORDER BY duration_s) \
                     FILTER (WHERE duration_s IS NOT NULL AND duration_s > 0)    AS dur_p90, \
                 percentile_cont(0.95) WITHIN GROUP (ORDER BY duration_s) \
                     FILTER (WHERE duration_s IS NOT NULL AND duration_s > 0)    AS dur_p95 \
             FROM base",
        )
        .bind(repo_id)
        .bind(since)
        .bind(until)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        let total_runs: i64 = row.try_get("total_runs").map_err(map_sqlx)?;
        let success: i64 = row.try_get("success").map_err(map_sqlx)?;
        let failure: i64 = row.try_get("failure").map_err(map_sqlx)?;
        let cancelled: i64 = row.try_get("cancelled").map_err(map_sqlx)?;
        let other: i64 = row.try_get("other").map_err(map_sqlx)?;
        let dur_n: i64 = row.try_get("dur_n").map_err(map_sqlx)?;

        let success_rate = if success + failure == 0 {
            None
        } else {
            Some(success as f64 / (success + failure) as f64)
        };
        let duration_seconds = if dur_n < 5 {
            dp_domain::PercentileTriple::default()
        } else {
            dp_domain::PercentileTriple {
                p50: row.try_get("dur_p50").map_err(map_sqlx)?,
                p90: row.try_get("dur_p90").map_err(map_sqlx)?,
                p95: row.try_get("dur_p95").map_err(map_sqlx)?,
            }
        };

        Ok(dp_domain::RepoCiStats {
            total_runs,
            success,
            failure,
            cancelled,
            other,
            success_rate,
            duration_sample_n: dur_n,
            duration_seconds,
        })
    }

    async fn activity_heatmap_for_repo(
        &self,
        repo_id: Uuid,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
        timezone: &str,
    ) -> Result<dp_domain::RepoActivityHeatmap, StoreError> {
        // `(day_of_week, hour_of_day)` histogram of activity
        // events for one repo. The grid is always dense — we left
        // outer join against `generate_series` so every cell is
        // present with a `0` count, and the caller doesn't have
        // to know which buckets the DB happened to see.
        //
        // `AT TIME ZONE $4` shifts the UTC `ts` into the
        // requested zone *before* extraction, so "8am" means 8am
        // local to the viewer. PG validates the zone string and
        // raises `invalid_parameter_value` (mapped to a
        // `StoreError::Backend` here) on bad input — the REST
        // layer catches typos before they reach SQL.
        //
        // Postgres' `EXTRACT(DOW ...)` returns 0 = Sunday … 6 =
        // Saturday; we re-map to the ISO convention (0 = Monday
        // … 6 = Sunday) in the SELECT so the wire format matches
        // [`HeatmapBucket`]'s docs.
        let rows = sqlx::query(
            "WITH grid AS ( \
                 SELECT d::int2 AS dow, h::int2 AS hour \
                 FROM generate_series(0, 6) AS d \
                 CROSS JOIN generate_series(0, 23) AS h \
             ), \
             counted AS ( \
                 SELECT \
                     ((EXTRACT(DOW  FROM (ts AT TIME ZONE $4))::int + 6) % 7)::int2 AS dow, \
                     EXTRACT(HOUR FROM (ts AT TIME ZONE $4))::int2 AS hour, \
                     COUNT(*)::bigint AS count \
                 FROM dp_activity_events \
                 WHERE repo_id = $1 \
                   AND ts >= $2 AND ts < $3 \
                 GROUP BY 1, 2 \
             ) \
             SELECT g.dow, g.hour, COALESCE(c.count, 0)::bigint AS count \
             FROM grid g \
             LEFT JOIN counted c ON c.dow = g.dow AND c.hour = g.hour \
             ORDER BY g.dow, g.hour",
        )
        .bind(repo_id)
        .bind(since)
        .bind(until)
        .bind(timezone)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        let mut buckets = Vec::with_capacity(168);
        let mut total: i64 = 0;
        for row in &rows {
            let dow: i16 = row.try_get("dow").map_err(map_sqlx)?;
            let hour: i16 = row.try_get("hour").map_err(map_sqlx)?;
            let count: i64 = row.try_get("count").map_err(map_sqlx)?;
            total += count;
            buckets.push(dp_domain::HeatmapBucket { dow, hour, count });
        }

        Ok(dp_domain::RepoActivityHeatmap {
            timezone: timezone.to_string(),
            total,
            buckets,
        })
    }

    async fn review_velocity_for_repo(
        &self,
        repo_id: Uuid,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
    ) -> Result<dp_domain::RepoReviewVelocity, StoreError> {
        // Time-to-merge straight from the merged-PR webhook
        // payload — both `created_at` (PR open) and `merged_at`
        // ship in the same row, so no self-join is needed.
        //
        // Strict-positive delta filter: clock skew between
        // GitHub-side timestamps has been observed to produce
        // `merged_at < created_at` by a handful of seconds; we
        // drop those rather than letting them turn into negative
        // durations that would compress the percentile distance.
        //
        // `EventKind::PullRequestMerged` is the wire kind for
        // closed-and-merged PRs (squash, rebase, or merge-commit
        // all funnel into the same event); closed-without-merge
        // is a different kind so we don't accidentally count
        // abandoned PRs.
        let row = sqlx::query(
            "WITH base AS ( \
                 SELECT \
                     CASE \
                         WHEN payload ? 'created_at' AND payload ? 'merged_at' \
                         THEN EXTRACT(EPOCH FROM ( \
                                 (payload->>'merged_at')::timestamptz \
                                 - (payload->>'created_at')::timestamptz \
                             )) \
                         ELSE NULL \
                     END AS ttm_s \
                 FROM dp_activity_events \
                 WHERE repo_id = $1 \
                   AND kind = 'pull_request_merged' \
                   AND ts >= $2 AND ts < $3 \
             ) \
             SELECT \
                 COUNT(*) FILTER (WHERE ttm_s IS NOT NULL AND ttm_s > 0)::bigint AS sample_n, \
                 percentile_cont(0.5)  WITHIN GROUP (ORDER BY ttm_s) \
                     FILTER (WHERE ttm_s IS NOT NULL AND ttm_s > 0) AS p50, \
                 percentile_cont(0.9)  WITHIN GROUP (ORDER BY ttm_s) \
                     FILTER (WHERE ttm_s IS NOT NULL AND ttm_s > 0) AS p90, \
                 percentile_cont(0.95) WITHIN GROUP (ORDER BY ttm_s) \
                     FILTER (WHERE ttm_s IS NOT NULL AND ttm_s > 0) AS p95 \
             FROM base",
        )
        .bind(repo_id)
        .bind(since)
        .bind(until)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        let sample_n: i64 = row.try_get("sample_n").map_err(map_sqlx)?;
        let time_to_merge_seconds = if sample_n < 5 {
            dp_domain::PercentileTriple::default()
        } else {
            dp_domain::PercentileTriple {
                p50: row.try_get("p50").map_err(map_sqlx)?,
                p90: row.try_get("p90").map_err(map_sqlx)?,
                p95: row.try_get("p95").map_err(map_sqlx)?,
            }
        };

        Ok(dp_domain::RepoReviewVelocity {
            sample_n,
            time_to_merge_seconds,
        })
    }

    async fn contributor_diversity_for_repo(
        &self,
        repo_id: Uuid,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
    ) -> Result<dp_domain::RepoContributorDiversity, StoreError> {
        // Bus-factor view of the repo's merged-PR authorship.
        //
        // The (event, user) grain matters: we *don't* dedupe to
        // "events with at least one author from user X". Counting
        // pairs means a co-authored PR splits its weight, which
        // matches the operational question — "how much load
        // disappears if X is unavailable?" — better than a
        // binary present/absent.
        //
        // The aggregate is computed in two passes inside one
        // round trip via CTEs: per-author counts, then totals
        // (distinct authors, sample size, top-1 and top-3 sums).
        // ARRAY_AGG ordered DESC + slicing in SQL keeps the
        // top-N picks server-side; we never ship author rows
        // back over the wire (SCOPE §4 — diversity, not ranking).
        //
        // §15.9: top1 / top3 shares are masked to NULL when
        // `sample_n < 5` — concentration ratios on n=2 always
        // look catastrophic and are noise.
        let row = sqlx::query(
            "WITH per_author AS ( \
                 SELECT ea.user_id, COUNT(*)::bigint AS c \
                 FROM dp_event_actors ea \
                 JOIN dp_activity_events e ON e.id = ea.event_id \
                 WHERE e.repo_id = $1 \
                   AND e.kind = 'pull_request_merged' \
                   AND e.ts >= $2 AND e.ts < $3 \
                   AND ea.role = 'author' \
                 GROUP BY ea.user_id \
             ), \
             ordered AS ( \
                 SELECT c FROM per_author ORDER BY c DESC \
             ) \
             SELECT \
                 COALESCE(SUM(c), 0)::bigint                          AS sample_n, \
                 COUNT(*)::bigint                                     AS distinct_authors, \
                 COALESCE((SELECT c FROM ordered LIMIT 1), 0)::bigint AS top1, \
                 COALESCE( \
                     (SELECT SUM(c) FROM (SELECT c FROM ordered LIMIT 3) t), \
                     0 \
                 )::bigint                                            AS top3 \
             FROM per_author",
        )
        .bind(repo_id)
        .bind(since)
        .bind(until)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        let sample_n: i64 = row.try_get("sample_n").map_err(map_sqlx)?;
        let distinct_authors: i64 = row.try_get("distinct_authors").map_err(map_sqlx)?;
        let top1: i64 = row.try_get("top1").map_err(map_sqlx)?;
        let top3: i64 = row.try_get("top3").map_err(map_sqlx)?;

        let (top1_share, top3_share) = if sample_n < 5 {
            (None, None)
        } else {
            let n = sample_n as f64;
            (Some(top1 as f64 / n), Some(top3 as f64 / n))
        };

        Ok(dp_domain::RepoContributorDiversity {
            sample_n,
            distinct_authors,
            top1_share,
            top3_share,
        })
    }

    async fn upsert_membership(&self, membership: &Membership) -> Result<Membership, StoreError> {
        // home_org intentionally NOT clobbered — only `set_home_org`
        // writes it (TODO §0.5 / SCOPE §3 manual mapping).
        let role_text = membership_role_to_text(&membership.role).to_string();
        let row = sqlx::query(
            "INSERT INTO dp_memberships (user_id, org_id, role, home_org, joined_at) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (user_id, org_id) DO UPDATE SET \
                 role      = EXCLUDED.role, \
                 home_org  = COALESCE(EXCLUDED.home_org, dp_memberships.home_org), \
                 joined_at = LEAST(dp_memberships.joined_at, EXCLUDED.joined_at) \
             RETURNING user_id, org_id, role, home_org, joined_at",
        )
        .bind(membership.user_id)
        .bind(membership.org_id)
        .bind(&role_text)
        .bind(membership.home_org)
        .bind(membership.joined_at)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_membership(&row)
    }

    async fn list_memberships_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<Membership>, StoreError> {
        let rows = sqlx::query(
            "SELECT user_id, org_id, role, home_org, joined_at \
             FROM dp_memberships WHERE user_id = $1 ORDER BY org_id",
        )
        .bind(user_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_membership).collect()
    }

    async fn set_home_org(
        &self,
        user_id: Uuid,
        org_id: Uuid,
        home_org: Option<Uuid>,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(
            "UPDATE dp_memberships SET home_org = $3 \
             WHERE user_id = $1 AND org_id = $2",
        )
        .bind(user_id)
        .bind(org_id)
        .bind(home_org)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        if result.rows_affected() == 0 {
            return Err(not_found("membership", format!("({user_id}, {org_id})")));
        }
        Ok(())
    }

    async fn set_home_org_for_user(
        &self,
        user_id: Uuid,
        org_id: Uuid,
    ) -> Result<(), StoreError> {
        // One transaction: clear every other home_org for this user
        // and set the (user, org_id) row in one shot so a concurrent
        // reader cannot observe two home_org=Some rows. The single
        // statement uses a CASE expression keyed on org_id; the
        // ROW_COUNT after execution tells us whether the target row
        // existed at all (we look it up explicitly so the error path
        // mirrors set_home_org).
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;
        let exists: Option<(Uuid,)> = sqlx::query_as(
            "SELECT user_id FROM dp_memberships \
             WHERE user_id = $1 AND org_id = $2",
        )
        .bind(user_id)
        .bind(org_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        if exists.is_none() {
            return Err(not_found("membership", format!("({user_id}, {org_id})")));
        }
        sqlx::query(
            "UPDATE dp_memberships \
             SET home_org = CASE WHEN org_id = $2 THEN $2 ELSE NULL END \
             WHERE user_id = $1",
        )
        .bind(user_id)
        .bind(org_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        tx.commit().await.map_err(map_sqlx)?;
        Ok(())
    }

    async fn list_orgs(&self) -> Result<Vec<Org>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, github_id, login, name FROM dp_orgs ORDER BY login",
        )
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_org).collect()
    }

    async fn list_teams_for_org(&self, org_id: Uuid) -> Result<Vec<Team>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, org_id, github_id, slug, name \
             FROM dp_teams WHERE org_id = $1 ORDER BY slug",
        )
        .bind(org_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_team).collect()
    }

    async fn list_users_for_org(&self, org_id: Uuid) -> Result<Vec<User>, StoreError> {
        let rows = sqlx::query(
            "SELECT u.id, u.github_id, u.login, u.email, u.name, u.deleted_at \
             FROM dp_users u \
             JOIN dp_memberships m ON m.user_id = u.id \
             WHERE m.org_id = $1 AND u.deleted_at IS NULL \
             ORDER BY u.login",
        )
        .bind(org_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_user).collect()
    }

    // ---- repos / issues read surface --------------------------------

    async fn get_repo(&self, id: Uuid) -> Result<Option<Repo>, StoreError> {
        // Point lookup by PK. Used by the §8 issue write path to
        // resolve `repo_id -> (org_id, name)` before calling the
        // GitHub backend; without this override the default trait
        // impl returns `None` and every issue mutation 404s.
        let row = sqlx::query(
            "SELECT id, org_id, github_id, name FROM dp_repos WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.as_ref().map(row_to_repo).transpose()
    }

    async fn list_repos(&self, filter: &RepoListFilter) -> Result<Vec<RepoSummary>, StoreError> {
        // Open-issue count + last-activity timestamp are computed
        // via LATERAL subselects so the repo→issue join doesn't
        // multiply rows. Both subselects hit the indexes already
        // declared on dp_issues (repo_updated, org_state). For the
        // expected scale (100s of repos) this stays well under
        // 100ms; if it ever creeps up the obvious fix is a
        // materialised `dp_repo_stats` table refreshed by the
        // webhook worker.
        let q_norm = filter.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let rows = sqlx::query(
            "SELECT r.id, r.org_id, o.login AS org_login, r.name,
                    COALESCE(c.open_issue_count, 0) AS open_issue_count,
                    a.last_activity_at
             FROM dp_repos r
             JOIN dp_orgs o ON o.id = r.org_id
             LEFT JOIN LATERAL (
                 SELECT COUNT(*)::bigint AS open_issue_count
                 FROM dp_issues i WHERE i.repo_id = r.id AND i.state = 'open'
             ) c ON TRUE
             LEFT JOIN LATERAL (
                 SELECT MAX(updated_at) AS last_activity_at
                 FROM dp_issues i WHERE i.repo_id = r.id
             ) a ON TRUE
             WHERE ($1::uuid IS NULL OR r.org_id = $1)
               AND ($2::text IS NULL
                    OR r.name ILIKE '%' || $2 || '%'
                    OR o.login ILIKE '%' || $2 || '%')
             ORDER BY a.last_activity_at DESC NULLS LAST, o.login ASC, r.name ASC
             LIMIT $3 OFFSET $4",
        )
        .bind(filter.org_id)
        .bind(q_norm)
        .bind(filter.limit)
        .bind(filter.offset)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_repo_summary).collect()
    }

    async fn count_repos(&self, filter: &RepoListFilter) -> Result<i64, StoreError> {
        let q_norm = filter.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*)::bigint
             FROM dp_repos r
             JOIN dp_orgs o ON o.id = r.org_id
             WHERE ($1::uuid IS NULL OR r.org_id = $1)
               AND ($2::text IS NULL
                    OR r.name ILIKE '%' || $2 || '%'
                    OR o.login ILIKE '%' || $2 || '%')",
        )
        .bind(filter.org_id)
        .bind(q_norm)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(count)
    }

    async fn list_issues(&self, filter: &IssueListFilter) -> Result<Vec<Issue>, StoreError> {
        let q_norm = filter.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let state_text = filter.state.map(|s| s.as_str().to_string());
        let labels_json = labels_or_assignees_json(&filter.labels);
        let assignees_json = labels_or_assignees_json(&filter.assignees);
        let rows = sqlx::query(
            "SELECT id, org_id, repo_id, github_id, number, title, body, state,
                    labels, assignees, milestone, version,
                    github_node_id, updated_at, is_local
             FROM dp_issues
             WHERE ($1::uuid IS NULL OR repo_id = $1)
               AND ($2::uuid IS NULL OR org_id  = $2)
               AND ($3::text IS NULL OR state   = $3)
               AND ($4::text IS NULL OR assignees @> to_jsonb(ARRAY[$4::text]))
               AND ($5::text IS NULL OR title ILIKE '%' || $5 || '%')
               AND (cardinality($8::uuid[]) = 0 OR repo_id = ANY($8::uuid[]))
               AND (cardinality($9::uuid[]) = 0 OR org_id  = ANY($9::uuid[]))
               AND ($10::jsonb IS NULL OR assignees @> $10::jsonb)
               AND ($11::jsonb IS NULL OR labels    @> $11::jsonb)
               AND ($12::text  IS NULL OR author = $12)
               AND ($13::text  IS NULL OR state_reason = $13)
               AND ($14::timestamptz IS NULL OR updated_at >= $14)
               AND (NOT $15::bool OR (assignees = '[]'::jsonb AND labels = '[]'::jsonb))
             ORDER BY updated_at DESC
             LIMIT $6 OFFSET $7",
        )
        .bind(filter.repo_id)
        .bind(filter.org_id)
        .bind(state_text)
        .bind(filter.assignee.as_deref())
        .bind(q_norm)
        .bind(filter.limit)
        .bind(filter.offset)
        .bind(&filter.repo_ids)
        .bind(&filter.org_ids)
        .bind(assignees_json.as_ref())
        .bind(labels_json.as_ref())
        .bind(filter.author.as_deref())
        .bind(filter.state_reason.as_deref())
        .bind(filter.updated_since)
        .bind(filter.untriaged_only)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_issue).collect()
    }

    async fn count_issues(&self, filter: &IssueListFilter) -> Result<i64, StoreError> {
        let q_norm = filter.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let state_text = filter.state.map(|s| s.as_str().to_string());
        let labels_json = labels_or_assignees_json(&filter.labels);
        let assignees_json = labels_or_assignees_json(&filter.assignees);
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*)::bigint
             FROM dp_issues
             WHERE ($1::uuid IS NULL OR repo_id = $1)
               AND ($2::uuid IS NULL OR org_id  = $2)
               AND ($3::text IS NULL OR state   = $3)
               AND ($4::text IS NULL OR assignees @> to_jsonb(ARRAY[$4::text]))
               AND ($5::text IS NULL OR title ILIKE '%' || $5 || '%')
               AND (cardinality($6::uuid[]) = 0 OR repo_id = ANY($6::uuid[]))
               AND (cardinality($7::uuid[]) = 0 OR org_id  = ANY($7::uuid[]))
               AND ($8::jsonb  IS NULL OR assignees @> $8::jsonb)
               AND ($9::jsonb  IS NULL OR labels    @> $9::jsonb)
               AND ($10::text  IS NULL OR author = $10)
               AND ($11::text  IS NULL OR state_reason = $11)
               AND ($12::timestamptz IS NULL OR updated_at >= $12)
               AND (NOT $13::bool OR (assignees = '[]'::jsonb AND labels = '[]'::jsonb))",
        )
        .bind(filter.repo_id)
        .bind(filter.org_id)
        .bind(state_text)
        .bind(filter.assignee.as_deref())
        .bind(q_norm)
        .bind(&filter.repo_ids)
        .bind(&filter.org_ids)
        .bind(assignees_json.as_ref())
        .bind(labels_json.as_ref())
        .bind(filter.author.as_deref())
        .bind(filter.state_reason.as_deref())
        .bind(filter.updated_since)
        .bind(filter.untriaged_only)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(count)
    }

    async fn get_issue(&self, id: Uuid) -> Result<Option<Issue>, StoreError> {
        let row = sqlx::query(
            "SELECT id, org_id, repo_id, github_id, number, title, body, state,
                    labels, assignees, milestone, version,
                    github_node_id, updated_at, is_local
             FROM dp_issues WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.as_ref().map(row_to_issue).transpose()
    }

    async fn get_issue_by_repo_and_number(
        &self,
        repo_id: Uuid,
        number: i64,
    ) -> Result<Option<Issue>, StoreError> {
        let row = sqlx::query(
            "SELECT id, org_id, repo_id, github_id, number, title, body, state,
                    labels, assignees, milestone, version,
                    github_node_id, updated_at, is_local
             FROM dp_issues WHERE repo_id = $1 AND number = $2",
        )
        .bind(repo_id)
        .bind(number)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.as_ref().map(row_to_issue).transpose()
    }

    async fn upsert_issue_from_github(
        &self,
        upsert: &IssueUpsert,
        pending_remote_window: chrono::Duration,
    ) -> Result<(Issue, IssueUpsertOutcome), StoreError> {
        // The upsert is a single round-trip so insert / freshness
        // check / version bump / §13.7 guard all happen atomically:
        //
        //   INSERT … ON CONFLICT (repo_id, number) DO UPDATE …
        //   WHERE
        //     -- §13.7 guard: skip if a recent optimistic write is
        //     -- still in flight (the dp-rest §8 path cleared
        //     -- `pending_remote` on completion / rollback, so a
        //     -- TRUE flag with a fresh timestamp means "do not
        //     -- clobber").
        //     (dp_issues.pending_remote = FALSE
        //      OR dp_issues.pending_remote_at <= now() - window)
        //     -- Freshness: only bump on strictly-newer payloads.
        //     AND excluded.updated_at > dp_issues.updated_at
        //   RETURNING …, (xmax = 0) AS inserted
        //
        // `xmax = 0` is the canonical Postgres trick to tell INSERT
        // from UPDATE inside an UPSERT — the inserted row has a
        // zero transaction-deleter marker. We use it (combined with
        // a follow-up `is_some()` on the rowcount) to decode the
        // three writing outcomes.
        let labels_json = serde_json::to_value(&upsert.labels)
            .map_err(|e| StoreError::Invalid(format!("labels not serialisable: {e}")))?;
        let assignees_json = serde_json::to_value(&upsert.assignees)
            .map_err(|e| StoreError::Invalid(format!("assignees not serialisable: {e}")))?;
        let new_id = Uuid::new_v4();
        let row = sqlx::query(
            "INSERT INTO dp_issues (
                 id, org_id, repo_id, github_id, number, title, body, state,
                 labels, assignees, milestone, author, state_reason,
                 created_at, updated_at, closed_at, version, github_node_id
             ) VALUES (
                 $1, $2, $3, $4, $5, $6, $7, $8,
                 $9, $10, $11, $12, $13,
                 $14, $15, $16, 1, $18
             )
             ON CONFLICT (repo_id, number) DO UPDATE SET
                 title        = EXCLUDED.title,
                 body         = EXCLUDED.body,
                 state        = EXCLUDED.state,
                 labels       = EXCLUDED.labels,
                 assignees    = EXCLUDED.assignees,
                 milestone    = EXCLUDED.milestone,
                 author       = EXCLUDED.author,
                 state_reason = EXCLUDED.state_reason,
                 updated_at   = EXCLUDED.updated_at,
                 closed_at    = EXCLUDED.closed_at,
                 -- github_id stays put — once we learn an issue's
                 -- numeric id, it never changes (transfers move
                 -- the number, not the id).
                 version      = dp_issues.version + 1,
                 -- §3.10 — opportunistic backfill: a row that
                 -- pre-dates migration 0021 has NULL here; the
                 -- first webhook / reconciler payload after the
                 -- migration carries `node_id`, populating the
                 -- column so the Projects v2 mirror can skip
                 -- the lazy GraphQL lookup on the next save.
                 github_node_id = COALESCE(EXCLUDED.github_node_id, dp_issues.github_node_id)
             WHERE
                 (dp_issues.pending_remote = FALSE
                  OR dp_issues.pending_remote_at IS NULL
                  OR dp_issues.pending_remote_at <= (now() - ($17::bigint || ' seconds')::interval))
                 AND EXCLUDED.updated_at > dp_issues.updated_at
             RETURNING
                 id, org_id, repo_id, github_id, number, title, body, state,
                 labels, assignees, milestone, version,
                 github_node_id, updated_at,
                 (xmax = 0) AS inserted",
        )
        .bind(new_id)
        .bind(upsert.org_id)
        .bind(upsert.repo_id)
        .bind(upsert.github_id)
        .bind(upsert.number)
        .bind(&upsert.title)
        .bind(upsert.body.as_deref())
        .bind(upsert.state.as_str())
        .bind(&labels_json)
        .bind(&assignees_json)
        .bind(upsert.milestone.as_deref())
        .bind(upsert.author.as_deref())
        .bind(upsert.state_reason.as_deref())
        .bind(upsert.created_at)
        .bind(upsert.updated_at)
        .bind(upsert.closed_at)
        .bind(pending_remote_window.num_seconds())
        .bind(upsert.github_node_id.as_deref())
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        if let Some(row) = row {
            let inserted: bool = row.try_get("inserted").map_err(map_sqlx)?;
            let issue = row_to_issue(&row)?;
            let outcome = if inserted {
                IssueUpsertOutcome::Inserted
            } else {
                IssueUpsertOutcome::Updated
            };
            return Ok((issue, outcome));
        }

        // No row returned → either the freshness guard fired
        // (stale payload — local copy is at least as new) or the
        // §13.7 reconciler guard fired (pending_remote within
        // window). Disambiguate with a single follow-up read so
        // the caller's metrics are accurate and so the caller
        // always receives the *current* local row.
        let existing = sqlx::query(
            "SELECT id, org_id, repo_id, github_id, number, title, body, state,
                    labels, assignees, milestone, version,
                    github_node_id, updated_at,
                    pending_remote, pending_remote_at
             FROM dp_issues
             WHERE repo_id = $1 AND number = $2",
        )
        .bind(upsert.repo_id)
        .bind(upsert.number)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?
        .ok_or_else(|| {
            // Can't happen unless someone deleted the row between
            // the upsert and the follow-up read — surface loudly.
            StoreError::Invalid(format!(
                "upsert for ({}, {}) returned no row and follow-up read missed",
                upsert.repo_id, upsert.number
            ))
        })?;

        let issue = row_to_issue(&existing)?;
        let pending: bool = existing.try_get("pending_remote").map_err(map_sqlx)?;
        let pending_at: Option<DateTime<Utc>> =
            existing.try_get("pending_remote_at").map_err(map_sqlx)?;
        let now = Utc::now();
        let in_pending_window = pending
            && pending_at
                .map(|at| now.signed_duration_since(at) < pending_remote_window)
                .unwrap_or(false);
        let outcome = if in_pending_window {
            IssueUpsertOutcome::Deferred
        } else {
            IssueUpsertOutcome::Skipped
        };
        Ok((issue, outcome))
    }

    /// SCOPE.md §4.1 amendment — direct insert of a local-only
    /// issue. Allocates a synthetic per-repo negative number /
    /// `github_id` from `dp_repos.local_issue_counter` (decremented
    /// in the same transaction) so the existing `UNIQUE (repo_id,
    /// number)` and `UNIQUE (repo_id, github_id)` invariants hold
    /// without widening the columns to NULL.
    async fn create_local_issue(
        &self,
        org_id: Uuid,
        repo_id: Uuid,
        title: &str,
        body: Option<&str>,
    ) -> Result<Issue, StoreError> {
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;

        // Allocate the next negative slot. The first local issue
        // in a repo gets number = -1, the second -2, …
        let (next,): (i64,) = sqlx::query_as(
            "UPDATE dp_repos
                SET local_issue_counter = local_issue_counter - 1
              WHERE id = $1
            RETURNING local_issue_counter",
        )
        .bind(repo_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;

        let new_id = Uuid::new_v4();
        let now = Utc::now();
        let row = sqlx::query(
            "INSERT INTO dp_issues (
                 id, org_id, repo_id, github_id, number, title, body, state,
                 labels, assignees, milestone, author, state_reason,
                 created_at, updated_at, closed_at, version, github_node_id,
                 is_local
             ) VALUES (
                 $1, $2, $3, $4, $4, $5, $6, 'open',
                 '[]'::jsonb, '[]'::jsonb, NULL, NULL, NULL,
                 $7, $7, NULL, 1, NULL,
                 TRUE
             )
             RETURNING id, org_id, repo_id, github_id, number, title, body, state,
                       labels, assignees, milestone, version,
                       github_node_id, updated_at, is_local",
        )
        .bind(new_id)
        .bind(org_id)
        .bind(repo_id)
        .bind(next)
        .bind(title)
        .bind(body)
        .bind(now)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;

        let issue = row_to_issue(&row)?;
        tx.commit().await.map_err(map_sqlx)?;
        Ok(issue)
    }

    /// SCOPE.md §4.1.1 — direct CAS-gated field update for a
    /// local-only issue (no GitHub round-trip, no pending_remote
    /// dance). The WHERE clause performs the CAS; COALESCE on
    /// each lane preserves untouched fields. `is_local = TRUE` is
    /// in the WHERE clause too so this method cannot accidentally
    /// be used to bypass the GitHub two-way-sync on a real issue.
    async fn update_local_issue(
        &self,
        issue_id: Uuid,
        expected_version: i64,
        title: Option<&str>,
        body: Option<Option<&str>>,
        state: Option<&str>,
        labels: Option<&[String]>,
        assignees: Option<&[String]>,
    ) -> Result<Issue, StoreError> {
        // `body` uses Option<Option<&str>> so an explicit
        // `Some(None)` lane clears the column; `None` leaves it
        // alone. Encode the "clear" intent with a sentinel bool
        // bound separately so the COALESCE chain stays simple.
        let (body_provided, body_value): (bool, Option<&str>) = match body {
            None => (false, None),
            Some(v) => (true, v),
        };
        let labels_json = labels
            .map(|l| serde_json::to_value(l))
            .transpose()
            .map_err(|e| StoreError::Invalid(format!("labels not serialisable: {e}")))?;
        let assignees_json = assignees
            .map(|a| serde_json::to_value(a))
            .transpose()
            .map_err(|e| {
                StoreError::Invalid(format!("assignees not serialisable: {e}"))
            })?;
        // `closed_at` is derived from the state transition: closing
        // stamps now(); reopening clears it. When state isn't being
        // touched, leave both `state` and `closed_at` alone.
        let row = sqlx::query(
            "UPDATE dp_issues SET
                 title       = COALESCE($3, title),
                 body        = CASE WHEN $4::bool THEN $5 ELSE body END,
                 state       = COALESCE($6, state),
                 closed_at   = CASE
                                   WHEN $6 = 'closed' THEN COALESCE(closed_at, now())
                                   WHEN $6 = 'open'   THEN NULL
                                   ELSE closed_at
                               END,
                 labels      = COALESCE($7, labels),
                 assignees   = COALESCE($8, assignees),
                 version     = version + 1,
                 updated_at  = now()
              WHERE id = $1
                AND version = $2
                AND is_local = TRUE
              RETURNING id, org_id, repo_id, github_id, number, title, body, state,
                        labels, assignees, milestone, version,
                        github_node_id, updated_at, is_local",
        )
        .bind(issue_id)
        .bind(expected_version)
        .bind(title)
        .bind(body_provided)
        .bind(body_value)
        .bind(state)
        .bind(labels_json.as_ref())
        .bind(assignees_json.as_ref())
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        match row {
            Some(r) => row_to_issue(&r),
            None => {
                // Distinguish "no such local issue" from "stale
                // CAS" so the REST handler can return a useful
                // 404 vs 409.
                let exists: Option<(bool,)> = sqlx::query_as(
                    "SELECT is_local FROM dp_issues WHERE id = $1",
                )
                .bind(issue_id)
                .fetch_optional(self.pool.sqlx())
                .await
                .map_err(map_sqlx)?;
                match exists {
                    None => Err(not_found("issue", issue_id)),
                    Some((false,)) => Err(StoreError::Invalid(format!(
                        "issue {issue_id} is not a local-only issue"
                    ))),
                    Some((true,)) => Err(StoreError::Conflict(format!(
                        "stale expected_version {expected_version} for local issue {issue_id}"
                    ))),
                }
            }
        }
    }

    // ---- per-user inbox (triage spine, slice 1) -------------------

    async fn list_inbox_issues(
        &self,
        user_id: Uuid,
        filter: &IssueListFilter,
    ) -> Result<Vec<InboxIssueRow>, StoreError> {
        let q_norm = filter.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let state_text = filter.state.map(|s| s.as_str().to_string());
        let labels_json = labels_or_assignees_json(&filter.labels);
        let assignees_json = labels_or_assignees_json(&filter.assignees);
        // LEFT JOIN so issues with no `dp_user_issue_state` row
        // surface as default-state (`Inbox`, last_seen_version 0).
        // Inbox visibility predicate:
        //   * status IS NULL OR status <> 'done'      — dismissed rows hide
        //   * status <> 'snoozed' OR snoozed_until < now()  — active snoozes hide
        let rows = sqlx::query(
            "SELECT i.id, i.org_id, i.repo_id, i.github_id, i.number, i.title, i.body,
                    i.state, i.labels, i.assignees, i.milestone, i.version,
                    i.github_node_id, i.updated_at, i.is_local,
                    COALESCE(s.last_seen_version, 0)            AS last_seen_version,
                    COALESCE(s.status, 'inbox')                 AS inbox_status,
                    s.snoozed_until                             AS snoozed_until
             FROM dp_issues i
             LEFT JOIN dp_user_issue_state s
                    ON s.user_id = $16::uuid AND s.issue_id = i.id
             WHERE (s.status IS NULL OR s.status <> 'done')
               AND (s.status IS NULL OR s.status <> 'snoozed'
                    OR s.snoozed_until IS NULL OR s.snoozed_until < now())
               AND ($1::uuid IS NULL OR i.repo_id = $1)
               AND ($2::uuid IS NULL OR i.org_id  = $2)
               AND ($3::text IS NULL OR i.state   = $3)
               AND ($4::text IS NULL OR i.assignees @> to_jsonb(ARRAY[$4::text]))
               AND ($5::text IS NULL OR i.title ILIKE '%' || $5 || '%')
               AND (cardinality($8::uuid[]) = 0 OR i.repo_id = ANY($8::uuid[]))
               AND (cardinality($9::uuid[]) = 0 OR i.org_id  = ANY($9::uuid[]))
               AND ($10::jsonb IS NULL OR i.assignees @> $10::jsonb)
               AND ($11::jsonb IS NULL OR i.labels    @> $11::jsonb)
               AND ($12::text  IS NULL OR i.author = $12)
               AND ($13::text  IS NULL OR i.state_reason = $13)
               AND ($14::timestamptz IS NULL OR i.updated_at >= $14)
               AND (NOT $15::bool OR (i.assignees = '[]'::jsonb AND i.labels = '[]'::jsonb))
               AND ($17::timestamptz IS NULL
                    OR (i.updated_at, i.id) < ($17::timestamptz, $18::uuid))
             ORDER BY i.updated_at DESC, i.id DESC
             LIMIT $6 OFFSET $7",
        )
        .bind(filter.repo_id)
        .bind(filter.org_id)
        .bind(state_text)
        .bind(filter.assignee.as_deref())
        .bind(q_norm)
        .bind(filter.limit)
        .bind(filter.offset)
        .bind(&filter.repo_ids)
        .bind(&filter.org_ids)
        .bind(assignees_json.as_ref())
        .bind(labels_json.as_ref())
        .bind(filter.author.as_deref())
        .bind(filter.state_reason.as_deref())
        .bind(filter.updated_since)
        .bind(filter.untriaged_only)
        .bind(user_id)
        .bind(filter.keyset_after.map(|(ts, _)| ts))
        .bind(filter.keyset_after.map(|(_, id)| id))
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_inbox_issue_row).collect()
    }

    async fn count_inbox_issues(
        &self,
        user_id: Uuid,
        filter: &IssueListFilter,
    ) -> Result<i64, StoreError> {
        let q_norm = filter.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let state_text = filter.state.map(|s| s.as_str().to_string());
        let labels_json = labels_or_assignees_json(&filter.labels);
        let assignees_json = labels_or_assignees_json(&filter.assignees);
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*)::bigint
             FROM dp_issues i
             LEFT JOIN dp_user_issue_state s
                    ON s.user_id = $14::uuid AND s.issue_id = i.id
             WHERE (s.status IS NULL OR s.status <> 'done')
               AND (s.status IS NULL OR s.status <> 'snoozed'
                    OR s.snoozed_until IS NULL OR s.snoozed_until < now())
               AND ($1::uuid IS NULL OR i.repo_id = $1)
               AND ($2::uuid IS NULL OR i.org_id  = $2)
               AND ($3::text IS NULL OR i.state   = $3)
               AND ($4::text IS NULL OR i.assignees @> to_jsonb(ARRAY[$4::text]))
               AND ($5::text IS NULL OR i.title ILIKE '%' || $5 || '%')
               AND (cardinality($6::uuid[]) = 0 OR i.repo_id = ANY($6::uuid[]))
               AND (cardinality($7::uuid[]) = 0 OR i.org_id  = ANY($7::uuid[]))
               AND ($8::jsonb  IS NULL OR i.assignees @> $8::jsonb)
               AND ($9::jsonb  IS NULL OR i.labels    @> $9::jsonb)
               AND ($10::text  IS NULL OR i.author = $10)
               AND ($11::text  IS NULL OR i.state_reason = $11)
               AND ($12::timestamptz IS NULL OR i.updated_at >= $12)
               AND (NOT $13::bool OR (i.assignees = '[]'::jsonb AND i.labels = '[]'::jsonb))",
        )
        .bind(filter.repo_id)
        .bind(filter.org_id)
        .bind(state_text)
        .bind(filter.assignee.as_deref())
        .bind(q_norm)
        .bind(&filter.repo_ids)
        .bind(&filter.org_ids)
        .bind(assignees_json.as_ref())
        .bind(labels_json.as_ref())
        .bind(filter.author.as_deref())
        .bind(filter.state_reason.as_deref())
        .bind(filter.updated_since)
        .bind(filter.untriaged_only)
        .bind(user_id)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(count)
    }

    async fn mark_issues_seen(
        &self,
        user_id: Uuid,
        issue_ids: &[Uuid],
    ) -> Result<(), StoreError> {
        if issue_ids.is_empty() {
            return Ok(());
        }
        // Upsert one row per (user_id, issue_id), pulling
        // `last_seen_version` from `dp_issues.version` so the row
        // always reflects what the user actually saw. ON CONFLICT
        // promotes the value monotonically (GREATEST) so a stale
        // "seen" write from a slow client cannot regress a higher
        // value already on the row.
        sqlx::query(
            "INSERT INTO dp_user_issue_state
                 (user_id, issue_id, last_seen_version, status, snoozed_until, updated_at)
             SELECT $1, i.id, i.version, 'inbox', NULL, now()
               FROM dp_issues i
              WHERE i.id = ANY($2::uuid[])
             ON CONFLICT (user_id, issue_id) DO UPDATE
                 SET last_seen_version =
                         GREATEST(dp_user_issue_state.last_seen_version,
                                  EXCLUDED.last_seen_version),
                     updated_at        = now()",
        )
        .bind(user_id)
        .bind(issue_ids)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    async fn set_inbox_state(
        &self,
        user_id: Uuid,
        issue_id: Uuid,
        status: InboxStatus,
        snoozed_until: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<UserIssueState, StoreError> {
        // Upsert; preserve `last_seen_version` on update so
        // snooze / dismiss never moves the seen marker. The
        // application validates (status, snoozed_until)
        // consistency — see the trait doc.
        let row = sqlx::query(
            "INSERT INTO dp_user_issue_state
                 (user_id, issue_id, last_seen_version, status, snoozed_until, updated_at)
             VALUES ($1, $2, 0, $3, $4, now())
             ON CONFLICT (user_id, issue_id) DO UPDATE
                 SET status        = EXCLUDED.status,
                     snoozed_until = EXCLUDED.snoozed_until,
                     updated_at    = now()
             RETURNING user_id, issue_id, last_seen_version, status, snoozed_until, updated_at",
        )
        .bind(user_id)
        .bind(issue_id)
        .bind(status.as_str())
        .bind(snoozed_until)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_user_issue_state(&row)
    }

    async fn set_inbox_state_bulk(
        &self,
        user_id: Uuid,
        issue_ids: &[Uuid],
        status: InboxStatus,
        snoozed_until: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<u64, StoreError> {
        if issue_ids.is_empty() {
            return Ok(0);
        }
        // Done / Inbox ignore the snooze deadline (Inbox clears it;
        // Done has no wake target). Only Snoozed carries it through.
        let effective_snooze = match status {
            InboxStatus::Snoozed => snoozed_until,
            InboxStatus::Inbox | InboxStatus::Done => None,
        };
        let res = sqlx::query(
            "INSERT INTO dp_user_issue_state
                 (user_id, issue_id, last_seen_version, status, snoozed_until, updated_at)
             SELECT $1, i.id, 0, $3, $4, now()
               FROM dp_issues i
              WHERE i.id = ANY($2::uuid[])
             ON CONFLICT (user_id, issue_id) DO UPDATE
                 SET status        = EXCLUDED.status,
                     snoozed_until = EXCLUDED.snoozed_until,
                     updated_at    = now()",
        )
        .bind(user_id)
        .bind(issue_ids)
        .bind(status.as_str())
        .bind(effective_snooze)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(res.rows_affected())
    }

    async fn record_audit_log(&self, entry: &AuditEntry) -> Result<(), StoreError> {
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

    // ---- issue timeline (triage slice 2 — §5.6) ------------------

    async fn list_events_for_issue(
        &self,
        repo_id: Uuid,
        number: i64,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<IssueTimelineRow>, StoreError> {
        // The §6 guarded expression index on dp_activity_events
        // ensures the cast cannot raise on malformed rows — the
        // `payload ? 'number' AND payload->>'number' ~ '^[0-9]+$'`
        // predicate is repeated in the WHERE clause verbatim so
        // the planner picks the partial expression index.
        let rows = sqlx::query(
            "SELECT id, kind, ts, payload
             FROM dp_activity_events
             WHERE repo_id = $1
               AND kind = ANY(ARRAY['issue_opened','issue_closed','issue_comment']::text[])
               AND payload ? 'number'
               AND payload->>'number' ~ '^[0-9]+$'
               AND (payload->>'number')::int = $2
             ORDER BY ts DESC, id DESC
             LIMIT $3 OFFSET $4",
        )
        .bind(repo_id)
        .bind(number)
        .bind(limit.max(1))
        .bind(offset.max(0))
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows.iter() {
            let id: Uuid = r.try_get("id").map_err(map_sqlx)?;
            let kind_text: String = r.try_get("kind").map_err(map_sqlx)?;
            let ts: DateTime<Utc> = r.try_get("ts").map_err(map_sqlx)?;
            let payload: JsonValue = r.try_get("payload").map_err(map_sqlx)?;
            let kind: EventKind = serde_json::from_value(JsonValue::String(kind_text.clone()))
                .map_err(|e| StoreError::Invalid(format!("unknown event kind {kind_text}: {e}")))?;
            let payload_summary = summarise_timeline_payload(kind, &payload);
            out.push(IssueTimelineRow {
                id,
                kind,
                ts,
                payload_summary,
            });
        }
        Ok(out)
    }

    async fn count_events_for_issue(
        &self,
        repo_id: Uuid,
        number: i64,
    ) -> Result<i64, StoreError> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*)::bigint
             FROM dp_activity_events
             WHERE repo_id = $1
               AND kind = ANY(ARRAY['issue_opened','issue_closed','issue_comment']::text[])
               AND payload ? 'number'
               AND payload->>'number' ~ '^[0-9]+$'
               AND (payload->>'number')::int = $2",
        )
        .bind(repo_id)
        .bind(number)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(count)
    }

    // ---- repo sync status (triage slice 2 — §5.9) -----------------

    async fn get_repo_sync_status(
        &self,
        repo_id: Uuid,
    ) -> Result<Option<RepoSyncStatus>, StoreError> {
        // Synthesise per-repo freshness from dp_fetch_cursors. The
        // table carries one row per (org, repo, resource_kind);
        // newest `updated_at` is the most recent successful pull.
        let row: Option<(Option<DateTime<Utc>>,)> = sqlx::query_as(
            "SELECT MAX(updated_at)
             FROM dp_fetch_cursors
             WHERE repo_id = $1",
        )
        .bind(repo_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some((Some(ts),)) => Ok(Some(RepoSyncStatus {
                last_synced_at: Some(ts),
                last_attempt_at: Some(ts),
                last_error: None,
            })),
            _ => Ok(Some(RepoSyncStatus {
                last_synced_at: None,
                last_attempt_at: None,
                last_error: None,
            })),
        }
    }

    // ---- issue metrics (triage slice 2 — §5.10) -------------------

    async fn issue_metrics(
        &self,
        filter: &IssueMetricsFilter,
    ) -> Result<Vec<IssueMetricRow>, StoreError> {
        // Common scope: caller-supplied org / repo id sets are
        // applied as `= ANY(...)` so an empty slice = "no
        // restriction". The §5.10 SQL shapes are spelled out
        // inline so the planner sees a stable shape per metric.
        let bucket_sql = match (filter.metric, filter.group_by) {
            // `wip` group-by is fixed to assignee (§5.10).
            (IssueMetric::Wip, _) => "assignee_login",
            (_, IssueMetricGroupBy::Repo) => "i.repo_id::text",
            (_, IssueMetricGroupBy::Org) => "i.org_id::text",
            (_, IssueMetricGroupBy::Assignee) => "assignee_login",
            (_, IssueMetricGroupBy::Week) => {
                "to_char(date_trunc('week', coalesce(i.closed_at, i.updated_at)), 'YYYY-MM-DD')"
            }
            (_, IssueMetricGroupBy::Day) => {
                "to_char(date_trunc('day', coalesce(i.closed_at, i.updated_at)), 'YYYY-MM-DD')"
            }
        };

        // The §5.10 corrected SQL — see header comments in
        // linear-projects-idea.md §5.10:
        //
        //   * `wip`         uses `CROSS JOIN LATERAL jsonb_array_elements_text(assignees)`
        //   * `untriaged`   uses `jsonb_array_length(...) = 0`
        //   * `lead_time`   uses `EXTRACT(EPOCH FROM (closed_at - created_at))`
        let (select_clause, from_extra, where_extra) = match filter.metric {
            IssueMetric::Throughput => (
                "COUNT(*)::float8 AS value, COUNT(*)::bigint AS cnt",
                "",
                "i.state = 'closed' AND ($3::timestamptz IS NULL OR i.closed_at >= $3)
                 AND ($4::timestamptz IS NULL OR i.closed_at < $4)",
            ),
            IssueMetric::LeadTime => (
                "COALESCE(percentile_cont(0.5) WITHIN GROUP (
                     ORDER BY EXTRACT(EPOCH FROM (i.closed_at - i.created_at))
                 ), 0)::float8 AS value,
                 COUNT(*)::bigint AS cnt",
                "",
                "i.state = 'closed' AND i.closed_at IS NOT NULL
                 AND ($3::timestamptz IS NULL OR i.closed_at >= $3)
                 AND ($4::timestamptz IS NULL OR i.closed_at < $4)",
            ),
            IssueMetric::Wip => (
                "COUNT(*)::float8 AS value, COUNT(*)::bigint AS cnt",
                "CROSS JOIN LATERAL jsonb_array_elements_text(i.assignees) AS assignee_login",
                "i.state = 'open'",
            ),
            IssueMetric::Stale => (
                "COUNT(*)::float8 AS value, COUNT(*)::bigint AS cnt",
                "",
                "i.state = 'open' AND i.updated_at < now() - interval '30 days'",
            ),
            IssueMetric::Untriaged => (
                "COUNT(*)::float8 AS value, COUNT(*)::bigint AS cnt",
                "",
                "i.state = 'open'
                 AND jsonb_array_length(i.assignees) = 0
                 AND jsonb_array_length(i.labels)    = 0",
            ),
        };

        let sql = format!(
            "SELECT {bucket} AS bucket, {select}
             FROM dp_issues i
             {from_extra}
             WHERE (cardinality($1::uuid[]) = 0 OR i.org_id  = ANY($1::uuid[]))
               AND (cardinality($2::uuid[]) = 0 OR i.repo_id = ANY($2::uuid[]))
               AND {where_extra}
             GROUP BY bucket
             ORDER BY bucket",
            bucket = bucket_sql,
            select = select_clause,
            from_extra = from_extra,
            where_extra = where_extra,
        );

        let rows = sqlx::query(&sql)
            .bind(&filter.org_ids)
            .bind(&filter.repo_ids)
            .bind(filter.since)
            .bind(filter.until)
            .fetch_all(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows.iter() {
            let bucket: String = r.try_get("bucket").map_err(map_sqlx)?;
            let value: f64 = r.try_get("value").map_err(map_sqlx)?;
            let count: i64 = r.try_get("cnt").map_err(map_sqlx)?;
            out.push(IssueMetricRow {
                bucket,
                value,
                count,
            });
        }
        Ok(out)
    }

    // ---- events + actors ------------------------------------------

    async fn record_event(&self, event: &ActivityEvent) -> Result<ActivityEvent, StoreError> {
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

    async fn add_event_actors(&self, actors: &[EventActor]) -> Result<(), StoreError> {
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

    async fn list_event_actor_rows_in_window(
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

    // ---- cursors + run log ----------------------------------------

    async fn get_cursor(
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

    async fn put_cursor(&self, cursor: &FetchCursor) -> Result<(), StoreError> {
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

    async fn start_fetch_run(&self, kind: FetchRunKind) -> Result<Uuid, StoreError> {
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

    async fn finish_fetch_run(
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

    async fn record_fetch_run_errors(
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

    async fn list_recent_fetch_runs(&self, limit: i64) -> Result<Vec<FetchRun>, StoreError> {
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

    async fn list_fetch_runs(
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

    async fn list_event_actor_rows_for_user_page(
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

    async fn data_as_of(&self) -> Result<DataAsOf, StoreError> {
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

    // ---- webhook inbox --------------------------------------------

    async fn enqueue_webhook(&self, delivery: &WebhookDelivery) -> Result<(), StoreError> {
        // No ON CONFLICT — we WANT the unique-violation on
        // `delivery_id` to surface so the caller can translate it to
        // a 200 OK and avoid double-processing.
        sqlx::query(
            "INSERT INTO dp_webhook_inbox \
                 (id, delivery_id, event, payload, received_at, processed_at, error) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(delivery.id)
        .bind(&delivery.delivery_id)
        .bind(&delivery.event)
        .bind(&delivery.payload)
        .bind(delivery.received_at)
        .bind(delivery.processed_at)
        .bind(&delivery.error)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    async fn claim_webhooks(&self, max: i64) -> Result<Vec<WebhookDelivery>, StoreError> {
        // `FOR UPDATE SKIP LOCKED` is how multiple workers cooperate
        // without serialising — Postgres-canonical queue pattern.
        // The CTE writes the lock; the outer SELECT returns the
        // rows shaped like the regular read.
        let rows = sqlx::query(
            "WITH claimed AS ( \
                 SELECT id FROM dp_webhook_inbox \
                 WHERE processed_at IS NULL \
                 ORDER BY received_at \
                 LIMIT $1 \
                 FOR UPDATE SKIP LOCKED \
             ) \
             SELECT w.id, w.delivery_id, w.event, w.payload, \
                    w.received_at, w.processed_at, w.error \
             FROM dp_webhook_inbox w \
             JOIN claimed c ON c.id = w.id",
        )
        .bind(max)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_webhook_delivery).collect()
    }

    async fn mark_webhook_processed(&self, id: Uuid) -> Result<(), StoreError> {
        let result = sqlx::query(
            "UPDATE dp_webhook_inbox SET processed_at = NOW(), error = NULL \
             WHERE id = $1",
        )
        .bind(id)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        if result.rows_affected() == 0 {
            return Err(not_found("webhook", id));
        }
        Ok(())
    }

    async fn mark_webhook_failed(&self, id: Uuid, error: &str) -> Result<(), StoreError> {
        let result = sqlx::query("UPDATE dp_webhook_inbox SET error = $2 WHERE id = $1")
            .bind(id)
            .bind(error)
            .execute(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        if result.rows_affected() == 0 {
            return Err(not_found("webhook", id));
        }
        Ok(())
    }

    // ---- pins (SCOPE-PROJECTS §6.3) ------------------------------------

    async fn list_pins_for_user(&self, user_id: Uuid) -> Result<Vec<Pin>, StoreError> {
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

    async fn add_pin(&self, pin: &Pin) -> Result<Pin, StoreError> {
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

    async fn remove_pin(
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

    async fn reorder_pins(
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

    // ---- per-user settings (migration 0029) ---------------------

    async fn list_user_settings(
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

    async fn get_user_setting(
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

    async fn upsert_user_setting(
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

    async fn delete_user_setting(
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

    // ---- issue mutations (SCOPE-PROJECTS §8.2 + §8.5 + §13.7) ----

    async fn try_acquire_issue_pending_remote(
        &self,
        issue_id: Uuid,
        expected_version: i64,
        actor_user_id: Uuid,
    ) -> Result<Option<i64>, StoreError> {
        // One atomic statement does the §8.2 step 5 CAS: bump
        // version, raise pending_remote, stamp _at + _actor. The
        // WHERE clause rejects both `expected_version` mismatch
        // and a second concurrent writer (`pending_remote = false`
        // guard). RETURNING gives us the post-bump version so the
        // caller can plumb it into the IssueMutation audit row.
        let row: Option<(i64,)> = sqlx::query_as(
            "UPDATE dp_issues
                SET version = version + 1,
                    pending_remote = TRUE,
                    pending_remote_at = now(),
                    pending_remote_actor = $3
              WHERE id = $1
                AND version = $2
                AND pending_remote = FALSE
              RETURNING version",
        )
        .bind(issue_id)
        .bind(expected_version)
        .bind(actor_user_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(row.map(|(v,)| v))
    }

    async fn release_issue_pending_remote(
        &self,
        issue_id: Uuid,
        bump_version_again: bool,
    ) -> Result<i64, StoreError> {
        // §8.2 step 7 (success) clears the flag only; §8.2 step 8
        // (failure) additionally bumps `version` again so any
        // concurrent reader sees the rollback as a change. The
        // CHECK constraint dp_issues_pending_remote_consistent
        // means we have to NULL all three pending_* columns
        // together.
        let sql = if bump_version_again {
            "UPDATE dp_issues
                SET pending_remote = FALSE,
                    pending_remote_at = NULL,
                    pending_remote_actor = NULL,
                    version = version + 1
              WHERE id = $1
              RETURNING version"
        } else {
            "UPDATE dp_issues
                SET pending_remote = FALSE,
                    pending_remote_at = NULL,
                    pending_remote_actor = NULL
              WHERE id = $1
              RETURNING version"
        };
        let row: Option<(i64,)> = sqlx::query_as(sql)
            .bind(issue_id)
            .fetch_optional(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        match row {
            Some((v,)) => Ok(v),
            None => Err(not_found("issue", issue_id)),
        }
    }

    async fn get_issue_version(&self, issue_id: Uuid) -> Result<i64, StoreError> {
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT version FROM dp_issues WHERE id = $1")
                .bind(issue_id)
                .fetch_optional(self.pool.sqlx())
                .await
                .map_err(map_sqlx)?;
        row.map(|(v,)| v).ok_or_else(|| not_found("issue", issue_id))
    }

    async fn list_issues_with_pending_remote_older_than(
        &self,
        cutoff: DateTime<Utc>,
    ) -> Result<Vec<PendingRemoteIssue>, StoreError> {
        // Partial index `dp_issues_pending_remote_idx` covers this
        // exactly — empty / near-empty in steady state.
        let rows = sqlx::query(
            "SELECT id, repo_id, version, pending_remote_actor, pending_remote_at
               FROM dp_issues
              WHERE pending_remote = TRUE
                AND pending_remote_at < $1
              ORDER BY pending_remote_at ASC",
        )
        .bind(cutoff)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let issue_id: Uuid = r.try_get("id").map_err(map_sqlx)?;
            let repo_id: Uuid = r.try_get("repo_id").map_err(map_sqlx)?;
            let version: i64 = r.try_get("version").map_err(map_sqlx)?;
            // `pending_remote_actor` is NOT NULL whenever
            // `pending_remote = TRUE` per the CHECK constraint, so
            // the unwrap-via-Option is safe.
            let actor_user_id: Uuid =
                r.try_get("pending_remote_actor").map_err(map_sqlx)?;
            let pending_remote_at: DateTime<Utc> =
                r.try_get("pending_remote_at").map_err(map_sqlx)?;
            out.push(PendingRemoteIssue {
                issue_id,
                repo_id,
                version,
                actor_user_id,
                pending_remote_at,
            });
        }
        Ok(out)
    }

    async fn record_issue_mutation(
        &self,
        mutation: &IssueMutation,
    ) -> Result<IssueMutation, StoreError> {
        sqlx::query(
            "INSERT INTO dp_issue_mutations (
                 id, actor_user_id, issue_id, repo_id,
                 op, version_before, version_after, diff, result,
                 github_delivery_id, error,
                 created_at, finished_at
             ) VALUES (
                 $1, $2, $3, $4,
                 $5, $6, $7, $8, $9,
                 $10, $11,
                 $12, $13
             )",
        )
        .bind(mutation.id)
        .bind(mutation.actor_user_id)
        .bind(mutation.issue_id)
        .bind(mutation.repo_id)
        .bind(issue_mutation_op_to_text(mutation.op))
        .bind(mutation.version_before)
        .bind(mutation.version_after)
        .bind(&mutation.diff)
        .bind(issue_mutation_result_to_text(mutation.result))
        .bind(mutation.github_delivery_id.as_deref())
        .bind(mutation.error.as_deref())
        .bind(mutation.created_at)
        .bind(mutation.finished_at)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(mutation.clone())
    }

    async fn update_issue_mutation_result(
        &self,
        id: Uuid,
        result: IssueMutationResult,
        github_delivery_id: Option<&str>,
        error: Option<&str>,
    ) -> Result<(), StoreError> {
        // Stamp `finished_at = now()` whenever the row leaves
        // `pending` — the CHECK on the table requires this. We
        // pass `now()` from Postgres, not the host's clock, so the
        // sweeper's audit row timestamp matches the wall-clock
        // observation.
        let n = sqlx::query(
            "UPDATE dp_issue_mutations
                SET result = $2,
                    github_delivery_id = COALESCE($3, github_delivery_id),
                    error = COALESCE($4, error),
                    finished_at = now()
              WHERE id = $1
                AND result = 'pending'",
        )
        .bind(id)
        .bind(issue_mutation_result_to_text(result))
        .bind(github_delivery_id)
        .bind(error)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        if n.rows_affected() == 0 {
            // Either the id is bogus or the row already left
            // `pending`. The sweeper / handler interleave is
            // designed so this is never a race; surface it
            // explicitly so a bug shows up loudly.
            return Err(not_found("dp_issue_mutations(pending)", id));
        }
        Ok(())
    }

    async fn list_pending_issue_mutations_older_than(
        &self,
        cutoff: DateTime<Utc>,
    ) -> Result<Vec<IssueMutation>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, actor_user_id, issue_id, repo_id, op,
                    version_before, version_after, diff, result,
                    github_delivery_id, error, created_at, finished_at
               FROM dp_issue_mutations
              WHERE result = 'pending'
                AND created_at < $1
              ORDER BY created_at ASC",
        )
        .bind(cutoff)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_issue_mutation).collect()
    }

    // ---- §13.7 reconciler guard + webhook replay buffer --------------

    async fn find_repo_id_by_github_id(
        &self,
        github_repo_id: i64,
    ) -> Result<Option<Uuid>, StoreError> {
        // `dp_repos.github_id` is UNIQUE — index probe.
        let row: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM dp_repos WHERE github_id = $1")
                .bind(github_repo_id)
                .fetch_optional(self.pool.sqlx())
                .await
                .map_err(map_sqlx)?;
        Ok(row.map(|(id,)| id))
    }

    async fn find_issue_id_by_repo_and_github_id(
        &self,
        repo_id: Uuid,
        github_issue_id: i64,
    ) -> Result<Option<Uuid>, StoreError> {
        // The `(repo_id, github_id)` UNIQUE on `dp_issues` (per
        // `0001_init.sql`) makes this an index-only probe.
        let row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM dp_issues WHERE repo_id = $1 AND github_id = $2",
        )
        .bind(repo_id)
        .bind(github_issue_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(row.map(|(id,)| id))
    }

    async fn is_issue_pending_remote_fresh(
        &self,
        issue_id: Uuid,
        timeout: chrono::Duration,
    ) -> Result<bool, StoreError> {
        // Push the cutoff comparison into SQL so `now()` stays the
        // same clock the §8.2 CAS used to stamp `pending_remote_at`.
        // The seconds bind is i64 — saturating because chrono's
        // Duration can in principle hold values that won't fit, but
        // the production timeout knob is in tens of seconds.
        let secs = timeout.num_seconds().max(0);
        let row: Option<(bool,)> = sqlx::query_as(
            "SELECT (pending_remote
                  AND pending_remote_at IS NOT NULL
                  AND pending_remote_at >= now() - make_interval(secs => $2))
               FROM dp_issues
              WHERE id = $1",
        )
        .bind(issue_id)
        .bind(secs)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(row.map(|(b,)| b).unwrap_or(false))
    }

    async fn buffer_pending_remote_webhook(
        &self,
        issue_id: Uuid,
        delivery: &WebhookDelivery,
    ) -> Result<(), StoreError> {
        // No `ON CONFLICT` — duplicate `delivery_id` is a benign
        // re-deflection of the same logical webhook, and surfacing
        // the conflict matches the inbox's contract (the caller
        // translates it to "already buffered, drop").
        sqlx::query(
            "INSERT INTO dp_pending_remote_webhook_buffer \
                 (id, issue_id, delivery_id, event, payload, received_at) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(delivery.id)
        .bind(issue_id)
        .bind(&delivery.delivery_id)
        .bind(&delivery.event)
        .bind(&delivery.payload)
        .bind(delivery.received_at)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    async fn take_buffered_webhooks_for_issue(
        &self,
        issue_id: Uuid,
    ) -> Result<Vec<WebhookDelivery>, StoreError> {
        // `DELETE … RETURNING` is the at-least-once-replay primitive
        // §13.7 calls for: the buffered rows leave the table in the
        // same statement that produces the replay batch, so a crash
        // between this call and `apply_delivery` loses the buffer
        // copy. GitHub's at-least-once redelivery + the next
        // reconciler tick make this acceptable (the authoritative
        // state will be re-observed shortly).
        let rows = sqlx::query(
            "DELETE FROM dp_pending_remote_webhook_buffer \
              WHERE issue_id = $1 \
             RETURNING id, delivery_id, event, payload, received_at, \
                       NULL::timestamptz AS processed_at, \
                       NULL::text       AS error",
        )
        .bind(issue_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        // Oldest first — preserves the relative ordering of inbound
        // GitHub events on the issue. We sort in-memory because the
        // RETURNING clause does not guarantee row order.
        let mut out: Vec<WebhookDelivery> =
            rows.iter().map(row_to_webhook_delivery).collect::<Result<_, _>>()?;
        out.sort_by_key(|d| d.received_at);
        Ok(out)
    }

    // ---- issue dates (triage slice 2 — §3.10) --------------------

    async fn get_issue_dates(
        &self,
        issue_id: Uuid,
    ) -> Result<Option<IssueDates>, StoreError> {
        let row = sqlx::query(
            r#"SELECT issue_id, start_at, due_at, mirror_node_id,
                      mirror_synced_at, mirror_error, updated_at
                 FROM dp_issue_dates WHERE issue_id = $1"#,
        )
        .bind(issue_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(row.map(|r| row_to_issue_dates(&r)).transpose()?)
    }

    async fn upsert_issue_dates(
        &self,
        issue_id: Uuid,
        start_at: Option<DateTime<Utc>>,
        due_at: Option<DateTime<Utc>>,
    ) -> Result<IssueDates, StoreError> {
        // The CHECK on the table guards start <= due; surface a
        // violation as Invalid so the handler can return 400
        // rather than a generic backend error.
        let row = sqlx::query(
            r#"
            INSERT INTO dp_issue_dates (issue_id, start_at, due_at, updated_at)
            VALUES ($1, $2, $3, now())
            ON CONFLICT (issue_id) DO UPDATE
              SET start_at  = EXCLUDED.start_at,
                  due_at    = EXCLUDED.due_at,
                  updated_at = now()
            RETURNING issue_id, start_at, due_at, mirror_node_id,
                      mirror_synced_at, mirror_error, updated_at
            "#,
        )
        .bind(issue_id)
        .bind(start_at)
        .bind(due_at)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db)
                if db.constraint().is_some()
                    && db.message().contains("dp_issue_dates_check") =>
            {
                invalid("start_at must be <= due_at")
            }
            _ => map_sqlx(e),
        })?;
        row_to_issue_dates(&row)
    }

    async fn record_issue_dates_mirror_result(
        &self,
        issue_id: Uuid,
        outcome: IssueDatesMirrorOutcome<'_>,
    ) -> Result<(), StoreError> {
        match outcome {
            IssueDatesMirrorOutcome::Success { node_id } => {
                sqlx::query(
                    r#"UPDATE dp_issue_dates
                          SET mirror_node_id   = COALESCE($2, mirror_node_id),
                              mirror_synced_at = now(),
                              mirror_error     = NULL
                        WHERE issue_id = $1"#,
                )
                .bind(issue_id)
                .bind(node_id)
                .execute(self.pool.sqlx())
                .await
                .map_err(map_sqlx)?;
            }
            IssueDatesMirrorOutcome::Failure { error } => {
                sqlx::query(
                    r#"UPDATE dp_issue_dates
                          SET mirror_error = $2
                        WHERE issue_id = $1"#,
                )
                .bind(issue_id)
                .bind(error)
                .execute(self.pool.sqlx())
                .await
                .map_err(map_sqlx)?;
            }
        }
        Ok(())
    }

    // Note: the legacy per-repo board link surface is retired —
    // migration 0026 dropped the backing table, the dp-rest admin
    // handler is gone (stage 11), and the trait no longer exposes
    // get / upsert / delete for it. Project-scoped state lives on
    // `dp_project_board_links` (migration 0025); reach for
    // `list_board_links` / `get_board_item` instead.

    async fn set_issue_github_node_id(
        &self,
        issue_id: Uuid,
        node_id: &str,
    ) -> Result<(), StoreError> {
        // Only stamp when currently NULL — the column is immutable
        // once known. A racing webhook upsert that observes the
        // same value is a harmless no-op.
        sqlx::query(
            r#"UPDATE dp_issues
                  SET github_node_id = $2
                WHERE id = $1
                  AND github_node_id IS NULL"#,
        )
        .bind(issue_id)
        .bind(node_id)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    async fn enqueue_projectv2_mirror_task(
        &self,
        issue_id: Uuid,
        repo_id: Uuid,
        kind: ProjectV2MirrorTaskKind,
        payload: serde_json::Value,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"INSERT INTO dp_projectv2_mirror_tasks
                   (issue_id, repo_id, kind, payload)
               VALUES ($1, $2, $3, $4)"#,
        )
        .bind(issue_id)
        .bind(repo_id)
        .bind(kind.as_str())
        .bind(payload)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    async fn claim_projectv2_mirror_tasks(
        &self,
        max: i64,
    ) -> Result<Vec<ProjectV2MirrorTask>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT id, issue_id, repo_id, kind, payload, attempts,
                      last_error, enqueued_at, processed_at
                 FROM dp_projectv2_mirror_tasks
                WHERE processed_at IS NULL
             ORDER BY enqueued_at ASC
                LIMIT $1
                FOR UPDATE SKIP LOCKED"#,
        )
        .bind(max)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_projectv2_mirror_task).collect()
    }

    // ---- projects (linear-projects-v2.md slice A) ----------------

    async fn list_projects(
        &self,
        filter: &ProjectListFilter,
    ) -> Result<Vec<Project>, StoreError> {
        let q_norm = filter
            .q
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let status_text = filter.status.map(|s| s.as_str().to_string());
        let rows = sqlx::query(
            r#"SELECT id, org_id, name, description, lead_user_id, status,
                      start_at, due_at, issue_count, closed_issue_count,
                      created_by, created_at, updated_at, version,
                      primary_milestone_id
                 FROM dp_projects
                WHERE ($1::uuid IS NULL OR org_id = $1)
                  AND ($2::text IS NULL OR status = $2)
                  AND ($3::text IS NULL OR name ILIKE '%' || $3 || '%')
             ORDER BY
                  CASE status
                      WHEN 'active'   THEN 0
                      WHEN 'backlog'  THEN 1
                      WHEN 'done'     THEN 2
                      WHEN 'archived' THEN 3
                  END,
                  due_at ASC NULLS LAST,
                  name ASC
                LIMIT $4 OFFSET $5"#,
        )
        .bind(filter.org_id)
        .bind(status_text)
        .bind(q_norm)
        .bind(filter.limit)
        .bind(filter.offset)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_project).collect()
    }

    async fn count_projects(
        &self,
        filter: &ProjectListFilter,
    ) -> Result<i64, StoreError> {
        let q_norm = filter
            .q
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let status_text = filter.status.map(|s| s.as_str().to_string());
        let (count,): (i64,) = sqlx::query_as(
            r#"SELECT COUNT(*)::bigint
                 FROM dp_projects
                WHERE ($1::uuid IS NULL OR org_id = $1)
                  AND ($2::text IS NULL OR status = $2)
                  AND ($3::text IS NULL OR name ILIKE '%' || $3 || '%')"#,
        )
        .bind(filter.org_id)
        .bind(status_text)
        .bind(q_norm)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(count)
    }

    async fn list_project_portfolio(
        &self,
        filter: &PortfolioQueryFilter,
    ) -> Result<Vec<PortfolioRawRow>, StoreError> {
        let sql = dp_reports::build_project_portfolio_sql(filter.sort);
        let statuses: Vec<String> = filter
            .statuses
            .iter()
            .map(|s| s.as_str().to_string())
            .collect();
        let (window_start, window_end) = match filter.window {
            Some((s, e)) => (Some(s), Some(e)),
            None => (None, None),
        };
        let rows = sqlx::query(&sql)
            .bind(&filter.orgs)
            .bind(&statuses)
            .bind(window_start)
            .bind(window_end)
            .bind(filter.hide_overdue)
            .bind(filter.now)
            .bind(filter.limit)
            .bind(filter.offset)
            .fetch_all(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        rows.iter().map(row_to_portfolio_raw).collect()
    }

    async fn get_project(&self, id: Uuid) -> Result<Option<Project>, StoreError> {
        let row = sqlx::query(
            r#"SELECT id, org_id, name, description, lead_user_id, status,
                      start_at, due_at, issue_count, closed_issue_count,
                      created_by, created_at, updated_at, version,
                      primary_milestone_id
                 FROM dp_projects WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.map(|r| row_to_project(&r)).transpose()
    }

    async fn create_project(
        &self,
        upsert: &ProjectUpsert,
    ) -> Result<Project, StoreError> {
        let row = sqlx::query(
            r#"INSERT INTO dp_projects
                   (id, org_id, name, description, lead_user_id, status,
                    start_at, due_at, created_by, created_at, updated_at, version)
               VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6, $7, $8,
                       now(), now(), 1)
               RETURNING id, org_id, name, description, lead_user_id, status,
                         start_at, due_at, issue_count, closed_issue_count,
                         created_by, created_at, updated_at, version,
                         primary_milestone_id"#,
        )
        .bind(upsert.org_id)
        .bind(&upsert.name)
        .bind(upsert.description.as_deref())
        .bind(upsert.lead_user_id)
        .bind(upsert.status.as_str())
        .bind(upsert.start_at)
        .bind(upsert.due_at)
        .bind(upsert.created_by)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db)
                if db.constraint().is_some()
                    && (db.message().contains("dp_projects_check")
                        || db.message().contains("dp_projects")
                            && db.message().contains("check")) =>
            {
                invalid("project violates a CHECK constraint (status / dates / counts)")
            }
            _ => map_sqlx(e),
        })?;
        row_to_project(&row)
    }

    async fn update_project(
        &self,
        id: Uuid,
        expected_version: i64,
        upsert: &ProjectUpsert,
    ) -> Result<Project, StoreError> {
        // §8.2 CAS: WHERE id = ? AND version = ?. A miss is either
        // "row gone" (NotFound) or "stale version" (Conflict). One
        // extra SELECT distinguishes them; cheaper than a serializable
        // transaction and lets the REST layer pick its 404 vs 409.
        let row = sqlx::query(
            r#"UPDATE dp_projects
                  SET name         = $3,
                      description  = $4,
                      lead_user_id = $5,
                      status       = $6,
                      start_at     = $7,
                      due_at       = $8,
                      version      = version + 1,
                      updated_at   = now()
                WHERE id = $1 AND version = $2
               RETURNING id, org_id, name, description, lead_user_id, status,
                         start_at, due_at, issue_count, closed_issue_count,
                         created_by, created_at, updated_at, version,
                         primary_milestone_id"#,
        )
        .bind(id)
        .bind(expected_version)
        .bind(&upsert.name)
        .bind(upsert.description.as_deref())
        .bind(upsert.lead_user_id)
        .bind(upsert.status.as_str())
        .bind(upsert.start_at)
        .bind(upsert.due_at)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_project(&r),
            None => disambiguate_project_miss(self, id).await,
        }
    }

    async fn archive_project(
        &self,
        id: Uuid,
        expected_version: i64,
    ) -> Result<Project, StoreError> {
        // Idempotent: archiving an already-archived row returns the
        // row as-is without a version bump (§9.2 wording). Anything
        // else CAS-gates on version.
        let current = self.get_project(id).await?;
        let Some(current) = current else {
            return Err(not_found("project", id));
        };
        if current.status == ProjectStatus::Archived {
            // No-op: caller's expected_version may even be stale but
            // there is nothing to bump. Return the row unchanged.
            return Ok(current);
        }
        let row = sqlx::query(
            r#"UPDATE dp_projects
                  SET status     = 'archived',
                      version    = version + 1,
                      updated_at = now()
                WHERE id = $1 AND version = $2
               RETURNING id, org_id, name, description, lead_user_id, status,
                         start_at, due_at, issue_count, closed_issue_count,
                         created_by, created_at, updated_at, version,
                         primary_milestone_id"#,
        )
        .bind(id)
        .bind(expected_version)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_project(&r),
            None => disambiguate_project_miss(self, id).await,
        }
    }

    async fn add_issues_to_project(
        &self,
        project_id: Uuid,
        expected_version: i64,
        issue_ids: &[Uuid],
        actor: Option<Uuid>,
    ) -> Result<ProjectIssueAddOutcome, StoreError> {
        // One transaction so a concurrent writer cannot observe the
        // half-bumped counts or race the version gate. `FOR UPDATE`
        // serialises against any other writer touching this project
        // row.
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;

        let project_row: Option<(Uuid, i64, String)> = sqlx::query_as(
            "SELECT org_id, version, status FROM dp_projects WHERE id = $1 FOR UPDATE",
        )
        .bind(project_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let (project_org, current_version, _status) = match project_row {
            Some(r) => r,
            None => return Err(not_found("project", project_id)),
        };
        if current_version != expected_version {
            return Err(StoreError::Conflict(format!(
                "project version mismatch: expected {expected_version}, found {current_version}"
            )));
        }

        let mut added: Vec<Uuid> = Vec::new();
        let mut skipped: Vec<ProjectIssueAddSkip> = Vec::new();

        for &issue_id in issue_ids {
            // Resolve the issue plus its current membership (if any)
            // in a single round-trip so the per-row decision below
            // doesn't need a second query.
            let row: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(
                r#"SELECT i.org_id, pi.project_id
                     FROM dp_issues i
                     LEFT JOIN dp_project_issues pi ON pi.issue_id = i.id
                    WHERE i.id = $1"#,
            )
            .bind(issue_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_sqlx)?;

            let Some((issue_org, existing_project)) = row else {
                skipped.push(ProjectIssueAddSkip {
                    issue_id,
                    reason: "unknown_issue".into(),
                    existing_project_id: None,
                });
                continue;
            };
            if issue_org != project_org {
                skipped.push(ProjectIssueAddSkip {
                    issue_id,
                    reason: "cross_org".into(),
                    existing_project_id: None,
                });
                continue;
            }
            if let Some(existing) = existing_project {
                // Already attached — either to this project (idempotent
                // re-add) or to another. v1 collapses both to
                // `already_in_project`; the existing project id lets
                // the UI offer `Move here?` when it's a different one.
                skipped.push(ProjectIssueAddSkip {
                    issue_id,
                    reason: "already_in_project".into(),
                    existing_project_id: Some(existing),
                });
                continue;
            }

            sqlx::query(
                r#"INSERT INTO dp_project_issues (project_id, issue_id, added_by, added_at)
                       VALUES ($1, $2, $3, now())"#,
            )
            .bind(project_id)
            .bind(issue_id)
            .bind(actor)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx)?;
            added.push(issue_id);
        }

        // Recompute counts + bump version inside the same tx so the
        // returned outcome reflects committed state. We only bump
        // `version` when at least one issue was added — the §7.2
        // contract.
        if !added.is_empty() {
            sqlx::query(
                r#"UPDATE dp_projects p
                      SET issue_count = (
                              SELECT COUNT(*) FROM dp_project_issues
                               WHERE project_id = p.id),
                          closed_issue_count = (
                              SELECT COUNT(*)
                                FROM dp_project_issues pi
                                JOIN dp_issues i ON i.id = pi.issue_id
                               WHERE pi.project_id = p.id AND i.state = 'closed'),
                          version    = version + 1,
                          updated_at = now()
                    WHERE id = $1"#,
            )
            .bind(project_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx)?;
        }

        tx.commit().await.map_err(map_sqlx)?;

        Ok(ProjectIssueAddOutcome { added, skipped })
    }

    async fn remove_issue_from_project(
        &self,
        project_id: Uuid,
        issue_id: Uuid,
        expected_version: i64,
    ) -> Result<Project, StoreError> {
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;

        let project_row: Option<(i64,)> = sqlx::query_as(
            "SELECT version FROM dp_projects WHERE id = $1 FOR UPDATE",
        )
        .bind(project_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let current_version = match project_row {
            Some((v,)) => v,
            None => return Err(not_found("project", project_id)),
        };
        if current_version != expected_version {
            return Err(StoreError::Conflict(format!(
                "project version mismatch: expected {expected_version}, found {current_version}"
            )));
        }

        let res = sqlx::query(
            "DELETE FROM dp_project_issues WHERE project_id = $1 AND issue_id = $2",
        )
        .bind(project_id)
        .bind(issue_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(not_found("project_issue", issue_id));
        }

        let row = sqlx::query(
            r#"UPDATE dp_projects p
                  SET issue_count = (
                          SELECT COUNT(*) FROM dp_project_issues
                           WHERE project_id = p.id),
                      closed_issue_count = (
                          SELECT COUNT(*)
                            FROM dp_project_issues pi
                            JOIN dp_issues i ON i.id = pi.issue_id
                           WHERE pi.project_id = p.id AND i.state = 'closed'),
                      version    = version + 1,
                      updated_at = now()
                WHERE id = $1
               RETURNING id, org_id, name, description, lead_user_id, status,
                         start_at, due_at, issue_count, closed_issue_count,
                         created_by, created_at, updated_at, version,
                         primary_milestone_id"#,
        )
        .bind(project_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let project = row_to_project(&row)?;
        tx.commit().await.map_err(map_sqlx)?;
        Ok(project)
    }

    async fn get_project_for_issue(
        &self,
        issue_id: Uuid,
    ) -> Result<Option<Project>, StoreError> {
        let row = sqlx::query(
            r#"SELECT p.id, p.org_id, p.name, p.description, p.lead_user_id, p.status,
                      p.start_at, p.due_at, p.issue_count, p.closed_issue_count,
                      p.created_by, p.created_at, p.updated_at, p.version,
                      p.primary_milestone_id
                 FROM dp_projects p
                 JOIN dp_project_issues pi ON pi.project_id = p.id
                WHERE pi.issue_id = $1"#,
        )
        .bind(issue_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.map(|r| row_to_project(&r)).transpose()
    }

    async fn list_issue_ids_for_project(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<Uuid>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT issue_id
                 FROM dp_project_issues
                WHERE project_id = $1
             ORDER BY added_at ASC, issue_id ASC"#,
        )
        .bind(project_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter()
            .map(|r| r.try_get::<Uuid, _>("issue_id").map_err(map_sqlx))
            .collect()
    }

    async fn list_project_issue_tag_values(
        &self,
        project_id: Uuid,
        tag_key: &str,
    ) -> Result<Vec<(Uuid, String)>, StoreError> {
        // Walk the project's issue ids through dp_tag_links → dp_tags
        // and pull `(issue_id, value)` for the requested kv key.
        // Archived tags are excluded so the workbench's bucket list
        // tracks live data only (PROJECT-VIEW.md §5.1).
        let rows = sqlx::query(
            r#"SELECT tl.target_issue_id AS issue_id, t.value AS value
                 FROM dp_project_issues pi
                 JOIN dp_tag_links tl ON tl.target_issue_id = pi.issue_id
                                     AND tl.kind = 'issue'
                 JOIN dp_tags t       ON t.id = tl.tag_id
                                     AND t.kind = 'kv'
                                     AND t.key = $2
                                     AND t.archived_at IS NULL
                WHERE pi.project_id = $1"#,
        )
        .bind(project_id)
        .bind(tag_key)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter()
            .map(|r| {
                let id: Uuid = r.try_get("issue_id").map_err(map_sqlx)?;
                let v: String = r.try_get("value").map_err(map_sqlx)?;
                Ok((id, v))
            })
            .collect()
    }

    async fn list_issue_tag_values(
        &self,
        issue_ids: &[Uuid],
        tag_key: &str,
    ) -> Result<Vec<(Uuid, String)>, StoreError> {
        if issue_ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(
            r#"SELECT tl.target_issue_id AS issue_id, t.value AS value
                 FROM dp_tag_links tl
                 JOIN dp_tags t ON t.id = tl.tag_id
                                AND t.kind = 'kv'
                                AND t.key = $2
                                AND t.archived_at IS NULL
                WHERE tl.kind = 'issue'
                  AND tl.target_issue_id = ANY($1)"#,
        )
        .bind(issue_ids)
        .bind(tag_key)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter()
            .map(|r| {
                let id: Uuid = r.try_get("issue_id").map_err(map_sqlx)?;
                let v: String = r.try_get("value").map_err(map_sqlx)?;
                Ok((id, v))
            })
            .collect()
    }

    async fn list_project_issue_tag_keys(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<String>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT DISTINCT t.key AS key
                 FROM dp_project_issues pi
                 JOIN dp_tag_links tl ON tl.target_issue_id = pi.issue_id
                                     AND tl.kind = 'issue'
                 JOIN dp_tags t       ON t.id = tl.tag_id
                                     AND t.kind = 'kv'
                                     AND t.archived_at IS NULL
                                     AND t.key IS NOT NULL
                WHERE pi.project_id = $1
             ORDER BY key ASC"#,
        )
        .bind(project_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter()
            .map(|r| r.try_get::<String, _>("key").map_err(map_sqlx))
            .collect()
    }

    // ---- project saved views (PROJECT-VIEW.md §6.1) --------------

    async fn list_project_views(
        &self,
        project_id: Uuid,
        owner_user_id: Uuid,
    ) -> Result<Vec<ProjectView>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT id, project_id, owner_user_id, name, group_by,
                      filter_json, sort, position, visibility,
                      start_date, due_date, categories,
                      created_at, updated_at
                 FROM dp_project_views
                WHERE project_id = $1 AND owner_user_id = $2
             ORDER BY position ASC, created_at ASC"#,
        )
        .bind(project_id)
        .bind(owner_user_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(project_view_from_row).collect()
    }

    async fn get_project_view(
        &self,
        id: Uuid,
        owner_user_id: Uuid,
    ) -> Result<Option<ProjectView>, StoreError> {
        let row_opt = sqlx::query(
            r#"SELECT id, project_id, owner_user_id, name, group_by,
                      filter_json, sort, position, visibility,
                      start_date, due_date, categories,
                      created_at, updated_at
                 FROM dp_project_views
                WHERE id = $1 AND owner_user_id = $2"#,
        )
        .bind(id)
        .bind(owner_user_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_opt.as_ref().map(project_view_from_row).transpose()
    }

    async fn create_project_view(
        &self,
        project_id: Uuid,
        owner_user_id: Uuid,
        upsert: &ProjectViewUpsert,
    ) -> Result<ProjectView, StoreError> {
        let id = Uuid::new_v4();
        let filter_json = serde_json::to_value(&upsert.filter_clauses)
            .map_err(|e| StoreError::Invalid(format!("filter_json encode: {e}")))?;
        let categories_json = serde_json::to_value(&upsert.categories)
            .map_err(|e| StoreError::Invalid(format!("categories encode: {e}")))?;
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;
        // Append-at-end position. Per-(project, owner) so two users'
        // tab strips never collide on position.
        let (next_pos,): (i64,) = sqlx::query_as(
            r#"SELECT COUNT(*)::bigint
                 FROM dp_project_views
                WHERE project_id = $1 AND owner_user_id = $2"#,
        )
        .bind(project_id)
        .bind(owner_user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let row = sqlx::query(
            r#"INSERT INTO dp_project_views
                  (id, project_id, owner_user_id, name, group_by,
                   filter_json, sort, position, visibility,
                   start_date, due_date, categories)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
               RETURNING id, project_id, owner_user_id, name, group_by,
                         filter_json, sort, position, visibility,
                         start_date, due_date, categories,
                         created_at, updated_at"#,
        )
        .bind(id)
        .bind(project_id)
        .bind(owner_user_id)
        .bind(&upsert.name)
        .bind(&upsert.group_by)
        .bind(&filter_json)
        .bind(if upsert.sort.is_empty() {
            "updated_desc"
        } else {
            upsert.sort.as_str()
        })
        .bind(next_pos as i32)
        .bind(upsert.visibility.as_str())
        .bind(upsert.start_date)
        .bind(upsert.due_date)
        .bind(&categories_json)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        tx.commit().await.map_err(map_sqlx)?;
        project_view_from_row(&row)
    }

    async fn update_project_view(
        &self,
        id: Uuid,
        owner_user_id: Uuid,
        upsert: &ProjectViewUpsert,
    ) -> Result<ProjectView, StoreError> {
        let filter_json = serde_json::to_value(&upsert.filter_clauses)
            .map_err(|e| StoreError::Invalid(format!("filter_json encode: {e}")))?;
        let categories_json = serde_json::to_value(&upsert.categories)
            .map_err(|e| StoreError::Invalid(format!("categories encode: {e}")))?;
        let row_opt = sqlx::query(
            r#"UPDATE dp_project_views
                  SET name = $3,
                      group_by = $4,
                      filter_json = $5,
                      sort = $6,
                      visibility = $7,
                      start_date = $8,
                      due_date = $9,
                      categories = $10,
                      updated_at = now()
                WHERE id = $1 AND owner_user_id = $2
                RETURNING id, project_id, owner_user_id, name, group_by,
                          filter_json, sort, position, visibility,
                          start_date, due_date, categories,
                          created_at, updated_at"#,
        )
        .bind(id)
        .bind(owner_user_id)
        .bind(&upsert.name)
        .bind(&upsert.group_by)
        .bind(&filter_json)
        .bind(if upsert.sort.is_empty() {
            "updated_desc"
        } else {
            upsert.sort.as_str()
        })
        .bind(upsert.visibility.as_str())
        .bind(upsert.start_date)
        .bind(upsert.due_date)
        .bind(&categories_json)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row_opt {
            Some(r) => project_view_from_row(&r),
            None => Err(not_found("project_view", id)),
        }
    }

    async fn delete_project_view(
        &self,
        id: Uuid,
        owner_user_id: Uuid,
    ) -> Result<(), StoreError> {
        let res = sqlx::query(
            r#"DELETE FROM dp_project_views
                WHERE id = $1 AND owner_user_id = $2"#,
        )
        .bind(id)
        .bind(owner_user_id)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(not_found("project_view", id));
        }
        Ok(())
    }

    async fn reorder_project_views(
        &self,
        project_id: Uuid,
        owner_user_id: Uuid,
        ordered_ids: &[Uuid],
    ) -> Result<Vec<ProjectView>, StoreError> {
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;
        // Lock the caller's views so a concurrent reorder / create
        // can't shift the set out from under us.
        let existing: Vec<(Uuid,)> = sqlx::query_as(
            r#"SELECT id FROM dp_project_views
                WHERE project_id = $1 AND owner_user_id = $2
                FOR UPDATE"#,
        )
        .bind(project_id)
        .bind(owner_user_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let existing_set: std::collections::HashSet<Uuid> =
            existing.into_iter().map(|(i,)| i).collect();
        let req_set: std::collections::HashSet<Uuid> =
            ordered_ids.iter().copied().collect();
        if existing_set != req_set {
            return Err(StoreError::Invalid(
                "reorder ordered_ids must match the existing view set".into(),
            ));
        }
        // Two-phase rewrite to dodge the UNIQUE on (project_id,
        // owner_user_id, position) — none exists today but if it's
        // added the swap-via-negatives keeps us safe.
        for (idx, vid) in ordered_ids.iter().enumerate() {
            sqlx::query(
                r#"UPDATE dp_project_views
                      SET position = $3, updated_at = now()
                    WHERE id = $1 AND owner_user_id = $2"#,
            )
            .bind(vid)
            .bind(owner_user_id)
            .bind(-(idx as i32) - 1)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx)?;
        }
        for (idx, vid) in ordered_ids.iter().enumerate() {
            sqlx::query(
                r#"UPDATE dp_project_views
                      SET position = $3
                    WHERE id = $1 AND owner_user_id = $2"#,
            )
            .bind(vid)
            .bind(owner_user_id)
            .bind(idx as i32)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx)?;
        }
        let rows = sqlx::query(
            r#"SELECT id, project_id, owner_user_id, name, group_by,
                      filter_json, sort, position, visibility,
                      start_date, due_date, categories,
                      created_at, updated_at
                 FROM dp_project_views
                WHERE project_id = $1 AND owner_user_id = $2
             ORDER BY position ASC"#,
        )
        .bind(project_id)
        .bind(owner_user_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        tx.commit().await.map_err(map_sqlx)?;
        rows.iter().map(project_view_from_row).collect()
    }

    // ---- per-view (per-tab) issue membership ----------------------

    async fn list_issue_ids_for_view(
        &self,
        view_id: Uuid,
    ) -> Result<Vec<Uuid>, StoreError> {
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            r#"SELECT issue_id
                 FROM dp_project_view_issues
                WHERE view_id = $1
             ORDER BY added_at ASC, issue_id ASC"#,
        )
        .bind(view_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(rows.into_iter().map(|(i,)| i).collect())
    }

    async fn add_issues_to_view(
        &self,
        view_id: Uuid,
        issue_ids: &[Uuid],
    ) -> Result<(), StoreError> {
        if issue_ids.is_empty() {
            return Ok(());
        }
        // One round-trip via UNNEST; ON CONFLICT keeps the call
        // idempotent so retries after a partial network failure
        // don't churn `added_at`.
        sqlx::query(
            r#"INSERT INTO dp_project_view_issues (view_id, issue_id)
                    SELECT $1, x.issue_id
                      FROM UNNEST($2::uuid[]) AS x(issue_id)
                ON CONFLICT (view_id, issue_id) DO NOTHING"#,
        )
        .bind(view_id)
        .bind(issue_ids)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    async fn remove_issue_from_view(
        &self,
        view_id: Uuid,
        issue_id: Uuid,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"DELETE FROM dp_project_view_issues
                WHERE view_id = $1 AND issue_id = $2"#,
        )
        .bind(view_id)
        .bind(issue_id)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    // ---- project ↔ repo associations -----------------------------

    async fn list_project_repos(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<ProjectRepo>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT project_id, repo_id, added_by, added_at
                 FROM dp_project_repos
                WHERE project_id = $1
             ORDER BY added_at ASC, repo_id ASC"#,
        )
        .bind(project_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.into_iter()
            .map(|r| {
                Ok(ProjectRepo {
                    project_id: r.try_get("project_id").map_err(map_sqlx)?,
                    repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
                    added_by: r.try_get("added_by").map_err(map_sqlx)?,
                    added_at: r.try_get("added_at").map_err(map_sqlx)?,
                })
            })
            .collect()
    }

    async fn add_project_repo(
        &self,
        project_id: Uuid,
        repo_id: Uuid,
        actor: Option<Uuid>,
    ) -> Result<ProjectRepo, StoreError> {
        // ON CONFLICT DO NOTHING + RETURNING returns no row for an
        // existing PK; fall back to a SELECT so callers see the
        // pre-existing row.
        let row_opt = sqlx::query(
            r#"INSERT INTO dp_project_repos (project_id, repo_id, added_by)
               VALUES ($1, $2, $3)
               ON CONFLICT (project_id, repo_id) DO NOTHING
               RETURNING project_id, repo_id, added_by, added_at"#,
        )
        .bind(project_id)
        .bind(repo_id)
        .bind(actor)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        if let Some(r) = row_opt {
            return Ok(ProjectRepo {
                project_id: r.try_get("project_id").map_err(map_sqlx)?,
                repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
                added_by: r.try_get("added_by").map_err(map_sqlx)?,
                added_at: r.try_get("added_at").map_err(map_sqlx)?,
            });
        }
        let r = sqlx::query(
            r#"SELECT project_id, repo_id, added_by, added_at
                 FROM dp_project_repos
                WHERE project_id = $1 AND repo_id = $2"#,
        )
        .bind(project_id)
        .bind(repo_id)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(ProjectRepo {
            project_id: r.try_get("project_id").map_err(map_sqlx)?,
            repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
            added_by: r.try_get("added_by").map_err(map_sqlx)?,
            added_at: r.try_get("added_at").map_err(map_sqlx)?,
        })
    }

    async fn remove_project_repo(
        &self,
        project_id: Uuid,
        repo_id: Uuid,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"DELETE FROM dp_project_repos
                WHERE project_id = $1 AND repo_id = $2"#,
        )
        .bind(project_id)
        .bind(repo_id)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    // ---- project ↔ board mirror (linear-projects-v2.md slice B) --

    async fn list_board_links(
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

    async fn get_board_link(&self, id: Uuid) -> Result<Option<BoardLink>, StoreError> {
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

    async fn create_board_link(
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

    async fn delete_board_link(&self, id: Uuid) -> Result<(), StoreError> {
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

    async fn refresh_board_link_cache(
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

    async fn list_board_items_for_issue(
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

    async fn get_board_item(
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

    async fn record_board_item_result(
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

    // ---- tags + tag links (SCOPE-PROJECTS §7) -------------------

    async fn get_tag(&self, id: Uuid) -> Result<Tag, StoreError> {
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

    async fn create_tag(&self, tag: &Tag) -> Result<Tag, StoreError> {
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

    async fn update_tag(
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

    async fn list_tags_visible_to(
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

    async fn list_tag_links(
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

    async fn list_tag_links_for_targets(
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

    async fn add_tag_links(&self, links: &[TagLink]) -> Result<Vec<TagLink>, StoreError> {
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

    async fn remove_tag_links(&self, link_ids: &[Uuid]) -> Result<(), StoreError> {
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

    async fn resolve_tag_targets(
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

    // ---- milestones (tagging.md §9.3) -----------------------------

    async fn upsert_milestone(
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

    async fn list_milestones_for_repo(
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

    async fn list_project_milestones(
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

    async fn set_project_primary_milestone(
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

    async fn delete_milestone(
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

/// Resolve whether an `UPDATE dp_projects ... WHERE id = ? AND
/// version = ?` that affected zero rows was caused by the row going
/// away (NotFound) or by a stale `expected_version` (Conflict).
/// Pulled out so `update_project` and `archive_project` share one
/// place that picks the right `StoreError` variant.
async fn disambiguate_project_miss(
    store: &PgStore,
    id: Uuid,
) -> Result<Project, StoreError> {
    let existing: Option<(i64,)> =
        sqlx::query_as("SELECT version FROM dp_projects WHERE id = $1")
            .bind(id)
            .fetch_optional(store.pool().sqlx())
            .await
            .map_err(map_sqlx)?;
    match existing {
        Some((v,)) => Err(StoreError::Conflict(format!(
            "project version mismatch: row currently at version {v}"
        ))),
        None => Err(not_found("project", id)),
    }
}

fn row_to_project(r: &sqlx::postgres::PgRow) -> Result<Project, StoreError> {
    let status_text: String = r.try_get("status").map_err(map_sqlx)?;
    let status = ProjectStatus::from_str(&status_text)
        .ok_or_else(|| invalid(format!("unknown project status: {status_text}")))?;
    Ok(Project {
        id: r.try_get("id").map_err(map_sqlx)?,
        org_id: r.try_get("org_id").map_err(map_sqlx)?,
        name: r.try_get("name").map_err(map_sqlx)?,
        description: r.try_get("description").map_err(map_sqlx)?,
        lead_user_id: r.try_get("lead_user_id").map_err(map_sqlx)?,
        status,
        start_at: r.try_get("start_at").map_err(map_sqlx)?,
        due_at: r.try_get("due_at").map_err(map_sqlx)?,
        issue_count: r.try_get("issue_count").map_err(map_sqlx)?,
        closed_issue_count: r.try_get("closed_issue_count").map_err(map_sqlx)?,
        created_by: r.try_get("created_by").map_err(map_sqlx)?,
        created_at: r.try_get("created_at").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
        version: r.try_get("version").map_err(map_sqlx)?,
        primary_milestone_id: r.try_get("primary_milestone_id").map_err(map_sqlx)?,
    })
}

fn row_to_portfolio_raw(r: &sqlx::postgres::PgRow) -> Result<PortfolioRawRow, StoreError> {
    let status_text: String = r.try_get("status").map_err(map_sqlx)?;
    let status = ProjectStatus::from_str(&status_text)
        .ok_or_else(|| invalid(format!("unknown project status: {status_text}")))?;
    let lead_id: Option<Uuid> = r.try_get("lead_user_id").map_err(map_sqlx)?;
    let lead_login: Option<String> = r.try_get("lead_login").map_err(map_sqlx)?;
    let lead = match (lead_id, lead_login) {
        (Some(id), Some(login)) => Some((id, login)),
        _ => None,
    };
    Ok(PortfolioRawRow {
        id: r.try_get("id").map_err(map_sqlx)?,
        org_id: r.try_get("org_id").map_err(map_sqlx)?,
        org_login: r.try_get("org_login").map_err(map_sqlx)?,
        name: r.try_get("name").map_err(map_sqlx)?,
        status,
        start_at: r.try_get("start_at").map_err(map_sqlx)?,
        due_at: r.try_get("due_at").map_err(map_sqlx)?,
        issue_count: r.try_get("issue_count").map_err(map_sqlx)?,
        closed_issue_count: r.try_get("closed_issue_count").map_err(map_sqlx)?,
        progress_pct: r.try_get("progress_pct").map_err(map_sqlx)?,
        slip_days: r.try_get("slip_days").map_err(map_sqlx)?,
        issue_overdue_count: r.try_get("issue_overdue_count").map_err(map_sqlx)?,
        lead,
        mirrored_to_github: r.try_get("mirrored_to_github").map_err(map_sqlx)?,
        version: r.try_get("version").map_err(map_sqlx)?,
        total: r.try_get("total").map_err(map_sqlx)?,
    })
}

fn row_to_board_link(r: &sqlx::postgres::PgRow) -> Result<BoardLink, StoreError> {
    Ok(BoardLink {
        id: r.try_get("id").map_err(map_sqlx)?,
        project_id: r.try_get("project_id").map_err(map_sqlx)?,
        github_board_node_id: r.try_get("github_board_node_id").map_err(map_sqlx)?,
        github_board_title: r.try_get("github_board_title").map_err(map_sqlx)?,
        github_board_url: r.try_get("github_board_url").map_err(map_sqlx)?,
        github_board_cached_at: r.try_get("github_board_cached_at").map_err(map_sqlx)?,
        start_field_node_id: r.try_get("start_field_node_id").map_err(map_sqlx)?,
        due_field_node_id: r.try_get("due_field_node_id").map_err(map_sqlx)?,
        status_field_node_id: r.try_get("status_field_node_id").map_err(map_sqlx)?,
        last_mirror_at: r.try_get("last_mirror_at").map_err(map_sqlx)?,
        last_mirror_error: r.try_get("last_mirror_error").map_err(map_sqlx)?,
        created_by: r.try_get("created_by").map_err(map_sqlx)?,
        created_at: r.try_get("created_at").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
    })
}

fn row_to_board_item(r: &sqlx::postgres::PgRow) -> Result<BoardItem, StoreError> {
    Ok(BoardItem {
        link_id: r.try_get("link_id").map_err(map_sqlx)?,
        issue_id: r.try_get("issue_id").map_err(map_sqlx)?,
        item_node_id: r.try_get("item_node_id").map_err(map_sqlx)?,
        last_synced_at: r.try_get("last_synced_at").map_err(map_sqlx)?,
        last_error: r.try_get("last_error").map_err(map_sqlx)?,
        created_at: r.try_get("created_at").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
    })
}

fn row_to_issue_dates(r: &sqlx::postgres::PgRow) -> Result<IssueDates, StoreError> {
    Ok(IssueDates {
        issue_id: r.try_get("issue_id").map_err(map_sqlx)?,
        start_at: r.try_get("start_at").map_err(map_sqlx)?,
        due_at: r.try_get("due_at").map_err(map_sqlx)?,
        mirror_node_id: r.try_get("mirror_node_id").map_err(map_sqlx)?,
        mirror_synced_at: r.try_get("mirror_synced_at").map_err(map_sqlx)?,
        mirror_error: r.try_get("mirror_error").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
    })
}

fn row_to_projectv2_mirror_task(
    r: &sqlx::postgres::PgRow,
) -> Result<ProjectV2MirrorTask, StoreError> {
    let kind_s: String = r.try_get("kind").map_err(map_sqlx)?;
    let kind = match kind_s.as_str() {
        "mirror_dates" => ProjectV2MirrorTaskKind::MirrorDates,
        "pull_back" => ProjectV2MirrorTaskKind::PullBack,
        other => return Err(invalid(format!("unknown mirror task kind: {other}"))),
    };
    Ok(ProjectV2MirrorTask {
        id: r.try_get("id").map_err(map_sqlx)?,
        issue_id: r.try_get("issue_id").map_err(map_sqlx)?,
        repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
        kind,
        payload: r.try_get::<JsonValue, _>("payload").map_err(map_sqlx)?,
        attempts: r.try_get("attempts").map_err(map_sqlx)?,
        last_error: r.try_get("last_error").map_err(map_sqlx)?,
        enqueued_at: r.try_get("enqueued_at").map_err(map_sqlx)?,
        processed_at: r.try_get("processed_at").map_err(map_sqlx)?,
    })
}

fn issue_mutation_op_to_text(op: IssueMutationOp) -> &'static str {
    match op {
        IssueMutationOp::Create => "create",
        IssueMutationOp::Update => "update",
        IssueMutationOp::Close => "close",
        IssueMutationOp::Reopen => "reopen",
        IssueMutationOp::Comment => "comment",
    }
}

fn issue_mutation_op_from_text(s: &str) -> Result<IssueMutationOp, StoreError> {
    match s {
        "create" => Ok(IssueMutationOp::Create),
        "update" => Ok(IssueMutationOp::Update),
        "close" => Ok(IssueMutationOp::Close),
        "reopen" => Ok(IssueMutationOp::Reopen),
        "comment" => Ok(IssueMutationOp::Comment),
        other => Err(invalid(format!("unknown issue mutation op: {other}"))),
    }
}

fn issue_mutation_result_to_text(r: IssueMutationResult) -> &'static str {
    match r {
        IssueMutationResult::Pending => "pending",
        IssueMutationResult::Committed => "committed",
        IssueMutationResult::Failed => "failed",
        IssueMutationResult::PendingRemoteTimeout => "pending_remote_timeout",
    }
}

fn issue_mutation_result_from_text(s: &str) -> Result<IssueMutationResult, StoreError> {
    match s {
        "pending" => Ok(IssueMutationResult::Pending),
        "committed" => Ok(IssueMutationResult::Committed),
        "failed" => Ok(IssueMutationResult::Failed),
        "pending_remote_timeout" => Ok(IssueMutationResult::PendingRemoteTimeout),
        other => Err(invalid(format!("unknown issue mutation result: {other}"))),
    }
}

fn row_to_issue_mutation(r: &sqlx::postgres::PgRow) -> Result<IssueMutation, StoreError> {
    let op_s: String = r.try_get("op").map_err(map_sqlx)?;
    let result_s: String = r.try_get("result").map_err(map_sqlx)?;
    Ok(IssueMutation {
        id: r.try_get("id").map_err(map_sqlx)?,
        actor_user_id: r.try_get("actor_user_id").map_err(map_sqlx)?,
        issue_id: r.try_get("issue_id").map_err(map_sqlx)?,
        repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
        op: issue_mutation_op_from_text(&op_s)?,
        version_before: r.try_get("version_before").map_err(map_sqlx)?,
        version_after: r.try_get("version_after").map_err(map_sqlx)?,
        diff: r.try_get::<JsonValue, _>("diff").map_err(map_sqlx)?,
        result: issue_mutation_result_from_text(&result_s)?,
        github_delivery_id: r.try_get("github_delivery_id").map_err(map_sqlx)?,
        error: r.try_get("error").map_err(map_sqlx)?,
        created_at: r.try_get("created_at").map_err(map_sqlx)?,
        finished_at: r.try_get("finished_at").map_err(map_sqlx)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Object-safety guard. Every surface holds an
    /// `Arc<dyn Store>`; if `PgStore` ever picked up a generic that
    /// broke object-safety, this test would fail at compile time.
    #[allow(dead_code)]
    fn pg_store_is_a_store(s: PgStore) -> Box<dyn Store> {
        Box::new(s)
    }
}
