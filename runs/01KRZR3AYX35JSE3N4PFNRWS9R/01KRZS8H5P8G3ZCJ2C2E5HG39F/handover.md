## Done

- crates/dp-store-pg/Cargo.toml: added starter-store-postgres, sqlx (postgres+macros+migrate+uuid+chrono+json), async-trait, chrono, serde_json, thiserror, tracing, uuid.
- migrations/dp/0001_init.sql: all 10 v1 tables + report/claim/cursor indexes. Applies cleanly to postgres:16-alpine (verified by spinning a container, applying, listing tables).
- src/lib.rs: static `DP_MIGRATOR` via `sqlx::migrate!("./migrations/dp")`; `sources()` returns `Vec<MigrationSource>` (one entry, name "dp") for the host binary to feed into `starter_store_postgres::migrate`.
- src/store.rs: `PgStore { pool: Pool }` implementing every `dp_domain::Store` method with real SQL via runtime `sqlx::query`/`query_as` (no compile-time macro so no DATABASE_URL needed at build time). Upserts on github_id; `upsert_membership` preserves `home_org` via COALESCE; `add_event_actors` batches via UNNEST; `list_event_actor_rows_in_window` short-circuits empty filters with `cardinality($N)=0`; `claim_webhooks` uses FOR UPDATE SKIP LOCKED. `pseudonymise_user` rewrites login → `deleted-user-<16-hex>`, nulls email/name, stamps `deleted_at`.
- src/encode.rs: TEXT ↔ enum helpers covering ActorRole, EventKind, MembershipRole (Other(s) verbatim), ResourceKind, FetchRunKind. snake_case wire form matches JSON.
- Error mapping: SQLSTATE 23505 → `StoreError::Conflict` (webhook replay path); 0-rowcount UPDATEs → `StoreError::NotFound`; everything else boxed into `Backend`.
- scripts/check-boundaries.sh: expanded the dp-store-pg allowlist to `starter_(spi|store_postgres)`. Boundary check still green.
- `cargo test --workspace` green (22 unit tests: 16 dp-domain + 5 dp-store-pg encode + 1 sources guard). `cargo clippy --workspace --all-targets -- -D warnings` clean. `cargo fmt -p dp-store-pg` clean.
- Committed as `stage 5: dp-store-pg — Postgres Store impl, v1 migrations, sources() wiring` (8e21577) on `codeless/bootstrap-domain-store`. Not pushed (no remote auth in this session, matches prior stages).

## Next

- Stage 6: dp-server / wiring — register `dp_store_pg::sources()` with the starter migration runner, build an Arc<dyn Store> in main, and (likely) decide between testcontainers vs DEV_PULSE_TEST_DATABASE_URL for the integration test that exercises an actual round-trip.
- Add a `#[ignore]` integration test under `crates/dp-store-pg/tests/` that runs the full Store surface against a live PG (testcontainers or env-URL) — current tests cover encoding + sources() but not SQL bodies end-to-end.

## What you need to know

- The job goal text said "MigrationSource wiring via starter_spi::MigrationSource" — that type actually lives in `starter_store_postgres::migrate`, not starter_spi. I expanded the boundary allowlist accordingly. If the original wording is load-bearing, reverse it and re-export `MigrationSource` from somewhere else.
- All SQL bodies use runtime `sqlx::query` (no `query!` macros), so the crate builds without `DATABASE_URL` or a .sqlx cache. Switching to macros later would require committing query metadata.
- `dp_fetch_cursors` uses `UNIQUE NULLS NOT DISTINCT` — Postgres 15+ feature. If targeting older PG, swap to a partial unique index + a separate one for NULL repo_id.
- Table names are prefixed `dp_` so they coexist cleanly with `starter_auth_users_*` and any future migration sources the host wires in.
- `upsert_membership` uses `LEAST(existing, EXCLUDED)` for `joined_at` so the earliest observed timestamp wins (re-observing an old membership doesn't push the join date forward).
- Tables created in fresh-postgres test: dp_users, dp_orgs, dp_teams, dp_repos, dp_memberships, dp_activity_events, dp_event_actors, dp_fetch_runs, dp_fetch_cursors, dp_webhook_inbox (10 / 10).

## Open questions

- (carried) ActivityEvent.payload — trimmed projection vs raw GitHub body. Schema stores JSONB either way; decide before the fetcher writes real rows.
- (carried) event_actors.role storage: stayed TEXT (matches encode helpers). Switch to PG enum would require a migration.
- Integration test strategy: testcontainers (Docker on CI) vs `DEV_PULSE_TEST_DATABASE_URL` env (faster, requires external PG). Stage 5 deferred — encode helpers are unit-tested, but no end-to-end SQL test exists yet.
- `sources()` returns `Vec<MigrationSource>` not `&'static [MigrationSource]` — fine because `MigrationSource` is `Copy`, but if performance ever matters this could be a static.
