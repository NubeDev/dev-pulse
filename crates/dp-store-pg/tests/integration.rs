//! End-to-end exercises for [`dp_store_pg::PgStore`] against a real
//! Postgres.
//!
//! These tests are the only thing in the workspace that proves the
//! SQL in `migrations/dp/0001_init.sql` and the query bodies in
//! `src/store.rs` actually agree with each other and with the
//! `dp_domain::Store` contract. Unit tests in `src/store.rs` only get
//! us object-safety; everything below depends on a live database.
//!
//! ## How a test gets a database
//!
//! Two paths, picked at runtime:
//!
//! 1. `DP_TEST_DATABASE_URL` — if set, every test connects to that
//!    URL. The caller is responsible for it being empty (the
//!    migrator will refuse to re-apply an already-applied schema
//!    with different checksums). Useful for `psql` debugging or
//!    when Docker is unavailable.
//! 2. Otherwise, an ephemeral Postgres container is started directly
//!    via `testcontainers-modules`, pinned to PG15+
//!    (`DP_TEST_PG_TAG`, default `16-alpine`). We deliberately do
//!    NOT call `starter_store_postgres::testing::with_database()`
//!    because that helper hard-codes `postgres:11-alpine` and PG11
//!    pre-dates `UNIQUE NULLS NOT DISTINCT` — the schema would fail
//!    to apply.
//!
//! Each test calls [`fixture`] which: connects, runs every
//! `dp_store_pg::sources()` migration, and hands back a [`PgStore`].
//!
//! ## Why `#[ignore]`
//!
//! Plain `cargo test --workspace` must stay Docker-free (job goal
//! "cargo test --workspace green"). Integration runs in its own CI
//! job with `cargo test -p dp-store-pg -- --ignored` where Docker is
//! guaranteed.
//!
//! ## Coverage
//!
//! Every Store-trait method that has non-trivial SQL behaviour is
//! exercised:
//!
//! * upsert paths (user / org / team / repo / membership) and the
//!   `home_org` preservation invariant on `upsert_membership`
//!   (TODO §0.5 / SCOPE §3 — only `set_home_org` writes it);
//! * cursor put/get with both a concrete `repo_id` and a NULL
//!   `repo_id`, relying on PG15 `NULLS NOT DISTINCT` on the unique
//!   constraint;
//! * webhook inbox: enqueue → conflict on duplicate `delivery_id`
//!   (idempotent replay path), `claim_webhooks` FIFO drain, then
//!   `mark_webhook_processed` / `mark_webhook_failed`;
//! * `record_event` + `add_event_actors` + the windowed report read
//!   `list_event_actor_rows_in_window` with org / repo / user /
//!   role filter combinations;
//! * `pseudonymise_user` rewrites login + clears email/name + stamps
//!   `deleted_at` and hides the row from `list_users`, but the row
//!   itself stays referenced by historical events (FK integrity per
//!   TODO §0.5).

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use chrono::{DateTime, Duration, TimeZone, Utc};
use dp_domain::event::{ActivityEvent, ActorRole, EventActor, EventKind};
use dp_domain::issue::{IssueState, IssueUpsert};
use dp_domain::board_link::{BoardItemMirrorOutcome, BoardLinkUpsert};
use dp_domain::project::{ProjectListFilter, ProjectStatus, ProjectUpsert};
use dp_domain::fetch::{FetchCursor, FetchRunKind, ResourceKind};
use dp_domain::membership::{Membership, MembershipRole};
use dp_domain::org::Org;
use dp_domain::repo::{Repo, RepoMetadata};
use dp_domain::store::{Store, StoreError};
use dp_domain::team::Team;
use dp_domain::user::User;
use dp_domain::webhook::WebhookDelivery;
use dp_domain::window::{Window, WindowAnchor};
use dp_store_pg::PgStore;
use serde_json::json;
use starter_store_postgres::migrate;
use starter_store_postgres::pool::connect;
use testcontainers::runners::AsyncRunner;
use testcontainers::ImageExt;
use testcontainers_modules::postgres::Postgres as PostgresImage;
use uuid::Uuid;

// ---------- fixture --------------------------------------------------

/// Holds whatever needs to outlive the [`PgStore`]. Drop order
/// matters: `_guard` (if present) tears down the container, and the
/// pool inside `store` must already be done by then. The guard's
/// concrete type isn't re-exported from `starter_store_postgres`, so
/// we store it type-erased — we only ever drop it, never inspect it.
struct Fixture {
    store: Arc<PgStore>,
    _guard: Option<Box<dyn std::any::Any + Send>>,
}

impl Fixture {
    fn store(&self) -> &PgStore {
        &self.store
    }
}

/// Image tag that ships PG15+ semantics — the `UNIQUE NULLS NOT
/// DISTINCT` constraint on `dp_fetch_cursors` and the partial-index
/// predicate features both need it. Override with `DP_TEST_PG_TAG`
/// (CI may want to pin to a specific patch tag).
const DEFAULT_PG_TAG: &str = "16-alpine";

/// Acquire a migrated [`PgStore`]. Two paths:
///
/// * `DP_TEST_DATABASE_URL` → connect to that URL (caller manages a
///   clean schema). Useful for `psql` debugging against a local DB.
/// * otherwise → start an ephemeral PG container at `DP_TEST_PG_TAG`
///   (default `DEFAULT_PG_TAG`). We do **not** use
///   `starter_store_postgres::testing::with_database()` because it
///   hard-codes `postgres:11-alpine` and PG11 rejects
///   `UNIQUE NULLS NOT DISTINCT`.
async fn fixture() -> Fixture {
    let (pool, guard): (_, Option<Box<dyn std::any::Any + Send>>) =
        if let Ok(url) = std::env::var("DP_TEST_DATABASE_URL") {
            let pool = connect(&url)
                .await
                .expect("connect DP_TEST_DATABASE_URL");
            (pool, None)
        } else {
            let tag = std::env::var("DP_TEST_PG_TAG")
                .unwrap_or_else(|_| DEFAULT_PG_TAG.to_string());
            let container = PostgresImage::default()
                .with_tag(tag)
                .start()
                .await
                .expect("start postgres container");
            let port = container
                .get_host_port_ipv4(5432)
                .await
                .expect("container port");
            let url =
                format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
            let pool = connect(&url).await.expect("connect to test postgres");
            (pool, Some(Box::new(container)))
        };

    // Apply every migration source this crate publishes. There is
    // exactly one today (`dp`); using the published API keeps the
    // test honest about how the host binary will run it.
    let mut m = migrate(&pool);
    for source in dp_store_pg::sources() {
        m = m.with_source(source);
    }
    m.run().await.expect("apply dp migrations");

    Fixture {
        store: Arc::new(PgStore::new(pool)),
        _guard: guard,
    }
}

// ---------- seed helpers --------------------------------------------
//
// Every test that touches events needs at least one org + repo; most
// also need users. Inline-seed locally rather than reach for fixtures
// — the rows are small and being able to read the test top-to-bottom
// matters more than DRY here.

async fn seed_org(s: &PgStore, github_id: i64, login: &str) -> Org {
    s.upsert_org(&Org {
        id: Uuid::new_v4(),
        github_id,
        login: login.into(),
        name: Some(login.to_string()),
    })
    .await
    .unwrap()
}

async fn seed_repo(s: &PgStore, org: &Org, github_id: i64, name: &str) -> Repo {
    s.upsert_repo(&Repo {
        id: Uuid::new_v4(),
        org_id: org.id,
        github_id,
        name: name.into(),
    })
    .await
    .unwrap()
}

async fn seed_user(s: &PgStore, github_id: i64, login: &str) -> User {
    s.upsert_user(&User {
        id: Uuid::new_v4(),
        github_id,
        login: login.into(),
        email: Some(format!("{login}@example.com")),
        name: Some(login.to_string()),
        deleted_at: None,
    })
    .await
    .unwrap()
}

// ---------- tests ---------------------------------------------------

/// `upsert_user` round-trips, dedupes on `github_id`, and the
/// lookup-by-github-id path returns the same row. `list_users` is
/// the soft-delete-aware projection.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn upsert_user_and_lookups() {
    let f = fixture().await;
    let s = f.store();

    let u1 = seed_user(s, 1001, "alice").await;
    assert_eq!(u1.login, "alice");

    // Same github_id with a renamed login → upsert keeps the id and
    // overwrites the login (mirrors GitHub rename replay).
    let u1_renamed = s
        .upsert_user(&User {
            id: Uuid::new_v4(), // ignored by ON CONFLICT
            github_id: 1001,
            login: "alice2".into(),
            email: Some("alice2@example.com".into()),
            name: Some("Alice".into()),
            deleted_at: None,
        })
        .await
        .unwrap();
    assert_eq!(u1_renamed.id, u1.id, "id stable across rename");
    assert_eq!(u1_renamed.login, "alice2");

    let by_gh = s.get_user_by_github_id(1001).await.unwrap();
    assert_eq!(by_gh.id, u1.id);

    let by_id = s.get_user(u1.id).await.unwrap();
    assert_eq!(by_id.login, "alice2");

    // Negative path: unknown github_id → NotFound.
    let miss = s.get_user_by_github_id(9_999_999).await.unwrap_err();
    assert!(matches!(miss, StoreError::NotFound { .. }));

    // list_users sees the live row.
    let listed = s.list_users().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, u1.id);
}

/// `upsert_membership` must not clobber `home_org`. Only
/// `set_home_org` writes it (the invariant the schema can't catch
/// — `home_org` is a normal nullable column).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn upsert_membership_preserves_home_org() {
    let f = fixture().await;
    let s = f.store();

    let org_a = seed_org(s, 1, "org-a").await;
    let org_b = seed_org(s, 2, "org-b").await;
    let user = seed_user(s, 42, "bob").await;

    // Initial upsert with no home_org.
    let joined = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    s.upsert_membership(&Membership {
        user_id: user.id,
        org_id: org_a.id,
        role: MembershipRole::Member,
        home_org: None,
        joined_at: joined,
    })
    .await
    .unwrap();

    // Admin then sets home_org via the dedicated path.
    s.set_home_org(user.id, org_a.id, Some(org_b.id))
        .await
        .unwrap();

    // Fetcher replays the upsert (e.g. periodic reconciliation).
    // Even though it carries `home_org: None`, the COALESCE in the
    // SQL must keep the previously-set value.
    let later = joined + Duration::days(30);
    let after = s
        .upsert_membership(&Membership {
            user_id: user.id,
            org_id: org_a.id,
            role: MembershipRole::Admin,
            home_org: None,
            joined_at: later,
        })
        .await
        .unwrap();
    assert_eq!(after.role, MembershipRole::Admin, "role updated");
    assert_eq!(
        after.home_org,
        Some(org_b.id),
        "home_org preserved across upsert"
    );
    // joined_at takes the LEAST of old/new so an out-of-order
    // replay never moves the join-date forward.
    assert_eq!(after.joined_at, joined);

    // Clearing home_org goes through set_home_org with None.
    s.set_home_org(user.id, org_a.id, None).await.unwrap();
    let memberships = s.list_memberships_for_user(user.id).await.unwrap();
    assert_eq!(memberships.len(), 1);
    assert_eq!(memberships[0].home_org, None);

    // set_home_org on a non-existent (user, org) pair → NotFound.
    let bogus = s
        .set_home_org(Uuid::new_v4(), org_a.id, None)
        .await
        .unwrap_err();
    assert!(matches!(bogus, StoreError::NotFound { .. }));
}

/// `upsert_repo_metadata` round-trips, `get_repo_metadata` returns
/// the latest values, and a second upsert with nullable text fields
/// set to `None` does **not** wipe previously-recorded values (the
/// COALESCE guard in `upsert_repo_metadata`'s ON CONFLICT clause).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn repo_metadata_roundtrip_and_coalesce() {
    let f = fixture().await;
    let s = f.store();
    let org = seed_org(s, 1, "octo").await;
    let repo = seed_repo(s, &org, 100, "hello").await;

    // No snapshot yet → None.
    assert!(s.get_repo_metadata(repo.id).await.unwrap().is_none());

    // First write — full payload.
    let t0 = Utc.with_ymd_and_hms(2025, 1, 1, 12, 0, 0).unwrap();
    s.upsert_repo_metadata(&RepoMetadata {
        repo_id: repo.id,
        stars: 42,
        forks: 7,
        watchers: 3,
        open_issues_remote: 5,
        primary_language: Some("Rust".into()),
        default_branch: Some("main".into()),
        description: Some("a thing".into()),
        homepage: Some("https://example.com".into()),
        is_archived: false,
        is_fork: false,
        is_private: false,
        pushed_at: Some(t0),
        metadata_updated_at: t0,
    })
    .await
    .unwrap();

    let got = s.get_repo_metadata(repo.id).await.unwrap().unwrap();
    assert_eq!(got.stars, 42);
    assert_eq!(got.primary_language.as_deref(), Some("Rust"));
    assert_eq!(got.description.as_deref(), Some("a thing"));

    // Second write — counter bump + nullable text fields set to None
    // (simulates a partial webhook payload). Counters update, text
    // fields are preserved by COALESCE.
    let t1 = Utc.with_ymd_and_hms(2025, 1, 2, 12, 0, 0).unwrap();
    s.upsert_repo_metadata(&RepoMetadata {
        repo_id: repo.id,
        stars: 50,
        forks: 8,
        watchers: 4,
        open_issues_remote: 6,
        primary_language: None,
        default_branch: None,
        description: None,
        homepage: None,
        is_archived: true,
        is_fork: false,
        is_private: false,
        pushed_at: None,
        metadata_updated_at: t1,
    })
    .await
    .unwrap();

    let got = s.get_repo_metadata(repo.id).await.unwrap().unwrap();
    // Counters updated.
    assert_eq!(got.stars, 50);
    assert_eq!(got.forks, 8);
    assert_eq!(got.open_issues_remote, 6);
    // Flags updated unconditionally.
    assert!(got.is_archived);
    // Nullable text + pushed_at preserved by COALESCE.
    assert_eq!(got.primary_language.as_deref(), Some("Rust"));
    assert_eq!(got.default_branch.as_deref(), Some("main"));
    assert_eq!(got.description.as_deref(), Some("a thing"));
    assert_eq!(got.homepage.as_deref(), Some("https://example.com"));
    assert_eq!(got.pushed_at, Some(t0));
    assert_eq!(got.metadata_updated_at, t1);
}

/// `pr_size_stats_for_repo` returns p50/p90/p95 over the JSONB
/// `additions` / `deletions` / `changed_files` / `commits` fields
/// of `pull_request_merged` events, scoped to a repo + window.
///
/// Three cases in one test for speed:
///
/// 1. Below the §15.9 minimum (`n < 5`) — every percentile is
///    `None` regardless of what was inserted.
/// 2. At / above the minimum — percentiles match the values
///    `compute_percentiles` would produce on the same input.
/// 3. Window + repo + kind filters all apply.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn pr_size_stats_for_repo_filters_and_guards_sample_size() {
    let f = fixture().await;
    let s = f.store();

    let org = seed_org(s, 1, "octo").await;
    let repo = seed_repo(s, &org, 100, "hello").await;
    let other_repo = seed_repo(s, &org, 101, "other").await;

    let t0 = Utc.with_ymd_and_hms(2025, 5, 10, 12, 0, 0).unwrap();
    let mk_pr = |ts: chrono::DateTime<Utc>,
                 repo_id: Uuid,
                 ext: &str,
                 add: i64,
                 del: i64,
                 cf: i64,
                 commits: i64|
     -> ActivityEvent {
        ActivityEvent {
            id: Uuid::new_v4(),
            org_id: org.id,
            repo_id,
            kind: EventKind::PullRequestMerged,
            ts,
            external_id: ext.into(),
            payload: json!({
                "additions": add,
                "deletions": del,
                "changed_files": cf,
                "commits": commits,
            }),
        }
    };

    // ----- case 1: only two merged PRs in window → n=2 < 5 ----
    s.record_event(&mk_pr(t0, repo.id, "small-1", 10, 5, 2, 1)).await.unwrap();
    s.record_event(&mk_pr(t0 + Duration::hours(1), repo.id, "small-2", 100, 50, 3, 2))
        .await
        .unwrap();

    let since = t0 - Duration::days(1);
    let until = t0 + Duration::days(1);
    let stats = s.pr_size_stats_for_repo(repo.id, since, until).await.unwrap();
    assert_eq!(stats.sample_n, 2);
    assert!(stats.additions.p50.is_none(), "n<5 must mask p50");
    assert!(stats.additions.p90.is_none());
    assert!(stats.additions.p95.is_none());

    // ----- case 2: top up to n=5 → percentiles populated -------
    // additions sequence becomes: [10, 100, 20, 30, 40]
    // sorted:                     [10, 20, 30, 40, 100]
    // p50 (percentile_cont, linear interp on 4 intervals @ pos 2) → 30
    s.record_event(&mk_pr(t0 + Duration::hours(2), repo.id, "m-3", 20, 10, 4, 2)).await.unwrap();
    s.record_event(&mk_pr(t0 + Duration::hours(3), repo.id, "m-4", 30, 15, 5, 3)).await.unwrap();
    s.record_event(&mk_pr(t0 + Duration::hours(4), repo.id, "m-5", 40, 20, 6, 4)).await.unwrap();

    let stats = s.pr_size_stats_for_repo(repo.id, since, until).await.unwrap();
    assert_eq!(stats.sample_n, 5);
    let p50 = stats.additions.p50.expect("p50 populated at n=5");
    assert!((p50 - 30.0).abs() < 1e-9, "p50 of [10,20,30,40,100] = 30, got {p50}");
    assert!(stats.additions.p90.unwrap() > p50);
    assert!(stats.additions.p95.unwrap() >= stats.additions.p90.unwrap());
    // total_lines = additions + deletions; sorted: [15, 30, 45, 60, 150] → p50 = 45.
    let tot_p50 = stats.total_lines.p50.unwrap();
    assert!((tot_p50 - 45.0).abs() < 1e-9, "got {tot_p50}");

    // ----- case 3: out-of-window + other-repo + wrong-kind events
    // must NOT affect the stats. Insert one of each and assert the
    // numbers are unchanged.
    s.record_event(&mk_pr(t0 - Duration::days(5), repo.id, "old", 99999, 9, 9, 9))
        .await
        .unwrap();
    s.record_event(&mk_pr(t0, other_repo.id, "wrong-repo", 99999, 9, 9, 9))
        .await
        .unwrap();
    // Closed-without-merge PR — same payload shape, wrong kind.
    s.record_event(&ActivityEvent {
        id: Uuid::new_v4(),
        org_id: org.id,
        repo_id: repo.id,
        kind: EventKind::PullRequestClosed,
        ts: t0,
        external_id: "closed-unmerged".into(),
        payload: json!({
            "additions": 99999, "deletions": 9, "changed_files": 9, "commits": 9
        }),
    })
    .await
    .unwrap();

    let stats2 = s.pr_size_stats_for_repo(repo.id, since, until).await.unwrap();
    assert_eq!(stats2.sample_n, 5, "n must not include out-of-scope rows");
    assert_eq!(stats2.additions.p50, stats.additions.p50);

    // Empty window → n=0, every triple None.
    let empty = s
        .pr_size_stats_for_repo(repo.id, t0 + Duration::days(30), t0 + Duration::days(40))
        .await
        .unwrap();
    assert_eq!(empty.sample_n, 0);
    assert!(empty.additions.p50.is_none());
}

/// `ci_stats_for_repo` aggregates `workflow_run` events: counts
/// split by `conclusion`, success rate over (success+failure)
/// only, and `updated_at - run_started_at` duration percentiles
/// guarded by the §15.9 minimum-sample rule.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn ci_stats_for_repo_counts_and_durations() {
    let f = fixture().await;
    let s = f.store();
    let org = seed_org(s, 1, "octo").await;
    let repo = seed_repo(s, &org, 100, "hello").await;
    let other_repo = seed_repo(s, &org, 101, "other").await;

    let t0 = Utc.with_ymd_and_hms(2025, 5, 10, 12, 0, 0).unwrap();
    let mk_run = |ts: chrono::DateTime<Utc>,
                  repo_id: Uuid,
                  ext: &str,
                  conclusion: &str,
                  started_offset_s: i64,
                  ended_offset_s: i64|
     -> ActivityEvent {
        let started = ts + Duration::seconds(started_offset_s);
        let ended = ts + Duration::seconds(ended_offset_s);
        ActivityEvent {
            id: Uuid::new_v4(),
            org_id: org.id,
            repo_id,
            kind: EventKind::WorkflowRun,
            ts: ended,
            external_id: ext.into(),
            payload: json!({
                "conclusion": conclusion,
                "run_started_at": started.to_rfc3339(),
                "updated_at":     ended.to_rfc3339(),
            }),
        }
    };

    // 6 successes (durations: 60, 120, 180, 240, 300, 600 s),
    // 2 failures (durations: 90, 90 s),
    // 1 cancelled (60 s), 1 skipped (0 s — no duration sample).
    let runs = [
        mk_run(t0, repo.id, "s-1", "success", 0, 60),
        mk_run(t0, repo.id, "s-2", "success", 0, 120),
        mk_run(t0, repo.id, "s-3", "success", 0, 180),
        mk_run(t0, repo.id, "s-4", "success", 0, 240),
        mk_run(t0, repo.id, "s-5", "success", 0, 300),
        mk_run(t0, repo.id, "s-6", "success", 0, 600),
        mk_run(t0, repo.id, "f-1", "failure", 0, 90),
        mk_run(t0, repo.id, "f-2", "failure", 0, 90),
        mk_run(t0, repo.id, "c-1", "cancelled", 0, 60),
        // `updated_at == run_started_at` → duration_s = 0,
        // filtered out of the duration sample but counted in the
        // total. Conclusion "skipped" → counted under `other`.
        mk_run(t0, repo.id, "k-1", "skipped", 0, 0),
    ];
    for r in &runs {
        s.record_event(r).await.unwrap();
    }

    // Out-of-window + other-repo: must not affect any number.
    s.record_event(&mk_run(t0 - Duration::days(5), repo.id, "old", "success", 0, 9999))
        .await
        .unwrap();
    s.record_event(&mk_run(t0, other_repo.id, "wrong-repo", "failure", 0, 9999))
        .await
        .unwrap();
    // Wrong event kind.
    s.record_event(&ActivityEvent {
        id: Uuid::new_v4(),
        org_id: org.id,
        repo_id: repo.id,
        kind: EventKind::Commit,
        ts: t0,
        external_id: "commit-noise".into(),
        payload: json!({"conclusion": "success", "run_started_at": "2025-01-01T00:00:00Z", "updated_at": "2025-01-01T00:00:01Z"}),
    })
    .await
    .unwrap();

    let since = t0 - Duration::days(1);
    let until = t0 + Duration::days(1);
    let stats = s.ci_stats_for_repo(repo.id, since, until).await.unwrap();

    assert_eq!(stats.total_runs, 10);
    assert_eq!(stats.success, 6);
    assert_eq!(stats.failure, 2);
    assert_eq!(stats.cancelled, 1);
    assert_eq!(stats.other, 1, "skipped counts under other");

    // success_rate = 6 / (6 + 2) = 0.75
    let sr = stats.success_rate.expect("rate populated when success+failure > 0");
    assert!((sr - 0.75).abs() < 1e-9, "got {sr}");

    // Duration sample is the 9 runs with strictly-positive
    // duration (the skipped run with delta=0 is excluded).
    assert_eq!(stats.duration_sample_n, 9);
    // Sorted durations: [60, 60, 90, 90, 120, 180, 240, 300, 600]
    // percentile_cont(0.5) on 9 sorted values → element at
    // 0.5 * (9-1) = 4.0 → 120.
    let p50 = stats.duration_seconds.p50.expect("p50 populated at n>=5");
    assert!((p50 - 120.0).abs() < 1e-9, "got {p50}");

    // Empty window → zero counts, success_rate=None,
    // duration_seconds all None.
    let empty = s
        .ci_stats_for_repo(repo.id, t0 + Duration::days(30), t0 + Duration::days(40))
        .await
        .unwrap();
    assert_eq!(empty.total_runs, 0);
    assert!(empty.success_rate.is_none());
    assert_eq!(empty.duration_sample_n, 0);
    assert!(empty.duration_seconds.p50.is_none());
}

/// `activity_heatmap_for_repo` produces a dense 168-cell
/// `(dow, hour)` grid in the requested timezone, with zero
/// counts for empty buckets, and respects repo / window filters.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn activity_heatmap_for_repo_dense_grid_and_tz_shift() {
    let f = fixture().await;
    let s = f.store();
    let org = seed_org(s, 1, "octo").await;
    let repo = seed_repo(s, &org, 100, "hello").await;
    let other_repo = seed_repo(s, &org, 101, "other").await;

    // Pick a UTC instant that lands on different (dow, hour)
    // cells in UTC vs. America/Los_Angeles so the tz shift is
    // observable. 2025-05-12 (Mon) 04:00 UTC = 2025-05-11 (Sun)
    // 21:00 PDT (UTC-7). ISO dow: Mon=0, Sun=6.
    let utc_mon_4am = Utc.with_ymd_and_hms(2025, 5, 12, 4, 0, 0).unwrap();

    let mk = |ts: chrono::DateTime<Utc>, repo_id: Uuid, ext: &str| ActivityEvent {
        id: Uuid::new_v4(),
        org_id: org.id,
        repo_id,
        kind: EventKind::Commit,
        ts,
        external_id: ext.into(),
        payload: json!({}),
    };

    // 3 events in the same target bucket, plus 1 event one hour
    // later (different bucket), plus noise that must NOT count:
    //   * out-of-window event
    //   * other-repo event
    for i in 0..3 {
        s.record_event(&mk(utc_mon_4am, repo.id, &format!("a-{i}"))).await.unwrap();
    }
    s.record_event(&mk(utc_mon_4am + Duration::hours(1), repo.id, "b"))
        .await
        .unwrap();
    s.record_event(&mk(utc_mon_4am - Duration::days(60), repo.id, "old"))
        .await
        .unwrap();
    s.record_event(&mk(utc_mon_4am, other_repo.id, "wrong-repo"))
        .await
        .unwrap();

    let since = utc_mon_4am - Duration::days(1);
    let until = utc_mon_4am + Duration::days(1);

    // --- UTC view: 3 events at (Mon=0, 04), 1 at (Mon=0, 05).
    let utc = s
        .activity_heatmap_for_repo(repo.id, since, until, "UTC")
        .await
        .unwrap();
    assert_eq!(utc.timezone, "UTC");
    assert_eq!(utc.buckets.len(), 168, "grid must be dense");
    assert_eq!(utc.total, 4);

    let cell = |hm: &dp_domain::RepoActivityHeatmap, dow: i16, hour: i16| -> i64 {
        hm.buckets
            .iter()
            .find(|b| b.dow == dow && b.hour == hour)
            .expect("dense grid contains every (dow, hour)")
            .count
    };
    assert_eq!(cell(&utc, 0, 4), 3);
    assert_eq!(cell(&utc, 0, 5), 1);
    // Spot-check a few unrelated cells are zero (not missing).
    assert_eq!(cell(&utc, 6, 23), 0);
    assert_eq!(cell(&utc, 3, 12), 0);

    // --- PDT view: same UTC instant shifts to Sunday 21:00.
    let la = s
        .activity_heatmap_for_repo(repo.id, since, until, "America/Los_Angeles")
        .await
        .unwrap();
    assert_eq!(la.total, 4);
    assert_eq!(cell(&la, 6, 21), 3, "Sun 21:00 PDT");
    assert_eq!(cell(&la, 6, 22), 1, "Sun 22:00 PDT");
    // The UTC bucket must now be empty in the PDT view.
    assert_eq!(cell(&la, 0, 4), 0);

    // --- Empty window: still a dense grid, total = 0.
    let empty = s
        .activity_heatmap_for_repo(
            repo.id,
            utc_mon_4am + Duration::days(30),
            utc_mon_4am + Duration::days(40),
            "UTC",
        )
        .await
        .unwrap();
    assert_eq!(empty.total, 0);
    assert_eq!(empty.buckets.len(), 168);
    assert!(empty.buckets.iter().all(|b| b.count == 0));
}

/// `review_velocity_for_repo` derives time-to-merge from the
/// `pull_request_merged` event payload (`merged_at -
/// created_at`), applies the §15.9 sample-size guard, and
/// excludes clock-skew negatives, wrong kind, wrong repo, and
/// out-of-window events.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn review_velocity_for_repo_time_to_merge_and_guards() {
    let f = fixture().await;
    let s = f.store();
    let org = seed_org(s, 1, "octo").await;
    let repo = seed_repo(s, &org, 100, "hello").await;
    let other_repo = seed_repo(s, &org, 101, "other").await;

    let t0 = Utc.with_ymd_and_hms(2025, 5, 10, 12, 0, 0).unwrap();
    // Builds a PullRequestMerged event whose ts is the merge
    // moment and whose payload carries the open / merge
    // timestamps the SQL aggregator reads.
    let mk_merged = |ts: chrono::DateTime<Utc>,
                     repo_id: Uuid,
                     ext: &str,
                     ttm_secs: i64|
     -> ActivityEvent {
        let created = ts - Duration::seconds(ttm_secs);
        ActivityEvent {
            id: Uuid::new_v4(),
            org_id: org.id,
            repo_id,
            kind: EventKind::PullRequestMerged,
            ts,
            external_id: ext.into(),
            payload: json!({
                "created_at": created.to_rfc3339(),
                "merged_at":  ts.to_rfc3339(),
            }),
        }
    };

    // --- n=3 first: percentiles must be masked.
    for (i, ttm) in [3600_i64, 7200, 14_400].iter().enumerate() {
        s.record_event(&mk_merged(t0 + Duration::minutes(i as i64), repo.id, &format!("pr-small-{i}"), *ttm))
            .await
            .unwrap();
    }
    let since = t0 - Duration::days(1);
    let until = t0 + Duration::days(1);
    let small = s.review_velocity_for_repo(repo.id, since, until).await.unwrap();
    assert_eq!(small.sample_n, 3);
    assert!(small.time_to_merge_seconds.p50.is_none(), "n<5 must mask");

    // --- Top up to n=5 (+ noise). Sorted: [3600, 7200, 14400,
    // 28800, 86400]. percentile_cont(0.5) on 5 sorted values
    // picks the element at 0.5 * (5-1) = 2.0 → 14400.
    s.record_event(&mk_merged(t0 + Duration::hours(1), repo.id, "pr-big-1", 28_800))
        .await
        .unwrap();
    s.record_event(&mk_merged(t0 + Duration::hours(2), repo.id, "pr-big-2", 86_400))
        .await
        .unwrap();

    // Noise: clock-skew negative (excluded by `ttm_s > 0`).
    let skew_ts = t0 + Duration::hours(3);
    s.record_event(&ActivityEvent {
        id: Uuid::new_v4(),
        org_id: org.id,
        repo_id: repo.id,
        kind: EventKind::PullRequestMerged,
        ts: skew_ts,
        external_id: "pr-skew".into(),
        payload: json!({
            "created_at": (skew_ts + Duration::seconds(5)).to_rfc3339(),
            "merged_at":  skew_ts.to_rfc3339(),
        }),
    })
    .await
    .unwrap();
    // Noise: closed-not-merged (wrong kind).
    s.record_event(&ActivityEvent {
        id: Uuid::new_v4(),
        org_id: org.id,
        repo_id: repo.id,
        kind: EventKind::PullRequestClosed,
        ts: t0,
        external_id: "pr-abandoned".into(),
        payload: json!({
            "created_at": (t0 - Duration::days(7)).to_rfc3339(),
            "merged_at":  t0.to_rfc3339(),
        }),
    })
    .await
    .unwrap();
    // Noise: wrong repo.
    s.record_event(&mk_merged(t0, other_repo.id, "pr-wrong-repo", 999_999))
        .await
        .unwrap();
    // Noise: out of window (must not count).
    s.record_event(&mk_merged(t0 - Duration::days(10), repo.id, "pr-old", 999))
        .await
        .unwrap();

    let v = s.review_velocity_for_repo(repo.id, since, until).await.unwrap();
    assert_eq!(v.sample_n, 5, "5 valid in-window merged PRs");
    let p50 = v.time_to_merge_seconds.p50.expect("populated at n>=5");
    assert!((p50 - 14_400.0).abs() < 1e-6, "got {p50}");

    // Empty window.
    let empty = s
        .review_velocity_for_repo(
            repo.id,
            t0 + Duration::days(30),
            t0 + Duration::days(40),
        )
        .await
        .unwrap();
    assert_eq!(empty.sample_n, 0);
    assert!(empty.time_to_merge_seconds.p50.is_none());
}

/// `contributor_diversity_for_repo` counts `(merged-PR, author)`
/// pairs, derives top-1 / top-3 concentration shares, and
/// applies the §15.9 mask. Reviewers, non-merged PRs, wrong-repo
/// events, and out-of-window events must NOT count.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn contributor_diversity_for_repo_counts_authors_and_shares() {
    let f = fixture().await;
    let s = f.store();
    let org = seed_org(s, 1, "octo").await;
    let repo = seed_repo(s, &org, 100, "hello").await;
    let other_repo = seed_repo(s, &org, 101, "other").await;

    // Four authors so the top-1 vs top-3 distinction is visible.
    let alice = seed_user(s, 2001, "alice").await;
    let bob = seed_user(s, 2002, "bob").await;
    let carol = seed_user(s, 2003, "carol").await;
    let dave = seed_user(s, 2004, "dave").await;

    let t0 = Utc.with_ymd_and_hms(2025, 5, 10, 12, 0, 0).unwrap();

    // Helper: record a merged-PR event and attach a single
    // author. Returns the persisted event so the test can stitch
    // co-authors on top.
    async fn merged_with_author(
        s: &PgStore,
        org_id: Uuid,
        repo_id: Uuid,
        ts: chrono::DateTime<Utc>,
        ext: &str,
        author: Uuid,
    ) -> ActivityEvent {
        let ev = s
            .record_event(&ActivityEvent {
                id: Uuid::new_v4(),
                org_id,
                repo_id,
                kind: EventKind::PullRequestMerged,
                ts,
                external_id: ext.into(),
                payload: json!({}),
            })
            .await
            .unwrap();
        s.add_event_actors(&[EventActor {
            event_id: ev.id,
            user_id: author,
            role: ActorRole::Author,
        }])
        .await
        .unwrap();
        ev
    }

    // --- Phase 1: n=3 — guard must mask the share fields.
    for i in 0..3 {
        merged_with_author(
            s,
            org.id,
            repo.id,
            t0 + Duration::minutes(i),
            &format!("small-{i}"),
            alice.id,
        )
        .await;
    }

    let since = t0 - Duration::days(1);
    let until = t0 + Duration::days(1);
    let small = s
        .contributor_diversity_for_repo(repo.id, since, until)
        .await
        .unwrap();
    assert_eq!(small.sample_n, 3);
    assert_eq!(small.distinct_authors, 1);
    assert!(small.top1_share.is_none(), "n<5 must mask");
    assert!(small.top3_share.is_none());

    // --- Phase 2: top up to a realistic shape.
    // Distribution after this phase (author => count):
    //   alice: 5 (3 from phase 1 + 2 here)
    //   bob:   3
    //   carol: 1
    //   dave:  1   ← plus 1 co-author share on a bob-led PR
    // sample_n = 5 + 3 + 1 + 1 + 1 = 11
    // top1 = 5/11, top3 = (5+3+1)/11 = 9/11
    for i in 0..2 {
        merged_with_author(s, org.id, repo.id, t0 + Duration::hours(1 + i as i64), &format!("a-{i}"), alice.id).await;
    }
    let bob_evs = [
        merged_with_author(s, org.id, repo.id, t0 + Duration::hours(3), "b-0", bob.id).await,
        merged_with_author(s, org.id, repo.id, t0 + Duration::hours(4), "b-1", bob.id).await,
        merged_with_author(s, org.id, repo.id, t0 + Duration::hours(5), "b-2", bob.id).await,
    ];
    merged_with_author(s, org.id, repo.id, t0 + Duration::hours(6), "c-0", carol.id).await;
    merged_with_author(s, org.id, repo.id, t0 + Duration::hours(7), "d-0", dave.id).await;

    // Co-author: add dave as a second author on bob's PR. The
    // aggregate counts pairs, so this adds 1 to dave's tally
    // (and to sample_n) without removing one from bob.
    s.add_event_actors(&[EventActor {
        event_id: bob_evs[0].id,
        user_id: dave.id,
        role: ActorRole::Author,
    }])
    .await
    .unwrap();

    // --- Noise that must NOT count toward the aggregate:
    //   * a Reviewer role (wrong role — we filter on 'author')
    //   * a closed-but-not-merged PR (wrong kind)
    //   * the same login authoring on a different repo
    //   * an out-of-window merged PR
    let review_ev = s
        .record_event(&ActivityEvent {
            id: Uuid::new_v4(),
            org_id: org.id,
            repo_id: repo.id,
            kind: EventKind::Review,
            ts: t0 + Duration::hours(8),
            external_id: "noise-review".into(),
            payload: json!({}),
        })
        .await
        .unwrap();
    s.add_event_actors(&[EventActor {
        event_id: review_ev.id,
        user_id: carol.id,
        role: ActorRole::Reviewer,
    }])
    .await
    .unwrap();

    let closed_ev = s
        .record_event(&ActivityEvent {
            id: Uuid::new_v4(),
            org_id: org.id,
            repo_id: repo.id,
            kind: EventKind::PullRequestClosed,
            ts: t0 + Duration::hours(9),
            external_id: "noise-closed".into(),
            payload: json!({}),
        })
        .await
        .unwrap();
    s.add_event_actors(&[EventActor {
        event_id: closed_ev.id,
        user_id: carol.id,
        role: ActorRole::Author,
    }])
    .await
    .unwrap();

    merged_with_author(s, org.id, other_repo.id, t0, "wrong-repo", alice.id).await;
    merged_with_author(s, org.id, repo.id, t0 - Duration::days(10), "old", alice.id).await;

    let v = s
        .contributor_diversity_for_repo(repo.id, since, until)
        .await
        .unwrap();
    assert_eq!(v.sample_n, 11);
    assert_eq!(v.distinct_authors, 4);
    let t1 = v.top1_share.expect("populated at n>=5");
    let t3 = v.top3_share.expect("populated at n>=5");
    assert!((t1 - 5.0 / 11.0).abs() < 1e-9, "got top1={t1}");
    assert!((t3 - 9.0 / 11.0).abs() < 1e-9, "got top3={t3}");

    // --- Empty window: all zeros, share masked to None.
    let empty = s
        .contributor_diversity_for_repo(repo.id, t0 + Duration::days(30), t0 + Duration::days(40))
        .await
        .unwrap();
    assert_eq!(empty.sample_n, 0);
    assert_eq!(empty.distinct_authors, 0);
    assert!(empty.top1_share.is_none());
    assert!(empty.top3_share.is_none());
}

/// `put_cursor` / `get_cursor` must treat `repo_id IS NULL` as a
/// distinct cursor (org-scoped resources: members, teams). Relies on
/// PG15 `NULLS NOT DISTINCT` on the cursor's unique constraint —
/// the schema decision called out in `0001_init.sql`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn cursor_roundtrip_with_and_without_repo() {
    let f = fixture().await;
    let s = f.store();

    let org = seed_org(s, 1, "org-a").await;
    let repo = seed_repo(s, &org, 10, "dev-pulse").await;

    // Use a whole-second timestamp: Postgres `TIMESTAMPTZ` is
    // microsecond-precision but `Utc::now()` is nanosecond, so a
    // round-trip via PG truncates and a literal struct-equality
    // assertion fails. Whole-second values are exactly representable
    // in both, which keeps the assert honest about *what* round-trips
    // (the value) rather than *how* PG stores it.
    let now = Utc.with_ymd_and_hms(2025, 5, 19, 10, 0, 0).unwrap();
    // Org-scoped cursor (members) — repo_id is NULL.
    let members_cursor = FetchCursor {
        org_id: org.id,
        repo_id: None,
        resource_kind: ResourceKind::Members,
        since: Some(now - Duration::days(7)),
        etag: Some("W/\"abc\"".into()),
        last_event_id: None,
        updated_at: now,
    };
    s.put_cursor(&members_cursor).await.unwrap();

    // Repo-scoped cursor (commits) — same org, same NOW, distinct
    // row because resource_kind differs and repo_id is concrete.
    let commits_cursor = FetchCursor {
        org_id: org.id,
        repo_id: Some(repo.id),
        resource_kind: ResourceKind::Commits,
        since: None,
        etag: None,
        last_event_id: Some("evt_99".into()),
        updated_at: now,
    };
    s.put_cursor(&commits_cursor).await.unwrap();

    let got_members = s
        .get_cursor(org.id, None, ResourceKind::Members)
        .await
        .unwrap();
    assert_eq!(got_members, members_cursor);

    let got_commits = s
        .get_cursor(org.id, Some(repo.id), ResourceKind::Commits)
        .await
        .unwrap();
    assert_eq!(got_commits, commits_cursor);

    // Replay with a fresher timestamp — `put_cursor` upserts on the
    // unique constraint, so we get exactly one row back, not a
    // duplicate.
    let advanced = FetchCursor {
        updated_at: now + Duration::minutes(5),
        last_event_id: Some("evt_100".into()),
        ..commits_cursor.clone()
    };
    s.put_cursor(&advanced).await.unwrap();
    let after = s
        .get_cursor(org.id, Some(repo.id), ResourceKind::Commits)
        .await
        .unwrap();
    assert_eq!(after.last_event_id.as_deref(), Some("evt_100"));

    // Unknown cursor → NotFound (the report layer relies on this
    // to seed an initial cursor).
    let miss = s
        .get_cursor(org.id, Some(repo.id), ResourceKind::Releases)
        .await
        .unwrap_err();
    assert!(matches!(miss, StoreError::NotFound { .. }));
}

/// Webhook inbox path: enqueue → duplicate `delivery_id` surfaces
/// `Conflict` (so the receiver can return 200 OK on replays), then
/// `claim_webhooks` drains FIFO and `mark_*` flips the row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn webhook_inbox_enqueue_claim_mark() {
    let f = fixture().await;
    let s = f.store();

    let t0 = Utc.with_ymd_and_hms(2025, 5, 1, 0, 0, 0).unwrap();
    let d1 = WebhookDelivery {
        id: Uuid::new_v4(),
        delivery_id: "delivery-1".into(),
        event: "pull_request".into(),
        payload: json!({"action": "opened"}),
        received_at: t0,
        processed_at: None,
        error: None,
    };
    let d2 = WebhookDelivery {
        id: Uuid::new_v4(),
        delivery_id: "delivery-2".into(),
        event: "push".into(),
        payload: json!({"ref": "refs/heads/main"}),
        received_at: t0 + Duration::seconds(1),
        processed_at: None,
        error: None,
    };

    s.enqueue_webhook(&d1).await.unwrap();
    s.enqueue_webhook(&d2).await.unwrap();

    // Replay of d1 (same delivery_id, fresh row id) → Conflict.
    let replay = WebhookDelivery {
        id: Uuid::new_v4(),
        ..d1.clone()
    };
    let dup = s.enqueue_webhook(&replay).await.unwrap_err();
    assert!(
        matches!(dup, StoreError::Conflict(_)),
        "expected Conflict on duplicate delivery_id, got {dup:?}"
    );

    // FIFO drain: d1 (older `received_at`) comes first.
    let claimed = s.claim_webhooks(10).await.unwrap();
    assert_eq!(claimed.len(), 2);
    assert_eq!(claimed[0].delivery_id, "delivery-1");
    assert_eq!(claimed[1].delivery_id, "delivery-2");

    // Happy path on d1, failure recorded on d2.
    s.mark_webhook_processed(claimed[0].id).await.unwrap();
    s.mark_webhook_failed(claimed[1].id, "boom: parse error")
        .await
        .unwrap();

    // d1 is no longer claimable (processed_at set); d2 is — the
    // partial index `WHERE processed_at IS NULL` still sees it.
    let again = s.claim_webhooks(10).await.unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].delivery_id, "delivery-2");
    assert_eq!(again[0].error.as_deref(), Some("boom: parse error"));

    // Marking an unknown id → NotFound.
    let nope = s
        .mark_webhook_processed(Uuid::new_v4())
        .await
        .unwrap_err();
    assert!(matches!(nope, StoreError::NotFound { .. }));
}

/// Events + multi-actor attribution + the windowed report read.
/// Verifies the (event, actor) join, the four filter dimensions
/// (org, repo, user, role), and the window bounds (`[start, end)`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn events_actors_and_window_filters() {
    let f = fixture().await;
    let s = f.store();

    let org_a = seed_org(s, 1, "org-a").await;
    let org_b = seed_org(s, 2, "org-b").await;
    let repo_a = seed_repo(s, &org_a, 10, "alpha").await;
    let repo_b = seed_repo(s, &org_b, 20, "beta").await;
    let alice = seed_user(s, 100, "alice").await;
    let bob = seed_user(s, 200, "bob").await;

    // Three events spread across the window.
    let t0 = Utc.with_ymd_and_hms(2025, 5, 10, 12, 0, 0).unwrap();
    let e_in = ActivityEvent {
        id: Uuid::new_v4(),
        org_id: org_a.id,
        repo_id: repo_a.id,
        kind: EventKind::PullRequestMerged,
        ts: t0,
        external_id: "PR_in".into(),
        payload: json!({"number": 1}),
    };
    let e_in_other = ActivityEvent {
        id: Uuid::new_v4(),
        org_id: org_b.id,
        repo_id: repo_b.id,
        kind: EventKind::Review,
        ts: t0 + Duration::hours(1),
        external_id: "RV_in".into(),
        payload: json!({}),
    };
    let e_out = ActivityEvent {
        id: Uuid::new_v4(),
        org_id: org_a.id,
        repo_id: repo_a.id,
        kind: EventKind::Commit,
        ts: t0 - Duration::days(2),
        external_id: "C_out".into(),
        payload: json!({}),
    };
    for e in [&e_in, &e_in_other, &e_out] {
        s.record_event(e).await.unwrap();
    }

    // Idempotency on (kind, external_id): re-recording the same
    // event returns a row with the same id.
    let replay = s.record_event(&e_in).await.unwrap();
    assert_eq!(replay.id, e_in.id);

    // Multi-actor: e_in has Alice as author + Bob as merger;
    // e_in_other has Bob as reviewer; e_out has Alice as author.
    s.add_event_actors(&[
        EventActor {
            event_id: e_in.id,
            user_id: alice.id,
            role: ActorRole::Author,
        },
        EventActor {
            event_id: e_in.id,
            user_id: bob.id,
            role: ActorRole::Merger,
        },
        EventActor {
            event_id: e_in_other.id,
            user_id: bob.id,
            role: ActorRole::Reviewer,
        },
        EventActor {
            event_id: e_out.id,
            user_id: alice.id,
            role: ActorRole::Author,
        },
    ])
    .await
    .unwrap();

    // Idempotency on actors: replay must be a no-op (ON CONFLICT
    // DO NOTHING).
    s.add_event_actors(&[EventActor {
        event_id: e_in.id,
        user_id: alice.id,
        role: ActorRole::Author,
    }])
    .await
    .unwrap();

    let window = Window {
        start: t0 - Duration::hours(1),
        end: t0 + Duration::days(1),
        label: "test".into(),
        tz: "UTC".into(),
        anchor: WindowAnchor::Utc,
    };

    // No filters — both in-window events surface, the out-of-window
    // one does not.
    let all = s
        .list_event_actor_rows_in_window(&window, &[], &[], &[], &[])
        .await
        .unwrap();
    assert_eq!(all.len(), 3, "alice author + bob merger + bob reviewer");

    // Filter by org → only org_a's events.
    let by_org = s
        .list_event_actor_rows_in_window(&window, &[org_a.id], &[], &[], &[])
        .await
        .unwrap();
    assert_eq!(by_org.len(), 2);
    assert!(by_org.iter().all(|r| r.org_id == org_a.id));

    // Filter by user (Bob) → his two rows across both orgs.
    let by_user = s
        .list_event_actor_rows_in_window(&window, &[], &[], &[bob.id], &[])
        .await
        .unwrap();
    assert_eq!(by_user.len(), 2);
    assert!(by_user.iter().all(|r| r.user_id == bob.id));

    // Filter by role (Author) → only Alice's row in e_in (e_out
    // is out of window).
    let by_role = s
        .list_event_actor_rows_in_window(&window, &[], &[], &[], &[ActorRole::Author])
        .await
        .unwrap();
    assert_eq!(by_role.len(), 1);
    assert_eq!(by_role[0].user_id, alice.id);
    assert_eq!(by_role[0].role, ActorRole::Author);

    // Conjunctive filter: repo_a × Bob → just Bob-merger on e_in.
    let conj = s
        .list_event_actor_rows_in_window(&window, &[], &[repo_a.id], &[bob.id], &[])
        .await
        .unwrap();
    assert_eq!(conj.len(), 1);
    assert_eq!(conj[0].role, ActorRole::Merger);
    assert_eq!(conj[0].repo_id, repo_a.id);
}

/// `start_fetch_run` → `finish_fetch_run` writes the run log and
/// `list_recent_fetch_runs` returns newest-first.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn fetch_run_lifecycle() {
    let f = fixture().await;
    let s = f.store();

    let r1 = s.start_fetch_run(FetchRunKind::Backfill).await.unwrap();
    s.finish_fetch_run(r1, 100, 2, true).await.unwrap();

    let r2 = s.start_fetch_run(FetchRunKind::Reconciler).await.unwrap();
    s.finish_fetch_run(r2, 5, 0, false).await.unwrap();

    let runs = s.list_recent_fetch_runs(10).await.unwrap();
    assert_eq!(runs.len(), 2);
    // Newest first.
    assert_eq!(runs[0].id, r2);
    assert_eq!(runs[0].kind, FetchRunKind::Reconciler);
    assert_eq!(runs[0].items, 5);
    assert!(!runs[0].partial);

    assert_eq!(runs[1].id, r1);
    assert_eq!(runs[1].items, 100);
    assert_eq!(runs[1].errors, 2);
    assert!(runs[1].partial);

    // finish on an unknown id is NotFound.
    let nope = s
        .finish_fetch_run(Uuid::new_v4(), 0, 0, false)
        .await
        .unwrap_err();
    assert!(matches!(nope, StoreError::NotFound { .. }));
}

/// `data_as_of` reports the latest finished `webhook_worker` /
/// `reconciler` runs and groups cursor `updated_at` per org.
///
/// Covers the three things SCOPE §11.7 needs visible on every report
/// response (TODO §0.3):
///
/// * unfinished runs are excluded from the headline `MAX(finished)`;
/// * the latest finished run *of each kind* wins (older finishes are
///   ignored even if their `started` is newer);
/// * per-org freshness collapses multiple cursors (different
///   resource_kinds / repos) into the org's `MAX(updated_at)`;
/// * orgs with no cursor rows are absent from `per_org` (the UI
///   treats absence as "pending first reconcile").
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn data_as_of_snapshots_freshness_headline_and_per_org() {
    let f = fixture().await;
    let s = f.store();

    // Empty state: every field is None / empty.
    let empty = s.data_as_of().await.unwrap();
    assert!(empty.webhook_latest.is_none());
    assert!(empty.reconciler_latest.is_none());
    assert!(empty.per_org.is_empty());

    // Two webhook runs: only the second one finishes, so it wins.
    let wh_started_first = s
        .start_fetch_run(FetchRunKind::WebhookWorker)
        .await
        .unwrap();
    let wh_finished_first = s
        .start_fetch_run(FetchRunKind::WebhookWorker)
        .await
        .unwrap();
    s.finish_fetch_run(wh_finished_first, 3, 0, false)
        .await
        .unwrap();

    // One reconciler tick, finished.
    let rec = s.start_fetch_run(FetchRunKind::Reconciler).await.unwrap();
    s.finish_fetch_run(rec, 7, 0, false).await.unwrap();

    // Two orgs with cursors at different times. Org A has two
    // cursors (different resource kinds) — its `per_org` value
    // should be the later of the two. Org B has one.
    let org_a = seed_org(s, 1, "org-a").await;
    let org_b = seed_org(s, 2, "org-b").await;
    // Org C exists but has no cursor — must be absent from per_org.
    let org_c = seed_org(s, 3, "org-c").await;
    let repo_a = seed_repo(s, &org_a, 10, "alpha").await;

    let t_old = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).single().unwrap();
    let t_mid = Utc.with_ymd_and_hms(2025, 1, 2, 0, 0, 0).single().unwrap();
    let t_new = Utc.with_ymd_and_hms(2025, 1, 3, 0, 0, 0).single().unwrap();

    s.put_cursor(&FetchCursor {
        org_id: org_a.id,
        repo_id: Some(repo_a.id),
        resource_kind: ResourceKind::PullRequests,
        since: None,
        etag: None,
        last_event_id: None,
        updated_at: t_old,
    })
    .await
    .unwrap();
    s.put_cursor(&FetchCursor {
        org_id: org_a.id,
        repo_id: None,
        resource_kind: ResourceKind::Members,
        since: None,
        etag: None,
        last_event_id: None,
        updated_at: t_new,
    })
    .await
    .unwrap();
    s.put_cursor(&FetchCursor {
        org_id: org_b.id,
        repo_id: None,
        resource_kind: ResourceKind::Members,
        since: None,
        etag: None,
        last_event_id: None,
        updated_at: t_mid,
    })
    .await
    .unwrap();

    let snap = s.data_as_of().await.unwrap();

    // Headlines: only finished runs count, and they pick the latest
    // per kind.
    assert!(snap.webhook_latest.is_some(), "webhook tick finished");
    assert!(snap.reconciler_latest.is_some(), "reconciler tick finished");
    // The unfinished webhook run must NOT mask the finished one.
    let _ = wh_started_first; // keep id alive in scope for clarity

    // Per-org: org_a picks the max across its cursors; org_b matches
    // its single cursor; org_c is absent.
    assert_eq!(snap.per_org.get(&org_a.id).copied(), Some(t_new));
    assert_eq!(snap.per_org.get(&org_b.id).copied(), Some(t_mid));
    assert!(
        !snap.per_org.contains_key(&org_c.id),
        "orgs with no cursors must be absent (treated as pending)",
    );
}

/// `pseudonymise_user` rewrites identifying columns + sets
/// `deleted_at`, but keeps the row id so historical events still
/// resolve their actor. Idempotent: a second call doesn't disturb
/// the existing `deleted_at`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn pseudonymise_user_clears_pii_keeps_id() {
    let f = fixture().await;
    let s = f.store();

    let org = seed_org(s, 1, "org-a").await;
    let repo = seed_repo(s, &org, 10, "dev-pulse").await;
    let user = seed_user(s, 42, "carol").await;

    // Park one event so we can prove the FK still resolves after
    // pseudonymisation.
    let event = ActivityEvent {
        id: Uuid::new_v4(),
        org_id: org.id,
        repo_id: repo.id,
        kind: EventKind::Commit,
        ts: Utc::now(),
        external_id: "C_1".into(),
        payload: json!({}),
    };
    s.record_event(&event).await.unwrap();
    s.add_event_actors(&[EventActor {
        event_id: event.id,
        user_id: user.id,
        role: ActorRole::Author,
    }])
    .await
    .unwrap();

    s.pseudonymise_user(user.id).await.unwrap();

    let after = s.get_user(user.id).await.unwrap();
    assert_eq!(after.id, user.id, "row id stable");
    assert!(
        after.login.starts_with("deleted-user-"),
        "login rewritten, got {:?}",
        after.login
    );
    assert!(after.email.is_none());
    assert!(after.name.is_none());
    let first_deleted_at: DateTime<Utc> = after.deleted_at.expect("deleted_at set");

    // Soft-deleted rows are hidden from list_users.
    assert!(s.list_users().await.unwrap().is_empty());

    // Historical event still has the user attached via the
    // foreign key — the report read still finds it.
    let window = Window {
        start: event.ts - Duration::hours(1),
        end: event.ts + Duration::hours(1),
        label: "test".into(),
        tz: "UTC".into(),
        anchor: WindowAnchor::Utc,
    };
    let rows = s
        .list_event_actor_rows_in_window(&window, &[], &[], &[user.id], &[])
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].user_id, user.id);

    // Idempotent second call: deleted_at must not be reset.
    s.pseudonymise_user(user.id).await.unwrap();
    let again = s.get_user(user.id).await.unwrap();
    assert_eq!(
        again.deleted_at, Some(first_deleted_at),
        "deleted_at preserved across re-pseudonymisation"
    );

    // Pseudonymising an unknown user → NotFound.
    let miss = s.pseudonymise_user(Uuid::new_v4()).await.unwrap_err();
    assert!(matches!(miss, StoreError::NotFound { .. }));
}

/// `upsert_team` is keyed on `(org_id, github_id)` so a slug rename
/// updates the same row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn upsert_team_dedupes_on_org_and_github_id() {
    let f = fixture().await;
    let s = f.store();

    let org = seed_org(s, 1, "org-a").await;
    let t1 = s
        .upsert_team(&Team {
            id: Uuid::new_v4(),
            org_id: org.id,
            github_id: 7,
            slug: "backend".into(),
            name: "Backend".into(),
        })
        .await
        .unwrap();
    let t2 = s
        .upsert_team(&Team {
            id: Uuid::new_v4(),
            org_id: org.id,
            github_id: 7,
            slug: "platform".into(),
            name: "Platform".into(),
        })
        .await
        .unwrap();
    assert_eq!(t1.id, t2.id, "team id stable on (org, github_id) replay");
    assert_eq!(t2.slug, "platform");
    assert_eq!(t2.name, "Platform");
}

// ---------- identities (users.md §4 Slice A) ----------------------

/// Backfill from migration 0019 attaches one primary identity per
/// existing dp-user; `list_identities_for_user` finds it; the
/// `find_user_by_github_user_id` reverse-lookup agrees with
/// `get_user_by_github_id`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn identities_backfill_visible_via_store() {
    use dp_domain::identity::VerifiedVia;

    let f = fixture().await;
    let s = f.store();

    // Seeding via upsert_user happens *after* migrations, so the
    // 0019 backfill SELECT saw an empty `dp_users`. We have to
    // exercise link_identity here for any rows to exist.
    let alice = seed_user(s, 1001, "alice").await;

    s.link_identity(&dp_domain::identity::UserIdentity {
        user_id: alice.id,
        github_user_id: 1001,
        github_login: "alice".into(),
        is_primary: true,
        linked_at: chrono::Utc::now(),
        verified_via: VerifiedVia::Oauth,
    })
    .await
    .unwrap();

    let rows = s.list_identities_for_user(alice.id).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].is_primary);
    assert_eq!(rows[0].github_login, "alice");
    assert_eq!(rows[0].verified_via, VerifiedVia::Oauth);

    let by_gh = s.find_user_by_github_user_id(1001).await.unwrap();
    assert_eq!(by_gh.unwrap().id, alice.id);

    assert!(s.find_user_by_github_user_id(9999).await.unwrap().is_none());
}

/// Linking a second identity keeps the first primary unless the
/// caller asks otherwise; `set_primary_identity` flips atomically.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn link_then_set_primary_flips_atomically() {
    use dp_domain::identity::{UserIdentity, VerifiedVia};

    let f = fixture().await;
    let s = f.store();
    let alice = seed_user(s, 2001, "alice").await;

    s.link_identity(&UserIdentity {
        user_id: alice.id,
        github_user_id: 2001,
        github_login: "alice".into(),
        is_primary: true,
        linked_at: chrono::Utc::now(),
        verified_via: VerifiedVia::Oauth,
    })
    .await
    .unwrap();

    // Second identity: caller passes is_primary = false → stays
    // secondary even though it's freshly linked.
    s.link_identity(&UserIdentity {
        user_id: alice.id,
        github_user_id: 2002,
        github_login: "alice-oncall".into(),
        is_primary: false,
        linked_at: chrono::Utc::now(),
        verified_via: VerifiedVia::Oauth,
    })
    .await
    .unwrap();

    let rows = s.list_identities_for_user(alice.id).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].github_user_id, 2001);
    assert!(rows[0].is_primary);
    assert!(!rows[1].is_primary);

    // Promote the oncall identity. The previous primary must drop
    // to FALSE in the same transaction.
    s.set_primary_identity(alice.id, 2002).await.unwrap();
    let rows = s.list_identities_for_user(alice.id).await.unwrap();
    let alice_primary = rows.iter().find(|r| r.is_primary).unwrap();
    assert_eq!(alice_primary.github_user_id, 2002);
    assert_eq!(rows.iter().filter(|r| r.is_primary).count(), 1);
}

/// Claim conflict: linking a github_user_id already owned by
/// another dp-user must surface `StoreError::Conflict` (the REST
/// layer maps this to HTTP 409 + IDENTITY_CLAIM_CONFLICT audit).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn link_identity_rejects_cross_user_claim() {
    use dp_domain::identity::{UserIdentity, VerifiedVia};

    let f = fixture().await;
    let s = f.store();
    let alice = seed_user(s, 3001, "alice").await;
    let bob = seed_user(s, 3002, "bob").await;

    s.link_identity(&UserIdentity {
        user_id: alice.id,
        github_user_id: 3001,
        github_login: "alice".into(),
        is_primary: true,
        linked_at: chrono::Utc::now(),
        verified_via: VerifiedVia::Oauth,
    })
    .await
    .unwrap();

    let err = s
        .link_identity(&UserIdentity {
            user_id: bob.id,
            github_user_id: 3001, // already claimed by alice
            github_login: "alice".into(),
            is_primary: false,
            linked_at: chrono::Utc::now(),
            verified_via: VerifiedVia::Oauth,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, StoreError::Conflict(_)), "got: {err:?}");
}

/// Unlink rules: cannot remove the last identity, cannot remove
/// the current primary. Removing a non-primary secondary works.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn unlink_identity_enforces_last_and_primary_rules() {
    use dp_domain::identity::{UserIdentity, VerifiedVia};

    let f = fixture().await;
    let s = f.store();
    let alice = seed_user(s, 4001, "alice").await;

    s.link_identity(&UserIdentity {
        user_id: alice.id,
        github_user_id: 4001,
        github_login: "alice".into(),
        is_primary: true,
        linked_at: chrono::Utc::now(),
        verified_via: VerifiedVia::Oauth,
    })
    .await
    .unwrap();

    // Last-identity rule.
    let err = s.unlink_identity(alice.id, 4001).await.unwrap_err();
    assert!(matches!(err, StoreError::Invalid(_)), "got: {err:?}");

    s.link_identity(&UserIdentity {
        user_id: alice.id,
        github_user_id: 4002,
        github_login: "alice-oncall".into(),
        is_primary: false,
        linked_at: chrono::Utc::now(),
        verified_via: VerifiedVia::Oauth,
    })
    .await
    .unwrap();

    // Primary rule.
    let err = s.unlink_identity(alice.id, 4001).await.unwrap_err();
    assert!(matches!(err, StoreError::Invalid(_)), "primary-rule, got: {err:?}");

    // Non-primary unlinks cleanly.
    s.unlink_identity(alice.id, 4002).await.unwrap();
    let rows = s.list_identities_for_user(alice.id).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].github_user_id, 4001);
}

/// OAuth `state` nonce: create, consume, double-consume is None,
/// expired sweep removes the row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn identity_link_pending_round_trip() {
    use dp_domain::identity::IdentityLinkPending;

    let f = fixture().await;
    let s = f.store();
    let alice = seed_user(s, 5001, "alice").await;

    let nonce = Uuid::new_v4();
    let now = chrono::Utc::now();
    s.create_identity_link_pending(&IdentityLinkPending {
        nonce,
        dp_user_id: alice.id,
        session_id: "sess-1".into(),
        created_at: now,
        expires_at: now + chrono::Duration::minutes(5),
    })
    .await
    .unwrap();

    let consumed = s.consume_identity_link_pending(nonce).await.unwrap();
    let row = consumed.expect("nonce should exist on first consume");
    assert_eq!(row.dp_user_id, alice.id);
    assert_eq!(row.session_id, "sess-1");

    // Second consume must be None — single-use.
    assert!(s.consume_identity_link_pending(nonce).await.unwrap().is_none());

    // Expired-sweep deletes only past-deadline rows.
    let nonce2 = Uuid::new_v4();
    s.create_identity_link_pending(&IdentityLinkPending {
        nonce: nonce2,
        dp_user_id: alice.id,
        session_id: "sess-2".into(),
        created_at: now - chrono::Duration::hours(1),
        expires_at: now - chrono::Duration::minutes(30),
    })
    .await
    .unwrap();
    let purged = s.purge_expired_identity_link_pending(now).await.unwrap();
    assert!(purged >= 1);
    assert!(s.consume_identity_link_pending(nonce2).await.unwrap().is_none());
}

// ---------- projects (linear-projects-v2.md slice A) ----------------

/// Seed an open issue under a repo via the GitHub upsert path. Used
/// by the project-membership tests so the FK to `dp_issues` is real.
async fn seed_issue(s: &PgStore, org: &Org, repo: &Repo, github_id: i64, number: i64) -> Uuid {
    let now = Utc::now();
    let (issue, _outcome) = s
        .upsert_issue_from_github(
            &IssueUpsert {
                org_id: org.id,
                repo_id: repo.id,
                github_id,
                github_node_id: None,
                number,
                title: format!("seed #{number}"),
                body: None,
                state: IssueState::Open,
                labels: vec![],
                assignees: vec![],
                milestone: None,
                author: None,
                state_reason: None,
                created_at: now,
                updated_at: now,
                closed_at: None,
            },
            Duration::seconds(60),
        )
        .await
        .unwrap();
    issue.id
}

/// Create / list / update / archive round-trip plus the partial-
/// unique name index (archived rows can reuse the name).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn projects_crud_round_trip() {
    let f = fixture().await;
    let s = f.store();

    let org = seed_org(s, 7000, "acme").await;
    let lead = seed_user(s, 7001, "lead").await;

    // Create.
    let p = s
        .create_project(&ProjectUpsert {
            org_id: org.id,
            name: "Rubix v2 launch".into(),
            description: Some("ship it".into()),
            lead_user_id: Some(lead.id),
            status: ProjectStatus::Active,
            start_at: None,
            due_at: None,
            created_by: Some(lead.id),
        })
        .await
        .unwrap();
    assert_eq!(p.status, ProjectStatus::Active);
    assert_eq!(p.version, 1);
    assert_eq!(p.issue_count, 0);

    // Duplicate (case-insensitive, same status) → Conflict via the
    // partial-unique index `dp_projects_org_name_unique`.
    let dup = s
        .create_project(&ProjectUpsert {
            org_id: org.id,
            name: "rubix v2 LAUNCH".into(),
            description: None,
            lead_user_id: None,
            status: ProjectStatus::Active,
            start_at: None,
            due_at: None,
            created_by: Some(lead.id),
        })
        .await
        .unwrap_err();
    assert!(matches!(dup, StoreError::Conflict(_)));

    // PATCH with the right version bumps `version` and persists
    // the new fields.
    let p2 = s
        .update_project(
            p.id,
            p.version,
            &ProjectUpsert {
                org_id: org.id, // ignored on update
                name: "Rubix v2 launch".into(),
                description: Some("ship it sooner".into()),
                lead_user_id: Some(lead.id),
                status: ProjectStatus::Backlog,
                start_at: None,
                due_at: None,
                created_by: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(p2.version, p.version + 1);
    assert_eq!(p2.status, ProjectStatus::Backlog);
    assert_eq!(p2.description.as_deref(), Some("ship it sooner"));

    // Stale `expected_version` → Conflict (not Backend).
    let stale = s
        .update_project(
            p.id,
            p.version, // already bumped
            &ProjectUpsert {
                org_id: org.id,
                name: "renamed".into(),
                description: None,
                lead_user_id: None,
                status: ProjectStatus::Active,
                start_at: None,
                due_at: None,
                created_by: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(stale, StoreError::Conflict(_)));

    // Filter list by org + status.
    let listed = s
        .list_projects(&ProjectListFilter {
            org_id: Some(org.id),
            status: Some(ProjectStatus::Backlog),
            q: None,
            limit: 50,
            offset: 0,
        })
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, p.id);
    assert_eq!(
        s.count_projects(&ProjectListFilter {
            org_id: Some(org.id),
            status: Some(ProjectStatus::Backlog),
            q: None,
            limit: 50,
            offset: 0
        })
        .await
        .unwrap(),
        1
    );

    // Archive bumps version + status.
    let archived = s.archive_project(p.id, p2.version).await.unwrap();
    assert_eq!(archived.status, ProjectStatus::Archived);
    assert_eq!(archived.version, p2.version + 1);

    // Re-archive is idempotent (no version bump).
    let again = s
        .archive_project(p.id, archived.version)
        .await
        .unwrap();
    assert_eq!(again.version, archived.version);

    // Now the original name can be reused — the partial index
    // excludes archived rows.
    let p3 = s
        .create_project(&ProjectUpsert {
            org_id: org.id,
            name: "Rubix v2 launch".into(),
            description: None,
            lead_user_id: None,
            status: ProjectStatus::Active,
            start_at: None,
            due_at: None,
            created_by: Some(lead.id),
        })
        .await
        .unwrap();
    assert_ne!(p3.id, p.id);
}

/// Bulk membership: per-row outcomes, the `UNIQUE (issue_id)` v1
/// rule, the cross-org rejection, and the denormalised counters.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn project_issue_membership_outcomes() {
    let f = fixture().await;
    let s = f.store();

    let org_a = seed_org(s, 7100, "org-a").await;
    let org_b = seed_org(s, 7101, "org-b").await;
    let repo_a = seed_repo(s, &org_a, 9100, "ra").await;
    let repo_b = seed_repo(s, &org_b, 9101, "rb").await;
    let actor = seed_user(s, 7102, "actor").await;

    let i1 = seed_issue(s, &org_a, &repo_a, 1, 1).await;
    let i2 = seed_issue(s, &org_a, &repo_a, 2, 2).await;
    let i_cross = seed_issue(s, &org_b, &repo_b, 3, 3).await;
    let bogus_issue = Uuid::new_v4();

    let project = s
        .create_project(&ProjectUpsert {
            org_id: org_a.id,
            name: "p".into(),
            description: None,
            lead_user_id: None,
            status: ProjectStatus::Active,
            start_at: None,
            due_at: None,
            created_by: Some(actor.id),
        })
        .await
        .unwrap();

    // First add: i1, i2 land; i_cross is `cross_org`; bogus is
    // `unknown_issue`.
    let outcome = s
        .add_issues_to_project(
            project.id,
            project.version,
            &[i1, i2, i_cross, bogus_issue],
            Some(actor.id),
        )
        .await
        .unwrap();
    assert_eq!(outcome.added.len(), 2);
    assert!(outcome.added.contains(&i1) && outcome.added.contains(&i2));
    assert_eq!(outcome.skipped.len(), 2);
    let reasons: Vec<&str> = outcome.skipped.iter().map(|sk| sk.reason.as_str()).collect();
    assert!(reasons.contains(&"cross_org"));
    assert!(reasons.contains(&"unknown_issue"));

    let after = s.get_project(project.id).await.unwrap().unwrap();
    assert_eq!(after.issue_count, 2);
    assert_eq!(after.closed_issue_count, 0);
    assert_eq!(after.version, project.version + 1);

    // Adding i1 again surfaces `already_in_project` with the
    // current project id filled in.
    let again = s
        .add_issues_to_project(after.id, after.version, &[i1], Some(actor.id))
        .await
        .unwrap();
    assert!(again.added.is_empty());
    assert_eq!(again.skipped.len(), 1);
    assert_eq!(again.skipped[0].reason, "already_in_project");
    assert_eq!(again.skipped[0].existing_project_id, Some(project.id));

    // Zero added → no version bump.
    let after2 = s.get_project(project.id).await.unwrap().unwrap();
    assert_eq!(after2.version, after.version);

    // Reverse lookup.
    let proj_for_i1 = s.get_project_for_issue(i1).await.unwrap().unwrap();
    assert_eq!(proj_for_i1.id, project.id);
    assert!(s.get_project_for_issue(i_cross).await.unwrap().is_none());

    // Listed ids.
    let ids = s.list_issue_ids_for_project(project.id).await.unwrap();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&i1) && ids.contains(&i2));

    // Stale CAS on bulk add → Conflict.
    let conflict = s
        .add_issues_to_project(project.id, project.version, &[i1], None)
        .await
        .unwrap_err();
    assert!(matches!(conflict, StoreError::Conflict(_)));

    // Remove i1 bumps version, decrements count.
    let removed = s
        .remove_issue_from_project(project.id, i1, after2.version)
        .await
        .unwrap();
    assert_eq!(removed.issue_count, 1);
    assert_eq!(removed.version, after2.version + 1);
    // Removing an issue that is not in the project → NotFound.
    let miss = s
        .remove_issue_from_project(project.id, bogus_issue, removed.version)
        .await
        .unwrap_err();
    assert!(matches!(miss, StoreError::NotFound { .. }));
}

// ---------- board links + items (linear-projects-v2.md slice B) ----

/// Round-trips the §7.3 board-link CRUD plus the §6.5 per-(link,
/// issue) mirror state. Covers: create with cached picker fields,
/// natural-key conflict on re-link, list ordering, item upsert on
/// success / failure, aggregate roll-up to the link row,
/// `refresh_board_link_cache` not clobbering set fields, cascade
/// delete cleaning up items, and the not-found delete path.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]
async fn board_link_crud_and_item_outcomes() {
    let f = fixture().await;
    let s = f.store();

    let org = seed_org(s, 7200, "bl-org").await;
    let repo = seed_repo(s, &org, 9200, "bl-repo").await;
    let actor = seed_user(s, 7201, "bl-actor").await;
    let issue_a = seed_issue(s, &org, &repo, 1, 1).await;
    let issue_b = seed_issue(s, &org, &repo, 2, 2).await;

    let project = s
        .create_project(&ProjectUpsert {
            org_id: org.id,
            name: "bl-proj".into(),
            description: None,
            lead_user_id: None,
            status: ProjectStatus::Active,
            start_at: None,
            due_at: None,
            created_by: Some(actor.id),
        })
        .await
        .unwrap();

    // Create with picker-cached fields → cached_at stamped.
    let link = s
        .create_board_link(&BoardLinkUpsert {
            project_id: project.id,
            github_board_node_id: "PVT_kw_roadmap".into(),
            github_board_title: Some("Rubix Roadmap".into()),
            github_board_url: Some("https://github.com/orgs/NubeIO/projects/12".into()),
            start_field_node_id: Some("PVF_start".into()),
            due_field_node_id: Some("PVF_due".into()),
            status_field_node_id: None,
            created_by: Some(actor.id),
        })
        .await
        .unwrap();
    assert_eq!(link.project_id, project.id);
    assert_eq!(link.github_board_title.as_deref(), Some("Rubix Roadmap"));
    assert!(link.github_board_cached_at.is_some());
    assert!(link.last_mirror_at.is_none());
    assert!(link.last_mirror_error.is_none());

    // Re-link the same board → 409 Conflict via the natural-key
    // UNIQUE constraint.
    let dup = s
        .create_board_link(&BoardLinkUpsert {
            project_id: project.id,
            github_board_node_id: "PVT_kw_roadmap".into(),
            github_board_title: None,
            github_board_url: None,
            start_field_node_id: None,
            due_field_node_id: None,
            status_field_node_id: None,
            created_by: Some(actor.id),
        })
        .await
        .unwrap_err();
    assert!(matches!(dup, StoreError::Conflict(_)));

    // Second link to a different board on the same project — the
    // §6.4 fan-out shape: a project may carry many links.
    let link2 = s
        .create_board_link(&BoardLinkUpsert {
            project_id: project.id,
            github_board_node_id: "PVT_kw_sprint".into(),
            github_board_title: Some("Eng Sprint".into()),
            github_board_url: None,
            start_field_node_id: None,
            due_field_node_id: Some("PVF_due_b".into()),
            status_field_node_id: None,
            created_by: Some(actor.id),
        })
        .await
        .unwrap();

    // list_board_links returns both, in created_at ASC order.
    let links = s.list_board_links(project.id).await.unwrap();
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].id, link.id);
    assert_eq!(links[1].id, link2.id);

    // Record a success against (link, issue_a) — item row stamped,
    // aggregate rolled up.
    s.record_board_item_result(
        link.id,
        issue_a,
        BoardItemMirrorOutcome::Success {
            item_node_id: "PVTI_card_a",
        },
    )
    .await
    .unwrap();
    let item = s.get_board_item(link.id, issue_a).await.unwrap().unwrap();
    assert_eq!(item.item_node_id, "PVTI_card_a");
    assert!(item.last_synced_at.is_some());
    assert!(item.last_error.is_none());
    let link_after = s.get_board_link(link.id).await.unwrap().unwrap();
    assert!(link_after.last_mirror_at.is_some());
    assert!(link_after.last_mirror_error.is_none());

    // Record a failure against (link, issue_b) — item row carries
    // the error, aggregate now reports the error.
    s.record_board_item_result(
        link.id,
        issue_b,
        BoardItemMirrorOutcome::Failure {
            error: "field not found",
        },
    )
    .await
    .unwrap();
    let fail_item = s.get_board_item(link.id, issue_b).await.unwrap().unwrap();
    assert_eq!(fail_item.last_error.as_deref(), Some("field not found"));
    assert!(fail_item.last_synced_at.is_none());
    let link_err = s.get_board_link(link.id).await.unwrap().unwrap();
    assert_eq!(
        link_err.last_mirror_error.as_deref(),
        Some("field not found")
    );

    // A subsequent success against issue_b clears its error,
    // overwrites the placeholder item_node_id, and clears the
    // aggregate error.
    s.record_board_item_result(
        link.id,
        issue_b,
        BoardItemMirrorOutcome::Success {
            item_node_id: "PVTI_card_b",
        },
    )
    .await
    .unwrap();
    let ok_item = s.get_board_item(link.id, issue_b).await.unwrap().unwrap();
    assert_eq!(ok_item.item_node_id, "PVTI_card_b");
    assert!(ok_item.last_error.is_none());
    let link_clear = s.get_board_link(link.id).await.unwrap().unwrap();
    assert!(link_clear.last_mirror_error.is_none());

    // list_board_items_for_issue returns one row per (link, issue).
    let items_a = s.list_board_items_for_issue(issue_a).await.unwrap();
    assert_eq!(items_a.len(), 1);
    assert_eq!(items_a[0].link_id, link.id);

    // refresh_board_link_cache: a title-only refresh does not
    // clobber a previously cached url.
    s.refresh_board_link_cache(link.id, Some("Rubix Roadmap v2"), None)
        .await
        .unwrap();
    let refreshed = s.get_board_link(link.id).await.unwrap().unwrap();
    assert_eq!(
        refreshed.github_board_title.as_deref(),
        Some("Rubix Roadmap v2")
    );
    assert_eq!(
        refreshed.github_board_url.as_deref(),
        Some("https://github.com/orgs/NubeIO/projects/12")
    );

    // Delete cascades to items.
    s.delete_board_link(link.id).await.unwrap();
    assert!(s.get_board_link(link.id).await.unwrap().is_none());
    assert!(s.get_board_item(link.id, issue_a).await.unwrap().is_none());
    assert!(s.list_board_items_for_issue(issue_a).await.unwrap().is_empty());

    // Re-delete → NotFound.
    let miss = s.delete_board_link(link.id).await.unwrap_err();
    assert!(matches!(miss, StoreError::NotFound { .. }));

    // The other link survives.
    let leftover = s.list_board_links(project.id).await.unwrap();
    assert_eq!(leftover.len(), 1);
    assert_eq!(leftover[0].id, link2.id);
}
