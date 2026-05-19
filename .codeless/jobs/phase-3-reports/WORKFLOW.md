# Workflow — phase-3-reports

How to drive this job. The shape is "lock the envelope and metric
mapping first because every later stage is shaped by them, then
land the lenses (pure functions, easy to test), then the
aggregation layer, then the data_as_of envelope (which needs a new
Store method), then prove it all with three recorded-fixture tests
and six smoke tests."

## Sequencing

- Stage 1 is **prose-only**. Lock the four Phase-3-specific open
  questions in [SCOPE.md](./SCOPE.md), record under "Decisions",
  commit. No code.
- Stage 3 (envelope + window resolution) lands first because
  **every other stage uses these types**. A wrong envelope shape
  here cascades into Phase 4 (`dp-rest`) and Phase 5 (`dp-mcp`)
  later.
- Stage 4 (the three lenses) lands before aggregation because
  lenses produce the shape aggregation consumes. Each lens is a
  pure function over `Vec<EventActorRow>` — fast to unit-test
  with seeded in-memory data.
- Stage 5 (aggregation) reuses the lens output and the
  role→metric `const` table from stage 1. Percentiles need a
  Postgres helper — touch `dp-store-pg` here, route through a
  new `Store` method on `dp-domain`. Never reach into
  `dp-store-pg` from `dp-reports`.
- Stage 6 (data_as_of) adds one more `Store` method
  (per-org / per-kind freshness from `fetch_runs`) and the
  `DataAsOf` envelope every response carries. Without this,
  Phase 4 cannot wire a handler that respects R-data-as-of.
- Stage 8 (recorded-fixture harness) is the **trust gate**
  (SCOPE §11.4). If the numbers do not match recorded GitHub
  payloads within tolerance, the report layer is wrong — fix the
  lens, the aggregation, or the role→metric mapping; do not
  loosen the tolerance.
- Stage 9 (smoke tests in CI) is the merge gate.

## Per-stage discipline

- Before any code change in a stage:
  - `git log -20 --oneline` for the surrounding history.
  - Re-read the rule numbers in [SCOPE.md](./SCOPE.md) that the
    stage touches. R-boundary, R-events, R-window-server-side,
    R-no-means, and R-data-as-of are the load-bearing ones for
    Phase 3.
  - Re-read the relevant §0 decision in
    [`../../../TODO.md`](../../../TODO.md): §0.2 (multi-actor) for
    every lens / aggregation stage, §0.3 (cursors / freshness) for
    the `data_as_of` stage, §0.4 (TZ) for the envelope /
    window-resolution stage.
  - For any stage touching the Store trait, read
    `crates/dp-domain/src/store.rs` first. New methods go through
    `dp-domain` then `dp-store-pg`. `dp-reports` does not import
    `dp-store-pg` directly — it takes `Arc<dyn Store>`.
- Touch only what the stage names. No drive-by refactors.
- Verify before commit:
  - **Boundary check first**: run
    `scripts/check-boundaries.sh`. A failure here is the cheapest
    signal the change is shaped wrong. Do not silence the script.
  - **Rust**: `cargo check -p dp-reports -p dp-domain -p dp-store-pg`,
    then `cargo test -p dp-reports`, then
    `cargo clippy --workspace --all-targets -- -D warnings`.
  - **No-means grep**:
    `grep -rn 'avg\|mean' crates/dp-reports/src | grep -v '// not used'`
    must yield no hits in metric code.
  - **Stage-specific smoke**: every stage's Done column below
    lists the smoke subset it must pass. The full sweep gates
    stage 9; per-stage passes gate per-stage merges.
- Commit only if green. One logical batch per commit; commit
  message stage-tagged: `stage N: <one-line title>`.

## REVIEW gates

Two:

- **After stage 1** — decisions sign-off before any code lands.
  The four Phase-3-specific questions (envelope shape, role→metric
  mapping, trend bucket granularity, percentile sample-size
  guard) ripple through every later stage and across Phases 4–5.
- **After stage 7** — report layer end-to-end against fixtures.
  The three lenses produce correct numbers on the co-author
  cross-org fixture (the SCOPE §11.4 trust gate), counts +
  percentiles match expected values, data_as_of echoes the right
  timestamps, the resolved Window echoes back with anchor
  preserved. Phase 4 wires these into `dp-rest` next; gating here
  costs less than rewinding from Phase 4 with a lens bug.

Write a one-line summary into the handover at each gate. Do not
proceed.

## What "done" looks like per stage

| Stage | Done when |
|---|---|
| 1 | SCOPE.md "Decisions" section filled in for all four open questions; no code changed; boundary check green (trivially). |
| 3 | `ReportRequest`, `WindowSpec`, `ScopeMode`, `GroupBy`, `ActivityType` types live in `dp-reports::envelope`; `resolve_window(spec)` is table-driven-tested for viewer/org/utc across DST + leap day + year-end; `Window` from `dp-domain::window` is re-used (not redefined); resolved-window-echoes-back smoke passes; boundary check green. |
| 4 | Three pure lens functions in `dp-reports::lenses` over `Vec<EventActorRow>`; single-org / all-orgs-combined / per-org-split each unit-tested with a seeded fixture; three-lens-numbers-correct-on-co-author-fixture smoke passes (de-dup on `(user_id, event_id)` correct in combined); boundary check green. |
| 5 | `dp-reports::aggregate` exposes counts + p50/p90/p95 + group-by buckets; the role→metric `const` table covers the v1 metric set from stage 1's decision; percentiles route through one SQL helper in `dp-store-pg` exposed via one new `Store` method on `dp-domain`; percentile_cont-returns-none-when-sample-under-five smoke passes; no-means grep clean; boundary check green. |
| 6 | `DataAsOf {webhook_latest, reconciler_latest, per_org}` type lives in `dp-reports::freshness`; one new `Store` method computes it from `fetch_runs`; every report response carries it; `data_as_of-per-org-and-combined-match-fetch_runs` smoke passes; boundary check green. |
| 8 | Three checked-in fixtures under `crates/dp-reports/tests/fixtures/`: single-user-single-org, co-authored-commit-spanning-two-orgs, home-org-split-on-shared-org; each has one JSON payload + one test loading it, seeding an in-memory `Store`, running the report pipeline, asserting numbers within tolerance; `percentiles-match-expected-on-recorded-fixture` smoke passes; boundary check green. |
| 9 | Six Phase-3 smoke tests all green in CI: resolved-window-echoes-back, three-lens-numbers-correct, percentile-none-under-five, percentiles-match-recorded-fixture, data_as_of-matches-fetch_runs, boundary-check-green. No-means grep guard clean. |

## Anti-patterns

- A second role→metric mapping. R-events §0.2 + the stage-1
  decision — one `const` table; every metric reads it. If two
  modules define their own filter for the same metric, numbers
  diverge silently.
- A second percentile implementation in Rust. R-no-means + the
  Phase-3 percentile rule — one SQL helper using `percentile_cont`
  in `dp-store-pg`; everything else routes through it.
- Pulling raw durations into Rust and sorting in memory. At
  expected scale (~10k events/day, growing) the round-trip
  cost matters; the SQL helper is the right shape.
- Resolving "last week" on the frontend. R-window-server-side
  §0.4 — the server resolves `(label, tz, anchor)` to UTC and
  echoes the resolved window back. The frontend takes a string
  label and renders the echoed UTC range.
- Returning a percentile when the sample is small. R-no-means +
  the stage-1 sample-size guard (`n >= 5`) — return `None`, let
  the UI render `—`. A p95 from three samples is misinformation.
- De-duplicating on event rows alone in the all-orgs-combined
  lens. R-events §0.2 — de-dup operates on `(user_id, event_id)`
  pairs. A co-authored commit spanning two orgs counts once per
  user, not once total.
- Reaching into `dp-store-pg` from `dp-reports`. The trait
  surface is `dp-domain::store::Store`; `dp-reports` takes
  `Arc<dyn Store>`. New methods land on the trait first.
- Importing anything from `starter_*` into `dp-reports`.
  R-boundary §0.6 — zero starter imports allowed in this crate.
  The boundary script enforces; a workaround is wrong code.
- A `mean` or `avg` in metric code. SCOPE §6 — long-tail
  distortion makes means useless for these metrics. Grep-guarded.
- A single-score / leaderboard affordance "because it would be
  easy to add". SCOPE §4 — explicit non-goal, enforced by
  *not building* the affordance.
- Trying to land Phase 4 routes "while we're here." Out of
  scope; Phase 4 is its own job.
- Loosening a fixture's tolerance to make a test pass.
  SCOPE §11.4 — recorded-fixture numbers are the trust gate.
  If they do not match, the bug is in the report code, not the
  tolerance.

## Closing trio — the last three todos of every stage

Every stage's todo checklist ends with the same three items, in
order. The user watches these tick over in the `Stages` overview;
they are how the user confirms a long-running stage actually
landed instead of just looking like it did. Do **not** rename or
reorder them.

1. `checks` — run the stage's verify list, **including**
   `scripts/check-boundaries.sh` before any cargo invocation,
   **and** the no-means grep guard for any stage touching
   `dp-reports/src`. Every step must pass. On failure: stop,
   fix, re-run; do not advance to `docs`.
2. `docs` — update `handover.md` for the next stage and the active
   session doc, in the same worktree, so the fresh agent that opens
   the next stage has the context it needs.
3. `git` — stage the changes, commit with the message
   `stage N: <one-line title from template.yaml>`, and push to the
   job's branch (`codeless/phase-3-reports`).

A stage is not "done" until all three are green and the push
succeeds. Never `--force`, never `--no-verify`; if a hook fails,
fix the cause.
