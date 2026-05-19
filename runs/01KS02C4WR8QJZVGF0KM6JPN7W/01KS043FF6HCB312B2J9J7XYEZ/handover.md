## Done

- Added three JSON fixtures under `crates/dp-reports/tests/fixtures/`: `single-user-single-org.json`, `co-authored-commit-spanning-two-orgs.json`, `home-org-split-on-shared-org.json`. Each pairs an `EventActorRow` projection (recorded-GitHub-shape) with one or more `checks` listing scope_mode + metric + expected totals / per-user / per-(user,org) counts.
- Added `crates/dp-reports/tests/spot_check.rs` — loads each fixture, runs the public pipeline (`filter_rows_for_metric` → `single_org` / `all_orgs_combined` / `per_org_split` → `count_by_user`), and asserts the checks. One `#[test]` per fixture so a failure points at the responsible file.
- `cargo test -p dp-reports` (all 3 spot-check tests pass) and `scripts/check-boundaries.sh` both green.
- Committed as `7e4d8dd` on `codeless/phase-3-reports` with subject starting with the stage title.

## Next

- (none) — stage 9 picks up next session.

## What you need to know

- Fixture rows are projected into `dp_domain::store::EventActorRow` via a test-local `RowSpec` because the production row type does not derive serde (it is a store-projection, not a wire type). If a future EventActorRow gains a field, update `RowSpec::into_row`.
- The harness uses the *public* `dp_reports` surface; no internal/private helpers. The lens functions are re-exported via `dp_reports::lenses::{single_org, all_orgs_combined, per_org_split}` (they are not at the crate root — `lib.rs` does not re-export them; if you want to flatten, add `pub use lenses::{…}` to `lib.rs`).
- The §0.2 cross-org dedup fixture credits both the `author` and the `co_author` once per (user, event) across both orgs in `all_orgs_combined`. `single_org` on either org credits each contributor once for the shared event because the row is present under each org_id.
- The exec fixture's `all_orgs_combined` total is 9 (U1: 4 distinct events, U2: 5), since the shared-org rows are *distinct* events from the home-org ones — no dedup collapses them.
- Raw GitHub JSON payloads are not checked in to keep fixtures small; provenance is recorded in the fixture's `description` / `github_recorded_payload_ref` strings. The convention noted at the top of `spot_check.rs` is to drop raw payloads under `tests/fixtures/raw/` when added.

## Open questions

- (none)
