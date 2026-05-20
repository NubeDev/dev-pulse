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

use crate::audit::AuditEntry;
use crate::event::{ActivityEvent, ActorRole, EventActor, EventKind};
use crate::fetch::{FetchCursor, FetchRun, FetchRunKind, ResourceKind};
use crate::freshness::DataAsOf;
use crate::membership::Membership;
use crate::org::Org;
use crate::repo::Repo;
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
