# Scope — bootstrap-domain-store

> The full product scope lives in [SCOPE.md](../../../SCOPE.md). The
> phased plan this job implements lives in [TODO.md](../../../TODO.md)
> §Phase 0 and §Phase 1, plus the decisions in §0.1–§0.6. This file is
> the trimmed brief; read both linked docs before stage 1.

## Goal

Stand up the dev-pulse Cargo workspace and the domain + Postgres-store
layer that every later phase depends on. The end state is: every crate
from TODO §2 exists, the starter-* import boundaries are enforced in
CI, `cargo test --workspace` is green, and `cargo run -- migrate`
applies the full v1 schema to a fresh Postgres database.

## In scope

- Cargo workspace at the repo root with all crates from TODO §2
  (`dp-domain`, `dp-store-pg`, `dp-fetcher`, `dp-reports`, `dp-rest`,
  `dp-mcp`, `dp-cli`, `dp-server`, and the `dev-pulse` binary).
- `starter-*` pinned via `path = "../starter/crates/<name>"`. No
  edits to starter crates — composition only.
- `scripts/check-boundaries.sh` per TODO §0.6, wired into a CI job
  that runs from this job onward.
- `dp-domain` — entities and the `Store` trait, zero `starter_*`
  imports, mirroring `starter/examples/notes/src/domain.rs` in shape.
- `dp-store-pg` — Postgres `Store` impl owning `PgPool`, with a
  `sources()` function returning `[starter_auth_users, dp]` per the
  starter migrations namespacing rule. Only allowed starter import
  is `starter_spi::MigrationSource` (plus zero-dep contract types).
- All v1 tables from TODO §Phase 1: `users` (with soft-delete +
  pseudonymisation), `orgs`, `teams`, `repos`, `memberships`,
  `activity_events` (no `user_id` column), `event_actors` (composite
  PK `(event_id, user_id, role)`), `issues`, `webhook_inbox`,
  `fetch_cursors`, `fetch_runs`, `audit_log`.
- All mandatory indexes from TODO §Phase 1 (the explicit list).
- Integration tests for the `Store` trait against a real Postgres.

## Out of scope

- The webhook receiver, reconciler, backfill, GitHub client — all
  Phase 2 work, not this job.
- Any report query logic — Phase 3.
- HTTP routing, auth wiring, OpenAPI — Phase 4.
- MCP tools, CLI subcommands beyond `migrate` — Phases 5–6.
- Frontend — Phase 7.
- The materialised `event_actor_facts` table (TODO §Phase 1 notes
  it as a load-test-driven decision; do not pre-create it).
- Any starter-* edits. If a starter API is missing, compose around
  it in dev-pulse or stop the stage with a `[!]` and surface it at
  the next REVIEW gate.

## Constraints

- **Boundary rule (TODO §0.6) is enforced in CI from stage 2
  onward.** `dp-domain`, `dp-fetcher`, `dp-reports` must have zero
  `starter_*` imports; `dp-store-pg` may import only
  `starter_spi::MigrationSource` and zero-dep contract types.
- **Multi-actor schema (TODO §0.2).** Events do not carry
  `user_id`; attribution lives in `event_actors`. Do not collapse
  this into a single column even if it looks simpler now.
- **Per-(org, repo, resource_kind) cursors (TODO §0.3).** No
  global cursor on `fetch_runs`. `fetch_runs` is a run log only.
- **Soft-delete + pseudonymisation (TODO §0.5).** `users` has
  `deleted_at`; hard-delete is admin-only and not exercised here.
  `event_actors` rows survive deletion; anonymisation runs on
  `users`.
- **Time zones (TODO §0.4).** All timestamp columns are
  `timestamptz` in UTC. No naive timestamps.
- **MSRV 1.78**, `cargo clippy -D warnings`, `cargo fmt --check`
  all must be green before any stage commits.
- **No `--force`, no `--no-verify`** — see ../../CLAUDE.md and
  ADDING-JOB.md Hard rule 5. If a hook fails, fix the cause.
- **Single starter import in `dp-store-pg`**: only
  `starter_spi::MigrationSource` (and the zero-dep contract types
  that ships with). Anything else trips the boundary script.

## Open questions (resolve in stage 1 or at first REVIEW)

1. **Postgres in tests** — testcontainers vs an env-var
   `DEV_PULSE_TEST_DATABASE_URL`. Pick one and document it; do not
   support both.
2. **`issues` table fields** — TODO §4.1 says future-phase CRUD,
   so include the edit-capable fields (title, body, labels,
   assignees, state, milestone) now to avoid reshaping. Confirm
   the assignees/labels representation (json vs join table) at the
   first REVIEW gate.
3. **`payload jsonb` schema discipline** — do we keep raw GitHub
   payloads or a trimmed projection? Default: store the trimmed
   projection plus the raw `external_id`, drop the rest. Confirm
   at REVIEW.
4. **`event_actors.role` enum** — TODO §0.2 lists author,
   co_author, committer, merger, reviewer, commenter, assignee,
   requester, closer. Store as `text` + a CHECK constraint, or as
   a PG enum? Default: `text` + CHECK so we can add roles without
   a schema migration. Confirm at REVIEW.
