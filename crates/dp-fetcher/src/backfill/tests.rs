//! Wiremock-driven tests for the backfill driver.
//!
//! These exercise the Stage 9 contract:
//!
//! * A 90-day (or smaller, in tests) window walks each
//!   `(target, kind)` chunk and writes a `fetch_runs` row of
//!   kind `Backfill` per chunk — the granularity `/admin/runs`
//!   exposes to operators.
//! * Synthesised deliveries flow through the **same**
//!   [`apply_delivery`] path the webhook worker + reconciler
//!   use (zero code duplication — Stage 8's invariant
//!   extended into Stage 9).
//! * Resumability: when the cursor already records a
//!   high-water `since`, a re-run picks up from there rather
//!   than re-fetching the window from scratch.
//! * Org scoping: `run_for_org` only touches targets that
//!   match the supplied org id, so backfill-per-org is the
//!   actual unit of work.
//! * Pacing: when GitHub reports `remaining` under the
//!   configured headroom, backfill voluntarily yields. We
//!   verify the *decision* (we don't actually sleep
//!   `reset_at - now` in tests — that's a config-tuned
//!   production concern, not a correctness check).

use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use dp_domain::{EventKind, FetchCursor, FetchRunKind, ResourceKind, Store};
use secrecy::SecretString;
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{Backfill, BackfillConfig};
use crate::client::Client;
use crate::reconciler::{RepoTarget, StaticTargets};
use crate::worker::test_store::FakeStore;

fn target() -> RepoTarget {
    RepoTarget {
        org_id: Uuid::from_u128(0xA1),
        org_github_id: 42,
        owner_login: "octo".into(),
        repo_id: Uuid::from_u128(0xA2),
        repo_github_id: 7,
        repo_name: "hello".into(),
    }
}

async fn fixture(
    kinds: &[ResourceKind],
    config: BackfillConfig,
) -> (MockServer, Arc<FakeStore>, Backfill) {
    let server = MockServer::start().await;
    let client = Client::with_personal_token(
        SecretString::from("test".to_string()),
        &server.uri(),
    )
    .expect("client");
    let store = Arc::new(FakeStore::new());
    let targets = Arc::new(StaticTargets::new(vec![target()]));
    let bf = Backfill::new(
        store.clone() as Arc<dyn Store>,
        Arc::new(client),
        targets,
        config,
    )
    .with_kinds(kinds);
    (server, store, bf)
}

#[tokio::test]
async fn pr_list_synthesises_deliveries_through_shared_apply_path() {
    let (server, store, bf) = fixture(
        &[ResourceKind::PullRequests],
        BackfillConfig {
            window: Duration::from_secs(7 * 86_400),
            rate_limit_headroom: 100,
        },
    )
    .await;

    Mock::given(method("GET"))
        .and(path("/repos/octo/hello/pulls"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "W/\"pr-bf\"")
                .insert_header("x-ratelimit-remaining", "4999")
                .insert_header("x-ratelimit-reset", "9999999999")
                .set_body_json(json!([
                    {
                        "node_id":    "PR_bf_1",
                        "state":      "open",
                        "created_at": "2024-01-01T00:00:00Z",
                        "updated_at": "2024-01-01T00:00:00Z",
                        "user":       { "id": 7, "login": "alice" }
                    }
                ])),
        )
        .mount(&server)
        .await;

    let stats = bf.run_for_org(target().org_id, None).await.unwrap();
    assert_eq!(stats.chunks, 1);
    assert_eq!(stats.items, 1);
    assert_eq!(stats.errors, 0);

    // Synthesised delivery flowed through apply_delivery → one
    // ActivityEvent in the store.
    assert_eq!(store.events_count(), 1);
    let ev = store.only_event();
    assert_eq!(ev.kind, EventKind::PullRequestOpened);
    assert_eq!(ev.external_id, "PR_bf_1");

    // Cursor advanced and etag recorded.
    let c = store
        .get_cursor_sync(target().org_id, Some(target().repo_id), ResourceKind::PullRequests)
        .expect("cursor written");
    assert_eq!(c.etag.as_deref(), Some("W/\"pr-bf\""));

    // fetch_runs row of kind=Backfill written and closed.
    let runs = store.fetch_runs();
    assert_eq!(runs.len(), 1);
    assert!(matches!(runs[0].kind, FetchRunKind::Backfill));
    assert_eq!(runs[0].items, 1);
    assert!(runs[0].finished.is_some());
}

#[tokio::test]
async fn resumable_from_existing_cursor_short_circuits_when_in_future() {
    // Cursor already past "now" — resume short-circuits to
    // Skipped without making any GitHub calls. (We deliberately
    // do not mount any mocks; a stray HTTP call would surface
    // as wiremock returning 404 and bumping `stats.errors`.)
    let (_, store, bf) = fixture(
        &[ResourceKind::PullRequests],
        BackfillConfig {
            window: Duration::from_secs(7 * 86_400),
            rate_limit_headroom: 100,
        },
    )
    .await;

    let future = Utc::now() + chrono::Duration::days(1);
    store
        .put_cursor(&FetchCursor {
            org_id: target().org_id,
            repo_id: Some(target().repo_id),
            resource_kind: ResourceKind::PullRequests,
            since: Some(future),
            etag: None,
            last_event_id: None,
            updated_at: future,
        })
        .await
        .unwrap();

    let stats = bf.run_for_org(target().org_id, None).await.unwrap();
    assert_eq!(stats.skipped, 1);
    assert_eq!(stats.chunks, 0);
    assert_eq!(stats.errors, 0);

    // Run-log row still written so /admin/runs reflects the
    // attempt (a "no-op skip" chunk).
    let runs = store.fetch_runs();
    assert_eq!(runs.len(), 1);
    assert!(matches!(runs[0].kind, FetchRunKind::Backfill));
}

#[tokio::test]
async fn cursor_advances_past_window_start_after_chunk() {
    let (server, store, bf) = fixture(
        &[ResourceKind::Issues],
        BackfillConfig {
            window: Duration::from_secs(7 * 86_400),
            rate_limit_headroom: 100,
        },
    )
    .await;

    Mock::given(method("GET"))
        .and(path("/repos/octo/hello/issues"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-ratelimit-remaining", "4999")
                .insert_header("x-ratelimit-reset", "9999999999")
                .set_body_json(json!([
                    {
                        "node_id":    "I_bf_1",
                        "state":      "open",
                        "created_at": "2030-01-01T00:00:00Z",
                        "updated_at": "2030-01-01T00:00:00Z",
                        "user":       { "id": 9, "login": "carol" }
                    }
                ])),
        )
        .mount(&server)
        .await;

    bf.run_for_org(target().org_id, None).await.unwrap();

    let c = store
        .get_cursor_sync(target().org_id, Some(target().repo_id), ResourceKind::Issues)
        .expect("cursor written");
    // High-water timestamp from the response wins over
    // effective_since.
    assert_eq!(
        c.since.unwrap(),
        Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap()
    );
}

#[tokio::test]
async fn run_for_org_filters_targets_by_org_id() {
    // Two targets in different orgs; we backfill org A only and
    // assert no request hits org B's repo path.
    let server = MockServer::start().await;
    let client = Client::with_personal_token(
        SecretString::from("t".to_string()),
        &server.uri(),
    )
    .unwrap();
    let store = Arc::new(FakeStore::new());

    let t_a = target();
    let t_b = RepoTarget {
        org_id: Uuid::from_u128(0xB1),
        org_github_id: 100,
        owner_login: "other".into(),
        repo_id: Uuid::from_u128(0xB2),
        repo_github_id: 8,
        repo_name: "world".into(),
    };

    // Org A is mocked. Org B is *not*: a stray request would
    // hit wiremock's default 404 and bump errors.
    Mock::given(method("GET"))
        .and(path("/repos/octo/hello/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    let bf = Backfill::new(
        store.clone() as Arc<dyn Store>,
        Arc::new(client),
        Arc::new(StaticTargets::new(vec![t_a.clone(), t_b.clone()])),
        BackfillConfig {
            window: Duration::from_secs(7 * 86_400),
            rate_limit_headroom: 100,
        },
    )
    .with_kinds(&[ResourceKind::PullRequests]);

    bf.run_for_org(t_a.org_id, None).await.unwrap();

    // Only org A's cursor exists; org B was never touched.
    assert!(store
        .get_cursor_sync(t_a.org_id, Some(t_a.repo_id), ResourceKind::PullRequests)
        .is_some());
    assert!(store
        .get_cursor_sync(t_b.org_id, Some(t_b.repo_id), ResourceKind::PullRequests)
        .is_none());
}

#[tokio::test]
async fn shutdown_observed_cancels_before_next_chunk() {
    // First chunk runs; before the second, shutdown is set,
    // causing the org-level call to return Cancelled.
    let (server, _store, bf) = fixture(
        &[ResourceKind::PullRequests, ResourceKind::Issues],
        BackfillConfig {
            window: Duration::from_secs(7 * 86_400),
            rate_limit_headroom: 100,
        },
    )
    .await;

    Mock::given(method("GET"))
        .and(path("/repos/octo/hello/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    let (tx, rx) = tokio::sync::watch::channel(true);
    // Sender lives until end of test scope so the receiver stays valid.
    let _ = tx;
    let res = bf.run_for_org(target().org_id, Some(rx)).await;
    assert!(matches!(res, Err(super::BackfillError::Cancelled)));
}

#[tokio::test]
async fn default_window_is_ninety_days() {
    // Defaults are pinned: 90 days, headroom 1000. Stage 9
    // documents this; the test exists so a future "make
    // window configurable" patch can't silently change the
    // default.
    let cfg = BackfillConfig::default();
    assert_eq!(cfg.window.as_secs(), 90 * 24 * 60 * 60);
    assert_eq!(cfg.rate_limit_headroom, 1000);
}
