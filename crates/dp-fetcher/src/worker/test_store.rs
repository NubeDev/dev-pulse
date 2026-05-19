//! In-memory [`Store`] fake used by the worker + handler tests.
//!
//! Implements only the methods the worker drain-path actually
//! touches. The unused methods panic so a future regression that
//! reaches for them surfaces immediately (vs. silently passing
//! against a method that returned `Ok(default)`).
//!
//! Behaviour matches `dp-store-pg`'s semantics where it matters:
//!
//! * `enqueue_webhook` rejects duplicate `delivery_id` with
//!   [`StoreError::Conflict`] (the receiver's replay contract).
//! * `claim_webhooks` returns unprocessed rows and *does not*
//!   mark them claimed — `mark_webhook_processed` /
//!   `mark_webhook_failed` are how the worker advances them.
//! * `record_event` upserts on `(kind, external_id)` so
//!   redelivery of the same logical event collapses into one row.
//! * `add_event_actors` dedups on `(event_id, user_id, role)`
//!   matching the Postgres composite PK.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;
use dp_domain::store::EventActorRow;
use dp_domain::{
    ActivityEvent, ActorRole, EventActor, EventKind, FetchCursor, FetchRun, FetchRunKind,
    Membership, Org, Repo, ResourceKind, Store, StoreError, Team, User, WebhookDelivery, Window,
};
use uuid::Uuid;

#[derive(Default)]
pub(crate) struct FakeStore {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    users: HashMap<Uuid, User>,
    users_by_gid: HashMap<i64, Uuid>,
    orgs: HashMap<Uuid, Org>,
    orgs_by_gid: HashMap<i64, Uuid>,
    repos: HashMap<Uuid, Repo>,
    repos_by_org_gid: HashMap<(Uuid, i64), Uuid>,
    teams: Vec<Team>,
    memberships: HashMap<(Uuid, Uuid), Membership>,
    events: HashMap<Uuid, ActivityEvent>,
    events_by_ext: HashMap<(EventKind, String), Uuid>,
    actors: Vec<EventActor>,
    inbox: Vec<WebhookDelivery>,
    fetch_runs: HashMap<Uuid, FetchRun>,
    cursors: HashMap<(Uuid, Option<Uuid>, ResourceKind), FetchCursor>,
}

impl FakeStore {
    pub fn new() -> Self {
        Self::default()
    }

    // ---------- test-only inspection helpers ----------------------

    pub fn enqueue_webhook_for_test(&self, d: WebhookDelivery) {
        self.inner.lock().unwrap().inbox.push(d);
    }

    pub fn events_count(&self) -> usize {
        self.inner.lock().unwrap().events.len()
    }

    pub fn only_event(&self) -> ActivityEvent {
        let g = self.inner.lock().unwrap();
        assert_eq!(g.events.len(), 1, "expected exactly one event");
        g.events.values().next().cloned().unwrap()
    }

    pub fn find_event_by_kind(&self, kind: EventKind) -> Option<ActivityEvent> {
        self.inner
            .lock()
            .unwrap()
            .events
            .values()
            .find(|e| e.kind == kind)
            .cloned()
    }

    /// All `(login, role)` pairs for a given event.
    pub fn actors_for(&self, event_id: Uuid) -> Vec<(String, ActorRole)> {
        let g = self.inner.lock().unwrap();
        g.actors
            .iter()
            .filter(|a| a.event_id == event_id)
            .map(|a| {
                let login = g
                    .users
                    .get(&a.user_id)
                    .map(|u| u.login.clone())
                    .unwrap_or_else(|| "<missing>".into());
                (login, a.role)
            })
            .collect()
    }

    pub fn roles_for_login(&self, event_id: Uuid, login: &str) -> Vec<ActorRole> {
        self.actors_for(event_id)
            .into_iter()
            .filter(|(l, _)| l == login)
            .map(|(_, r)| r)
            .collect()
    }

    pub fn memberships_for_login(&self, login: &str) -> Vec<Membership> {
        let g = self.inner.lock().unwrap();
        let Some(uid) = g.users.values().find(|u| u.login == login).map(|u| u.id) else {
            return Vec::new();
        };
        g.memberships
            .values()
            .filter(|m| m.user_id == uid)
            .cloned()
            .collect()
    }

    pub fn teams(&self) -> Vec<Team> {
        self.inner.lock().unwrap().teams.clone()
    }

    pub fn fetch_runs(&self) -> Vec<FetchRun> {
        self.inner.lock().unwrap().fetch_runs.values().cloned().collect()
    }

    pub fn pending_count(&self) -> usize {
        self.inner
            .lock()
            .unwrap()
            .inbox
            .iter()
            .filter(|d| d.processed_at.is_none())
            .count()
    }

    pub fn get_cursor_sync(
        &self,
        org_id: Uuid,
        repo_id: Option<Uuid>,
        kind: ResourceKind,
    ) -> Option<FetchCursor> {
        self.inner
            .lock()
            .unwrap()
            .cursors
            .get(&(org_id, repo_id, kind))
            .cloned()
    }

    pub fn last_error_for_pending(&self) -> Option<String> {
        self.inner
            .lock()
            .unwrap()
            .inbox
            .iter()
            .find(|d| d.processed_at.is_none())
            .and_then(|d| d.error.clone())
    }
}

#[async_trait]
impl Store for FakeStore {
    // ---- users ---------------------------------------------------
    async fn upsert_user(&self, user: &User) -> Result<User, StoreError> {
        let mut g = self.inner.lock().unwrap();
        if let Some(&existing_id) = g.users_by_gid.get(&user.github_id) {
            // Update in place but preserve the assigned id.
            let row = g.users.get_mut(&existing_id).unwrap();
            row.login = user.login.clone();
            if user.email.is_some() {
                row.email = user.email.clone();
            }
            if user.name.is_some() {
                row.name = user.name.clone();
            }
            row.deleted_at = user.deleted_at;
            Ok(row.clone())
        } else {
            let mut row = user.clone();
            if row.id.is_nil() {
                row.id = Uuid::new_v4();
            }
            g.users_by_gid.insert(row.github_id, row.id);
            g.users.insert(row.id, row.clone());
            Ok(row)
        }
    }

    async fn get_user(&self, _: Uuid) -> Result<User, StoreError> {
        unimplemented!("FakeStore::get_user — worker path does not need it")
    }
    async fn get_user_by_github_id(&self, _: i64) -> Result<User, StoreError> {
        unimplemented!()
    }
    async fn list_users(&self) -> Result<Vec<User>, StoreError> {
        unimplemented!()
    }
    async fn pseudonymise_user(&self, _: Uuid) -> Result<(), StoreError> {
        unimplemented!()
    }

    // ---- orgs / teams / repos -----------------------------------
    async fn upsert_org(&self, org: &Org) -> Result<Org, StoreError> {
        let mut g = self.inner.lock().unwrap();
        if let Some(&id) = g.orgs_by_gid.get(&org.github_id) {
            let row = g.orgs.get_mut(&id).unwrap();
            row.login = org.login.clone();
            if org.name.is_some() {
                row.name = org.name.clone();
            }
            Ok(row.clone())
        } else {
            let mut row = org.clone();
            if row.id.is_nil() {
                row.id = Uuid::new_v4();
            }
            g.orgs_by_gid.insert(row.github_id, row.id);
            g.orgs.insert(row.id, row.clone());
            Ok(row)
        }
    }

    async fn upsert_team(&self, team: &Team) -> Result<Team, StoreError> {
        let mut g = self.inner.lock().unwrap();
        if let Some(t) = g
            .teams
            .iter_mut()
            .find(|t| t.org_id == team.org_id && t.github_id == team.github_id)
        {
            t.slug = team.slug.clone();
            t.name = team.name.clone();
            Ok(t.clone())
        } else {
            let mut row = team.clone();
            if row.id.is_nil() {
                row.id = Uuid::new_v4();
            }
            g.teams.push(row.clone());
            Ok(row)
        }
    }

    async fn upsert_repo(&self, repo: &Repo) -> Result<Repo, StoreError> {
        let mut g = self.inner.lock().unwrap();
        if let Some(&id) = g.repos_by_org_gid.get(&(repo.org_id, repo.github_id)) {
            let row = g.repos.get_mut(&id).unwrap();
            row.name = repo.name.clone();
            Ok(row.clone())
        } else {
            let mut row = repo.clone();
            if row.id.is_nil() {
                row.id = Uuid::new_v4();
            }
            g.repos_by_org_gid.insert((row.org_id, row.github_id), row.id);
            g.repos.insert(row.id, row.clone());
            Ok(row)
        }
    }

    async fn upsert_membership(&self, m: &Membership) -> Result<Membership, StoreError> {
        let mut g = self.inner.lock().unwrap();
        let key = (m.user_id, m.org_id);
        let entry = g.memberships.entry(key).or_insert_with(|| m.clone());
        // home_org preserved (matches pg semantics).
        if m.home_org.is_some() {
            entry.home_org = m.home_org;
        }
        entry.role = m.role.clone();
        Ok(entry.clone())
    }

    async fn list_memberships_for_user(&self, _: Uuid) -> Result<Vec<Membership>, StoreError> {
        unimplemented!()
    }
    async fn set_home_org(
        &self,
        _: Uuid,
        _: Uuid,
        _: Option<Uuid>,
    ) -> Result<(), StoreError> {
        unimplemented!()
    }

    // ---- events + actors ----------------------------------------
    async fn record_event(&self, event: &ActivityEvent) -> Result<ActivityEvent, StoreError> {
        let mut g = self.inner.lock().unwrap();
        let key = (event.kind, event.external_id.clone());
        if let Some(&id) = g.events_by_ext.get(&key) {
            // Idempotent upsert — keep the existing id (so actor
            // rows stay attached), refresh ts + payload.
            let row = g.events.get_mut(&id).unwrap();
            row.ts = event.ts;
            row.payload = event.payload.clone();
            return Ok(row.clone());
        }
        let mut row = event.clone();
        if row.id.is_nil() {
            row.id = Uuid::new_v4();
        }
        g.events_by_ext.insert(key, row.id);
        g.events.insert(row.id, row.clone());
        Ok(row)
    }

    async fn add_event_actors(&self, actors: &[EventActor]) -> Result<(), StoreError> {
        let mut g = self.inner.lock().unwrap();
        for a in actors {
            let dup = g
                .actors
                .iter()
                .any(|x| x.event_id == a.event_id && x.user_id == a.user_id && x.role == a.role);
            if !dup {
                g.actors.push(a.clone());
            }
        }
        Ok(())
    }

    async fn list_event_actor_rows_in_window(
        &self,
        _: &Window,
        _: &[Uuid],
        _: &[Uuid],
        _: &[Uuid],
        _: &[ActorRole],
    ) -> Result<Vec<EventActorRow>, StoreError> {
        unimplemented!()
    }

    // ---- cursors + run log --------------------------------------
    async fn get_cursor(
        &self,
        org_id: Uuid,
        repo_id: Option<Uuid>,
        resource_kind: ResourceKind,
    ) -> Result<FetchCursor, StoreError> {
        let g = self.inner.lock().unwrap();
        g.cursors
            .get(&(org_id, repo_id, resource_kind))
            .cloned()
            .ok_or(StoreError::NotFound {
                entity: "fetch_cursor",
                id: format!("({org_id},{repo_id:?},{resource_kind:?})"),
            })
    }
    async fn put_cursor(&self, c: &FetchCursor) -> Result<(), StoreError> {
        let mut g = self.inner.lock().unwrap();
        g.cursors
            .insert((c.org_id, c.repo_id, c.resource_kind), c.clone());
        Ok(())
    }

    async fn start_fetch_run(&self, kind: FetchRunKind) -> Result<Uuid, StoreError> {
        let mut g = self.inner.lock().unwrap();
        let id = Uuid::new_v4();
        g.fetch_runs.insert(
            id,
            FetchRun {
                id,
                kind,
                started: Utc::now(),
                finished: None,
                items: 0,
                errors: 0,
                partial: false,
            },
        );
        Ok(id)
    }

    async fn finish_fetch_run(
        &self,
        id: Uuid,
        items: i64,
        errors: i64,
        partial: bool,
    ) -> Result<(), StoreError> {
        let mut g = self.inner.lock().unwrap();
        let row = g.fetch_runs.get_mut(&id).ok_or(StoreError::NotFound {
            entity: "fetch_run",
            id: id.to_string(),
        })?;
        row.finished = Some(Utc::now());
        row.items = items;
        row.errors = errors;
        row.partial = partial;
        Ok(())
    }

    async fn list_recent_fetch_runs(&self, _: i64) -> Result<Vec<FetchRun>, StoreError> {
        unimplemented!()
    }

    async fn data_as_of(&self) -> Result<dp_domain::freshness::DataAsOf, StoreError> {
        // The webhook-worker tests don't exercise data_as_of; the
        // dp-store-pg integration suite is what proves the Postgres
        // body. Keep the fake honest with a Default so a caller who
        // *does* invoke this in future doesn't get a panic.
        Ok(dp_domain::freshness::DataAsOf::default())
    }

    // ---- webhook inbox ------------------------------------------
    async fn enqueue_webhook(&self, d: &WebhookDelivery) -> Result<(), StoreError> {
        let mut g = self.inner.lock().unwrap();
        if g.inbox.iter().any(|x| x.delivery_id == d.delivery_id) {
            return Err(StoreError::Conflict("duplicate delivery_id".into()));
        }
        g.inbox.push(d.clone());
        Ok(())
    }

    async fn claim_webhooks(&self, max: i64) -> Result<Vec<WebhookDelivery>, StoreError> {
        // No actual locking — the fake is single-process. Just
        // return up to `max` unprocessed rows in FIFO order.
        let g = self.inner.lock().unwrap();
        Ok(g.inbox
            .iter()
            .filter(|d| d.processed_at.is_none())
            .take(max.max(0) as usize)
            .cloned()
            .collect())
    }

    async fn mark_webhook_processed(&self, id: Uuid) -> Result<(), StoreError> {
        let mut g = self.inner.lock().unwrap();
        let row = g
            .inbox
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or(StoreError::NotFound {
                entity: "webhook",
                id: id.to_string(),
            })?;
        row.processed_at = Some(Utc::now());
        row.error = None;
        Ok(())
    }

    async fn mark_webhook_failed(&self, id: Uuid, error: &str) -> Result<(), StoreError> {
        let mut g = self.inner.lock().unwrap();
        let row = g
            .inbox
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or(StoreError::NotFound {
                entity: "webhook",
                id: id.to_string(),
            })?;
        row.error = Some(error.into());
        Ok(())
    }
}
