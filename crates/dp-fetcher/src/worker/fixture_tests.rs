//! Fixture-driven unit tests for SCOPE §6 actor-attribution edge
//! cases (TODO §Phase-2 stage 7).
//!
//! Each test loads a recorded GitHub webhook payload from
//! `crates/dp-fetcher/tests/fixtures/` via `include_str!` so the
//! fixture file is the *literal* on-disk contract — a human can
//! diff it against a live `gh api` capture. The tests pin five
//! attribution edge cases SCOPE §6 calls out by name:
//!
//! 1. **Co-authored commits** — `Co-authored-by:` trailers in the
//!    commit message footer fan out into one `author` row plus N
//!    `co_author` rows. The fixture covers both the GitHub
//!    `users.noreply.github.com` login form (the worker resolves
//!    to a stable login) and an arbitrary external email (the
//!    worker falls back to a synthetic user keyed off the email).
//!
//! 2. **Squash-merge** — the merged-PR event carries three
//!    distinct user ids: the PR opener (`author`), the human who
//!    pressed the merge button (`merger`), and the committer
//!    GitHub stamped on the squash commit (`committer`). The
//!    fixture has three different `id` values to prove the worker
//!    doesn't collapse them.
//!
//! 3. **Bot author** — `dependabot[bot]` (`type: Bot`) lands an
//!    actor row with `role = author`, same as a human. SCOPE §6's
//!    "tracked separately" requirement is a *report-layer* filter
//!    on the `[bot]` login suffix, not an ingest-time drop.
//!
//! 4. **Unattributed deletion** — a commit whose author email
//!    matches no GitHub user (the push payload omits `username`)
//!    must still produce an `ActivityEvent` so the org/repo
//!    commit count stays correct, but must NOT invent a synthetic
//!    user row. The fixture pins zero attached actor rows.
//!
//! 5. **Historical commits before the user existed** — backfill
//!    can replay a push whose author has never been seen. The
//!    worker's `upsert_user` calls happen *before*
//!    `add_event_actors` so the resulting FK always resolves
//!    against a freshly-created user row.

use std::sync::Arc;

use chrono::{TimeZone, Utc};
use dp_domain::{ActorRole, EventKind, WebhookDelivery};
use serde_json::Value;
use uuid::Uuid;

use super::handlers::apply_delivery;
use super::test_store::FakeStore;

/// Wrap a payload + `X-GitHub-Event` value in a `WebhookDelivery`
/// the same way the receiver (Stage 4) does. Fresh ids per call so
/// running two fixtures back-to-back in one test doesn't tangle
/// the inbox's `delivery_id` uniqueness.
fn delivery_from(event: &str, payload: Value) -> WebhookDelivery {
    WebhookDelivery {
        id: Uuid::new_v4(),
        delivery_id: format!("d-{}", Uuid::new_v4()),
        event: event.into(),
        payload,
        received_at: Utc::now(),
        processed_at: None,
        error: None,
    }
}

/// Strip the leading `_comment` documentation field (used to keep
/// the fixture self-describing on disk) before handing the payload
/// to the handler. The handler tolerates extra fields, but we'd
/// rather not have it copied verbatim into `ActivityEvent.payload`.
fn parse_fixture(raw: &str) -> Value {
    let mut v: Value = serde_json::from_str(raw).expect("fixture parses as JSON");
    if let Some(obj) = v.as_object_mut() {
        obj.remove("_comment");
    }
    v
}

#[tokio::test]
async fn fixture_push_with_coauthored_by_trailers() {
    // Case 1: SCOPE §6 co-authored commit. One author + two
    // co-authors via `Co-authored-by:` trailers.
    let raw = include_str!("../../tests/fixtures/push_coauthored.json");
    let payload = parse_fixture(raw);

    let s = Arc::new(FakeStore::new());
    let out = apply_delivery(s.as_ref(), &delivery_from("push", payload))
        .await
        .expect("handler accepts fixture");

    // One commit -> one event; three attribution rows total: alice
    // (author + committer, same user) and two co_authors.
    assert_eq!(out.events, 1, "one commit -> one event");
    assert_eq!(out.actors, 4, "alice author + alice committer + 2 co_authors");

    let ev = s.only_event();
    assert_eq!(ev.kind, EventKind::Commit);
    assert_eq!(ev.external_id, "deadbeef0000000000000000000000000000aaaa");

    // Author + committer collapse onto alice (same login, two
    // distinct roles — composite PK lets both rows live).
    let alice = s.roles_for_login(ev.id, "alice");
    assert!(
        alice.contains(&ActorRole::Author) && alice.contains(&ActorRole::Committer),
        "alice: {alice:?}"
    );

    // First co-author uses GitHub's `<id>+<login>@users.noreply…`
    // convention — the worker peels the login out and stores the
    // user under that name.
    let octocat = s.roles_for_login(ev.id, "octocat");
    assert!(
        octocat.contains(&ActorRole::CoAuthor),
        "octocat co_author missing: {octocat:?}"
    );

    // Second co-author has no recoverable login; the worker falls
    // back to the email as the synthetic login so the row is
    // stable across redeliveries.
    let mallory = s.roles_for_login(ev.id, "mallory@external.example");
    assert!(
        mallory.contains(&ActorRole::CoAuthor),
        "external co_author missing: {mallory:?}"
    );
}

#[tokio::test]
async fn fixture_pr_squash_merge_distinct_author_committer_merger() {
    // Case 2: SCOPE §6 squash-merge — three distinct user ids in
    // one event. The fixture's `user.id` / `merged_by.id` /
    // `committer.id` are all different, which is the hard case
    // (real squash merges where a release manager presses merge
    // and GitHub stamps a committer email distinct from both).
    let raw = include_str!("../../tests/fixtures/pr_squash_merge.json");
    let payload = parse_fixture(raw);

    let s = Arc::new(FakeStore::new());
    apply_delivery(s.as_ref(), &delivery_from("pull_request", payload))
        .await
        .expect("handler accepts fixture");

    let ev = s.only_event();
    assert_eq!(ev.kind, EventKind::PullRequestMerged);
    assert_eq!(ev.external_id, "PR_squash_distinct_ids");
    assert_eq!(
        ev.ts,
        Utc.with_ymd_and_hms(2024, 2, 2, 2, 2, 2).unwrap(),
        "merged_at, not closed_at or created_at"
    );

    // The three roles must land on three distinct logins —
    // SCOPE §6's whole point about squash-merges.
    assert!(s
        .roles_for_login(ev.id, "alice-author")
        .contains(&ActorRole::Author));
    assert!(s
        .roles_for_login(ev.id, "merger-bob")
        .contains(&ActorRole::Merger));
    assert!(s
        .roles_for_login(ev.id, "carol-committer")
        .contains(&ActorRole::Committer));

    // And no accidental overlaps — e.g. the merger must not also
    // appear as author (a common bug when payload destructuring
    // confuses `sender` with `pull_request.user`).
    assert!(!s
        .roles_for_login(ev.id, "merger-bob")
        .contains(&ActorRole::Author));
    assert!(!s
        .roles_for_login(ev.id, "alice-author")
        .contains(&ActorRole::Merger));
}

#[tokio::test]
async fn fixture_push_bot_author_records_author_role() {
    // Case 3: a bot push. The actor row is recorded just like a
    // human's — bot suppression is the report layer's job.
    let raw = include_str!("../../tests/fixtures/push_bot_author.json");
    let payload = parse_fixture(raw);

    let s = Arc::new(FakeStore::new());
    apply_delivery(s.as_ref(), &delivery_from("push", payload))
        .await
        .expect("handler accepts fixture");

    let ev = s.only_event();
    assert_eq!(ev.kind, EventKind::Commit);

    // The bot keeps its full `[bot]` login (the report layer
    // filters on this suffix downstream).
    let bot_roles = s.roles_for_login(ev.id, "dependabot[bot]");
    assert!(
        bot_roles.contains(&ActorRole::Author),
        "bot author role missing: {bot_roles:?}"
    );

    // GitHub stamps `web-flow` as the committer on merge-button
    // commits — that's a separate actor row, also kept.
    let webflow = s.roles_for_login(ev.id, "web-flow");
    assert!(
        webflow.contains(&ActorRole::Committer),
        "web-flow committer row missing: {webflow:?}"
    );
}

#[tokio::test]
async fn fixture_push_unattributed_records_event_without_actors() {
    // Case 4: SCOPE §6 unattributed bucket. The commit happened,
    // so the event row exists (the commit count must stay right),
    // but the author can't be linked to any GitHub user — the
    // worker emits zero actor rows rather than inventing a fake
    // user that would corrupt per-user totals.
    //
    // (Schema note: `event_actors.user_id` is non-null; once we
    // relax that to allow a NULL-user `role = author` row, this
    // assertion flips to "exactly one author row with user_id =
    // None". The contract under test today is the conservative
    // ingest-side behaviour.)
    let raw = include_str!("../../tests/fixtures/push_unattributed.json");
    let payload = parse_fixture(raw);

    let s = Arc::new(FakeStore::new());
    apply_delivery(s.as_ref(), &delivery_from("push", payload))
        .await
        .expect("handler accepts fixture");

    let ev = s.only_event();
    assert_eq!(ev.kind, EventKind::Commit);
    assert_eq!(ev.external_id, "cafef00d00000000000000000000000000001234");

    let actors = s.actors_for(ev.id);
    assert!(
        actors.is_empty(),
        "unattributed: expected zero actor rows, got {actors:?}"
    );
}

#[tokio::test]
async fn fixture_push_historical_lazy_upserts_user_then_attaches_actor() {
    // Case 5: backfill / replay against a fresh store. The author
    // does not exist in `users` when the delivery arrives. The
    // worker's contract is `upsert_user` BEFORE `add_event_actors`
    // — proven here by starting from an empty FakeStore and
    // checking the FK lookup resolves at the end.
    let raw = include_str!("../../tests/fixtures/push_historical_unknown_user.json");
    let payload = parse_fixture(raw);

    let s = Arc::new(FakeStore::new());
    apply_delivery(s.as_ref(), &delivery_from("push", payload))
        .await
        .expect("handler accepts fixture");

    let ev = s.only_event();
    assert_eq!(ev.kind, EventKind::Commit);

    // `actors_for` walks event_actors -> users; if `upsert_user`
    // hadn't happened first, the user_id would be a dangling
    // reference and the login would come back as `<missing>`.
    let actors = s.actors_for(ev.id);
    assert!(
        actors
            .iter()
            .any(|(login, r)| login == "history-hannah" && *r == ActorRole::Author),
        "lazy-upserted Author actor missing: {actors:?}"
    );
    // No `<missing>` placeholder rows — every actor resolves to a
    // real user row.
    assert!(
        !actors.iter().any(|(login, _)| login == "<missing>"),
        "dangling user_id in event_actors: {actors:?}"
    );
}
