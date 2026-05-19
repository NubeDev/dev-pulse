# Scope — phase-2-ingestion

> Source of truth: [`TODO.md`](../../../TODO.md) §"Phase 2 — ingestion"
> in the dev-pulse repo, plus [`SCOPE.md`](../../../SCOPE.md) for
> product scope. This file is the per-job brief the runner reads
> before every stage; intentionally short. When this file disagrees
> with TODO.md or SCOPE.md, those win — open an issue and update this
> file.

## Goal

Replace the original scheduled-fetcher model (SCOPE §10) with the
**webhook-primary** ingestion architecture decided in
[`TODO.md`](../../../TODO.md) §0.1: GitHub App webhooks are the live
feed, a worker drains the inbox with idempotent upserts and
multi-actor fan-out (§0.2), a reconciler runs every 4h to catch
missed webhooks via per-(org, repo, resource_kind) cursors and etag
conditional GETs (§0.3), and a one-shot bounded backfill seeds
history. All work in `dp-fetcher`, `dp-store-pg`, and the
`dev-pulse` main — **zero edits** to `crates/starter-*` or
`packages/`, enforced by `scripts/check-boundaries.sh` (§0.6).

## In scope

- **Octocrab client wrapper** (`dp-fetcher::client`): rate-limit
  pacing in one place, etag conditional GETs, per-installation
  token resolution via the GitHub App credentials from
  `starter-secrets-file`. The rest of `dp-fetcher` uses only this
  wrapper, never raw `octocrab`.
- **Webhook receiver** (`dp-fetcher::webhook`): axum `Router`
  fragment merged by `dp-server`, `POST /webhooks/github`, HMAC
  SHA-256 validation against the rotation-aware secret, enqueue
  to `webhook_inbox` keyed by `delivery_id` (so GitHub
  redeliveries dedupe at the inbox boundary), 200 in under 100ms.
  Deliberately **not** wrapped in `with_principal` — auth is HMAC.
- **Webhook worker** (`dp-fetcher::webhook`): drains the inbox via
  `claim_webhooks` (advisory-lock pattern so concurrent workers
  do not double-process), idempotent upserts via
  `activity_events.external_id`, multi-actor fan-out into
  `event_actors` per §0.2. Per-resource-kind handlers covering
  the webhook events listed in §0.1.
- **Reconciler** (`dp-fetcher::reconciler`): `tokio::time` interval
  (4h default, configurable via `starter-config`), per-(org, repo,
  resource_kind) cursors via `fetch_cursors`, conditional GETs
  through the octocrab wrapper, diff against local store to
  surface missed webhooks, applies through the **same** upsert
  path the webhook worker uses (zero duplication). Scheduler uses
  `Mutex<Option<JoinHandle>>` for coalescing; `fetch-now` CLI and
  `POST /admin/refresh` trigger the same path.
- **Backfill** (`dp-fetcher::backfill`): one-shot per-org job
  invoked by `dp-cli backfill` and at install time, bounded
  historical window (90 days default, configurable via
  `dp-config`), paced separately from the reconciler so it cannot
  starve real-time webhook processing of rate-limit budget,
  resumable via `fetch_cursors`.
- **Run-log entries** in `fetch_runs` per webhook drain batch
  (`kind = webhook_worker`), per reconciler tick (`reconciler`),
  per backfill chunk (`backfill`). Surfaced by `dp-rest`'s
  `/admin/runs` later in Phase 4 — Phase 2 just writes the rows.
- **Co-author / squash-merge / bot / unattributed handling** per
  SCOPE §6, with checked-in fixture payloads under
  `crates/dp-fetcher/tests/fixtures/` and one test per case.

## Out of scope

- Anything in `dp-domain`, `dp-store-pg` schema, or migrations —
  those are Phase 1 (already `[x]` in TODO).
- Reports / query layer / TZ resolution — Phase 3.
- HTTP route auth, OpenAPI, `data_as_of` envelope — Phase 4 (the
  webhook route is the only exception because it lands with the
  receiver).
- MCP, CLI command implementations beyond `fetch-now` / `backfill`,
  frontend, E2E.
- Re-opening any §0 decision from TODO.md. They are inputs, not
  questions for this phase.
- Auto-restart of the worker or reconciler beyond what
  `tokio::spawn` + cooperative shutdown gives us; production
  supervision is the deploy layer's problem.
- Materialised `event_actor_facts` table from §Phase 1 — deferred
  to the first 10k-event load test per TODO Phase 1 note.
- Editing anything under `crates/starter-*` or `packages/`. If
  the work seems to require that, stop and write it up; the
  boundary rule is the entire point.

## Hard rules (load-bearing)

These are inherited from `dev-pulse/TODO.md` §0 and SCOPE; restated
here so the runner re-reads them every stage.

- **R-boundary (§0.6)** — Zero `starter_*` imports in `dp-domain`,
  `dp-fetcher`, `dp-reports`. `dp-store-pg` may import only
  `starter_spi::MigrationSource` and `starter_spi`'s zero-dep
  contract types. `scripts/check-boundaries.sh` enforces in CI;
  this phase must keep it green.
- **R-events (§0.2)** — One `activity_events` row per real GitHub
  event; `(user_id, role)` rows in `event_actors` for every
  human attached. No `user_id` column on the event row. Reports
  join `event_actors` and filter by role per metric.
- **R-cursors (§0.3)** — Cursors live in
  `fetch_cursors(org_id, repo_id, resource_kind, since, etag,
  last_event_id, updated_at)`. The webhook worker is cursor-less.
  Reconciler and backfill both read/write here. `etag` enables
  GitHub's conditional GET (304 on no-change is the cheap path).
- **R-idempotence** — All three paths (worker, reconciler,
  backfill) write through one set of upsert functions keyed by
  `external_id`. Running the same event twice is a no-op. The
  webhook-replay smoke test gates this.
- **R-fast-200** — The webhook receiver returns 200 in under
  100ms (p95). All work happens in the worker. GitHub treats slow
  receivers as failed and redelivers, which inflates load and
  delays signals.
- **R-rate-limit** — Octocrab calls go through one wrapper that
  pauses when the remaining budget drops below the configured
  threshold (default `100` across primary + secondary buckets).
  The wrapper is the only place that knows about
  `X-RateLimit-*`; no caller reads those headers directly.
- **R-no-starter-edit** — Inherited from TODO §0.6. Every stage's
  closing `git` todo runs `scripts/check-boundaries.sh` before
  the commit lands; a failure rolls the commit back.

## Constraints

- GitHub App, not PAT, is the assumed credential model. Webhook
  delivery, per-installation tokens, and the rate-limit budget
  all assume App.
- HMAC secret rotation is supported: two-key window where both
  the current and previous secret verify, with a documented
  cut-over. In-flight signatures fail closed on cut-over; replay
  via `delivery_id` survives rotation because the dedupe is at
  the inbox key, not the signature.
- Conditional GETs use `If-None-Match` against the stored etag
  per `fetch_cursors` row. A 304 advances `updated_at` only —
  not `since` / `last_event_id`.
- Worker concurrency is `dp-config`-controlled (default 1 to
  start; safe to bump because `claim_webhooks` uses an
  advisory-lock pattern).
- Cooperative shutdown: every spawned task observes the
  cancellation channel `dp-server` hands it and exits within
  the configured grace window (default 5s; mirrors the value
  `starter-tools-services` uses elsewhere).

## Deliverables

- `dp-fetcher::client` octocrab wrapper with rate-limit + etag
  handling, full set of wiremock-driven tests.
- `dp-fetcher::webhook` receiver + worker, both wired through
  `dp-server` (receiver as a `Router<AppState>` fragment, worker
  as a spawned task with cooperative shutdown).
- `dp-fetcher::reconciler` interval driver with coalescing
  scheduler + shared do_tick(scope) used by `fetch-now` CLI and
  `POST /admin/refresh`.
- `dp-fetcher::backfill` one-shot job invoked by
  `dp-cli backfill` and at install time, resumable, separately
  paced.
- Fixture set under `crates/dp-fetcher/tests/fixtures/` covering
  co-author, squash-merge, bot, and unattributed cases.
- Seven Phase-2 smoke tests in CI (see §"Smoke tests" below).

## Open questions (resolve in stage 1)

The §0 decisions in TODO.md are **inputs**, not open questions for
this phase. The remaining four are Phase-2-specific:

1. **GitHub App vs PAT.** Bias: confirm App (TODO §6 working
   assumption); webhook delivery, per-installation tokens, and
   the rate-limit budget all assume App. Revisit only if a target
   deployment cannot install an App.
2. **Backfill default window.** Bias: confirm 90 days (TODO §6);
   first target deployment may override via `dp-config`.
3. **Webhook HMAC secret rotation path.** Bias: two-key window
   (current + previous secret both verify), documented cutover.
   `delivery_id` dedupe survives rotation because the inbox key
   is the delivery id, not the signature.
4. **Octocrab rate-limit headroom threshold.** Bias: pause when
   remaining < 100 across primary + secondary buckets. The
   threshold lives in `dp-config`, not hard-coded.

Record decisions in this file under "Decisions" before stage 3
(the first code stage) begins.

## Decisions

(populated in stage 1)

## Smoke tests (Phase-2 merge gate)

- **webhook-replay-same-delivery-id-yields-exactly-one-upsert** —
  POST the same payload twice with the same `X-GitHub-Delivery`;
  inbox has one row, `activity_events` has one row, `event_actors`
  rows match the multi-actor expectation for the event kind.
- **co-authored-commit-fans-out** — push event with N
  `Co-authored-by:` trailers produces N+1 `event_actors` rows
  (author + N `co_author`) for one `activity_events` row.
- **missed-webhook-detected-by-reconciler** — drop a webhook on
  the floor, run one reconciler tick, assert the event appears
  with full multi-actor attribution and the same upsert path the
  worker would have used.
- **backfill-respects-rate-limit-headroom** — wiremock returns
  `X-RateLimit-Remaining: 50` (below the configured threshold);
  backfill pauses until reset, then resumes; no 403s leak past
  the wrapper.
- **scheduler-coalesces-overlapping-ticks** — fire a second
  reconciler tick while the first is mid-run; the second is a
  no-op (the `Mutex<Option<JoinHandle>>` skip path is taken).
- **boundary-check-still-green** —
  `scripts/check-boundaries.sh` reports zero `starter_*` imports
  in `dp-domain` / `dp-fetcher` / `dp-reports`, and only
  `starter_spi::MigrationSource` in `dp-store-pg`.
- **fetch_runs-row-written-per-batch-per-kind** — webhook drain,
  reconciler tick, and backfill chunk each produce one
  `fetch_runs` row with `kind` ∈ `{webhook_worker, reconciler,
  backfill}`, `started`/`finished` timestamps, item/error counts.

## Cross-cutting checks the runner must keep honest

- The webhook receiver and worker share **one** upsert module
  with the reconciler and backfill paths. If two callers
  re-implement the same INSERT shape, idempotence breaks
  silently — refactor to one function.
- Every spawned task observes the cancellation channel.
  `cargo test -p dp-fetcher -- shutdown` exercises the path; a
  task that misses cooperative shutdown fails the smoke test.
- The octocrab wrapper is the **only** place that reads
  `X-RateLimit-*` headers. A grep
  `grep -rn 'X-RateLimit' crates/dp-fetcher/src | grep -v client.rs`
  must yield no hits.
- The boundary script runs in the per-stage closing trio's
  `checks` todo, not only in CI. Pushing a stage that breaks
  the boundary is wasted work — catch it locally.
