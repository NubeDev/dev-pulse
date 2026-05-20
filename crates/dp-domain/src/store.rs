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
