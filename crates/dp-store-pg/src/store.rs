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
use dp_domain::fetch::{FetchCursor, FetchRun, FetchRunKind, ResourceKind};
use dp_domain::freshness::DataAsOf;
use dp_domain::membership::Membership;
use dp_domain::org::Org;
use dp_domain::pin::{Pin, PinKind};
use dp_domain::repo::Repo;
use dp_domain::issue_mutation::{IssueMutation, IssueMutationOp, IssueMutationResult};
use dp_domain::store::{EventActorRow, PendingRemoteIssue, Store, StoreError};
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

    async fn list_recent_fetch_runs(&self, limit: i64) -> Result<Vec<FetchRun>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, kind, started, finished, items, errors, partial \
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
            "SELECT id, kind, started, finished, items, errors, partial \
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
