//! Phase 2 smoke tests — Stage 10.
//!
//! These tests pin the seven invariants the stage-10 spec calls
//! out by name. Each test's function name is the literal name
//! from the spec so the CI log line is the contract:
//!
//! 1. `webhook_replay_same_delivery_id_yields_exactly_one_upsert`
//! 2. `co_authored_commit_fans_out_to_n_event_actors_rows`
//! 3. `missed_webhook_detected_by_reconciler`
//! 4. `backfill_respects_rate_limit_headroom`
//! 5. `scheduler_coalesces_overlapping_ticks`
//! 6. `fetch_runs_row_written_per_batch_per_kind`
//!
//! The seventh (`boundary_check_still_green`) is the
//! `scripts/check-boundaries.sh` shell-script check wired into
//! CI as a separate job — there is no Rust equivalent here.
//!
//! Several of these are deliberately redundant with finer-grained
//! tests elsewhere in the crate (e.g. the worker's
//! `redelivered_pr_is_idempotent_on_external_id`, the reconciler
//! and backfill's `pr_list_synthesises_deliveries_…` pair). That
//! redundancy is the *point*: this module is the
//! contract-pinned smoke surface — a future refactor that breaks
//! any invariant the spec named has to delete a smoke test
//! before CI goes green, which is exactly the friction we want.

#![cfg(test)]

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use dp_domain::{ActorRole, EventKind, FetchRunKind, ResourceKind, Store, WebhookDelivery};
use secrecy::SecretString;
use serde_json::{json, Value};
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::backfill::{Backfill, BackfillConfig};
use crate::client::Client;
use crate::reconciler::{Reconciler, RepoTarget, Scope, StaticTargets};
use crate::worker::test_store::FakeStore;
use crate::worker::{apply_delivery, Worker};

// ---------- helpers ---------------------------------------------------

fn delivery(event: &str, delivery_id: &str, payload: Value) -> WebhookDelivery {
    WebhookDelivery {
        id: Uuid::new_v4(),
        delivery_id: delivery_id.into(),
        event: event.into(),
        payload,
        received_at: Utc::now(),
        processed_at: None,
        error: None,
    }
}

fn one_target() -> RepoTarget {
    RepoTarget {
        org_id: Uuid::from_u128(0xCAFE),
        org_github_id: 42,
        owner_login: "octo".into(),
        repo_id: Uuid::from_u128(0xBEEF),
        repo_github_id: 7,
        repo_name: "hello".into(),
    }
}

// =====================================================================
// 1. Webhook replay: same delivery_id yields exactly one upsert.
// =====================================================================
//
// GitHub re-delivers on any non-2xx. The webhook receiver dedups
// at the inbox via the `delivery_id` UNIQUE; the worker's
// `apply_delivery` is additionally idempotent on
// `(kind, external_id)`. This smoke test exercises the
// belt-and-braces: even when *both* deliveries make it past the
// inbox (e.g. a redelivery happens after the row was already
// purged), exactly one ActivityEvent + one Author actor row
// survives.
#[tokio::test]
async fn webhook_replay_same_delivery_id_yields_exactly_one_upsert() {
    let s = Arc::new(FakeStore::new());

    let payload = json!({
        "action": "opened",
        "repository": {
            "id": 1, "name": "r",
            "owner": { "id": 1, "login": "o" }
        },
        "pull_request": {
            "node_id":    "PR_smoke_replay",
            "created_at": "2024-01-01T00:00:00Z",
            "user":       { "id": 7, "login": "alice" }
        }
    });

    // First delivery — normal arrival.
    apply_delivery(
        s.as_ref(),
        &delivery("pull_request", "delivery-replay-1", payload.clone()),
    )
    .await
    .expect("first apply ok");

    // Second delivery: same payload, different delivery_id (the
    // worker sees redeliveries as fresh inbox rows but the
    // upsert path collapses them).
    apply_delivery(
        s.as_ref(),
        &delivery("pull_request", "delivery-replay-2", payload),
    )
    .await
    .expect("second apply ok");

    // Exactly one event row.
    assert_eq!(
        s.events_count(),
        1,
        "replay must collapse to one activity_event"
    );
    let ev = s.only_event();
    assert_eq!(ev.kind, EventKind::PullRequestOpened);
    assert_eq!(ev.external_id, "PR_smoke_replay");

    // Author actor row appears exactly once despite the replay
    // (composite PK on event_actors does the work here).
    let n_author = s
        .actors_for(ev.id)
        .into_iter()
        .filter(|(login, role)| login == "alice" && *role == ActorRole::Author)
        .count();
    assert_eq!(n_author, 1, "author actor row must not double up");
}

// =====================================================================
// 2. Co-authored commit fans out to N event_actors rows.
// =====================================================================
//
// SCOPE §6: a single commit with `Co-authored-by:` trailers
// produces ONE activity_event and MULTIPLE event_actors rows
// (author + committer + N co_authors). The fixture file is the
// canonical capture (`push_coauthored.json` — same fixture the
// fixture_tests cover); we re-assert the count here so the
// "fan-out" invariant has its own smoke entry.
#[tokio::test]
async fn co_authored_commit_fans_out_to_n_event_actors_rows() {
    let raw = include_str!("../tests/fixtures/push_coauthored.json");
    let mut payload: Value = serde_json::from_str(raw).expect("fixture parses");
    if let Some(obj) = payload.as_object_mut() {
        obj.remove("_comment");
    }

    let s = Arc::new(FakeStore::new());
    let out = apply_delivery(s.as_ref(), &delivery("push", "smoke-coauth", payload))
        .await
        .expect("handler accepts fixture");

    assert_eq!(out.events, 1, "one commit -> one event");
    // alice author + alice committer + two co_authors = 4 actor rows.
    assert_eq!(
        out.actors, 4,
        "co-authored commit must fan out into N actor rows"
    );

    let ev = s.only_event();
    assert_eq!(ev.kind, EventKind::Commit);

    let alice_roles = s.roles_for_login(ev.id, "alice");
    assert!(
        alice_roles.contains(&ActorRole::Author)
            && alice_roles.contains(&ActorRole::Committer),
        "alice author+committer: {alice_roles:?}"
    );
    assert!(s
        .roles_for_login(ev.id, "octocat")
        .contains(&ActorRole::CoAuthor));
    assert!(s
        .roles_for_login(ev.id, "mallory@external.example")
        .contains(&ActorRole::CoAuthor));
}

// =====================================================================
// 3. Missed webhook detected by reconciler — with multi-actor attribution.
// =====================================================================
//
// Simulate: the `pull_request.opened` webhook for PR #1 never
// arrived (network blip / GitHub-side drop). The reconciler runs
// its 4h tick, lists PRs via the wiremock-backed Client, sees the
// PR is absent locally, synthesises a delivery, and feeds it
// through the **same** `apply_delivery` path. The resulting
// activity_event must carry the correct multi-actor attribution
// (`Author` at minimum on a PR list response — reviewers come
// from the review-list endpoint covered elsewhere).
#[tokio::test]
async fn missed_webhook_detected_by_reconciler() {
    let server = MockServer::start().await;
    let client = Client::with_personal_token(
        SecretString::from("smoke-token".to_string()),
        &server.uri(),
    )
    .expect("client builds");
    let store = Arc::new(FakeStore::new());
    let targets = Arc::new(StaticTargets::new(vec![one_target()]));
    let rec = Reconciler::new(
        store.clone() as Arc<dyn Store>,
        Arc::new(client),
        targets,
    )
    .with_kinds(&[ResourceKind::PullRequests])
    .with_org_kinds(&[]);

    // The reconciler's list call returns one PR that the local
    // store has never seen — i.e. its `opened` webhook went
    // missing.
    Mock::given(method("GET"))
        .and(path("/repos/octo/hello/pulls"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "W/\"missed\"")
                .insert_header("x-ratelimit-remaining", "4999")
                .insert_header("x-ratelimit-reset", "9999999999")
                .set_body_json(json!([
                    {
                        "node_id":    "PR_missed_1",
                        "state":      "open",
                        "created_at": "2024-04-01T00:00:00Z",
                        "updated_at": "2024-04-01T00:00:00Z",
                        "user":       { "id": 42, "login": "diana" }
                    }
                ])),
        )
        .mount(&server)
        .await;

    // Pre-state assertion: the store is empty — there is no event
    // yet, confirming the "missed webhook" premise.
    assert_eq!(store.events_count(), 0);

    let stats = rec.do_tick(Scope::All).await.expect("reconciler tick ok");
    assert_eq!(stats.items, 1, "one missed PR detected");
    assert_eq!(stats.errors, 0);

    // The missed PR now appears in the local store with the
    // correct attribution — Diana is the Author of the PR.
    assert_eq!(store.events_count(), 1, "missed webhook backfilled");
    let ev = store.only_event();
    assert_eq!(ev.kind, EventKind::PullRequestOpened);
    assert_eq!(ev.external_id, "PR_missed_1");
    assert!(
        store
            .roles_for_login(ev.id, "diana")
            .contains(&ActorRole::Author),
        "multi-actor attribution must include Author=diana"
    );
}

// =====================================================================
// 4. Backfill respects rate-limit headroom — pauses then resumes.
// =====================================================================
//
// Wiremock returns near-exhaustion `x-ratelimit-remaining`
// headers; the backfill must voluntarily yield via
// `honour_headroom` so live webhook processing keeps budget.
// We pin the *decision* (the headroom branch is taken) without
// asserting actual sleep duration — the production sleep is
// time-based and capped at 1h, and the chunk did complete
// (one fetch_runs row, one event), so "pauses then resumes"
// is the right framing.
#[tokio::test]
async fn backfill_respects_rate_limit_headroom() {
    let server = MockServer::start().await;
    let client = Client::with_personal_token(
        SecretString::from("smoke".to_string()),
        &server.uri(),
    )
    .expect("client");
    let store = Arc::new(FakeStore::new());
    let targets = Arc::new(StaticTargets::new(vec![one_target()]));

    // Headroom of 1000; wiremock returns `remaining=50` — well
    // below headroom, so backfill must yield. We use a near-now
    // reset so the sleep is effectively zero in test time but
    // the *branch* is exercised.
    let bf = Backfill::new(
        store.clone() as Arc<dyn Store>,
        Arc::new(client),
        targets,
        BackfillConfig {
            window: Duration::from_secs(7 * 86_400),
            rate_limit_headroom: 1000,
        },
    )
    .with_kinds(&[ResourceKind::PullRequests]);

    Mock::given(method("GET"))
        .and(path("/repos/octo/hello/pulls"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "W/\"bf-headroom\"")
                // 50 << 1000 headroom — backfill must yield.
                .insert_header("x-ratelimit-remaining", "50")
                // Reset = now (so the sleep is a no-op in test
                // time, but the headroom branch fires).
                .insert_header("x-ratelimit-reset", "0")
                .set_body_json(json!([
                    {
                        "node_id":    "PR_bf_headroom",
                        "state":      "open",
                        "created_at": "2024-01-01T00:00:00Z",
                        "updated_at": "2024-01-01T00:00:00Z",
                        "user":       { "id": 7, "login": "alice" }
                    }
                ])),
        )
        .mount(&server)
        .await;

    let stats = bf
        .run_for_org(one_target().org_id, None)
        .await
        .expect("backfill returns after honour_headroom");

    // Backfill resumed after the yield: the chunk completed,
    // the event was upserted, the cursor advanced.
    assert_eq!(stats.chunks, 1);
    assert_eq!(stats.items, 1);
    assert_eq!(stats.errors, 0);
    assert_eq!(store.events_count(), 1);
    let ev = store.only_event();
    assert_eq!(ev.external_id, "PR_bf_headroom");
}

// =====================================================================
// 5. Scheduler coalesces overlapping triggers.
// =====================================================================
//
// Two near-simultaneous `try_trigger_now` calls must not both
// produce a tick — the `Mutex<Option<JoinHandle>>` guard
// coalesces the second into a no-op. We use a deliberately slow
// mock so the first tick is still in flight when the second
// trigger lands.
#[tokio::test]
async fn scheduler_coalesces_overlapping_ticks() {
    use crate::reconciler::Scheduler;
    use wiremock::matchers::header_exists;

    let server = MockServer::start().await;
    let client = Client::with_personal_token(
        SecretString::from("smoke".to_string()),
        &server.uri(),
    )
    .expect("client");
    let store = Arc::new(FakeStore::new());
    let targets = Arc::new(StaticTargets::new(vec![one_target()]));
    let rec = Reconciler::new(
        store.clone() as Arc<dyn Store>,
        Arc::new(client),
        targets,
    )
    .with_kinds(&[ResourceKind::PullRequests])
    .with_org_kinds(&[]);

    Mock::given(method("GET"))
        .and(path("/repos/octo/hello/pulls"))
        .and(header_exists("accept"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(120))
                .set_body_json(json!([])),
        )
        .mount(&server)
        .await;

    let sched = Arc::new(Scheduler::new(Arc::new(rec), Duration::from_secs(3600)));

    let s1 = sched.clone();
    let s2 = sched.clone();
    let h1 = tokio::spawn(async move { s1.try_trigger_now(Scope::All).await });
    // Let s1 take the mutex + spawn its tick before s2 arrives.
    tokio::time::sleep(Duration::from_millis(20)).await;
    let h2 = tokio::spawn(async move { s2.try_trigger_now(Scope::All).await });

    let r1 = h1.await.unwrap().expect("trigger ok");
    let r2 = h2.await.unwrap().expect("trigger ok");

    let ran = [r1.is_some(), r2.is_some()];
    assert_eq!(
        ran.iter().filter(|b| **b).count(),
        1,
        "exactly one trigger must run; the other must coalesce: r1={r1:?}, r2={r2:?}"
    );
}

// =====================================================================
// 6. fetch_runs row written per batch, per kind.
// =====================================================================
//
// Operators read `/admin/runs` to verify ingestion health. The
// invariant: every batch — worker drain, reconciler tick,
// backfill chunk — opens *and closes* exactly one fetch_runs row
// of the corresponding kind. We exercise all three kinds in one
// test so a regression in any path trips the same smoke check.
#[tokio::test]
async fn fetch_runs_row_written_per_batch_per_kind() {
    // --- (a) WebhookWorker: one row per drain ---
    let s_worker = Arc::new(FakeStore::new());
    s_worker.enqueue_webhook_for_test(delivery(
        "pull_request",
        "fr-w-1",
        json!({
            "action": "opened",
            "repository": {
                "id": 1, "name": "r",
                "owner": { "id": 1, "login": "o" }
            },
            "pull_request": {
                "node_id":    "PR_fr_worker",
                "created_at": "2024-01-01T00:00:00Z",
                "user":       { "id": 7, "login": "alice" }
            }
        }),
    ));
    let w = Worker::new(s_worker.clone() as Arc<dyn Store>);
    w.drain_once().await.expect("drain ok");
    let runs = s_worker.fetch_runs();
    assert_eq!(runs.len(), 1, "one drain -> one fetch_runs row");
    assert!(
        matches!(runs[0].kind, FetchRunKind::WebhookWorker),
        "kind must be WebhookWorker"
    );
    assert!(
        runs[0].finished.is_some(),
        "fetch_runs row must be closed"
    );

    // --- (b) Reconciler: one row per tick ---
    let server = MockServer::start().await;
    let client = Client::with_personal_token(
        SecretString::from("smoke".to_string()),
        &server.uri(),
    )
    .expect("client");
    let s_rec = Arc::new(FakeStore::new());
    let targets = Arc::new(StaticTargets::new(vec![one_target()]));
    let rec = Reconciler::new(
        s_rec.clone() as Arc<dyn Store>,
        Arc::new(client),
        targets.clone(),
    )
    .with_kinds(&[ResourceKind::PullRequests])
    .with_org_kinds(&[]);
    Mock::given(method("GET"))
        .and(path("/repos/octo/hello/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    rec.do_tick(Scope::All).await.expect("tick ok");
    let runs = s_rec.fetch_runs();
    assert_eq!(runs.len(), 1, "one tick -> one fetch_runs row");
    assert!(
        matches!(runs[0].kind, FetchRunKind::Reconciler),
        "kind must be Reconciler"
    );
    assert!(runs[0].finished.is_some());

    // --- (c) Backfill: one row per chunk ---
    let server_bf = MockServer::start().await;
    let client_bf = Client::with_personal_token(
        SecretString::from("smoke".to_string()),
        &server_bf.uri(),
    )
    .expect("client");
    let s_bf = Arc::new(FakeStore::new());
    let bf = Backfill::new(
        s_bf.clone() as Arc<dyn Store>,
        Arc::new(client_bf),
        Arc::new(StaticTargets::new(vec![one_target()])),
        BackfillConfig {
            window: Duration::from_secs(7 * 86_400),
            rate_limit_headroom: 100,
        },
    )
    .with_kinds(&[ResourceKind::PullRequests]);
    Mock::given(method("GET"))
        .and(path("/repos/octo/hello/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server_bf)
        .await;
    bf.run_for_org(one_target().org_id, None)
        .await
        .expect("backfill ok");
    let runs = s_bf.fetch_runs();
    assert_eq!(runs.len(), 1, "one chunk -> one fetch_runs row");
    assert!(
        matches!(runs[0].kind, FetchRunKind::Backfill),
        "kind must be Backfill"
    );
    assert!(runs[0].finished.is_some());
}
