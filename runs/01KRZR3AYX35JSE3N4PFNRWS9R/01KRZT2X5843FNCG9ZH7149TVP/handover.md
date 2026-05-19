## Done

- Added `crates/dp-store-pg/tests/integration.rs` — 8 `#[ignore]`d tests exercising every non-trivial Store SQL path against a live Postgres: upsert_user (with rename/lookup/NotFound), upsert_membership (home_org preservation across re-upsert + set_home_org), upsert_team (org+github_id dedupe), cursor put/get with concrete and NULL repo_id, webhook inbox (enqueue/Conflict-on-replay/FIFO claim/mark_processed/mark_failed), record_event + add_event_actors (idempotent) + list_event_actor_rows_in_window with org/repo/user/role filters, start/finish/list fetch runs, and pseudonymise_user (PII cleared, id stable, deleted_at idempotent, FK to historical events intact).
- Added `[dev-dependencies]` in `crates/dp-store-pg/Cargo.toml`: `starter-store-postgres` with `testing`, `tokio`, `testcontainers = "0.23"`, `testcontainers-modules = "0.11"` with `postgres`.
- Added `.github/workflows/integration.yml` running `cargo test -p dp-store-pg --test integration -- --ignored` on `ubuntu-latest`.
- Verified locally: all 8 integration tests pass against PG16-alpine; `cargo test --workspace` still green (integration tests show as 8 ignored); `./scripts/check-boundaries.sh` green.

## Next

- (none) — Phase 0/1 stages all landed; a fresh session picks up Phase 2.

## What you need to know

- The fixture honours `DP_TEST_DATABASE_URL` if set; otherwise spins a PG container per test via `testcontainers-modules`. Tag is `DP_TEST_PG_TAG` (default `16-alpine`).
- We deliberately do NOT use `starter_store_postgres::testing::with_database()` because it hard-codes `postgres:11-alpine`, and PG11 rejects the schema's `UNIQUE NULLS NOT DISTINCT` clause. The comment in `crates/dp-store-pg/Cargo.toml` records this.
- Cursor round-trip test uses whole-second timestamps because PG `TIMESTAMPTZ` is microsecond-precision while `Utc::now()` is nanosecond — struct-equality on `FetchCursor` would otherwise spuriously fail. Comment in-file explains it.
- Boundary script unchanged; integration-test imports of `starter_store_postgres::pool::connect` and `starter_store_postgres::migrate` are within the allowlist for `dp-store-pg`.

## Open questions

- (none)
