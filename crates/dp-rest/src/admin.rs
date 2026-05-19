//! `POST /admin/refresh` — operator-triggered reconciler tick.
//!
//! Stage 8 of the dev-pulse ingestion layer (TODO §0.1, §Phase 2)
//! mandates this route trigger the *same* code path the periodic
//! scheduler and the CLI `fetch-now` use:
//! [`Scheduler::try_trigger_now`]. The mutex inside the scheduler
//! ensures an admin-triggered tick coalesces against an in-flight
//! scheduled tick — there is exactly one reconciler tick running
//! at any moment.
//!
//! ## Request shape
//!
//! Query parameters select [`Scope`]:
//!
//! ```text
//! POST /admin/refresh                    # Scope::All
//! POST /admin/refresh?org_id=<uuid>      # Scope::Org
//! POST /admin/refresh?org_id=…&repo_id=… # Scope::Repo
//! ```
//!
//! ## Response shape
//!
//! `200 OK` with a JSON body:
//!
//! ```json
//! { "ran": true,  "items": 12, "errors": 0, "partial": false }
//! { "ran": false }   // coalesced into an in-flight tick
//! ```
//!
//! Errors map to `500` with a plain body — the only thing that
//! escapes the scheduler is a store-level failure, which is
//! operator-actionable.

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::post,
    Router,
};
use dp_fetcher::reconciler::{Scheduler, Scope};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// State the admin router reads. Held inside an `Arc` so axum
/// can clone it per request without cloning the scheduler.
pub struct AdminState {
    /// The scheduler whose coalescing mutex this route shares.
    pub scheduler: Arc<Scheduler>,
}

impl AdminState {
    /// Convenience constructor.
    pub fn new(scheduler: Arc<Scheduler>) -> Self {
        Self { scheduler }
    }
}

/// Query parameters parsed off `POST /admin/refresh`.
#[derive(Debug, Deserialize)]
pub struct RefreshQuery {
    /// Narrow the tick to one org (and optionally one repo within).
    pub org_id: Option<Uuid>,
    /// Narrow the tick to one repo. Requires `org_id` to also be
    /// supplied — `?repo_id=…` alone is rejected as 400.
    pub repo_id: Option<Uuid>,
}

impl RefreshQuery {
    fn to_scope(&self) -> Result<Scope, (StatusCode, &'static str)> {
        match (self.org_id, self.repo_id) {
            (None, None) => Ok(Scope::All),
            (Some(o), None) => Ok(Scope::Org(o)),
            (Some(o), Some(r)) => Ok(Scope::Repo {
                org_id: o,
                repo_id: r,
            }),
            (None, Some(_)) => Err((
                StatusCode::BAD_REQUEST,
                "repo_id requires org_id to also be specified",
            )),
        }
    }
}

/// Response body for `POST /admin/refresh`. Variants are flattened
/// so a coalesce comes out as `{ "ran": false }` and a real tick
/// as `{ "ran": true, "items": …, "errors": …, "partial": … }`.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum RefreshResponse {
    /// A tick ran to completion.
    Ran {
        /// Always `true` on this variant.
        ran: bool,
        /// Total deliveries applied during the tick.
        items: i64,
        /// `(target, kind)` failures during the tick.
        errors: i64,
        /// Whether the tick partially succeeded.
        partial: bool,
    },
    /// The trigger coalesced into an in-flight tick.
    Coalesced {
        /// Always `false` on this variant.
        ran: bool,
    },
}

/// Build the admin router fragment. Mount with `Router::merge`
/// (typically under `with_principal` so only authenticated
/// operators can trigger it).
pub fn admin_router(state: Arc<AdminState>) -> Router {
    Router::new()
        .route("/admin/refresh", post(refresh))
        .with_state(state)
}

async fn refresh(
    State(state): State<Arc<AdminState>>,
    Query(q): Query<RefreshQuery>,
) -> Result<Json<RefreshResponse>, (StatusCode, &'static str)> {
    let scope = q.to_scope()?;
    let out = state
        .scheduler
        .try_trigger_now(scope)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "admin refresh failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "reconciler failed")
        })?;
    let body = match out {
        Some(stats) => RefreshResponse::Ran {
            ran: true,
            items: stats.items,
            errors: stats.errors,
            partial: stats.partial,
        },
        None => RefreshResponse::Coalesced { ran: false },
    };
    Ok(Json(body))
}

// Surface IntoResponse for the success-only response so callers
// can return it directly when they don't need the error tuple.
impl IntoResponse for RefreshResponse {
    fn into_response(self) -> axum::response::Response {
        Json(self).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::Request;
    use dp_fetcher::client::Client;
    use dp_fetcher::reconciler::{Reconciler, StaticTargets};
    use secrecy::SecretString;
    use std::time::Duration;
    use tower::ServiceExt;

    // A minimal Store fake that supports only what the
    // reconciler's bookkeeping path touches when no targets exist
    // (start/finish fetch_run, list_targets is empty so no cursor
    // / event calls fire). Keeps this test crate self-contained —
    // we don't need to reach into dp-fetcher's pub(crate) FakeStore.
    use chrono::Utc;
    use dp_domain::store::EventActorRow;
    use dp_domain::{
        ActivityEvent, ActorRole, EventActor, FetchCursor, FetchRun, FetchRunKind, Membership,
        Org, Repo, ResourceKind, Store, StoreError, Team, User, WebhookDelivery, Window,
    };
    use std::sync::Mutex;

    #[derive(Default)]
    struct TinyStore {
        runs: Mutex<Vec<FetchRun>>,
    }
    #[async_trait::async_trait]
    impl Store for TinyStore {
        async fn upsert_user(&self, u: &User) -> Result<User, StoreError> {
            Ok(u.clone())
        }
        async fn get_user(&self, _: Uuid) -> Result<User, StoreError> {
            unimplemented!()
        }
        async fn get_user_by_github_id(&self, _: i64) -> Result<User, StoreError> {
            unimplemented!()
        }
        async fn list_users(&self) -> Result<Vec<User>, StoreError> {
            Ok(vec![])
        }
        async fn pseudonymise_user(&self, _: Uuid) -> Result<(), StoreError> {
            Ok(())
        }
        async fn upsert_org(&self, o: &Org) -> Result<Org, StoreError> {
            Ok(o.clone())
        }
        async fn upsert_team(&self, t: &Team) -> Result<Team, StoreError> {
            Ok(t.clone())
        }
        async fn upsert_repo(&self, r: &Repo) -> Result<Repo, StoreError> {
            Ok(r.clone())
        }
        async fn upsert_membership(&self, m: &Membership) -> Result<Membership, StoreError> {
            Ok(m.clone())
        }
        async fn list_memberships_for_user(
            &self,
            _: Uuid,
        ) -> Result<Vec<Membership>, StoreError> {
            Ok(vec![])
        }
        async fn set_home_org(
            &self,
            _: Uuid,
            _: Uuid,
            _: Option<Uuid>,
        ) -> Result<(), StoreError> {
            Ok(())
        }
        async fn record_event(&self, e: &ActivityEvent) -> Result<ActivityEvent, StoreError> {
            Ok(e.clone())
        }
        async fn add_event_actors(&self, _: &[EventActor]) -> Result<(), StoreError> {
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
            Ok(vec![])
        }
        async fn get_cursor(
            &self,
            _: Uuid,
            _: Option<Uuid>,
            _: ResourceKind,
        ) -> Result<FetchCursor, StoreError> {
            Err(StoreError::NotFound {
                entity: "fetch_cursor",
                id: String::new(),
            })
        }
        async fn put_cursor(&self, _: &FetchCursor) -> Result<(), StoreError> {
            Ok(())
        }
        async fn start_fetch_run(&self, kind: FetchRunKind) -> Result<Uuid, StoreError> {
            let id = Uuid::new_v4();
            self.runs.lock().unwrap().push(FetchRun {
                id,
                kind,
                started: Utc::now(),
                finished: None,
                items: 0,
                errors: 0,
                partial: false,
            });
            Ok(id)
        }
        async fn finish_fetch_run(
            &self,
            id: Uuid,
            items: i64,
            errors: i64,
            partial: bool,
        ) -> Result<(), StoreError> {
            let mut runs = self.runs.lock().unwrap();
            if let Some(r) = runs.iter_mut().find(|r| r.id == id) {
                r.finished = Some(Utc::now());
                r.items = items;
                r.errors = errors;
                r.partial = partial;
            }
            Ok(())
        }
        async fn list_recent_fetch_runs(&self, _: i64) -> Result<Vec<FetchRun>, StoreError> {
            Ok(self.runs.lock().unwrap().clone())
        }
        async fn enqueue_webhook(&self, _: &WebhookDelivery) -> Result<(), StoreError> {
            Ok(())
        }
        async fn claim_webhooks(&self, _: i64) -> Result<Vec<WebhookDelivery>, StoreError> {
            Ok(vec![])
        }
        async fn mark_webhook_processed(&self, _: Uuid) -> Result<(), StoreError> {
            Ok(())
        }
        async fn mark_webhook_failed(&self, _: Uuid, _: &str) -> Result<(), StoreError> {
            Ok(())
        }
    }

    fn build_router() -> Router {
        let store: Arc<dyn Store> = Arc::new(TinyStore::default());
        let client = Client::with_personal_token(
            SecretString::from("t".to_string()),
            "http://127.0.0.1:1",
        )
        .unwrap();
        let targets = Arc::new(StaticTargets::new(Vec::new()));
        let rec = Reconciler::new(store, Arc::new(client), targets);
        let sched = Arc::new(Scheduler::new(
            Arc::new(rec),
            Duration::from_secs(3600),
        ));
        admin_router(Arc::new(AdminState::new(sched)))
    }

    #[tokio::test]
    async fn admin_refresh_returns_ran_with_zero_targets() {
        let app = build_router();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/refresh")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["ran"], true);
        assert_eq!(v["items"], 0);
        assert_eq!(v["errors"], 0);
    }

    #[tokio::test]
    async fn admin_refresh_rejects_repo_id_without_org_id() {
        let app = build_router();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/admin/refresh?repo_id={}", Uuid::new_v4()))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
