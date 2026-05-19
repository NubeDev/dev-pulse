# Workflow — phase-2-ingestion

How to drive this job. The shape is "land the octocrab wrapper
first because every other path uses it, then the webhook
receiver+worker (the live path), then the reconciler and backfill
which share its upsert handlers, then prove it all with seven
smoke tests."

## Sequencing

- Stage 1 is **prose-only**. Lock the four Phase-2-specific open
  questions in [SCOPE.md](./SCOPE.md), record under "Decisions",
  commit. No code.
- Stage 3 (octocrab wrapper) lands first because **every other
  stage uses it**. A wrapper bug surfaces here against wiremock,
  not later against a real GitHub installation with a hot
  rate-limit budget.
- Stages 4 + 5 (webhook receiver + worker) land in order. The
  receiver can be tested in isolation against `webhook_inbox`;
  the worker hangs off the same table. Co-author / squash-merge /
  bot handling is stage 7 (its own stage) so the worker stage can
  focus on the drain loop and the upsert plumbing.
- Stage 7 (the §6 multi-actor handling) lands after the worker
  exists but **before** REVIEW would close Phase 2 — the §6
  caveats are the ones that make the worker correct, not just
  running.
- Stage 8 (reconciler) reuses the worker's upsert module. If a
  diff in the worker stage forced the upserts to live somewhere
  callable from both paths, this stage just wires it. If not,
  refactor now; do not duplicate.
- Stage 9 (backfill) reuses the same upsert path and the same
  octocrab wrapper, with a separate pacing budget.
- Stage 10 (smoke tests) is the merge gate.

## Per-stage discipline

- Before any code change in a stage:
  - `git log -20 --oneline` for the surrounding history.
  - Re-read the rule numbers in [SCOPE.md](./SCOPE.md) that the
    stage touches. R-boundary, R-idempotence, R-fast-200, and
    R-rate-limit are the load-bearing ones for Phase 2.
  - Re-read the relevant §0 decision in
    [`../../../TODO.md`](../../../TODO.md): §0.1 (architecture)
    and §0.3 (cursors) for every fetcher stage; §0.2
    (multi-actor) for the worker and the §6 stage; §0.5
    (deletion model) is read-only here but the upserts must
    respect the pseudonymisation pattern (insert `event_actors`
    rows even when the actor user might later be soft-deleted).
  - For any stage touching octocrab or fetch_cursors, read
    `crates/dp-fetcher/src/client.rs` (or its in-progress shape)
    first — the wrapper is the only place that knows about
    rate-limit headers and etags.
- Touch only what the stage names. No drive-by refactors.
- Verify before commit:
  - **Boundary check first**: run
    `scripts/check-boundaries.sh`. A failure here is the cheapest
    signal that the change is shaped wrong. Do not silence the
    script.
  - **Rust**: `cargo check -p dp-fetcher -p dp-store-pg -p dp-server`,
    then `cargo test -p dp-fetcher`, then
    `cargo clippy --workspace --all-targets -- -D warnings`.
  - **Stage-specific smoke**: every stage's Done column below
    lists the smoke subset it must pass. The full sweep gates
    stage 10; per-stage passes gate per-stage merges.
- Commit only if green. One logical batch per commit; commit
  message stage-tagged: `stage N: <one-line title>`.

## REVIEW gates

Two:

- **After stage 1** — decisions sign-off before any code lands.
  The four Phase-2-specific questions (App vs PAT, backfill
  window, HMAC rotation path, rate-limit headroom) are small but
  ripple through every later stage.
- **After stage 6** — webhook path end-to-end: receiver returns
  200 fast, worker drains the inbox, idempotent upserts hold,
  the webhook-replay smoke test passes. The reconciler and
  backfill ride on the same handlers; gating here costs less
  than rewinding from stage 9 if the handlers turn out wrong.

Write a one-line summary into the handover at each gate. Do not
proceed.

## What "done" looks like per stage

| Stage | Done when |
|---|---|
| 1 | SCOPE.md "Decisions" section filled in for all four open questions; no code changed; boundary check green (trivially). |
| 3 | `dp-fetcher::client` exposes a typed client used by every other dp-fetcher caller; wiremock tests cover happy / 304 / 401 / 403-secondary-rate / 429 / 5xx; no caller reads `X-RateLimit-*` headers directly (grep guard); boundary check green. |
| 4 | `POST /webhooks/github` validates HMAC SHA-256 with constant-time compare against the rotation-aware secret, enqueues `webhook_inbox` keyed by `delivery_id`, returns 200 in under 100ms (p95 against a fixture set); structured tracing emits `webhook.delivery_id`; route is NOT principal-wrapped; boundary check green. |
| 5 | Worker drains `webhook_inbox` via `claim_webhooks` advisory-lock pattern; idempotent upserts via `external_id` for every resource kind in §0.1; cooperative shutdown verified by a unit test that flips the cancellation channel and asserts the join handle resolves within 5s; writes one `fetch_runs` row per drain batch (`kind = webhook_worker`); boundary check green. |
| 7 | Each of the four §6 cases (co-authored push, squash-merge, bot author, unattributed) has one fixture under `crates/dp-fetcher/tests/fixtures/` and one test asserting the expected `event_actors` shape; the historical-commits-before-user-exists case lazily `upsert_user`s before `add_event_actors`; boundary check green. |
| 8 | Reconciler tokio interval (4h default, configurable via `starter-config`) drives `do_tick(scope)` shared with `fetch-now` and `POST /admin/refresh`; per-(org, repo, resource_kind) cursors read/write through `fetch_cursors`; conditional GETs hit `client.rs` etag plumbing; missed-webhook smoke passes; scheduler coalescing via `Mutex<Option<JoinHandle>>` works for overlapping ticks; writes `fetch_runs` rows with `kind = reconciler`; boundary check green. |
| 9 | `dp-cli backfill <org>` runs a one-shot bounded-window backfill (default 90 days from `dp-config`); separate rate-limit pacing so reconciler / worker budgets are untouched; resumable via `fetch_cursors` so a crashed backfill picks up where it left off; writes `fetch_runs` rows with `kind = backfill`; boundary check green. |
| 10 | Seven Phase-2 smoke tests all green in CI: webhook-replay-yields-one-upsert, co-authored-commit-fans-out, missed-webhook-detected-by-reconciler, backfill-respects-rate-limit-headroom, scheduler-coalesces, boundary-check-green, fetch_runs-row-per-batch-per-kind. |

## Anti-patterns

- A second place that parses `X-RateLimit-Remaining` or
  `X-RateLimit-Reset`. R-rate-limit — the octocrab wrapper is the
  only authority; every other caller is rate-limit-naive. The
  grep guard exists specifically to catch a second reader; do
  not silence it.
- Importing anything from `starter_*` into `dp-domain`,
  `dp-fetcher`, or `dp-reports`. R-boundary (§0.6) — the only
  starter import allowed is `starter_spi::MigrationSource`, and
  only in `dp-store-pg`. The boundary script enforces; a
  workaround is wrong code, not a tooling problem.
- Doing the upsert work in the webhook receiver instead of the
  worker. R-fast-200 — the receiver enqueues and returns 200
  fast (under 100ms p95). Anything else inflates GitHub's
  redelivery rate and delays signals across the board.
- A `user_id` column on `activity_events`. R-events (§0.2) — one
  event row, many `event_actors` rows. Co-authored commits and
  multi-reviewer PRs need this split; collapsing them either
  drops co-authors or double-counts.
- Two upsert implementations across the worker, reconciler, and
  backfill. R-idempotence — one set of functions, shared. If the
  shapes diverge, the webhook-replay smoke test masks bugs the
  reconciler will surface in production.
- A reconciler that performs full re-pulls on any failure. §0.3
  — the per-(org, repo, resource_kind) cursor with etag is the
  whole point. A full re-pull is rate-limit suicide at
  ~1000-repo scale.
- A backfill that shares the reconciler's rate-limit budget
  uniformly. Backfill is bounded-window and one-shot; if it
  shares the budget without a pacing offset, real-time signals
  starve during install. Separate budget or weighted pacing.
- A worker that crashes the whole process on a malformed event.
  GitHub sends weird things; log, mark the inbox row's `error`
  column, advance `processed_at`, continue. One bad event is not
  a fleet outage.
- An HMAC compare that uses `==`. Constant-time compare is the
  only correct shape; the smoke harness includes a timing-leak
  fuzz test as a follow-up if a reviewer finds the budget.
- Trying to land Phase 3 reports work "while we're here." Out of
  scope; Phase 3 is its own job.

## Closing trio — the last three todos of every stage

Every stage's todo checklist ends with the same three items, in
order. The user watches these tick over in the `Stages` overview;
they are how the user confirms a long-running stage actually
landed instead of just looking like it did. Do **not** rename or
reorder them.

1. `checks` — run the stage's verify list, **including**
   `scripts/check-boundaries.sh` before any cargo invocation.
   Every step must pass. On failure: stop, fix, re-run; do not
   advance to `docs`.
2. `docs` — update `handover.md` for the next stage and the active
   session doc, in the same worktree, so the fresh agent that opens
   the next stage has the context it needs.
3. `git` — stage the changes, commit with the message
   `stage N: <one-line title from template.yaml>`, and push to the
   job's branch (`codeless/phase-2-ingestion`).

A stage is not "done" until all three are green and the push
succeeds. Never `--force`, never `--no-verify`; if a hook fails,
fix the cause.
