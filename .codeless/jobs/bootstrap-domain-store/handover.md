# Handover — after stage 4 (dp-domain entities + Store trait)

## Done

- Filled in `crates/dp-domain/Cargo.toml` with `async-trait`, `chrono`,
  `serde`, `serde_json`, `thiserror`, and `uuid` workspace deps. Zero
  `starter-*` deps (TODO §0.6).
- Implemented every v1 entity called out in TODO §Phase 1, one file per
  concept under `crates/dp-domain/src/`:
  - `user.rs` — `User` with `deleted_at` soft-delete column.
  - `org.rs` — `Org`.
  - `team.rs` — `Team` (FK `org_id`).
  - `repo.rs` — `Repo` (FK `org_id`).
  - `membership.rs` — `Membership` + `MembershipRole`
    (`Admin | Member | Other(String)`) with `home_org: Option<Uuid>`
    and `joined_at`.
  - `event.rs` — `ActivityEvent` (no `user_id`, per TODO §0.2),
    `EventActor` (composite key `(event_id, user_id, role)`),
    `EventKind` (12 kinds), `ActorRole` (the canonical 9).
  - `fetch.rs` — `FetchRun` + `FetchRunKind`
    (`WebhookWorker | Reconciler | Backfill`), `FetchCursor` keyed
    `(org_id, repo_id?, resource_kind)` with `since`/`etag`/
    `last_event_id`/`updated_at`, and `ResourceKind` (11 variants).
  - `webhook.rs` — `WebhookDelivery` with `delivery_id` (unique),
    `received_at`, `processed_at`, `error`.
  - `window.rs` — `Window` + `WindowAnchor`
    (`Viewer | Org | Utc`) per TODO §0.4.
- Implemented `store.rs`: `Store` async trait (object-safe) covering
  TODO §Phase 1's named methods plus the supporting upserts the
  fetcher will need. `StoreError` is the boundary error type with
  `NotFound`, `Conflict`, `Invalid`, and `Backend(Box<dyn Error>)`
  variants so backends never leak `sqlx` up the stack. New struct
  `EventActorRow` is the projection `list_event_actor_rows_in_window`
  returns — the smallest shape that lets reports do
  `(user_id, event_id)` de-dup (SCOPE §8.1) without a second query.
- All entities derive `Debug + Clone + PartialEq + Eq + Serialize +
  Deserialize`; enums use `#[serde(rename_all = "snake_case")]` so the
  JSON wire form matches what reports and fixtures will expect.
- Unit tests: per-type JSON round-trip (16 tests, all green), plus a
  wire-form guard on `EventKind`/`ActorRole`/`WindowAnchor`
  snake_case and a compile-time `dyn Store` object-safety check.
- Closing trio green: `cargo fmt --check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo test --workspace`, and
  `./scripts/check-boundaries.sh` all pass.
- Boundary script confirms `dp-domain` has zero `starter_*` imports.

## Next

- **Stage 5: dp-store-pg.** Implement the `Store` trait against
  `sqlx::PgPool`. Decide the `starter_spi` vs `starter_store_postgres`
  allowlist question carried since stage 1 — `MigrationSource` actually
  lives in `starter_store_postgres::migrate`, not `starter_spi`. If we
  need to import from `starter_store_postgres`, update
  `scripts/check-boundaries.sh` to allow that path.
- Wire `sources()` returning `[starter_auth_users, dp]`. SQL files
  themselves are stage 6.
- Pick the testcontainers-vs-`DEV_PULSE_TEST_DATABASE_URL` question
  (open question #1 in `.codeless/jobs/bootstrap-domain-store/SCOPE.md`).

## What you need to know

- **No `Default` impls** anywhere in `dp-domain`. Entity rows are
  always either fetched from the store or constructed at a known
  call-site (event-actor pair, webhook receiver, etc.); a `Default`
  invites accidental `Uuid::nil()` writes. If a stage 5+ test fixture
  wants ergonomic constructors, add them in that crate, not here.
- **Multi-actor invariant** is in the type system, not just SQL:
  `ActivityEvent` has no `user_id` field at all, so a downstream crate
  cannot accidentally attribute. Attribution must go through
  `EventActor` / `add_event_actors`.
- **`ActorRole`/`EventKind`/`MembershipRole` are open vocabularies in
  spirit**. `ActorRole` and `EventKind` are closed Rust enums for
  type-safety; if GitHub ships a new event kind the fetcher will need
  a code change to ingest it (matches SCOPE intent — we'd want a
  conscious decision per kind). `MembershipRole::Other(String)` is the
  only place we accept unknown values, because GitHub Enterprise
  custom roles can't be pre-enumerated.
- **`payload: serde_json::Value`** on `ActivityEvent` and
  `WebhookDelivery`. SCOPE open question #3 (trimmed projection vs raw)
  is still deferred to REVIEW; the type doesn't constrain the choice.
- **`StoreError::Invalid` is reserved**, not currently raised by any
  method. Listed so `#[non_exhaustive]` is honest and stage 5+ can
  reach for it without a SemVer-feeling change.
- **`async_trait` was chosen** for `Store` to keep it object-safe
  today; native `async fn` in traits would lose `dyn Store`. Worth a
  revisit when starter publishes its own pattern.
- The two REVIEW questions carried from stages 1–2 (starter import
  allowlist, worktree symlink) are still open and must be settled in
  stage 5 or at the next REVIEW gate.

## Open questions

- (carried from stage 2 REVIEW) Expand the boundary allowlist to
  `starter_store_postgres::{migrate, Pool, pool}` so stage 5 can build,
  or compose around `MigrationSource` via `starter_spi` re-exports if
  starter has them?
- (carried from stage 1 REVIEW) Replace the
  `/home/user/.codeless/worktrees/starter` symlink with absolute paths
  or `[patch]` in the root `Cargo.toml`?
- (deferred from stage 4) Should `ActivityEvent.payload` be the
  trimmed projection (default) or raw GitHub body? Confirm at REVIEW
  before stage 6 writes the `jsonb` column.
- (deferred from stage 4) `event_actors.role` storage — `text` +
  CHECK constraint vs PG enum. The Rust side is fixed
  (closed `ActorRole` enum); stage 6 decides the column.
