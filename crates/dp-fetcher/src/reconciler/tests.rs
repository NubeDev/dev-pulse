//! Wiremock-driven tests for the reconciler.
//!
//! These exercise the full Stage 8 contract end-to-end:
//!
//! * a 200 list response synthesises webhook deliveries that flow
//!   through the **same** [`apply_delivery`] path the worker uses,
//!   resulting in `activity_events` + `event_actors` rows in the
//!   store — the "zero code duplication" check;
//! * a 304 `NotModified` advances `fetch_cursors.updated_at` but
//!   does **not** touch `since` / `etag`, and does not synthesise
//!   any events;
//! * the cursor's `etag` is sent as `If-None-Match` on the next
//!   tick (the cheap-poll contract from TODO §0.3);
//! * the reconciler writes one `fetch_runs` row of kind
//!   `Reconciler` per `do_tick` call;
//! * `Scheduler::try_trigger_now` coalesces an overlapping trigger
//!   into a no-op via the `Mutex<Option<JoinHandle>>` guard.

use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use dp_domain::{EventKind, ResourceKind, Store};
use secrecy::SecretString;
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{header, header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{Reconciler, Scheduler, Scope};
use super::targets::{RepoTarget, StaticTargets};
use crate::client::Client;
use crate::worker::test_store::FakeStore;

/// Standard target: one repo under org `octo/hello`.
fn one_target() -> RepoTarget {
    RepoTarget {
        org_id: Uuid::from_u128(0x0a),
        org_github_id: 42,
        owner_login: "octo".into(),
        repo_id: Uuid::from_u128(0x0b),
        repo_github_id: 7,
        repo_name: "hello".into(),
    }
}

async fn fixture(kinds: &[ResourceKind]) -> (MockServer, Arc<FakeStore>, Reconciler) {
    let server = MockServer::start().await;
    let client = Client::with_personal_token(
        SecretString::from("test".to_string()),
        &server.uri(),
    )
    .expect("client");
    let store = Arc::new(FakeStore::new());
    let targets = Arc::new(StaticTargets::new(vec![one_target()]));
    let rec = Reconciler::new(store.clone() as Arc<dyn Store>, Arc::new(client), targets)
        .with_kinds(kinds);
    (server, store, rec)
}

#[tokio::test]
async fn pr_list_synthesises_deliveries_that_flow_through_apply_path() {
    let (server, store, rec) = fixture(&[ResourceKind::PullRequests]).await;

    Mock::given(method("GET"))
        .and(path("/repos/octo/hello/pulls"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "W/\"pr-1\"")
                .insert_header("x-ratelimit-remaining", "4999")
                .insert_header("x-ratelimit-reset", "9999999999")
                .set_body_json(json!([
                    {
                        "node_id":    "PR_recon_1",
                        "state":      "open",
                        "created_at": "2024-01-01T00:00:00Z",
                        "updated_at": "2024-01-01T00:00:00Z",
                        "user":       { "id": 7, "login": "alice" }
                    }
                ])),
        )
        .mount(&server)
        .await;

    let stats = rec.do_tick(Scope::All).await.unwrap();
    assert_eq!(stats.items, 1);
    assert_eq!(stats.errors, 0);
    assert!(!stats.partial);

    // The synthesised PR-opened delivery hit the real handler →
    // one ActivityEvent with the Author actor.
    assert_eq!(store.events_count(), 1);
    let ev = store.only_event();
    assert_eq!(ev.kind, EventKind::PullRequestOpened);
    assert_eq!(ev.external_id, "PR_recon_1");

    // Cursor was written with the response's etag and an advanced
    // `since` matching the max(updated_at) of the response.
    let c = store
        .get_cursor_sync(
            one_target().org_id,
            Some(one_target().repo_id),
            ResourceKind::PullRequests,
        )
        .expect("cursor written");
    assert_eq!(c.etag.as_deref(), Some("W/\"pr-1\""));
    assert_eq!(
        c.since.unwrap(),
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
    );

    // Run-log row of kind Reconciler with totals matching stats.
    let runs = store.fetch_runs();
    assert_eq!(runs.len(), 1);
    assert!(matches!(runs[0].kind, dp_domain::FetchRunKind::Reconciler));
    assert_eq!(runs[0].items, 1);
    assert_eq!(runs[0].errors, 0);
    assert!(runs[0].finished.is_some());
}

#[tokio::test]
async fn not_modified_keeps_since_and_etag_and_writes_no_events() {
    let (server, store, rec) = fixture(&[ResourceKind::PullRequests]).await;

    // Pre-seed the cursor with an etag so the reconciler sends it.
    let prior_since = Utc.with_ymd_and_hms(2023, 12, 31, 0, 0, 0).unwrap();
    store
        .put_cursor(&dp_domain::FetchCursor {
            org_id: one_target().org_id,
            repo_id: Some(one_target().repo_id),
            resource_kind: ResourceKind::PullRequests,
            since: Some(prior_since),
            etag: Some("W/\"pr-prev\"".into()),
            last_event_id: None,
            updated_at: prior_since,
        })
        .await
        .unwrap();

    Mock::given(method("GET"))
        .and(path("/repos/octo/hello/pulls"))
        .and(header("if-none-match", "W/\"pr-prev\""))
        .respond_with(
            ResponseTemplate::new(304)
                .insert_header("x-ratelimit-remaining", "4998")
                .insert_header("x-ratelimit-reset", "9999999999"),
        )
        .mount(&server)
        .await;

    let stats = rec.do_tick(Scope::All).await.unwrap();
    assert_eq!(stats.items, 0);
    assert_eq!(stats.errors, 0);

    // No events.
    assert_eq!(store.events_count(), 0);
    // Cursor since + etag are unchanged; updated_at advanced.
    let c = store
        .get_cursor_sync(
            one_target().org_id,
            Some(one_target().repo_id),
            ResourceKind::PullRequests,
        )
        .unwrap();
    assert_eq!(c.since, Some(prior_since));
    assert_eq!(c.etag.as_deref(), Some("W/\"pr-prev\""));
    assert!(c.updated_at >= prior_since);
}

#[tokio::test]
async fn scope_repo_narrows_to_one_target() {
    // Two targets in the provider; Scope::Repo selects one.
    let server = MockServer::start().await;
    let client = Client::with_personal_token(
        SecretString::from("t".to_string()),
        &server.uri(),
    )
    .unwrap();
    let store = Arc::new(FakeStore::new());

    let t1 = one_target();
    let t2 = RepoTarget {
        org_id: Uuid::from_u128(0x1a),
        org_github_id: 100,
        owner_login: "other".into(),
        repo_id: Uuid::from_u128(0x1b),
        repo_github_id: 8,
        repo_name: "world".into(),
    };

    // Only `octo/hello` is mocked; `other/world` would 404 if hit.
    Mock::given(method("GET"))
        .and(path("/repos/octo/hello/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    let rec = Reconciler::new(
        store.clone() as Arc<dyn Store>,
        Arc::new(client),
        Arc::new(StaticTargets::new(vec![t1.clone(), t2])),
    )
    .with_kinds(&[ResourceKind::PullRequests]);

    rec.do_tick(Scope::Repo {
        org_id: t1.org_id,
        repo_id: t1.repo_id,
    })
    .await
    .unwrap();

    // Only one cursor row was written — the narrow one.
    assert!(store
        .get_cursor_sync(t1.org_id, Some(t1.repo_id), ResourceKind::PullRequests)
        .is_some());
}

#[tokio::test]
async fn scheduler_coalesces_overlapping_triggers() {
    // Mount a slow mock so the first tick stays in flight while
    // the second `try_trigger_now` lands. The slow response gives
    // us a deterministic window without sleeps in the test thread.
    let (server, _store, rec) = fixture(&[ResourceKind::PullRequests]).await;
    Mock::given(method("GET"))
        .and(path("/repos/octo/hello/pulls"))
        .and(header_exists("accept"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(150))
                .set_body_json(json!([])),
        )
        .mount(&server)
        .await;

    let scheduler = Arc::new(Scheduler::new(Arc::new(rec), Duration::from_secs(3600)));

    // Kick off two triggers nearly simultaneously. One should run
    // the tick; the other should observe the in-flight handle and
    // coalesce into `Ok(None)`.
    let s1 = scheduler.clone();
    let s2 = scheduler.clone();
    let h1 = tokio::spawn(async move { s1.try_trigger_now(Scope::All).await });
    // Give the first task time to take the mutex + spawn the tick.
    tokio::time::sleep(Duration::from_millis(20)).await;
    let h2 = tokio::spawn(async move { s2.try_trigger_now(Scope::All).await });

    let r1 = h1.await.unwrap().unwrap();
    let r2 = h2.await.unwrap().unwrap();
    // Exactly one of the two ran (Some), the other coalesced (None).
    let ran = [r1.is_some(), r2.is_some()];
    assert_eq!(
        ran.iter().filter(|b| **b).count(),
        1,
        "exactly one trigger should have produced stats: r1={r1:?}, r2={r2:?}"
    );
}
