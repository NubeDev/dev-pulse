## Done

- Added 5 recorded GitHub webhook fixture payloads under `crates/dp-fetcher/tests/fixtures/`: `push_coauthored.json`, `pr_squash_merge.json`, `push_bot_author.json`, `push_unattributed.json`, `push_historical_unknown_user.json`.
- Added `crates/dp-fetcher/src/worker/fixture_tests.rs` with one `#[tokio::test]` per fixture, registered into `worker/mod.rs` as a `#[cfg(test)] mod fixture_tests`.
- Each test loads its fixture via `include_str!`, strips the documenting `_comment` field, dispatches through `apply_delivery` against an in-memory `FakeStore`, and asserts the SCOPE §6 attribution contract.
- `cargo test -p dp-fetcher` → 62 passed (the 5 new + existing). `scripts/check-boundaries.sh` → OK.
- Committed as `co-author / squash-merge / bot / unattributed handling per SCOPE §6`.

## Next

- Stage 8: reconciler — per-(org, repo, resource_kind) cursor pagination + etag conditional GETs to detect missed webhooks, running on the configurable 4h interval (TODO §0.1, §0.3).

## What you need to know

- `FakeStore` lives in `src/worker/test_store.rs` as `pub(crate)`, so the fixture tests had to live inside the crate (not as a `tests/` integration test). Fixtures are still under `tests/fixtures/` per spec and loaded via `include_str!("../../tests/fixtures/...")` from `src/worker/fixture_tests.rs`.
- The unattributed-commit test pins **zero** actor rows (event row exists, no `EventActor`). The stage description suggested "actor row with user_id NULL", but `EventActor.user_id: Uuid` is non-nullable in `dp-domain`, and the pre-existing inline test `push_with_unresolvable_author_lands_unattributed` already asserts the actor-less contract. A comment in the test calls out the schema escape hatch if we later want to flip this.
- The historical-user case relies on `handle_push` calling `upsert_user_by_login` before `record(...)` which calls `add_event_actors`. `actors_for` resolves user_id → login through the FakeStore's users map and would surface `<missing>` for any dangling FK — the test asserts that doesn't happen.
- Co-author fixture covers both branches of `CoAuthor::noreply_login`: the `<id>+<login>@users.noreply.github.com` form (resolves to `octocat`) and an external email (`mallory@external.example`) which the handler stores under a synthetic login = the email itself.

## Open questions

- Whether to relax `EventActor.user_id` to `Option<Uuid>` so unattributed commits can carry an explicit `role=author` row with `user_id = NULL`. Would require a domain change + migration + report-layer updates; the current fixture's comment flags this. Not in scope for stage 7.
