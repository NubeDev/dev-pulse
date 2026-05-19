## Done

- Added workspace deps to crates/dp-domain/Cargo.toml (async-trait, chrono, serde, serde_json, thiserror, uuid). Zero starter-* deps.
- Wrote 11 v1 entity files under crates/dp-domain/src/ — user.rs, org.rs, team.rs, repo.rs, membership.rs, event.rs (ActivityEvent + EventActor + EventKind + ActorRole), fetch.rs (FetchRun + FetchCursor + ResourceKind + FetchRunKind), webhook.rs (WebhookDelivery), window.rs (Window + WindowAnchor) — one concept per file.
- Wrote crates/dp-domain/src/store.rs: object-safe `Store` async trait with every TODO §Phase 1 method (upsert_user, get_user, get_user_by_github_id, list_users, pseudonymise_user, upsert_org/team/repo/membership, list_memberships_for_user, set_home_org, record_event, add_event_actors, list_event_actor_rows_in_window, get_cursor, put_cursor, start_fetch_run, finish_fetch_run, list_recent_fetch_runs, enqueue_webhook, claim_webhooks, mark_webhook_processed, mark_webhook_failed). `StoreError` boxes backend errors so dp-domain stays sqlx-free.
- Enforced TODO §0.2 in the type system: ActivityEvent has no user_id field; attribution only via EventActor.
- 16 unit tests pass: JSON round-trip for every entity, wire-form guards on snake_case enums, and a compile-time `dyn Store` object-safety check.
- Closing trio green: cargo fmt --check, cargo clippy --workspace --all-targets -- -D warnings, cargo test --workspace, and ./scripts/check-boundaries.sh.
- Updated .codeless/jobs/bootstrap-domain-store/handover.md for stage 5.
- Committed as `stage 4: dp-domain — entity types and Store trait` (6cf5266) on branch codeless/bootstrap-domain-store. Not pushed (no remote auth in this session, matching stages 1–2).

## Next

- Stage 5: dp-store-pg implementation against sqlx::PgPool. Must resolve the starter_spi vs starter_store_postgres allowlist question (MigrationSource actually lives in starter_store_postgres::migrate); update scripts/check-boundaries.sh if expansion is needed.
- Pick testcontainers vs DEV_PULSE_TEST_DATABASE_URL (open question #1 in job SCOPE).

## What you need to know

- async_trait was chosen so `dyn Store` stays object-safe (every surface holds an Arc<dyn Store>). Native `async fn` in traits would break that.
- No Default impls — entity rows are always fetched or constructed at a known call site; a Default invites accidental Uuid::nil() writes. Add ergonomic constructors in test fixtures downstream, not here.
- ActorRole and EventKind are closed Rust enums (new GitHub event kinds need a code change to ingest — matches SCOPE intent). MembershipRole::Other(String) is the only open vocab, for GitHub Enterprise custom roles.
- EventActorRow is the projection list_event_actor_rows_in_window returns — smallest shape that lets reports do (user_id, event_id) de-dup without a second query.
- StoreError::Invalid is reserved (not raised today) so #[non_exhaustive] is honest and stage 5+ doesn't feel SemVer pressure to add it.
- payload: serde_json::Value on ActivityEvent and WebhookDelivery — SCOPE open question #3 (trimmed projection vs raw) is still deferred to REVIEW; the type doesn't constrain the choice.

## Open questions

- (carried from stage 2 REVIEW) Boundary allowlist: keep starter_spi::* only, or expand to starter_store_postgres::{migrate, Pool, pool} so stage 5 can build?
- (carried from stage 1 REVIEW) Replace /home/user/.codeless/worktrees/starter symlink with absolute paths or [patch] in root Cargo.toml?
- (deferred) ActivityEvent.payload — trimmed projection (default) or raw GitHub body? Decide before stage 6 writes the jsonb column.
- (deferred) event_actors.role storage in stage 6 — text + CHECK vs PG enum. Rust side is already a closed ActorRole enum.
