//! axum `Router` fragment for `POST /webhooks/github`.
//!
//! The router is built standalone here and merged into the main
//! app by `dp-server` via `Router::merge`. It carries its own
//! `State<Arc<WebhookState>>` because the route is deliberately
//! **not** under the `with_principal` layer (TODO §4) — auth on
//! this route is the HMAC we verify ourselves, not a session
//! cookie or API token.
//!
//! ## Response table
//!
//! | Condition                                  | Status |
//! |--------------------------------------------|--------|
//! | Happy path (enqueued)                      | 200    |
//! | Replay (`delivery_id` already in inbox)    | 200    |
//! | Missing `X-GitHub-Delivery`                | 400    |
//! | Missing `X-GitHub-Event`                   | 400    |
//! | Body is not valid JSON                     | 400    |
//! | Missing / malformed / mismatched signature | 401    |
//! | Store error other than `Conflict`          | 500    |
//!
//! 200-on-replay matters: GitHub retries on any non-2xx, so a
//! second 200 ends the retry loop cleanly. The actual processing
//! is the worker's job (Stage 5) and the unique constraint on
//! `webhook_inbox.delivery_id` keeps the inbox a set, not a bag.

use std::sync::Arc;
use std::time::Instant;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Router,
};
use chrono::Utc;
use dp_domain::{Store, StoreError, WebhookDelivery};
use uuid::Uuid;

use super::metrics::WebhookMetrics;
use super::verify::{verify_signature, SignatureError, WebhookSecretSource};

/// State the handler reads from. Cheap to clone (every field is
/// an `Arc`-equivalent).
///
/// Held inside an `Arc` by the router so axum can clone it per
/// request without cloning the inner trait objects.
pub struct WebhookState {
    /// Persistence — `enqueue_webhook` is the only method called
    /// on the receive path.
    pub store: Arc<dyn Store>,
    /// Rotation-aware secret source. Looked up once per request.
    pub secrets: Arc<dyn WebhookSecretSource>,
    /// Receipt-to-200 histogram.
    pub metrics: WebhookMetrics,
}

impl WebhookState {
    /// Convenience constructor — the bin layer typically has all
    /// three pieces in hand at the same call site.
    pub fn new(
        store: Arc<dyn Store>,
        secrets: Arc<dyn WebhookSecretSource>,
        metrics: WebhookMetrics,
    ) -> Self {
        Self { store, secrets, metrics }
    }
}

/// Build the router fragment. Mount with `Router::merge` from
/// `dp-server` so it shares no global state with the rest of the
/// app — the receiver is intentionally an island.
pub fn router(state: Arc<WebhookState>) -> Router {
    Router::new()
        .route("/webhooks/github", post(receive))
        .with_state(state)
}

/// The receive handler. `body: Bytes` must stay last per axum's
/// extractor rules — it consumes the request body.
async fn receive(
    State(state): State<Arc<WebhookState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let started = Instant::now();

    // Delivery id is the dedup key. Without it we'd never know
    // which row to upsert against, so absence is 400, not 401.
    let delivery_id = match header_str(&headers, "x-github-delivery") {
        Some(v) => v.to_string(),
        None => {
            tracing::warn!(target: "dp_fetcher::webhook", "missing X-GitHub-Delivery");
            return (StatusCode::BAD_REQUEST, "missing X-GitHub-Delivery").into_response();
        }
    };

    // Set the stable tracing field for the rest of this request.
    let span = tracing::info_span!(
        target: "dp_fetcher::webhook",
        "webhook.receive",
        webhook.delivery_id = %delivery_id,
    );
    let _enter = span.enter();

    let event = match header_str(&headers, "x-github-event") {
        Some(v) => v.to_string(),
        None => {
            tracing::warn!("missing X-GitHub-Event");
            return (StatusCode::BAD_REQUEST, "missing X-GitHub-Event").into_response();
        }
    };

    // HMAC verification. Fail-closed on every error category;
    // the table above maps them all to 401.
    let sig_header = header_str(&headers, "x-hub-signature-256");
    let secrets = state.secrets.current_secrets();
    if let Err(e) = verify_signature(sig_header, &body, &secrets) {
        // `Display` impl on SignatureError is operator-friendly;
        // we keep the variant in a `kind` field for log filtering.
        tracing::warn!(error = %e, kind = signature_error_kind(&e), "signature rejected");
        return (StatusCode::UNAUTHORIZED, "invalid signature").into_response();
    }

    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "body is not valid JSON");
            return (StatusCode::BAD_REQUEST, "body is not valid JSON").into_response();
        }
    };

    let delivery = WebhookDelivery {
        id: Uuid::new_v4(),
        delivery_id: delivery_id.clone(),
        event,
        payload,
        received_at: Utc::now(),
        processed_at: None,
        error: None,
    };

    let status = match state.store.enqueue_webhook(&delivery).await {
        Ok(()) => {
            tracing::info!(bytes = body.len(), "enqueued");
            StatusCode::OK
        }
        // Replay: the inbox already has this `delivery_id`. Per
        // §0.1 the unique constraint is the dedup point and a
        // replay is success at this boundary — the worker (or a
        // prior worker run) is responsible for the actual work.
        Err(StoreError::Conflict(_)) => {
            tracing::info!("replay deduped at inbox");
            StatusCode::OK
        }
        Err(e) => {
            tracing::error!(error = %e, "enqueue failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "enqueue failed").into_response();
        }
    };

    let elapsed = started.elapsed().as_secs_f64();
    state.metrics.receipt_seconds.observe(elapsed);
    tracing::debug!(elapsed_s = elapsed, "served");
    status.into_response()
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn signature_error_kind(e: &SignatureError) -> &'static str {
    match e {
        SignatureError::Missing => "missing",
        SignatureError::Malformed => "malformed",
        SignatureError::Mismatch => "mismatch",
        SignatureError::NoSecrets => "no_secrets",
    }
}

#[cfg(test)]
mod tests {
    //! End-to-end coverage of the route using `tower::ServiceExt::oneshot`.
    //!
    //! We use an in-memory fake Store (only `enqueue_webhook` is
    //! exercised on the receive path; every other method panics so
    //! a future regression that calls them surfaces immediately)
    //! and assert the response status table from the module doc.
    use super::*;
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::Request;
    use dp_domain::{
        ActivityEvent, ActorRole, EventActor, FetchCursor, FetchRun, FetchRunKind, Membership,
        Org, Repo, ResourceKind, Team, User, Window,
    };
    use dp_domain::store::EventActorRow;
    use hmac::{Hmac, Mac};
    use secrecy::SecretString;
    use sha2::Sha256;
    use std::sync::Mutex;
    use tower::ServiceExt;

    /// Minimal Store stub — only the receive path's calls are
    /// implemented. Every other method panics with a message
    /// pointing back at this test so a future refactor that
    /// reaches for them is loud.
    #[derive(Default)]
    struct FakeStore {
        enqueued: Mutex<Vec<WebhookDelivery>>,
        /// If set, the next `enqueue_webhook` returns this error
        /// instead of recording. Resets to `None` after firing.
        next_error: Mutex<Option<StoreError>>,
    }

    impl FakeStore {
        fn enqueue_count(&self) -> usize {
            self.enqueued.lock().unwrap().len()
        }
        fn set_next_error(&self, e: StoreError) {
            *self.next_error.lock().unwrap() = Some(e);
        }
    }

    #[async_trait]
    impl Store for FakeStore {
        async fn enqueue_webhook(&self, delivery: &WebhookDelivery) -> Result<(), StoreError> {
            if let Some(e) = self.next_error.lock().unwrap().take() {
                return Err(e);
            }
            self.enqueued.lock().unwrap().push(delivery.clone());
            Ok(())
        }

        // ---- the rest panic; the receive path does not need them.
        async fn upsert_user(&self, _: &User) -> Result<User, StoreError> { unimplemented!() }
        async fn get_user(&self, _: Uuid) -> Result<User, StoreError> { unimplemented!() }
        async fn get_user_by_github_id(&self, _: i64) -> Result<User, StoreError> { unimplemented!() }
        async fn list_users(&self) -> Result<Vec<User>, StoreError> { unimplemented!() }
        async fn pseudonymise_user(&self, _: Uuid) -> Result<(), StoreError> { unimplemented!() }
        async fn upsert_org(&self, _: &Org) -> Result<Org, StoreError> { unimplemented!() }
        async fn upsert_team(&self, _: &Team) -> Result<Team, StoreError> { unimplemented!() }
        async fn upsert_repo(&self, _: &Repo) -> Result<Repo, StoreError> { unimplemented!() }
        async fn upsert_membership(&self, _: &Membership) -> Result<Membership, StoreError> { unimplemented!() }
        async fn list_memberships_for_user(&self, _: Uuid) -> Result<Vec<Membership>, StoreError> { unimplemented!() }
        async fn set_home_org(&self, _: Uuid, _: Uuid, _: Option<Uuid>) -> Result<(), StoreError> { unimplemented!() }
        async fn record_event(&self, _: &ActivityEvent) -> Result<ActivityEvent, StoreError> { unimplemented!() }
        async fn add_event_actors(&self, _: &[EventActor]) -> Result<(), StoreError> { unimplemented!() }
        async fn list_event_actor_rows_in_window(
            &self,
            _: &Window,
            _: &[Uuid],
            _: &[Uuid],
            _: &[Uuid],
            _: &[ActorRole],
        ) -> Result<Vec<EventActorRow>, StoreError> { unimplemented!() }
        async fn get_cursor(&self, _: Uuid, _: Option<Uuid>, _: ResourceKind) -> Result<FetchCursor, StoreError> { unimplemented!() }
        async fn put_cursor(&self, _: &FetchCursor) -> Result<(), StoreError> { unimplemented!() }
        async fn start_fetch_run(&self, _: FetchRunKind) -> Result<Uuid, StoreError> { unimplemented!() }
        async fn finish_fetch_run(&self, _: Uuid, _: i64, _: i64, _: bool) -> Result<(), StoreError> { unimplemented!() }
        async fn list_recent_fetch_runs(&self, _: i64) -> Result<Vec<FetchRun>, StoreError> { unimplemented!() }
        async fn claim_webhooks(&self, _: i64) -> Result<Vec<WebhookDelivery>, StoreError> { unimplemented!() }
        async fn mark_webhook_processed(&self, _: Uuid) -> Result<(), StoreError> { unimplemented!() }
        async fn mark_webhook_failed(&self, _: Uuid, _: &str) -> Result<(), StoreError> { unimplemented!() }
    }

    const SECRET: &str = "It's a Secret to Everybody";

    fn sign(body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(SECRET.as_bytes()).unwrap();
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    fn build(store: Arc<FakeStore>) -> (Router, Arc<FakeStore>) {
        use crate::webhook::verify::StaticSecrets;
        let secrets = Arc::new(StaticSecrets::single(SecretString::from(SECRET.to_string())));
        let state = Arc::new(WebhookState::new(
            store.clone() as Arc<dyn Store>,
            secrets,
            WebhookMetrics::for_test(),
        ));
        (router(state), store)
    }

    fn req(body: &[u8], delivery_id: &str, event: &str, signature: Option<String>) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/webhooks/github")
            .header("x-github-delivery", delivery_id)
            .header("x-github-event", event)
            .header("content-type", "application/json");
        if let Some(sig) = signature {
            builder = builder.header("x-hub-signature-256", sig);
        }
        builder.body(Body::from(body.to_vec())).unwrap()
    }

    #[tokio::test]
    async fn happy_path_enqueues_and_returns_200() {
        let store = Arc::new(FakeStore::default());
        let (app, store) = build(store);
        let body = br#"{"action":"opened"}"#;
        let resp = app
            .oneshot(req(body, "d-1", "pull_request", Some(sign(body))))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(store.enqueue_count(), 1);
        let q = store.enqueued.lock().unwrap();
        assert_eq!(q[0].delivery_id, "d-1");
        assert_eq!(q[0].event, "pull_request");
    }

    #[tokio::test]
    async fn replay_returns_200_without_double_enqueue() {
        let store = Arc::new(FakeStore::default());
        // Second insert would be a unique-key violation in PG —
        // the store surface for that is `Conflict`.
        store.set_next_error(StoreError::Conflict("dup delivery_id".into()));
        let (app, _store) = build(store);
        let body = br#"{"action":"opened"}"#;
        let resp = app
            .oneshot(req(body, "d-1", "pull_request", Some(sign(body))))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn missing_signature_is_401_and_does_not_enqueue() {
        let store = Arc::new(FakeStore::default());
        let (app, store) = build(store);
        let body = br#"{"action":"opened"}"#;
        let resp = app
            .oneshot(req(body, "d-2", "pull_request", None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(store.enqueue_count(), 0);
    }

    #[tokio::test]
    async fn wrong_signature_is_401() {
        let store = Arc::new(FakeStore::default());
        let (app, store) = build(store);
        let body = br#"{"action":"opened"}"#;
        // Sign a *different* body so the HMAC fails to validate
        // against the body we actually send.
        let bad = {
            let mut mac = Hmac::<Sha256>::new_from_slice(b"wrong-secret").unwrap();
            mac.update(body);
            format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
        };
        let resp = app
            .oneshot(req(body, "d-3", "pull_request", Some(bad)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(store.enqueue_count(), 0);
    }

    #[tokio::test]
    async fn missing_delivery_header_is_400() {
        let store = Arc::new(FakeStore::default());
        let (app, store) = build(store);
        let body = br#"{}"#;
        let req = Request::builder()
            .method("POST")
            .uri("/webhooks/github")
            .header("x-github-event", "pull_request")
            .header("x-hub-signature-256", sign(body))
            .body(Body::from(body.to_vec()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert_eq!(store.enqueue_count(), 0);
    }

    #[tokio::test]
    async fn missing_event_header_is_400() {
        let store = Arc::new(FakeStore::default());
        let (app, _store) = build(store);
        let body = br#"{}"#;
        let req = Request::builder()
            .method("POST")
            .uri("/webhooks/github")
            .header("x-github-delivery", "d-4")
            .header("x-hub-signature-256", sign(body))
            .body(Body::from(body.to_vec()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn non_json_body_is_400() {
        let store = Arc::new(FakeStore::default());
        let (app, _store) = build(store);
        let body = b"this is not json";
        let resp = app
            .oneshot(req(body, "d-5", "pull_request", Some(sign(body))))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn store_failure_other_than_conflict_is_500() {
        let store = Arc::new(FakeStore::default());
        store.set_next_error(StoreError::Backend("db is on fire".into()));
        let (app, _store) = build(store);
        let body = br#"{}"#;
        let resp = app
            .oneshot(req(body, "d-6", "pull_request", Some(sign(body))))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
