## Done

- Added `crates/dp-fetcher/src/phase2_smoke.rs` (cfg(test)) with the six literal-named smoke tests required by stage 10:
- `webhook_replay_same_delivery_id_yields_exactly_one_upsert`
- `co_authored_commit_fans_out_to_n_event_actors_rows`
- `missed_webhook_detected_by_reconciler`
- `backfill_respects_rate_limit_headroom`
- `scheduler_coalesces_overlapping_ticks`
- `fetch_runs_row_written_per_batch_per_kind`
- Wired the module into `crates/dp-fetcher/src/lib.rs`.
- Added `.github/workflows/smoke.yml` (`phase2-smoke`) running `cargo test -p dp-fetcher --lib phase2_smoke` plus `./scripts/check-boundaries.sh`.
- Verified locally: all 6 smoke tests pass; full `cargo test --workspace` green (84 dp-fetcher unit tests); `check-boundaries.sh` reports OK.
- Committed on branch `codeless/phase-2-ingestion` with message starting `stage 10:`.

## Next

- Phase 3 — reports (`dp-reports`) per TODO §Phase-3.

## What you need to know

- Smoke tests are unit-style (use the in-crate `worker::test_store::FakeStore` + wiremock) so the workflow does not require Docker. Postgres-backed integration tests stay in `.github/workflows/integration.yml`.
- The boundary check `boundary-check-still-green` runs in both `boundaries.yml` (existing from Phase 0) and `smoke.yml` — duplicate-but-cheap so the smoke workflow is self-contained.
- The backfill near-exhaustion test pins the *branch* (headroom triggered + chunk still completes); the production `honour_headroom` sleep is capped at 1h and is exercised via a reset≈0 header so the test does not wall-clock-block.
- One stage-10 test (`fetch_runs_row_written_per_batch_per_kind`) covers all three FetchRunKind variants in a single test body — a regression in any drainer surfaces in the same CI line.

## Open questions

- (none)
