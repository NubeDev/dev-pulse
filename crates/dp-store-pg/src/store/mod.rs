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

// The trait impl here only delegates; the real bodies live in the per-domain
// submodules. Several types and helpers re-exported from `dp_domain` still
// need to be in scope because they appear in trait method signatures, but the
// Rust unused-imports lint can't see through the trait signatures vs body
// split, so silence it for this file specifically.

use std::error::Error as StdError;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dp_domain::audit::AuditEntry;
use dp_domain::event::{ActivityEvent, ActorRole, EventActor};
use dp_domain::fetch::{FetchCursor, FetchRun, FetchRunErrorSample, FetchRunKind, ResourceKind};
use dp_domain::freshness::DataAsOf;
use dp_domain::identity::{IdentityLinkPending, UserIdentity};
use dp_domain::inbox::{InboxIssueRow, InboxStatus, UserIssueState};
use dp_domain::membership::Membership;
use dp_domain::milestone::{Milestone, MilestoneUpsert};
use dp_domain::org::Org;
use dp_domain::pin::{Pin, PinKind};
use dp_domain::repo::Repo;
use dp_domain::setting::UserSetting;
use dp_domain::issue::{Issue, IssueUpsert, IssueUpsertOutcome, RepoSummary};
use dp_domain::issue_mutation::{IssueMutation, IssueMutationResult};
use dp_domain::tag::Tag;
use dp_domain::tag_link::{TagLink, TagLinkKind};
use dp_domain::board_link::{
    BoardItem, BoardItemMirrorOutcome, BoardLink, BoardLinkUpsert,
};
use dp_domain::issue_dates::{IssueDates, ProjectV2MirrorTask, ProjectV2MirrorTaskKind};
use dp_domain::project::{
    PortfolioQueryFilter, PortfolioRawRow, Project, ProjectIssueAddOutcome,
    ProjectListFilter, ProjectRepo, ProjectUpsert,
};
use dp_domain::project_view::{
    ProjectView, ProjectViewUpsert,
};
use dp_domain::store::{
    EventActorRow, IssueDatesMirrorOutcome, IssueListFilter,
    IssueMetricRow, IssueMetricsFilter, IssueTimelineRow, PendingRemoteIssue, RepoListFilter,
    RepoSyncStatus, Store, StoreError,
};
use dp_domain::team::Team;
use dp_domain::user::User;
use dp_domain::webhook::WebhookDelivery;
use dp_domain::window::Window;
use starter_store_postgres::Pool;
use uuid::Uuid;


mod rows;
mod users;
mod orgs;
mod repos;
mod issues;
mod issue_dates;
mod events;
mod fetch;
mod webhooks;
mod pins;
mod settings;
mod projects;
mod board_links;
mod tags;
mod milestones;
mod project_exec_summary;

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
pub(super) fn parse_tag_name_kv(name: &str) -> (&'static str, Option<String>, Option<String>) {
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

pub(super) fn map_sqlx(err: sqlx::Error) -> StoreError {
    if let sqlx::Error::Database(db) = &err {
        if db.code().as_deref() == Some("23505") {
            return StoreError::Conflict(db.message().to_string());
        }
    }
    StoreError::Backend(Box::new(err))
}

pub(super) fn not_found(entity: &'static str, id: impl ToString) -> StoreError {
    StoreError::NotFound {
        entity,
        id: id.to_string(),
    }
}

pub(super) fn invalid(msg: impl Into<String>) -> StoreError {
    let m: String = msg.into();
    let e: Box<dyn StdError + Send + Sync> = m.into();
    StoreError::Backend(e)
}

// ---------- Store impl ----------------------------------------------

#[async_trait]
impl Store for PgStore {
    // ---- users -----------------------------------------------------

    async fn upsert_user(&self, user: &User) -> Result<User, StoreError> {
        self.upsert_user_impl(user).await
    }

    async fn get_user(&self, id: Uuid) -> Result<User, StoreError> {
        self.get_user_impl(id).await
    }

    async fn get_user_by_github_id(&self, github_id: i64) -> Result<User, StoreError> {
        self.get_user_by_github_id_impl(github_id).await
    }

    async fn list_users(&self) -> Result<Vec<User>, StoreError> {
        self.list_users_impl().await
    }

    async fn find_user_by_login(&self, login: &str) -> Result<Option<User>, StoreError> {
        self.find_user_by_login_impl(login).await
    }

    async fn pseudonymise_user(&self, id: Uuid) -> Result<(), StoreError> {
        self.pseudonymise_user_impl(id).await
    }

    async fn set_user_role(
        &self,
        id: Uuid,
        role: dp_domain::user::Role,
    ) -> Result<User, StoreError> {
        self.set_user_role_impl(id, role).await
    }

    // ---- identities (users.md §4 Slice A) --------------------------

    async fn list_identities_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<UserIdentity>, StoreError> {
        self.list_identities_for_user_impl(user_id).await
    }

    async fn find_user_by_github_user_id(
        &self,
        github_user_id: i64,
    ) -> Result<Option<User>, StoreError> {
        self.find_user_by_github_user_id_impl(github_user_id).await
    }

    async fn create_identity_link_pending(
        &self,
        pending: &IdentityLinkPending,
    ) -> Result<(), StoreError> {
        self.create_identity_link_pending_impl(pending).await
    }

    async fn consume_identity_link_pending(
        &self,
        nonce: Uuid,
    ) -> Result<Option<IdentityLinkPending>, StoreError> {
        self.consume_identity_link_pending_impl(nonce).await
    }

    async fn purge_expired_identity_link_pending(
        &self,
        now: DateTime<Utc>,
    ) -> Result<u64, StoreError> {
        self.purge_expired_identity_link_pending_impl(now).await
    }

    async fn link_identity(
        &self,
        identity: &UserIdentity,
    ) -> Result<UserIdentity, StoreError> {
        self.link_identity_impl(identity).await
    }

    async fn unlink_identity(
        &self,
        user_id: Uuid,
        github_user_id: i64,
    ) -> Result<(), StoreError> {
        self.unlink_identity_impl(user_id, github_user_id).await
    }

    async fn set_primary_identity(
        &self,
        user_id: Uuid,
        github_user_id: i64,
    ) -> Result<(), StoreError> {
        self.set_primary_identity_impl(user_id, github_user_id).await
    }

    // ---- orgs / teams / repos --------------------------------------

    async fn upsert_org(&self, org: &Org) -> Result<Org, StoreError> {
        self.upsert_org_impl(org).await
    }

    async fn upsert_team(&self, team: &Team) -> Result<Team, StoreError> {
        self.upsert_team_impl(team).await
    }

    async fn upsert_repo(&self, repo: &Repo) -> Result<Repo, StoreError> {
        self.upsert_repo_impl(repo).await
    }

    async fn upsert_repo_metadata(
        &self,
        m: &dp_domain::RepoMetadata,
    ) -> Result<(), StoreError> {
        self.upsert_repo_metadata_impl(m).await
    }

    async fn get_repo_metadata(
        &self,
        repo_id: Uuid,
    ) -> Result<Option<dp_domain::RepoMetadata>, StoreError> {
        self.get_repo_metadata_impl(repo_id).await
    }

    async fn pr_size_stats_for_repo(
        &self,
        repo_id: Uuid,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
    ) -> Result<dp_domain::RepoPrSizeStats, StoreError> {
        self.pr_size_stats_for_repo_impl(repo_id, since, until).await
    }

    async fn ci_stats_for_repo(
        &self,
        repo_id: Uuid,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
    ) -> Result<dp_domain::RepoCiStats, StoreError> {
        self.ci_stats_for_repo_impl(repo_id, since, until).await
    }

    async fn activity_heatmap_for_repo(
        &self,
        repo_id: Uuid,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
        timezone: &str,
    ) -> Result<dp_domain::RepoActivityHeatmap, StoreError> {
        self.activity_heatmap_for_repo_impl(repo_id, since, until, timezone).await
    }

    async fn review_velocity_for_repo(
        &self,
        repo_id: Uuid,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
    ) -> Result<dp_domain::RepoReviewVelocity, StoreError> {
        self.review_velocity_for_repo_impl(repo_id, since, until).await
    }

    async fn contributor_diversity_for_repo(
        &self,
        repo_id: Uuid,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
    ) -> Result<dp_domain::RepoContributorDiversity, StoreError> {
        self.contributor_diversity_for_repo_impl(repo_id, since, until).await
    }

    async fn upsert_membership(&self, membership: &Membership) -> Result<Membership, StoreError> {
        self.upsert_membership_impl(membership).await
    }

    async fn list_memberships_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<Membership>, StoreError> {
        self.list_memberships_for_user_impl(user_id).await
    }

    async fn set_home_org(
        &self,
        user_id: Uuid,
        org_id: Uuid,
        home_org: Option<Uuid>,
    ) -> Result<(), StoreError> {
        self.set_home_org_impl(user_id, org_id, home_org).await
    }

    async fn set_home_org_for_user(
        &self,
        user_id: Uuid,
        org_id: Uuid,
    ) -> Result<(), StoreError> {
        self.set_home_org_for_user_impl(user_id, org_id).await
    }

    async fn list_orgs(&self) -> Result<Vec<Org>, StoreError> {
        self.list_orgs_impl().await
    }

    async fn list_teams_for_org(&self, org_id: Uuid) -> Result<Vec<Team>, StoreError> {
        self.list_teams_for_org_impl(org_id).await
    }

    async fn list_users_for_org(&self, org_id: Uuid) -> Result<Vec<User>, StoreError> {
        self.list_users_for_org_impl(org_id).await
    }

    // ---- repos / issues read surface --------------------------------

    async fn get_repo(&self, id: Uuid) -> Result<Option<Repo>, StoreError> {
        self.get_repo_impl(id).await
    }

    async fn list_repos(&self, filter: &RepoListFilter) -> Result<Vec<RepoSummary>, StoreError> {
        self.list_repos_impl(filter).await
    }

    async fn count_repos(&self, filter: &RepoListFilter) -> Result<i64, StoreError> {
        self.count_repos_impl(filter).await
    }

    async fn list_issues(&self, filter: &IssueListFilter) -> Result<Vec<Issue>, StoreError> {
        self.list_issues_impl(filter).await
    }

    async fn count_issues(&self, filter: &IssueListFilter) -> Result<i64, StoreError> {
        self.count_issues_impl(filter).await
    }

    async fn get_issue(&self, id: Uuid) -> Result<Option<Issue>, StoreError> {
        self.get_issue_impl(id).await
    }

    async fn get_issue_by_repo_and_number(
        &self,
        repo_id: Uuid,
        number: i64,
    ) -> Result<Option<Issue>, StoreError> {
        self.get_issue_by_repo_and_number_impl(repo_id, number).await
    }

    async fn upsert_issue_from_github(
        &self,
        upsert: &IssueUpsert,
        pending_remote_window: chrono::Duration,
    ) -> Result<(Issue, IssueUpsertOutcome), StoreError> {
        self.upsert_issue_from_github_impl(upsert, pending_remote_window).await
    }

    async fn create_local_issue(
        &self,
        org_id: Uuid,
        repo_id: Uuid,
        title: &str,
        body: Option<&str>,
    ) -> Result<Issue, StoreError> {
        self.create_local_issue_impl(org_id, repo_id, title, body).await
    }

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
        self.update_local_issue_impl(issue_id, expected_version, title, body, state, labels, assignees).await
    }

    // ---- per-user inbox (triage spine, slice 1) -------------------

    async fn list_inbox_issues(
        &self,
        user_id: Uuid,
        filter: &IssueListFilter,
    ) -> Result<Vec<InboxIssueRow>, StoreError> {
        self.list_inbox_issues_impl(user_id, filter).await
    }

    async fn count_inbox_issues(
        &self,
        user_id: Uuid,
        filter: &IssueListFilter,
    ) -> Result<i64, StoreError> {
        self.count_inbox_issues_impl(user_id, filter).await
    }

    async fn mark_issues_seen(
        &self,
        user_id: Uuid,
        issue_ids: &[Uuid],
    ) -> Result<(), StoreError> {
        self.mark_issues_seen_impl(user_id, issue_ids).await
    }

    async fn set_inbox_state(
        &self,
        user_id: Uuid,
        issue_id: Uuid,
        status: InboxStatus,
        snoozed_until: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<UserIssueState, StoreError> {
        self.set_inbox_state_impl(user_id, issue_id, status, snoozed_until).await
    }

    async fn set_inbox_state_bulk(
        &self,
        user_id: Uuid,
        issue_ids: &[Uuid],
        status: InboxStatus,
        snoozed_until: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<u64, StoreError> {
        self.set_inbox_state_bulk_impl(user_id, issue_ids, status, snoozed_until).await
    }

    async fn record_audit_log(&self, entry: &AuditEntry) -> Result<(), StoreError> {
        self.record_audit_log_impl(entry).await
    }

    // ---- issue timeline (triage slice 2 — §5.6) ------------------

    async fn list_events_for_issue(
        &self,
        repo_id: Uuid,
        number: i64,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<IssueTimelineRow>, StoreError> {
        self.list_events_for_issue_impl(repo_id, number, limit, offset).await
    }

    async fn count_events_for_issue(
        &self,
        repo_id: Uuid,
        number: i64,
    ) -> Result<i64, StoreError> {
        self.count_events_for_issue_impl(repo_id, number).await
    }

    // ---- repo sync status (triage slice 2 — §5.9) -----------------

    async fn get_repo_sync_status(
        &self,
        repo_id: Uuid,
    ) -> Result<Option<RepoSyncStatus>, StoreError> {
        self.get_repo_sync_status_impl(repo_id).await
    }

    // ---- issue metrics (triage slice 2 — §5.10) -------------------

    async fn issue_metrics(
        &self,
        filter: &IssueMetricsFilter,
    ) -> Result<Vec<IssueMetricRow>, StoreError> {
        self.issue_metrics_impl(filter).await
    }

    // ---- events + actors ------------------------------------------

    async fn record_event(&self, event: &ActivityEvent) -> Result<ActivityEvent, StoreError> {
        self.record_event_impl(event).await
    }

    async fn add_event_actors(&self, actors: &[EventActor]) -> Result<(), StoreError> {
        self.add_event_actors_impl(actors).await
    }

    async fn list_event_actor_rows_in_window(
        &self,
        window: &Window,
        orgs: &[Uuid],
        repos: &[Uuid],
        users: &[Uuid],
        roles: &[ActorRole],
    ) -> Result<Vec<EventActorRow>, StoreError> {
        self.list_event_actor_rows_in_window_impl(window, orgs, repos, users, roles).await
    }

    // ---- cursors + run log ----------------------------------------

    async fn get_cursor(
        &self,
        org_id: Uuid,
        repo_id: Option<Uuid>,
        resource_kind: ResourceKind,
    ) -> Result<FetchCursor, StoreError> {
        self.get_cursor_impl(org_id, repo_id, resource_kind).await
    }

    async fn put_cursor(&self, cursor: &FetchCursor) -> Result<(), StoreError> {
        self.put_cursor_impl(cursor).await
    }

    async fn start_fetch_run(&self, kind: FetchRunKind) -> Result<Uuid, StoreError> {
        self.start_fetch_run_impl(kind).await
    }

    async fn finish_fetch_run(
        &self,
        id: Uuid,
        items: i64,
        errors: i64,
        partial: bool,
    ) -> Result<(), StoreError> {
        self.finish_fetch_run_impl(id, items, errors, partial).await
    }

    async fn record_fetch_run_errors(
        &self,
        id: Uuid,
        samples: &[FetchRunErrorSample],
    ) -> Result<(), StoreError> {
        self.record_fetch_run_errors_impl(id, samples).await
    }

    async fn list_recent_fetch_runs(&self, limit: i64) -> Result<Vec<FetchRun>, StoreError> {
        self.list_recent_fetch_runs_impl(limit).await
    }

    async fn list_fetch_runs(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<FetchRun>, StoreError> {
        self.list_fetch_runs_impl(limit, offset).await
    }

    async fn list_event_actor_rows_for_user_page(
        &self,
        user_id: Uuid,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<EventActorRow>, StoreError> {
        self.list_event_actor_rows_for_user_page_impl(user_id, offset, limit).await
    }

    async fn data_as_of(&self) -> Result<DataAsOf, StoreError> {
        self.data_as_of_impl().await
    }

    // ---- webhook inbox --------------------------------------------

    async fn enqueue_webhook(&self, delivery: &WebhookDelivery) -> Result<(), StoreError> {
        self.enqueue_webhook_impl(delivery).await
    }

    async fn claim_webhooks(&self, max: i64) -> Result<Vec<WebhookDelivery>, StoreError> {
        self.claim_webhooks_impl(max).await
    }

    async fn mark_webhook_processed(&self, id: Uuid) -> Result<(), StoreError> {
        self.mark_webhook_processed_impl(id).await
    }

    async fn mark_webhook_failed(&self, id: Uuid, error: &str) -> Result<(), StoreError> {
        self.mark_webhook_failed_impl(id, error).await
    }

    // ---- pins (SCOPE-PROJECTS §6.3) ------------------------------------

    async fn list_pins_for_user(&self, user_id: Uuid) -> Result<Vec<Pin>, StoreError> {
        self.list_pins_for_user_impl(user_id).await
    }

    async fn add_pin(&self, pin: &Pin) -> Result<Pin, StoreError> {
        self.add_pin_impl(pin).await
    }

    async fn remove_pin(
        &self,
        user_id: Uuid,
        kind: PinKind,
        target_id: Uuid,
    ) -> Result<(), StoreError> {
        self.remove_pin_impl(user_id, kind, target_id).await
    }

    async fn reorder_pins(
        &self,
        user_id: Uuid,
        order: &[(PinKind, Uuid)],
    ) -> Result<(), StoreError> {
        self.reorder_pins_impl(user_id, order).await
    }

    // ---- per-user settings (migration 0029) ---------------------

    async fn list_user_settings(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<UserSetting>, StoreError> {
        self.list_user_settings_impl(user_id).await
    }

    async fn get_user_setting(
        &self,
        user_id: Uuid,
        key: &str,
    ) -> Result<Option<UserSetting>, StoreError> {
        self.get_user_setting_impl(user_id, key).await
    }

    async fn upsert_user_setting(
        &self,
        setting: &UserSetting,
    ) -> Result<UserSetting, StoreError> {
        self.upsert_user_setting_impl(setting).await
    }

    async fn delete_user_setting(
        &self,
        user_id: Uuid,
        key: &str,
    ) -> Result<(), StoreError> {
        self.delete_user_setting_impl(user_id, key).await
    }

    // ---- issue mutations (SCOPE-PROJECTS §8.2 + §8.5 + §13.7) ----

    async fn try_acquire_issue_pending_remote(
        &self,
        issue_id: Uuid,
        expected_version: i64,
        actor_user_id: Uuid,
    ) -> Result<Option<i64>, StoreError> {
        self.try_acquire_issue_pending_remote_impl(issue_id, expected_version, actor_user_id).await
    }

    async fn release_issue_pending_remote(
        &self,
        issue_id: Uuid,
        bump_version_again: bool,
    ) -> Result<i64, StoreError> {
        self.release_issue_pending_remote_impl(issue_id, bump_version_again).await
    }

    async fn get_issue_version(&self, issue_id: Uuid) -> Result<i64, StoreError> {
        self.get_issue_version_impl(issue_id).await
    }

    async fn list_issues_with_pending_remote_older_than(
        &self,
        cutoff: DateTime<Utc>,
    ) -> Result<Vec<PendingRemoteIssue>, StoreError> {
        self.list_issues_with_pending_remote_older_than_impl(cutoff).await
    }

    async fn record_issue_mutation(
        &self,
        mutation: &IssueMutation,
    ) -> Result<IssueMutation, StoreError> {
        self.record_issue_mutation_impl(mutation).await
    }

    async fn update_issue_mutation_result(
        &self,
        id: Uuid,
        result: IssueMutationResult,
        github_delivery_id: Option<&str>,
        error: Option<&str>,
    ) -> Result<(), StoreError> {
        self.update_issue_mutation_result_impl(id, result, github_delivery_id, error).await
    }

    async fn list_pending_issue_mutations_older_than(
        &self,
        cutoff: DateTime<Utc>,
    ) -> Result<Vec<IssueMutation>, StoreError> {
        self.list_pending_issue_mutations_older_than_impl(cutoff).await
    }

    // ---- §13.7 reconciler guard + webhook replay buffer --------------

    async fn find_repo_id_by_github_id(
        &self,
        github_repo_id: i64,
    ) -> Result<Option<Uuid>, StoreError> {
        self.find_repo_id_by_github_id_impl(github_repo_id).await
    }

    async fn find_issue_id_by_repo_and_github_id(
        &self,
        repo_id: Uuid,
        github_issue_id: i64,
    ) -> Result<Option<Uuid>, StoreError> {
        self.find_issue_id_by_repo_and_github_id_impl(repo_id, github_issue_id).await
    }

    async fn is_issue_pending_remote_fresh(
        &self,
        issue_id: Uuid,
        timeout: chrono::Duration,
    ) -> Result<bool, StoreError> {
        self.is_issue_pending_remote_fresh_impl(issue_id, timeout).await
    }

    async fn buffer_pending_remote_webhook(
        &self,
        issue_id: Uuid,
        delivery: &WebhookDelivery,
    ) -> Result<(), StoreError> {
        self.buffer_pending_remote_webhook_impl(issue_id, delivery).await
    }

    async fn take_buffered_webhooks_for_issue(
        &self,
        issue_id: Uuid,
    ) -> Result<Vec<WebhookDelivery>, StoreError> {
        self.take_buffered_webhooks_for_issue_impl(issue_id).await
    }

    // ---- issue dates (triage slice 2 — §3.10) --------------------

    async fn get_issue_dates(
        &self,
        issue_id: Uuid,
    ) -> Result<Option<IssueDates>, StoreError> {
        self.get_issue_dates_impl(issue_id).await
    }

    async fn upsert_issue_dates(
        &self,
        issue_id: Uuid,
        start_at: Option<DateTime<Utc>>,
        due_at: Option<DateTime<Utc>>,
    ) -> Result<IssueDates, StoreError> {
        self.upsert_issue_dates_impl(issue_id, start_at, due_at).await
    }

    async fn record_issue_dates_mirror_result(
        &self,
        issue_id: Uuid,
        outcome: IssueDatesMirrorOutcome<'_>,
    ) -> Result<(), StoreError> {
        self.record_issue_dates_mirror_result_impl(issue_id, outcome).await
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
        self.set_issue_github_node_id_impl(issue_id, node_id).await
    }

    async fn enqueue_projectv2_mirror_task(
        &self,
        issue_id: Uuid,
        repo_id: Uuid,
        kind: ProjectV2MirrorTaskKind,
        payload: serde_json::Value,
    ) -> Result<(), StoreError> {
        self.enqueue_projectv2_mirror_task_impl(issue_id, repo_id, kind, payload).await
    }

    async fn claim_projectv2_mirror_tasks(
        &self,
        max: i64,
    ) -> Result<Vec<ProjectV2MirrorTask>, StoreError> {
        self.claim_projectv2_mirror_tasks_impl(max).await
    }

    // ---- projects (linear-projects-v2.md slice A) ----------------

    async fn list_projects(
        &self,
        filter: &ProjectListFilter,
    ) -> Result<Vec<Project>, StoreError> {
        self.list_projects_impl(filter).await
    }

    async fn count_projects(
        &self,
        filter: &ProjectListFilter,
    ) -> Result<i64, StoreError> {
        self.count_projects_impl(filter).await
    }

    async fn list_project_portfolio(
        &self,
        filter: &PortfolioQueryFilter,
    ) -> Result<Vec<PortfolioRawRow>, StoreError> {
        self.list_project_portfolio_impl(filter).await
    }

    async fn get_project(&self, id: Uuid) -> Result<Option<Project>, StoreError> {
        self.get_project_impl(id).await
    }

    async fn create_project(
        &self,
        upsert: &ProjectUpsert,
    ) -> Result<Project, StoreError> {
        self.create_project_impl(upsert).await
    }

    async fn update_project(
        &self,
        id: Uuid,
        expected_version: i64,
        upsert: &ProjectUpsert,
    ) -> Result<Project, StoreError> {
        self.update_project_impl(id, expected_version, upsert).await
    }

    async fn archive_project(
        &self,
        id: Uuid,
        expected_version: i64,
    ) -> Result<Project, StoreError> {
        self.archive_project_impl(id, expected_version).await
    }

    async fn add_issues_to_project(
        &self,
        project_id: Uuid,
        expected_version: i64,
        issue_ids: &[Uuid],
        actor: Option<Uuid>,
    ) -> Result<ProjectIssueAddOutcome, StoreError> {
        self.add_issues_to_project_impl(project_id, expected_version, issue_ids, actor).await
    }

    async fn remove_issue_from_project(
        &self,
        project_id: Uuid,
        issue_id: Uuid,
        expected_version: i64,
    ) -> Result<Project, StoreError> {
        self.remove_issue_from_project_impl(project_id, issue_id, expected_version).await
    }

    async fn get_project_for_issue(
        &self,
        issue_id: Uuid,
    ) -> Result<Option<Project>, StoreError> {
        self.get_project_for_issue_impl(issue_id).await
    }

    async fn list_issue_ids_for_project(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<Uuid>, StoreError> {
        self.list_issue_ids_for_project_impl(project_id).await
    }

    async fn list_project_issue_tag_values(
        &self,
        project_id: Uuid,
        tag_key: &str,
    ) -> Result<Vec<(Uuid, String)>, StoreError> {
        self.list_project_issue_tag_values_impl(project_id, tag_key).await
    }

    async fn list_issue_tag_values(
        &self,
        issue_ids: &[Uuid],
        tag_key: &str,
    ) -> Result<Vec<(Uuid, String)>, StoreError> {
        self.list_issue_tag_values_impl(issue_ids, tag_key).await
    }

    async fn list_project_issue_tag_keys(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<String>, StoreError> {
        self.list_project_issue_tag_keys_impl(project_id).await
    }

    // ---- project saved views (PROJECT-VIEW.md §6.1) --------------

    async fn list_project_views(
        &self,
        project_id: Uuid,
        owner_user_id: Uuid,
    ) -> Result<Vec<ProjectView>, StoreError> {
        self.list_project_views_impl(project_id, owner_user_id).await
    }

    async fn get_project_view(
        &self,
        id: Uuid,
        owner_user_id: Uuid,
    ) -> Result<Option<ProjectView>, StoreError> {
        self.get_project_view_impl(id, owner_user_id).await
    }

    async fn create_project_view(
        &self,
        project_id: Uuid,
        owner_user_id: Uuid,
        upsert: &ProjectViewUpsert,
    ) -> Result<ProjectView, StoreError> {
        self.create_project_view_impl(project_id, owner_user_id, upsert).await
    }

    async fn update_project_view(
        &self,
        id: Uuid,
        owner_user_id: Uuid,
        upsert: &ProjectViewUpsert,
    ) -> Result<ProjectView, StoreError> {
        self.update_project_view_impl(id, owner_user_id, upsert).await
    }

    async fn delete_project_view(
        &self,
        id: Uuid,
        owner_user_id: Uuid,
    ) -> Result<(), StoreError> {
        self.delete_project_view_impl(id, owner_user_id).await
    }

    async fn reorder_project_views(
        &self,
        project_id: Uuid,
        owner_user_id: Uuid,
        ordered_ids: &[Uuid],
    ) -> Result<Vec<ProjectView>, StoreError> {
        self.reorder_project_views_impl(project_id, owner_user_id, ordered_ids).await
    }

    // ---- per-view (per-tab) issue membership ----------------------

    async fn list_issue_ids_for_view(
        &self,
        view_id: Uuid,
    ) -> Result<Vec<Uuid>, StoreError> {
        self.list_issue_ids_for_view_impl(view_id).await
    }

    async fn add_issues_to_view(
        &self,
        view_id: Uuid,
        issue_ids: &[Uuid],
    ) -> Result<(), StoreError> {
        self.add_issues_to_view_impl(view_id, issue_ids).await
    }

    async fn remove_issue_from_view(
        &self,
        view_id: Uuid,
        issue_id: Uuid,
    ) -> Result<(), StoreError> {
        self.remove_issue_from_view_impl(view_id, issue_id).await
    }

    // ---- project ↔ repo associations -----------------------------

    async fn list_project_repos(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<ProjectRepo>, StoreError> {
        self.list_project_repos_impl(project_id).await
    }

    async fn add_project_repo(
        &self,
        project_id: Uuid,
        repo_id: Uuid,
        actor: Option<Uuid>,
    ) -> Result<ProjectRepo, StoreError> {
        self.add_project_repo_impl(project_id, repo_id, actor).await
    }

    async fn remove_project_repo(
        &self,
        project_id: Uuid,
        repo_id: Uuid,
    ) -> Result<(), StoreError> {
        self.remove_project_repo_impl(project_id, repo_id).await
    }

    // ---- project ↔ board mirror (linear-projects-v2.md slice B) --

    async fn list_board_links(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<BoardLink>, StoreError> {
        self.list_board_links_impl(project_id).await
    }

    async fn get_board_link(&self, id: Uuid) -> Result<Option<BoardLink>, StoreError> {
        self.get_board_link_impl(id).await
    }

    async fn create_board_link(
        &self,
        upsert: &BoardLinkUpsert,
    ) -> Result<BoardLink, StoreError> {
        self.create_board_link_impl(upsert).await
    }

    async fn delete_board_link(&self, id: Uuid) -> Result<(), StoreError> {
        self.delete_board_link_impl(id).await
    }

    async fn refresh_board_link_cache(
        &self,
        id: Uuid,
        title: Option<&str>,
        url: Option<&str>,
    ) -> Result<(), StoreError> {
        self.refresh_board_link_cache_impl(id, title, url).await
    }

    async fn list_board_items_for_issue(
        &self,
        issue_id: Uuid,
    ) -> Result<Vec<BoardItem>, StoreError> {
        self.list_board_items_for_issue_impl(issue_id).await
    }

    async fn get_board_item(
        &self,
        link_id: Uuid,
        issue_id: Uuid,
    ) -> Result<Option<BoardItem>, StoreError> {
        self.get_board_item_impl(link_id, issue_id).await
    }

    async fn record_board_item_result(
        &self,
        link_id: Uuid,
        issue_id: Uuid,
        outcome: BoardItemMirrorOutcome<'_>,
    ) -> Result<(), StoreError> {
        self.record_board_item_result_impl(link_id, issue_id, outcome).await
    }

    // ---- tags + tag links (SCOPE-PROJECTS §7) -------------------

    async fn get_tag(&self, id: Uuid) -> Result<Tag, StoreError> {
        self.get_tag_impl(id).await
    }

    async fn create_tag(&self, tag: &Tag) -> Result<Tag, StoreError> {
        self.create_tag_impl(tag).await
    }

    async fn update_tag(
        &self,
        id: Uuid,
        name: Option<&str>,
        color: Option<&str>,
        description: Option<Option<&str>>,
        archived_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    ) -> Result<Tag, StoreError> {
        self.update_tag_impl(id, name, color, description, archived_at).await
    }

    async fn list_tags_visible_to(
        &self,
        viewer_user_id: Uuid,
        visible_team_ids: &[Uuid],
        visible_org_ids: &[Uuid],
        include_archived: bool,
    ) -> Result<Vec<Tag>, StoreError> {
        self.list_tags_visible_to_impl(viewer_user_id, visible_team_ids, visible_org_ids, include_archived).await
    }

    async fn list_tag_links(
        &self,
        tag_id: Uuid,
        kinds: &[TagLinkKind],
    ) -> Result<Vec<TagLink>, StoreError> {
        self.list_tag_links_impl(tag_id, kinds).await
    }

    async fn list_tag_links_for_targets(
        &self,
        kind: TagLinkKind,
        target_ids: &[Uuid],
    ) -> Result<Vec<TagLink>, StoreError> {
        self.list_tag_links_for_targets_impl(kind, target_ids).await
    }

    async fn add_tag_links(&self, links: &[TagLink]) -> Result<Vec<TagLink>, StoreError> {
        self.add_tag_links_impl(links).await
    }

    async fn remove_tag_links(&self, link_ids: &[Uuid]) -> Result<(), StoreError> {
        self.remove_tag_links_impl(link_ids).await
    }

    async fn resolve_tag_targets(
        &self,
        tag_ids: &[Uuid],
        visible_repo_ids: &[Uuid],
        visible_user_ids: &[Uuid],
        visible_team_ids: &[Uuid],
    ) -> Result<Vec<TagLink>, StoreError> {
        self.resolve_tag_targets_impl(tag_ids, visible_repo_ids, visible_user_ids, visible_team_ids).await
    }

    // ---- milestones (tagging.md §9.3) -----------------------------

    async fn upsert_milestone(
        &self,
        upsert: &MilestoneUpsert,
    ) -> Result<Milestone, StoreError> {
        self.upsert_milestone_impl(upsert).await
    }

    async fn list_milestones_for_repo(
        &self,
        repo_id: Uuid,
        include_closed: bool,
    ) -> Result<Vec<Milestone>, StoreError> {
        self.list_milestones_for_repo_impl(repo_id, include_closed).await
    }

    async fn list_project_milestones(
        &self,
        project_id: Uuid,
        include_closed: bool,
    ) -> Result<Vec<Milestone>, StoreError> {
        self.list_project_milestones_impl(project_id, include_closed).await
    }

    async fn set_project_primary_milestone(
        &self,
        project_id: Uuid,
        milestone_id: Option<Uuid>,
    ) -> Result<Project, StoreError> {
        self.set_project_primary_milestone_impl(project_id, milestone_id).await
    }

    async fn delete_milestone(
        &self,
        milestone_id: Uuid,
    ) -> Result<(), StoreError> {
        self.delete_milestone_impl(milestone_id).await
    }

    // ---- project executive summary (DOCS/SCOPE-PROJECT-EXECUTIVE-SUMMARY.md) ----

    async fn get_project_exec_summary(
        &self,
        project_id: Uuid,
    ) -> Result<
        Option<(
            dp_domain::project_exec_summary::ProjectExecSummary,
            dp_domain::project_exec_summary::ExecSummaryCompletion,
        )>,
        StoreError,
    > {
        self.get_project_exec_summary_impl(project_id).await
    }

    async fn upsert_project_exec_summary(
        &self,
        project_id: Uuid,
    ) -> Result<dp_domain::project_exec_summary::ProjectExecSummary, StoreError> {
        self.upsert_project_exec_summary_impl(project_id).await
    }

    async fn patch_project_exec_summary(
        &self,
        project_id: Uuid,
        patch: &dp_domain::project_exec_summary::ProjectExecSummaryPatch,
    ) -> Result<dp_domain::project_exec_summary::ProjectExecSummary, StoreError> {
        self.patch_project_exec_summary_impl(project_id, patch).await
    }

    async fn submit_project_exec_summary(
        &self,
        project_id: Uuid,
    ) -> Result<dp_domain::project_exec_summary::ProjectExecSummary, StoreError> {
        self.submit_project_exec_summary_impl(project_id).await
    }

    async fn approve_project_exec_summary(
        &self,
        project_id: Uuid,
        approval_notes: Option<&str>,
    ) -> Result<dp_domain::project_exec_summary::ProjectExecSummary, StoreError> {
        self.approve_project_exec_summary_impl(project_id, approval_notes).await
    }

    async fn revert_project_exec_summary(
        &self,
        project_id: Uuid,
    ) -> Result<dp_domain::project_exec_summary::ProjectExecSummary, StoreError> {
        self.revert_project_exec_summary_impl(project_id).await
    }

    async fn list_exec_summary_images(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<dp_domain::project_exec_summary::ExecSummaryImage>, StoreError> {
        self.list_exec_summary_images_impl(project_id).await
    }

    async fn get_exec_summary_image(
        &self,
        image_id: Uuid,
    ) -> Result<Option<dp_domain::project_exec_summary::ExecSummaryImage>, StoreError> {
        self.get_exec_summary_image_impl(image_id).await
    }

    async fn insert_exec_summary_image(
        &self,
        project_id: Uuid,
        blob_ref: &dp_domain::project_exec_summary::BlobRefJson,
        filename: &str,
        content_type: &str,
        caption: Option<&str>,
        ord: Option<i32>,
    ) -> Result<dp_domain::project_exec_summary::ExecSummaryImage, StoreError> {
        self.insert_exec_summary_image_impl(project_id, blob_ref, filename, content_type, caption, ord)
            .await
    }

    async fn update_exec_summary_image(
        &self,
        image_id: Uuid,
        caption: Option<Option<String>>,
        ord: Option<i32>,
    ) -> Result<dp_domain::project_exec_summary::ExecSummaryImage, StoreError> {
        self.update_exec_summary_image_impl(image_id, caption, ord).await
    }

    async fn delete_exec_summary_image(
        &self,
        image_id: Uuid,
    ) -> Result<(), StoreError> {
        self.delete_exec_summary_image_impl(image_id).await
    }

    async fn list_exec_summary_documents(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<dp_domain::project_exec_summary::ExecSummaryDocument>, StoreError> {
        self.list_exec_summary_documents_impl(project_id).await
    }

    async fn get_exec_summary_document(
        &self,
        document_id: Uuid,
    ) -> Result<Option<dp_domain::project_exec_summary::ExecSummaryDocument>, StoreError> {
        self.get_exec_summary_document_impl(document_id).await
    }

    async fn insert_exec_summary_document(
        &self,
        project_id: Uuid,
        blob_ref: &dp_domain::project_exec_summary::BlobRefJson,
        title: &str,
        doc_type: Option<&str>,
        notes: Option<&str>,
        required_action: Option<&str>,
        uploaded_by: Option<&str>,
    ) -> Result<dp_domain::project_exec_summary::ExecSummaryDocument, StoreError> {
        self.insert_exec_summary_document_impl(
            project_id, blob_ref, title, doc_type, notes, required_action, uploaded_by,
        )
        .await
    }

    async fn update_exec_summary_document(
        &self,
        document_id: Uuid,
        title: Option<String>,
        doc_type: Option<Option<String>>,
        notes: Option<Option<String>>,
        required_action: Option<Option<String>>,
    ) -> Result<dp_domain::project_exec_summary::ExecSummaryDocument, StoreError> {
        self.update_exec_summary_document_impl(document_id, title, doc_type, notes, required_action)
            .await
    }

    async fn delete_exec_summary_document(
        &self,
        document_id: Uuid,
    ) -> Result<(), StoreError> {
        self.delete_exec_summary_document_impl(document_id).await
    }

    async fn list_exec_summary_changelog(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<dp_domain::project_exec_summary::ExecSummaryChangelogEntry>, StoreError> {
        self.list_exec_summary_changelog_impl(project_id).await
    }

    async fn insert_exec_summary_changelog(
        &self,
        insert: &dp_domain::project_exec_summary::ExecSummaryChangelogInsert,
    ) -> Result<dp_domain::project_exec_summary::ExecSummaryChangelogEntry, StoreError> {
        self.insert_exec_summary_changelog_impl(insert).await
    }

    async fn delete_exec_summary_changelog(
        &self,
        entry_id: Uuid,
    ) -> Result<(), StoreError> {
        self.delete_exec_summary_changelog_impl(entry_id).await
    }
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

