//! [`Store`] — the persistence contract every dev-pulse surface
//! talks to. `dp-store-pg` implements it against Postgres.
//!
//! The method set is the v1 surface called out in TODO §Phase 1 plus
//! the obvious supporting upserts (orgs, teams, repos, memberships)
//! and run-log writes (`start_fetch_run`, `finish_fetch_run`). New
//! methods land here when a downstream crate needs them — not before.
//!
//! Errors flow through one type: [`StoreError`]. Concrete backends
//! (postgres, fakes in tests) wrap their native errors into
//! `StoreError::Backend(Box<dyn Error + Send + Sync>)` rather than
//! leaking sqlx / tokio-postgres types up the stack — that's what
//! lets `dp-domain` stay storage-agnostic per TODO §0.6.

use std::error::Error as StdError;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::app_install::OrgAppInstall;
use crate::audit::AuditEntry;
use crate::event::{ActivityEvent, ActorRole, EventActor, EventKind};
use crate::fetch::{FetchCursor, FetchRun, FetchRunKind, ResourceKind};
use crate::freshness::DataAsOf;
use crate::inbox::{InboxIssueRow, InboxStatus, UserIssueState};
use crate::issue::{Issue, IssueState, RepoSummary};
use crate::issue_dates::{
    IssueDates, ProjectV2MirrorTask, ProjectV2MirrorTaskKind, RepoProjectLink,
};
use crate::issue_mutation::{IssueMutation, IssueMutationResult};
use crate::membership::Membership;
use crate::org::Org;
use crate::pin::Pin;
use crate::repo::Repo;
use crate::tag::Tag;
use crate::tag_link::{TagLink, TagLinkKind};
use crate::team::Team;
use crate::user::User;
use crate::webhook::WebhookDelivery;
use crate::window::Window;

/// All [`Store`] methods return `Result<_, StoreError>`. Variants are
/// the smallest set we can usefully distinguish at the boundary
/// without leaking backend types.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// The requested row does not exist.
    #[error("not found: {entity} {id}")]
    NotFound {
        /// Entity name (`"user"`, `"org"`, …) — free-form for now.
        entity: &'static str,
        /// Identifier looked up (rendered with `Display`).
        id: String,
    },

    /// A unique constraint violation that the caller can reasonably
    /// recover from (e.g. webhook replay — same `delivery_id` twice).
    #[error("conflict: {0}")]
    Conflict(String),

    /// Input that failed a domain invariant the schema does not catch
    /// (e.g. a window with `end <= start`). Reserved for future use;
    /// listed now so `non_exhaustive` is honest.
    #[error("invalid input: {0}")]
    Invalid(String),

    /// Anything else from the backend — connection drops, serializer
    /// errors, hard SQL failures. Boxed so we don't drag sqlx into
    /// `dp-domain`.
    #[error("backend error: {0}")]
    Backend(#[source] Box<dyn StdError + Send + Sync>),
}

/// The set of [`EventActor`] rows joined back through their parent
/// [`ActivityEvent`], shaped for the report layer's de-dup
/// (`(user_id, event_id)`) and role-filter logic.
///
/// This is the row type `list_event_actor_rows_in_window` returns.
/// It is the smallest projection that lets reports compute all three
/// org-scope lenses (SCOPE §8.1) without a second query per row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventActorRow {
    /// FK to `activity_events.id`.
    pub event_id: Uuid,
    /// User credited for this row.
    pub user_id: Uuid,
    /// Role the user played (filter target per metric).
    pub role: ActorRole,
    /// Org the event happened in (lens / scope target).
    pub org_id: Uuid,
    /// Repo the event happened in.
    pub repo_id: Uuid,
    /// Event kind (filter target for "PRs merged" etc.).
    pub kind: EventKind,
    /// Event timestamp (UTC). Already filtered to the window, but
    /// returned so the trend-bucket logic can group by day/week.
    pub ts: DateTime<Utc>,
}

/// The persistence surface dev-pulse talks to.
///
/// Implementations live outside this crate (`dp-store-pg` for
/// Postgres, in-memory fakes in tests). Every method is `async`.
#[async_trait]
pub trait Store: Send + Sync {
    // ---- users ----------------------------------------------------

    /// Upsert by `github_id`. Returns the resulting row (after the
    /// upsert is applied) so the caller can see the assigned `id`.
    async fn upsert_user(&self, user: &User) -> Result<User, StoreError>;

    /// Fetch by primary key.
    async fn get_user(&self, id: Uuid) -> Result<User, StoreError>;

    /// Fetch by GitHub numeric id (the stable id GitHub exposes).
    async fn get_user_by_github_id(&self, github_id: i64) -> Result<User, StoreError>;

    /// Look up a non-deleted user by GitHub login. Returns `Ok(None)`
    /// when no row matches. Used by the webhook / commit-trailer
    /// path to avoid minting a synthetic duplicate when GitHub has
    /// already given us a real `github_id` row for the same login
    /// via the reconciler.
    ///
    /// If multiple rows share the login (e.g. a synthetic + a real
    /// row created in different orderings), implementations should
    /// prefer the row with the *positive* (real) `github_id` so the
    /// caller can collapse onto the canonical row.
    ///
    /// Default impl falls back to a `list_users` scan so test fakes
    /// don't need to override; production backends should use the
    /// `dp_users_login_idx` index.
    async fn find_user_by_login(&self, login: &str) -> Result<Option<User>, StoreError> {
        let needle = login.to_ascii_lowercase();
        let mut best: Option<User> = None;
        for u in self.list_users().await? {
            if u.login.to_ascii_lowercase() != needle {
                continue;
            }
            // Prefer a real (positive) github_id over a synthetic one;
            // among reals, prefer the lowest (oldest) id so this agrees
            // with the canonical rule in migration 0003.
            let better = match &best {
                None => true,
                Some(cur) => match (cur.github_id >= 0, u.github_id >= 0) {
                    (false, true) => true,
                    (true, true) => u.github_id < cur.github_id,
                    (false, false) => u.github_id < cur.github_id,
                    (true, false) => false,
                },
            };
            if better {
                best = Some(u);
            }
        }
        Ok(best)
    }

    /// List all non-deleted users.
    async fn list_users(&self) -> Result<Vec<User>, StoreError>;

    /// Soft-delete + pseudonymise (TODO §0.5). Rewrites
    /// `login`/`email`/`name` to `deleted-user-<hash>` form, sets
    /// `deleted_at`, leaves the row id stable so referential
    /// integrity holds.
    async fn pseudonymise_user(&self, id: Uuid) -> Result<(), StoreError>;

    // ---- orgs / teams / repos ------------------------------------

    /// Upsert org by `github_id`.
    async fn upsert_org(&self, org: &Org) -> Result<Org, StoreError>;

    /// Upsert team by `(org_id, github_id)`.
    async fn upsert_team(&self, team: &Team) -> Result<Team, StoreError>;

    /// Upsert repo by `(org_id, github_id)`.
    async fn upsert_repo(&self, repo: &Repo) -> Result<Repo, StoreError>;

    /// Upsert a `(user, org)` membership, preserving `home_org` if
    /// already set — `set_home_org` is the only way to change it.
    async fn upsert_membership(&self, membership: &Membership) -> Result<Membership, StoreError>;

    /// List memberships for one user. Empty vec if none.
    async fn list_memberships_for_user(&self, user_id: Uuid)
        -> Result<Vec<Membership>, StoreError>;

    /// Set / clear the home-org label on a `(user, org)` membership
    /// (SCOPE §3 manual mapping). `None` clears it.
    async fn set_home_org(
        &self,
        user_id: Uuid,
        org_id: Uuid,
        home_org: Option<Uuid>,
    ) -> Result<(), StoreError>;

    /// Atomically flip the user's home org to `(user_id, org_id)`.
    ///
    /// Postcondition: among the user's memberships, exactly one row
    /// has `home_org = Some(org_id)` — the `(user_id, org_id)` row —
    /// and every other membership row for the same user has
    /// `home_org = None`. Implementations must apply the
    /// set-and-clear in one transaction so a reader can never observe
    /// two `home_org` values for the same user (Phase 4 D-home-org
    /// atomicity).
    ///
    /// Returns [`StoreError::NotFound`] if there is no `(user_id,
    /// org_id)` membership row to flip — the caller has to add the
    /// user to the org first.
    ///
    /// Default impl is the obvious non-atomic two-step using
    /// [`Self::set_home_org`]; production backends override it for
    /// the transactional guarantee.
    async fn set_home_org_for_user(
        &self,
        user_id: Uuid,
        org_id: Uuid,
    ) -> Result<(), StoreError> {
        // Best-effort default: clear-all-then-set. Backends that care
        // about the atomicity guarantee override this.
        let memberships = self.list_memberships_for_user(user_id).await?;
        for m in &memberships {
            if m.org_id != org_id && m.home_org.is_some() {
                self.set_home_org(user_id, m.org_id, None).await?;
            }
        }
        self.set_home_org(user_id, org_id, Some(org_id)).await
    }

    /// List every org dev-pulse has observed. Stage 4 of Phase 4
    /// surfaces this for `GET /orgs`. Default impl returns an empty
    /// vec so test fakes that don't seed orgs stay compiling.
    async fn list_orgs(&self) -> Result<Vec<crate::org::Org>, StoreError> {
        Ok(vec![])
    }

    /// List every team inside one org. Stage 4 of Phase 4 surfaces
    /// this for `GET /teams?org_id=…`.
    async fn list_teams_for_org(
        &self,
        _org_id: Uuid,
    ) -> Result<Vec<crate::team::Team>, StoreError> {
        Ok(vec![])
    }

    /// List the users that have a membership in `org_id`. Stage 4 of
    /// Phase 4 surfaces this for `GET /users?org_id=…`.
    async fn list_users_for_org(
        &self,
        _org_id: Uuid,
    ) -> Result<Vec<crate::user::User>, StoreError> {
        Ok(vec![])
    }

    // ---- repos / issues read surface (workflow drill-down) -------

    /// Paginated listing for the workflow's "Repos" pane. Backs
    /// `GET /repos` (dp-rest). Filters are conjunctive; defaults
    /// (no filter) return every repo across every org the store
    /// knows about.
    ///
    /// Implementations should return rows ordered by
    /// `last_activity_at DESC NULLS LAST, name ASC` so the UI gets
    /// "hottest first" for free.
    async fn list_repos(
        &self,
        _filter: &RepoListFilter,
    ) -> Result<Vec<RepoSummary>, StoreError> {
        Ok(vec![])
    }

    /// Total count matching the same `filter` (ignoring `limit` /
    /// `offset`). Pairs with [`Store::list_repos`] so the UI can
    /// render an "X of Y" pager.
    async fn count_repos(&self, _filter: &RepoListFilter) -> Result<i64, StoreError> {
        Ok(0)
    }

    /// Paginated listing for the workflow's "Issues" pane. Backs
    /// `GET /issues`. Sort: `updated_at DESC` to match GitHub's
    /// default "recently updated" view.
    async fn list_issues(
        &self,
        _filter: &IssueListFilter,
    ) -> Result<Vec<Issue>, StoreError> {
        Ok(vec![])
    }

    /// Total count for the issue filter.
    async fn count_issues(&self, _filter: &IssueListFilter) -> Result<i64, StoreError> {
        Ok(0)
    }

    /// Fetch a single repo row by primary key. Used by the §8 issue
    /// write path to resolve `repo_id -> (org_id, name)` before
    /// calling the GitHub backend. Default impl returns `None`;
    /// in-memory test fakes that don't seed repos stay compiling.
    async fn get_repo(&self, _id: Uuid) -> Result<Option<crate::repo::Repo>, StoreError> {
        Ok(None)
    }

    /// Fetch a single org row by primary key. Pairs with
    /// [`Store::get_repo`] to resolve `(org_login, repo_name)` for
    /// the §8 GitHub call. Default impl scans [`Store::list_orgs`]
    /// so storage implementations that override the listing surface
    /// don't need a separate point lookup.
    async fn get_org(&self, id: Uuid) -> Result<Option<crate::org::Org>, StoreError> {
        Ok(self.list_orgs().await?.into_iter().find(|o| o.id == id))
    }

    /// Fetch a single issue by primary key. The §8 detail pane
    /// uses this to re-read after a successful CAS write.
    async fn get_issue(&self, _id: Uuid) -> Result<Option<Issue>, StoreError> {
        Ok(None)
    }

    /// Fetch a single issue by `(repo_id, number)`. Backs
    /// `GET /repos/{repo_id}/issues/{number}` — the canonical
    /// deep-link shape the audit log already records.
    async fn get_issue_by_repo_and_number(
        &self,
        _repo_id: Uuid,
        _number: i64,
    ) -> Result<Option<Issue>, StoreError> {
        Ok(None)
    }

    // ---- per-user inbox (triage spine, slice 1) -------------------
    //
    // Backs the `★ My queue` smart view + inbox UX
    // (`linear-projects-idea.md` §3.8). All methods key on
    // `(user_id, issue_id)`; row absence means "default state" —
    // implicitly `Inbox`, `last_seen_version = 0`. The store layer
    // materialises that convention on read so callers never have
    // to special-case the missing row.

    /// Issue rows for the user's inbox view, with the per-user
    /// inbox metadata folded in (unread bit + status +
    /// snoozed_until). The filter narrows the candidate issue set
    /// in the same way as [`list_issues`]; the join with
    /// `dp_user_issue_state` adds:
    ///
    ///   * `status <> 'done'` (Done rows are dismissed and never
    ///     appear in the inbox view), and
    ///   * `status <> 'snoozed' OR snoozed_until < now()` (active
    ///     snoozes are hidden; expired snoozes surface again).
    ///
    /// Sort: `updated_at DESC` (same as `list_issues`).
    ///
    /// Default impl returns empty — only `dp-store-pg` provides a
    /// real implementation; the in-memory fakes used by other
    /// crates do not need inbox semantics.
    async fn list_inbox_issues(
        &self,
        _user_id: Uuid,
        _filter: &IssueListFilter,
    ) -> Result<Vec<InboxIssueRow>, StoreError> {
        Ok(vec![])
    }

    /// Total count of inbox-visible rows for the same filter that
    /// would drive [`list_inbox_issues`]. Matches the contract of
    /// [`count_issues`].
    async fn count_inbox_issues(
        &self,
        _user_id: Uuid,
        _filter: &IssueListFilter,
    ) -> Result<i64, StoreError> {
        Ok(0)
    }

    /// Mark a batch of issues as "read up to their current
    /// `dp_issues.version`" for one user. Upserts one row per
    /// `(user_id, issue_id)` in `dp_user_issue_state`, setting
    /// `last_seen_version = (SELECT version FROM dp_issues …)`.
    /// Existing `status` / `snoozed_until` values are preserved
    /// (this is the "you read it" signal, not the "you dismissed
    /// it" signal). Idempotent — re-marking a row sets the value
    /// to the same version (or higher if the issue has been
    /// updated in the meantime).
    ///
    /// Empty `issue_ids` is a no-op (the empty-list edge case
    /// belongs to the caller's UX, not the store).
    async fn mark_issues_seen(
        &self,
        _user_id: Uuid,
        _issue_ids: &[Uuid],
    ) -> Result<(), StoreError> {
        Err(StoreError::Invalid(
            "inbox state not supported by this store".into(),
        ))
    }

    /// Set `(status, snoozed_until)` for one `(user_id, issue_id)`.
    /// Upserts the row, preserving `last_seen_version` (the snooze
    /// / dismiss / restore actions do not move the seen marker).
    /// Returns the resulting row so the caller can echo it back to
    /// the UI without a second round-trip.
    ///
    /// Validation the store leaves to the caller: when
    /// `status == Inbox` or `Done`, `snoozed_until` should be
    /// `None`; when `status == Snoozed`, `snoozed_until` should be
    /// `Some(future_instant)`. The store does not enforce this so
    /// the UX can transiently set inconsistent pairs (e.g. clear
    /// a snooze by writing `Inbox` without first wiping the date).
    async fn set_inbox_state(
        &self,
        _user_id: Uuid,
        _issue_id: Uuid,
        _status: InboxStatus,
        _snoozed_until: Option<DateTime<Utc>>,
    ) -> Result<UserIssueState, StoreError> {
        Err(StoreError::Invalid(
            "inbox state not supported by this store".into(),
        ))
    }

    /// Bulk variant of [`Store::set_inbox_state`]: apply one
    /// `(status, snoozed_until)` pair to a batch of issues for one
    /// user. Empty `issue_ids` is a no-op. Returns the number of
    /// rows touched (inserted + updated).
    ///
    /// Semantics:
    /// * `status = Inbox`   — restore to the inbox; clears any snooze.
    /// * `status = Snoozed` — `snoozed_until` should be `Some(future)`.
    /// * `status = Done`    — dismiss; ignores `snoozed_until`.
    ///
    /// Last-seen-version is preserved on existing rows (this is the
    /// dismiss / snooze / restore action, not a "saw it" signal).
    /// New rows are inserted with `last_seen_version = 0` so the
    /// next render still shows them as unread until the user
    /// actually opens them.
    async fn set_inbox_state_bulk(
        &self,
        _user_id: Uuid,
        _issue_ids: &[Uuid],
        _status: InboxStatus,
        _snoozed_until: Option<DateTime<Utc>>,
    ) -> Result<u64, StoreError> {
        Err(StoreError::Invalid(
            "bulk inbox state not supported by this store".into(),
        ))
    }

    // ---- issue timeline (triage slice 2 — §5.6) -------------------

    /// Page of `dp_activity_events` rows scoped to one issue, used
    /// by `GET /issues/{id}/timeline`. Rows are produced newest
    /// first so the peek panel can render without re-sorting.
    ///
    /// Implementations match on the §6 expression-index predicate:
    /// `repo_id = $repo_id AND kind IN ('issue_opened',
    /// 'issue_closed', 'issue_comment') AND payload ? 'number' AND
    /// payload->>'number' ~ '^[0-9]+$' AND
    /// (payload->>'number')::int = $number`. The guard makes the
    /// cast safe under malformed history.
    ///
    /// Default impl returns empty so non-Postgres fakes (used in
    /// other crates' tests) don't fail; only `dp-store-pg`
    /// provides a real implementation.
    async fn list_events_for_issue(
        &self,
        _repo_id: Uuid,
        _number: i64,
        _limit: i64,
        _offset: i64,
    ) -> Result<Vec<IssueTimelineRow>, StoreError> {
        Ok(Vec::new())
    }

    /// Total event count matching [`list_events_for_issue`]. Same
    /// scope; used for the `total` envelope field.
    async fn count_events_for_issue(
        &self,
        _repo_id: Uuid,
        _number: i64,
    ) -> Result<i64, StoreError> {
        Ok(0)
    }

    // ---- repo sync status (triage slice 2 — §5.9) -----------------

    /// Read sync freshness for one repo, synthesised from the
    /// per-resource [`FetchCursor`] rows. Returns `None` if no
    /// cursor exists yet (the repo has never been synced).
    async fn get_repo_sync_status(
        &self,
        _repo_id: Uuid,
    ) -> Result<Option<RepoSyncStatus>, StoreError> {
        Ok(None)
    }

    // ---- issue metrics report (triage slice 2 — §5.10) ------------

    /// Compute one issue-report metric over the §5.10 SQL shapes.
    /// Implementations dispatch on the metric kind.
    async fn issue_metrics(
        &self,
        _filter: &IssueMetricsFilter,
    ) -> Result<Vec<IssueMetricRow>, StoreError> {
        Ok(Vec::new())
    }

    // ---- events + actors -----------------------------------------

    /// Insert (or upsert by `external_id`) one event row.
    /// Returns the resulting row. Idempotent on `external_id`.
    async fn record_event(&self, event: &ActivityEvent) -> Result<ActivityEvent, StoreError>;

    /// Attach actor rows to an event. Multi-actor by design (TODO
    /// §0.2) — pass every actor for the event in one call so the
    /// implementation can batch the insert. Idempotent on the
    /// composite key `(event_id, user_id, role)`.
    async fn add_event_actors(&self, actors: &[EventActor]) -> Result<(), StoreError>;

    /// Return every `(event_actor × event)` row whose event timestamp
    /// falls in `window`, optionally filtered to a set of orgs /
    /// repos / users / roles. The report layer's primary read.
    ///
    /// Filters are conjunctive; an empty slice means "no filter on
    /// this dimension".
    async fn list_event_actor_rows_in_window(
        &self,
        window: &Window,
        orgs: &[Uuid],
        repos: &[Uuid],
        users: &[Uuid],
        roles: &[ActorRole],
    ) -> Result<Vec<EventActorRow>, StoreError>;

    // ---- cursors + run log ---------------------------------------

    /// Read the cursor for `(org_id, repo_id, resource_kind)`. Returns
    /// `NotFound` if there has never been one written.
    async fn get_cursor(
        &self,
        org_id: Uuid,
        repo_id: Option<Uuid>,
        resource_kind: ResourceKind,
    ) -> Result<FetchCursor, StoreError>;

    /// Upsert the cursor for `(org_id, repo_id, resource_kind)`.
    /// Composite PK — at most one row per tuple.
    async fn put_cursor(&self, cursor: &FetchCursor) -> Result<(), StoreError>;

    /// Insert a new `fetch_runs` row with `started = now()`. Returns
    /// the assigned id.
    async fn start_fetch_run(&self, kind: FetchRunKind) -> Result<Uuid, StoreError>;

    /// Mark a run finished, with item / error / partial flags.
    async fn finish_fetch_run(
        &self,
        id: Uuid,
        items: i64,
        errors: i64,
        partial: bool,
    ) -> Result<(), StoreError>;

    /// List the most recent `limit` runs of any kind, newest first.
    async fn list_recent_fetch_runs(&self, limit: i64) -> Result<Vec<FetchRun>, StoreError>;

    /// Paginated projection over `dp_fetch_runs` ordered newest
    /// first. Phase 4 stage 5 surfaces this on `GET /admin/runs`.
    ///
    /// Default impl falls back to [`Self::list_recent_fetch_runs`]
    /// reading `limit + offset` rows and discarding the prefix —
    /// inefficient but keeps every existing fake compiling. The PG
    /// backend overrides with `LIMIT … OFFSET …`.
    async fn list_fetch_runs(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<FetchRun>, StoreError> {
        let take = limit.max(0);
        let skip = offset.max(0);
        let total = take.saturating_add(skip);
        let mut rows = self.list_recent_fetch_runs(total).await?;
        let skip = skip as usize;
        if skip >= rows.len() {
            return Ok(Vec::new());
        }
        Ok(rows.split_off(skip))
    }

    /// Page through every `event_actor` row credited to `user_id`,
    /// joined back to its parent event. Ordered by `(ts ASC,
    /// event_id ASC)` for a stable streaming order across pages.
    ///
    /// Phase 4 stage 5 uses this to chunk the GDPR export so a
    /// 500MB user history does not need to materialise in process
    /// memory. Default impl returns the empty vec so test fakes
    /// that don't model events stay green.
    async fn list_event_actor_rows_for_user_page(
        &self,
        _user_id: Uuid,
        _offset: i64,
        _limit: i64,
    ) -> Result<Vec<EventActorRow>, StoreError> {
        Ok(Vec::new())
    }

    /// Snapshot the data-freshness envelope every report response
    /// carries (SCOPE §11.7 / TODO §0.3).
    ///
    /// Returns:
    ///
    /// * `webhook_latest` — `MAX(finished)` of `dp_fetch_runs` rows
    ///   where `kind = webhook_worker` and `finished IS NOT NULL`.
    /// * `reconciler_latest` — same, for `kind = reconciler`.
    /// * `per_org` — `MAX(updated_at)` of `dp_fetch_cursors` grouped
    ///   by `org_id`. Orgs that have no cursor rows yet (no
    ///   reconciler tick has ever touched them) are absent rather
    ///   than mapped to a sentinel.
    ///
    /// Cheap — three indexed aggregates, no per-row scan. Reports
    /// call this once per request and the result rides on the
    /// response envelope.
    async fn data_as_of(&self) -> Result<DataAsOf, StoreError>;

    // ---- webhook inbox -------------------------------------------

    /// Enqueue a webhook delivery. Unique constraint on `delivery_id`
    /// surfaces replays as [`StoreError::Conflict`] — the receiver
    /// translates that into a 200 OK (idempotent).
    async fn enqueue_webhook(&self, delivery: &WebhookDelivery) -> Result<(), StoreError>;

    /// Claim up to `max` unprocessed deliveries for the worker to
    /// drain. Implementations should use `SELECT ... FOR UPDATE SKIP
    /// LOCKED` (Postgres) so multiple workers don't fight over the
    /// same row.
    async fn claim_webhooks(&self, max: i64) -> Result<Vec<WebhookDelivery>, StoreError>;

    /// Mark a delivery processed (success path).
    async fn mark_webhook_processed(&self, id: Uuid) -> Result<(), StoreError>;

    /// Record a processing failure on a delivery so the worker can
    /// retry. Stores the error text and leaves `processed_at` NULL.
    async fn mark_webhook_failed(&self, id: Uuid, error: &str) -> Result<(), StoreError>;

    // ---- audit log ------------------------------------------------

    /// Insert one `dp_audit_log` row (SCOPE §9). Phase 4 D4.4 pins
    /// the `action` vocabulary in `dp-rest::audit`; this method is
    /// vocabulary-free so other surfaces can write their own verbs
    /// later. Default impl is a no-op so test fakes that don't care
    /// about the audit trail stay green.
    async fn record_audit_log(&self, _entry: &AuditEntry) -> Result<(), StoreError> {
        Ok(())
    }

    // ---- pins (SCOPE-PROJECTS §6) -------------------------------
    //
    // Default impls return empty / no-op so the existing in-memory
    // fakes used by `dp-reports`, `dp-rest`, `dp-mcp`, and the
    // fetcher integration tests do not have to grow new code to
    // keep compiling. The Postgres backend overrides each one.

    /// List a user's pins, ordered by `position` ascending. Returns
    /// an empty vec if the user has no pins. Default: empty.
    async fn list_pins_for_user(&self, _user_id: Uuid) -> Result<Vec<Pin>, StoreError> {
        Ok(Vec::new())
    }

    /// Append a pin to the end of a user's list. Implementations
    /// must reject the insert (return [`StoreError::Invalid`]) if it
    /// would exceed the configured per-user pin cap (working
    /// assumption 20; §6.1 + §13.5). The composite PK
    /// `(user_id, kind, target_id)` makes re-pinning idempotent at
    /// the schema level — a duplicate is a [`StoreError::Conflict`].
    async fn add_pin(&self, _pin: &Pin) -> Result<Pin, StoreError> {
        Err(StoreError::Invalid("pins not supported by this store".into()))
    }

    /// Remove a pin by its composite key. Returns
    /// [`StoreError::NotFound`] if the pin does not exist.
    async fn remove_pin(
        &self,
        _user_id: Uuid,
        _kind: crate::pin::PinKind,
        _target_id: Uuid,
    ) -> Result<(), StoreError> {
        Err(StoreError::Invalid("pins not supported by this store".into()))
    }

    /// Atomically rewrite the ordering of a user's pins. The slice
    /// is the new `(kind, target_id)` order — entry `i` becomes
    /// `position = i`. Implementations apply the rewrite in one
    /// transaction; partial reorders are not visible to readers.
    /// Returns [`StoreError::Invalid`] if `order` does not exactly
    /// cover the user's current pins.
    async fn reorder_pins(
        &self,
        _user_id: Uuid,
        _order: &[(crate::pin::PinKind, Uuid)],
    ) -> Result<(), StoreError> {
        Err(StoreError::Invalid("pins not supported by this store".into()))
    }

    // ---- tags + tag links (SCOPE-PROJECTS §7) -------------------

    /// Fetch a tag by primary key. Returns [`StoreError::NotFound`]
    /// if the row does not exist.
    async fn get_tag(&self, _id: Uuid) -> Result<Tag, StoreError> {
        Err(StoreError::NotFound {
            entity: "tag",
            id: _id.to_string(),
        })
    }

    /// Create a tag. The per-scope case-insensitive uniqueness on
    /// `(scope_kind, scope_id, lower(name))` is enforced by the
    /// migration-0005 expression index; the Postgres backend
    /// translates the unique-constraint violation into
    /// [`StoreError::Conflict`].
    async fn create_tag(&self, _tag: &Tag) -> Result<Tag, StoreError> {
        Err(StoreError::Invalid("tags not supported by this store".into()))
    }

    /// Patch a tag's `name` / `color` / `description` / `archived_at`
    /// in place. The `scope_*` columns and `created_by` are
    /// immutable from this method — promotion across scopes is a §12
    /// open question and out of scope for v1. Passing `None` for a
    /// field leaves it unchanged; passing `Some(None)` for
    /// `description` / `archived_at` clears it.
    async fn update_tag(
        &self,
        _id: Uuid,
        _name: Option<&str>,
        _color: Option<&str>,
        _description: Option<Option<&str>>,
        _archived_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    ) -> Result<Tag, StoreError> {
        Err(StoreError::Invalid("tags not supported by this store".into()))
    }

    /// List every tag visible to a viewer. Visibility filtering
    /// (§7.4) is the *caller's* responsibility — pass the set of
    /// scope ids the viewer can see for each scope kind.
    /// Implementations return rows whose `scope_kind` matches one of
    /// the provided scope id slices. An empty slice for a scope
    /// kind means "no tags of that kind". Archived tags are
    /// excluded unless `include_archived` is true.
    async fn list_tags_visible_to(
        &self,
        _viewer_user_id: Uuid,
        _visible_team_ids: &[Uuid],
        _visible_org_ids: &[Uuid],
        _include_archived: bool,
    ) -> Result<Vec<Tag>, StoreError> {
        Ok(Vec::new())
    }

    /// List the links attached to a tag, optionally filtered to a
    /// subset of link kinds (empty slice = all kinds). The
    /// viewer-visibility filter in §7.4 is the caller's job; this
    /// method returns every link the tag carries.
    async fn list_tag_links(
        &self,
        _tag_id: Uuid,
        _kinds: &[TagLinkKind],
    ) -> Result<Vec<TagLink>, StoreError> {
        Ok(Vec::new())
    }

    /// Attach a batch of links to a tag, **transactionally
    /// all-or-nothing** (§7.5). If any link fails validation
    /// (duplicate, target not visible, wrong kind, missing target
    /// row), the whole batch is rejected. The unique index
    /// `dp_tag_links_tag_target_uniq` provides the duplicate check
    /// at the schema level.
    async fn add_tag_links(&self, _links: &[TagLink]) -> Result<Vec<TagLink>, StoreError> {
        Err(StoreError::Invalid("tags not supported by this store".into()))
    }

    /// Detach a batch of links by id, transactionally all-or-nothing
    /// (§7.5). Returns [`StoreError::NotFound`] if any id is
    /// missing — no partial unlinks.
    async fn remove_tag_links(&self, _link_ids: &[Uuid]) -> Result<(), StoreError> {
        Err(StoreError::Invalid("tags not supported by this store".into()))
    }

    /// Resolve a set of tag ids to the `(repo_id, issue_id,
    /// user_id, team_id)` targets they currently link, for the
    /// §15.6 report-filter path (SCOPE-PROJECTS §7.7). Implementations
    /// apply the viewer-visibility filter using the supplied
    /// allow-lists.
    async fn resolve_tag_targets(
        &self,
        _tag_ids: &[Uuid],
        _visible_repo_ids: &[Uuid],
        _visible_user_ids: &[Uuid],
        _visible_team_ids: &[Uuid],
    ) -> Result<Vec<TagLink>, StoreError> {
        Ok(Vec::new())
    }

    // ---- GitHub App installation permissions (SCOPE-PROJECTS §8.4, §13.6) ----
    //
    // The reconciler / install-callback writes one row per org
    // capturing whether the install was granted `issues: write`.
    // The §8 write surface reads through this; the §13.6 banner
    // endpoint enumerates orgs whose row says writes are
    // unavailable. Stage 8 lands the trait method as a `None`-by-
    // default read; the postgres backend grows the
    // `dp_org_app_installs` table in a later migration of this
    // same job. Fakes / test stubs inherit the default and behave
    // as if no orgs have writes available — a fail-closed posture
    // that matches the §8.4 §13.6 decision.

    /// Look up the per-org GitHub App install record (if any).
    ///
    /// Returns `Ok(None)` when no install row has been observed
    /// for `org_id` yet — callers treat this as **writes not
    /// available** (§8.4 fail-closed). Returns `Ok(Some(_))` with
    /// the latest observed permissions otherwise.
    ///
    /// The default impl returns `Ok(None)` so existing test fakes
    /// and the partially-migrated postgres backend stay compiling
    /// through stage 8; a follow-up stage of this job overrides
    /// it with the real Postgres query.
    async fn get_org_app_install(
        &self,
        _org_id: Uuid,
    ) -> Result<Option<OrgAppInstall>, StoreError> {
        Ok(None)
    }

    // ---- issue mutations (SCOPE-PROJECTS §8.2 + §8.5 + §13.7) ----
    //
    // Storage landed in `0007_issues_optimistic_cas.sql` (stage 9 of
    // this same job): four new columns on `dp_issues` (`version`,
    // `pending_remote`, `pending_remote_at`, `pending_remote_actor`)
    // plus the `dp_issue_mutations` audit table. The trait surface
    // exposed here is the *primitive* set the §8.2 write path and
    // the §8.5 sweeper compose against — no GitHub I/O, no
    // octocrab; that wiring lives in the dp-rest handler.
    //
    // The CAS is split into two halves on purpose: writers
    // `try_acquire_issue_pending_remote` (bumps version, sets the
    // pending flag) *before* the GitHub round-trip, and
    // `release_issue_pending_remote` (clears the flag, optionally
    // bumps version again for the §8.2 step 8 rollback) *after*.
    // No row-lock is held across the network call (§13.4).

    /// §8.2 step 5: atomic CAS that bumps `dp_issues.version` and
    /// raises the `pending_remote` flag in one statement.
    ///
    /// The SQL clause is `WHERE id = ? AND version = ? AND
    /// pending_remote = false` — that is, the CAS rejects both
    /// `expected_version` mismatch *and* the case where another
    /// in-flight write already holds the slot.
    ///
    /// Returns:
    ///
    /// * `Ok(Some(new_version))` — one row updated, write may
    ///   proceed; `new_version = expected_version + 1`.
    /// * `Ok(None)` — zero rows updated; the dp-rest handler
    ///   translates this into the `409 stale_local_version`
    ///   response (§8.3).
    async fn try_acquire_issue_pending_remote(
        &self,
        _issue_id: Uuid,
        _expected_version: i64,
        _actor_user_id: Uuid,
    ) -> Result<Option<i64>, StoreError> {
        Err(StoreError::Invalid(
            "issue mutations not supported by this store".into(),
        ))
    }

    /// §8.2 step 7 (success) or §8.2 step 8 (failure / rollback) —
    /// clears `pending_remote`, `pending_remote_at`, and
    /// `pending_remote_actor` in a single statement. When
    /// `bump_version_again` is `true` (the §8.2 step 8 path) the
    /// SQL also runs `version = version + 1` so any concurrent
    /// reader sees the rollback as a change. Returns the row's
    /// `version` after this update.
    ///
    /// Idempotent: a row that is no longer pending (e.g. a sweeper
    /// already touched it) does not error — the method updates
    /// zero rows in that case and returns the current version.
    async fn release_issue_pending_remote(
        &self,
        _issue_id: Uuid,
        _bump_version_again: bool,
    ) -> Result<i64, StoreError> {
        Err(StoreError::Invalid(
            "issue mutations not supported by this store".into(),
        ))
    }

    /// Read `dp_issues.version` only. Tests and the §8.3 conflict
    /// response use this to surface the current version to the UI
    /// without rehydrating the whole row.
    async fn get_issue_version(
        &self,
        _issue_id: Uuid,
    ) -> Result<i64, StoreError> {
        Err(StoreError::Invalid(
            "issue mutations not supported by this store".into(),
        ))
    }

    /// §13.7 reconciler guard helper. Returns rows where
    /// `pending_remote = true` and `pending_remote_at < cutoff`.
    /// Drives the §8.5 timeout sweeper — every row returned needs
    /// (a) `release_issue_pending_remote(_, true)` to bump version
    /// and clear the flag and (b) a `pending_remote_timeout` audit
    /// row.
    async fn list_issues_with_pending_remote_older_than(
        &self,
        _cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<PendingRemoteIssue>, StoreError> {
        Ok(Vec::new())
    }

    /// Record a new [`IssueMutation`] row in `Pending` state. Called
    /// from §8.2 step 5, immediately after
    /// `try_acquire_issue_pending_remote` succeeded.
    async fn record_issue_mutation(
        &self,
        _mutation: &IssueMutation,
    ) -> Result<IssueMutation, StoreError> {
        Err(StoreError::Invalid(
            "issue mutations not supported by this store".into(),
        ))
    }

    /// Transition an [`IssueMutation`] out of `Pending` (§8.2 step 7
    /// / step 8 / sweeper). Sets `result`, optionally
    /// `github_delivery_id` / `error`, and stamps `finished_at =
    /// now()`. Updating an already-finished row is a no-op (the
    /// CHECK constraint on `dp_issue_mutations.result` would not
    /// catch the race, but the sweeper / handler interleaving is
    /// designed so only one writer ever calls this for a given id).
    async fn update_issue_mutation_result(
        &self,
        _id: Uuid,
        _result: IssueMutationResult,
        _github_delivery_id: Option<&str>,
        _error: Option<&str>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Invalid(
            "issue mutations not supported by this store".into(),
        ))
    }

    /// Find audit rows stuck in `Pending` past the
    /// `issues.pending_remote_timeout_secs` window. Mirror of
    /// [`Store::list_issues_with_pending_remote_older_than`] for
    /// the audit table — the sweeper joins the two by `issue_id`
    /// to decide whether to emit a fresh `pending_remote_timeout`
    /// row or update the existing one.
    async fn list_pending_issue_mutations_older_than(
        &self,
        _cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<IssueMutation>, StoreError> {
        Ok(Vec::new())
    }

    // ---- §13.7 reconciler guard + webhook replay buffer ---------------
    //
    // These primitives back the SCOPE-PROJECTS §13.7 invariant: the
    // fetcher / webhook reconciler must *not* overwrite a `dp_issues`
    // row whose `pending_remote = TRUE` and whose `pending_remote_at`
    // is younger than `issues.pending_remote_timeout_secs`. Webhook
    // payloads that would otherwise be applied to such a row are
    // buffered into `dp_pending_remote_webhook_buffer` and replayed
    // through the normal handler path once the flag clears (§8.2
    // step 7 / step 8 / §8.5 sweeper).
    //
    // Default impls keep test fakes and the in-memory MCP store
    // compiling; the `dp-store-pg` backend overrides each one.

    /// Look up `dp_repos.id` from GitHub's numeric repo id.
    /// Returns `Ok(None)` if no local repo row exists — the
    /// guard's "first sighting" branch. The §13.7 webhook guard
    /// uses this to resolve `payload.repository.id` to a local
    /// repo without forcing an upsert (which would mutate state
    /// before the guard decision had been made).
    async fn find_repo_id_by_github_id(
        &self,
        _github_repo_id: i64,
    ) -> Result<Option<Uuid>, StoreError> {
        Ok(None)
    }

    /// Look up `dp_issues.id` from `(repo_id, github_issue_id)`.
    /// Returns `Ok(None)` when no such row exists yet — meaning
    /// nothing on the dev-pulse side can be pending and the caller
    /// should just apply the delivery normally.
    ///
    /// `github_issue_id` is GitHub's per-issue numeric id (the
    /// `issue.id` field in webhook payloads), not the
    /// repo-relative `issue.number`. The §8 write path keys on
    /// `id`, not `number`, because numbers are reassigned when an
    /// issue is transferred between repos.
    async fn find_issue_id_by_repo_and_github_id(
        &self,
        _repo_id: Uuid,
        _github_issue_id: i64,
    ) -> Result<Option<Uuid>, StoreError> {
        Ok(None)
    }

    /// §13.7 guard predicate. Returns `true` when the row exists,
    /// `pending_remote = TRUE`, and `pending_remote_at >= now() -
    /// timeout`. A `false` result means the reconciler may apply
    /// its payload to the row.
    ///
    /// Centralising the timeout comparison in the store keeps the
    /// clock authoritative (SQL `now()` rather than the host wall
    /// clock) on the postgres backend, matching the §8.2 / §8.5
    /// `pending_remote_at` write side.
    async fn is_issue_pending_remote_fresh(
        &self,
        _issue_id: Uuid,
        _timeout: chrono::Duration,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Stash a webhook delivery on the §13.7 buffer so it can be
    /// replayed after the pending_remote flag clears. Inserted
    /// rows are de-duped on `delivery_id` (matching the inbox's
    /// at-least-once-from-GitHub invariant): a duplicate
    /// `delivery_id` returns `StoreError::Conflict`, which the
    /// caller should treat as a benign "already buffered, nothing
    /// more to do".
    async fn buffer_pending_remote_webhook(
        &self,
        _issue_id: Uuid,
        _delivery: &WebhookDelivery,
    ) -> Result<(), StoreError> {
        Err(StoreError::Invalid(
            "pending_remote webhook buffer not supported by this store".into(),
        ))
    }

    /// Drain every buffered webhook for `issue_id`, oldest first
    /// (`ORDER BY buffered_at`). Returned rows are deleted from
    /// the buffer in the same SQL statement so the replay is at-
    /// least-once but not at-most-once: a crash between this call
    /// and `apply_delivery` loses the buffered copy. That is
    /// considered acceptable — GitHub's at-least-once webhook
    /// delivery contract plus the next reconciler tick will
    /// re-observe the same authoritative state shortly.
    async fn take_buffered_webhooks_for_issue(
        &self,
        _issue_id: Uuid,
    ) -> Result<Vec<WebhookDelivery>, StoreError> {
        Ok(Vec::new())
    }

    // ---- issue dates (triage slice 2 — §3.10) --------------------

    /// Read the `dp_issue_dates` sidecar row for an issue, or
    /// `None` when none exists yet (the issue has never had dates
    /// set). Default impl returns `None` so in-memory fakes don't
    /// need to model dates.
    async fn get_issue_dates(
        &self,
        _issue_id: Uuid,
    ) -> Result<Option<IssueDates>, StoreError> {
        Ok(None)
    }

    /// Synchronous upsert of `(start_at, due_at)` on
    /// `dp_issue_dates`. Returns the post-upsert row so the
    /// handler can echo the canonical timestamps back to the UI.
    /// The schema CHECK guards `start_at <= due_at`; violations
    /// surface as [`StoreError::Invalid`] in the postgres backend.
    /// Default impl rejects the call so misuse from fakes is loud.
    async fn upsert_issue_dates(
        &self,
        _issue_id: Uuid,
        _start_at: Option<DateTime<Utc>>,
        _due_at: Option<DateTime<Utc>>,
    ) -> Result<IssueDates, StoreError> {
        Err(StoreError::Invalid(
            "issue dates not supported by this store".into(),
        ))
    }

    /// Write the mirror outcome back to `dp_issue_dates`. On
    /// success: clears `mirror_error`, stamps `mirror_synced_at`,
    /// and persists the Projects v2 *item* node id (so the next
    /// mirror reuses it). On failure: stamps `mirror_error` only.
    /// Default impl is a no-op so the date upsert always succeeds
    /// even when the store lacks the table.
    async fn record_issue_dates_mirror_result(
        &self,
        _issue_id: Uuid,
        _outcome: IssueDatesMirrorOutcome<'_>,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    /// Read the `dp_repo_project_link` row for a repo, or `None`
    /// when the repo is not linked to a Projects v2 project.
    async fn get_repo_project_link(
        &self,
        _repo_id: Uuid,
    ) -> Result<Option<RepoProjectLink>, StoreError> {
        Ok(None)
    }

    /// Enqueue a `dp_projectv2_mirror_tasks` row. Best-effort by
    /// contract — the handler ignores errors from this call so
    /// the local upsert is never blocked. Default impl is a no-op.
    async fn enqueue_projectv2_mirror_task(
        &self,
        _issue_id: Uuid,
        _repo_id: Uuid,
        _kind: ProjectV2MirrorTaskKind,
        _payload: serde_json::Value,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    /// Drain up to `max` pending `mirror_dates` / `pull_back` rows
    /// ordered by `enqueued_at ASC`. Slice-3 worker entry point;
    /// returns the empty vec here so existing fakes stay green.
    async fn claim_projectv2_mirror_tasks(
        &self,
        _max: i64,
    ) -> Result<Vec<ProjectV2MirrorTask>, StoreError> {
        Ok(Vec::new())
    }
}

/// Outcome of a single Projects v2 mirror attempt, fed back into
/// [`Store::record_issue_dates_mirror_result`]. Borrowed strings
/// so the worker can pass GraphQL error text straight from its
/// transport buffer without an intermediate allocation.
#[derive(Debug, Clone, Copy)]
pub enum IssueDatesMirrorOutcome<'a> {
    /// Mirror succeeded; `node_id` is the Projects v2 *item* node
    /// id GitHub returned (persist so the next edit updates the
    /// same item instead of creating a duplicate card).
    Success {
        /// The Projects v2 item node id to persist.
        node_id: &'a str,
    },
    /// Mirror failed; `error` is the verbatim GraphQL error text.
    Failure {
        /// Error text to persist to `mirror_error`.
        error: &'a str,
    },
}

/// Compact projection of `dp_issues` rows the §8.5 sweeper needs:
/// the issue id, the version after the abandoned CAS, the actor
/// who started the write, and the `pending_remote_at` timestamp.
/// Returned by [`Store::list_issues_with_pending_remote_older_than`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRemoteIssue {
    /// `dp_issues.id`.
    pub issue_id: Uuid,
    /// `dp_issues.repo_id`. Denormalised for the audit row.
    pub repo_id: Uuid,
    /// Current `dp_issues.version` (post-CAS, pre-rollback).
    pub version: i64,
    /// The dp-pulse user who initiated the abandoned write.
    pub actor_user_id: Uuid,
    /// When the abandoned CAS landed. The sweeper picks rows where
    /// this is older than `now() - pending_remote_timeout_secs`.
    pub pending_remote_at: DateTime<Utc>,
}

/// One row in the timeline returned by
/// [`Store::list_events_for_issue`]. Mirrors the shape `GET
/// /issues/{id}/timeline` emits — see `linear-projects-idea.md`
/// §5.6.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueTimelineRow {
    /// `dp_activity_events.id`.
    pub id: Uuid,
    /// Event kind, parsed back into the typed enum.
    pub kind: EventKind,
    /// Source timestamp (`ts`), UTC.
    pub ts: DateTime<Utc>,
    /// One-line summary derived from `payload` — `"opened"`,
    /// `"closed"`, `"commented: <body excerpt>"`, …
    pub payload_summary: String,
}

/// Repo sync freshness — synthesised from `dp_fetch_cursors` plus
/// scheduler state. See `linear-projects-idea.md` §3.9 / §5.9.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSyncStatus {
    /// Newest `dp_fetch_cursors.updated_at` seen for this repo.
    pub last_synced_at: Option<DateTime<Utc>>,
    /// Same source as `last_synced_at` until the schema grows a
    /// dedicated `attempted_at` column (no error column exists
    /// today; treat success and attempt as the same instant).
    pub last_attempt_at: Option<DateTime<Utc>>,
    /// Last sync error message, or `None` when the latest sync
    /// succeeded. Currently always `None` — the cursor row carries
    /// no error column; an explicit error projection would arrive
    /// in a follow-up migration.
    pub last_error: Option<String>,
}

/// Which §5.10 report metric to compute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueMetric {
    /// Closed issues per bucket.
    Throughput,
    /// Median open → close duration per bucket (seconds).
    LeadTime,
    /// Currently-open assigned count.
    Wip,
    /// Open + idle (`updated_at < now() - interval '30 days'`).
    Stale,
    /// Open + no assignee + no label.
    Untriaged,
}

/// Group-by axis for §5.10 metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueMetricGroupBy {
    /// Group by `repo_id`.
    Repo,
    /// Group by `org_id`.
    Org,
    /// Group by per-row `assignee` (`jsonb_array_elements_text`).
    Assignee,
    /// Group by ISO week (`date_trunc('week', ...)`).
    Week,
    /// Group by ISO day (`date_trunc('day', ...)`).
    Day,
}

/// Filter passed to [`Store::issue_metrics`].
#[derive(Debug, Clone)]
pub struct IssueMetricsFilter {
    /// Which metric to compute.
    pub metric: IssueMetric,
    /// Group-by axis.
    pub group_by: IssueMetricGroupBy,
    /// Inclusive lower bound on the event timestamp (`since`).
    pub since: Option<DateTime<Utc>>,
    /// Exclusive upper bound on the event timestamp (`until`).
    pub until: Option<DateTime<Utc>>,
    /// Restrict to these orgs (caller's `org_ids ∩ wire scope`).
    pub org_ids: Vec<Uuid>,
    /// Restrict to these repos.
    pub repo_ids: Vec<Uuid>,
}

/// One row in the §5.10 reports response.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueMetricRow {
    /// Bucket label — repo slug, org login, login, or RFC3339 date.
    pub bucket: String,
    /// Metric value. Unit depends on the metric: count for
    /// throughput / wip / stale / untriaged, seconds (median) for
    /// lead_time.
    pub value: f64,
    /// Row count contributing to `value` (used by the lead-time
    /// median so the frontend can show "n=12").
    pub count: i64,
}

/// Filter for [`Store::list_repos`] / [`Store::count_repos`].
///
/// All fields are conjunctive. `limit` is capped at
/// [`MAX_LIST_LIMIT`] by the dp-rest layer before it reaches the
/// store; the store treats it as a hard upper bound.
#[derive(Debug, Clone, Default)]
pub struct RepoListFilter {
    /// Restrict to one org. `None` ⇒ every org.
    pub org_id: Option<Uuid>,
    /// Case-insensitive substring search on `dp_repos.name` and
    /// `dp_orgs.login`. `None` or empty ⇒ no search.
    pub q: Option<String>,
    /// Page size. 1..=[`MAX_LIST_LIMIT`].
    pub limit: i64,
    /// Page offset.
    pub offset: i64,
}

/// Filter for [`Store::list_issues`] / [`Store::count_issues`] /
/// [`Store::list_inbox_issues`].
///
/// Fields combine conjunctively (AND across the struct).
/// Repeatable fields (`repo_ids`, `org_ids`, `assignees`, `labels`)
/// are ALSO conjunctive within themselves — matching Linear's
/// pill semantics, where adding a second label narrows the set
/// rather than widening it.
///
/// Scalar fields (`repo_id`, `org_id`, `assignee`) are retained for
/// back-compat with the early `GET /issues` callers. When both
/// scalar and array forms are populated, the predicate is the
/// intersection (both apply). The dp-rest layer normalises a
/// scalar into the matching array before calling, so most
/// callers should only populate the array form.
#[derive(Debug, Clone, Default)]
pub struct IssueListFilter {
    /// Restrict to one repo (back-compat shorthand for
    /// `repo_ids = vec![…]`).
    pub repo_id: Option<Uuid>,
    /// Restrict to one org (back-compat shorthand for
    /// `org_ids = vec![…]`).
    pub org_id: Option<Uuid>,
    /// Filter by state. `None` ⇒ open + closed.
    pub state: Option<IssueState>,
    /// Match an assignee login (back-compat shorthand for
    /// `assignees = vec![…]`).
    pub assignee: Option<String>,
    /// Case-insensitive substring search on `dp_issues.title`.
    pub q: Option<String>,
    /// Page size. 1..=[`MAX_LIST_LIMIT`].
    pub limit: i64,
    /// Page offset.
    pub offset: i64,

    // ---- triage-spine extensions (slice 1) --------------------

    /// Match issues whose `repo_id` is in this set. Empty ⇒ no
    /// constraint. Logically OR within the set (any of these
    /// repos) but AND with the other filter fields.
    pub repo_ids: Vec<Uuid>,
    /// Match issues whose `org_id` is in this set. Empty ⇒ no
    /// constraint. The `/me/queue` handler always populates this
    /// with the caller's org set so per-row authz is enforced in
    /// SQL even if the policy layer ever degrades open.
    pub org_ids: Vec<Uuid>,
    /// Match issues having **all** of these assignees (JSONB
    /// containment AND). Empty ⇒ no constraint.
    pub assignees: Vec<String>,
    /// Match issues having **all** of these labels (JSONB
    /// containment AND). Empty ⇒ no constraint.
    pub labels: Vec<String>,
    /// Match issues whose `author` column equals this value. Rows
    /// where `author IS NULL` (un-backfilled) never match — same
    /// behaviour as any other scalar filter.
    pub author: Option<String>,
    /// Match issues whose `state_reason` column equals this value
    /// (e.g. `"completed"` / `"not_planned"` / `"reopened"`).
    pub state_reason: Option<String>,
    /// Match issues with `updated_at >= updated_since`.
    pub updated_since: Option<DateTime<Utc>>,
    /// Untriaged smart-view shortcut: when true, restrict to rows
    /// with **no** assignees and **no** labels. Combines with the
    /// rest of the filter (so "Untriaged in org X" is one call).
    pub untriaged_only: bool,
    /// Optional keyset cursor used by `/me/queue` pagination.
    /// When `Some((ts, id))`, the store emits a strictly-less-than
    /// page on `(updated_at, id)` so concurrent inbox mutations do
    /// not produce drift across pages. Empty for non-keyset
    /// callers.
    pub keyset_after: Option<(DateTime<Utc>, Uuid)>,
}

/// Hard upper bound on `limit` across the workflow read surface.
pub const MAX_LIST_LIMIT: i64 = 200;

/// Default `limit` when the caller omits one.
pub const DEFAULT_LIST_LIMIT: i64 = 50;

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time check that [`Store`] is object-safe — every
    /// surface (rest / mcp / cli / reports) holds an
    /// `Arc<dyn Store>`, so a regression would break the world.
    #[allow(dead_code)]
    fn store_is_object_safe(_s: &dyn Store) {}

    #[test]
    fn store_error_displays_known_variants() {
        let e = StoreError::NotFound {
            entity: "user",
            id: "00000000-0000-0000-0000-000000000000".into(),
        };
        assert!(format!("{e}").contains("not found"));
        let c = StoreError::Conflict("dup delivery_id".into());
        assert!(format!("{c}").contains("conflict"));
    }
}
