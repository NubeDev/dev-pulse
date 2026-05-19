# Scope — phase-3-reports

> Source of truth: [`TODO.md`](../../../TODO.md) §"Phase 3 — reports"
> in the dev-pulse repo, plus [`SCOPE.md`](../../../SCOPE.md) for
> product scope (especially §6, §8.1, §11.4, §11.7). This file is
> the per-job brief the runner reads before every stage;
> intentionally short. When this file disagrees with TODO.md or
> SCOPE.md, those win — open an issue and update this file.

## Goal

Build the **report query layer** in `dp-reports` that turns the
Phase 1 schema (`activity_events` + `event_actors` + `memberships`,
populated by the Phase 2 ingestion path) into the
three-lens / windowed / role-filtered / percentile-aware data the
v1 UI renders. The layer is pure query + aggregation: zero GitHub
calls (SCOPE §10), zero HTTP / MCP / CLI surface (those are
Phases 4–6), zero `starter_*` imports (§0.6 R-boundary).

The phase succeeds when, given a `ReportRequest` envelope, the
crate returns a `ReportResponse` with the resolved UTC window,
the metric rows for the chosen lens + group_by, and the
`data_as_of` freshness object — and the numbers match a recorded
GitHub fixture within tolerance (SCOPE §11.4).

## In scope

- **Report envelope** (`dp-reports::envelope`): `ReportRequest`
  carries `orgs`, `users`, `teams`, `window_spec`, `scope_mode`
  (single-org / all-orgs-combined / per-org-split), `group_by`,
  `activity_types`, `actor_roles`. `WindowSpec` carries `label`,
  `tz`, `anchor`, and optional `custom_start` / `custom_end`. A
  `resolve_window(spec)` function turns the spec into a concrete
  UTC `[start, end)` and echoes the resolved `Window` back in the
  response (§0.4).
- **Three org-scope lenses** (`dp-reports::lenses`):
  - **single-org** — filter `EventActorRow`s to one org, group as
    requested.
  - **all-orgs-combined** — union across the requested orgs,
    de-dupe on `(user_id, event_id)` pairs so a co-authored commit
    spanning two orgs counts once per user (§0.2).
  - **per-org-split** — group by `(user_id, org_id)` so
    context-switching is visible.
  Each lens is a pure function over `Vec<EventActorRow>` returned
  by one `Store::list_event_actor_rows_in_window` call. No SQL
  branching per lens.
- **Aggregation** (`dp-reports::aggregate`):
  - **Counts** for discrete events. Each metric has a fixed
    `actor_roles` filter recorded in a `const` table — e.g.
    `commits_authored` filters `role IN (author, co_author)`,
    `prs_reviewed` filters `role = reviewer`, etc.
  - **Percentiles** for duration metrics: review turnaround time,
    time-to-first-review, time-to-merge. Implemented via a single
    SQL helper using `percentile_cont`. No means anywhere
    (SCOPE §6).
  - **Sample-size guard**: percentiles return `None` when the
    sample is fewer than 5 events, so the UI renders `—` instead
    of a misleading p95.
  - **Group-by buckets**: per-user, per-team, per-repo, plus the
    trend buckets (day for windows ≤ 31 days, week for
    32–183 days, month beyond — UTC-truncated then re-anchored to
    the window TZ for labelling).
- **`data_as_of` freshness** (`dp-reports::freshness`): every
  response carries a `DataAsOf {webhook_latest, reconciler_latest,
  per_org}` object computed from `fetch_runs`. UI rules
  (SCOPE §11.7):
  - single-org lens → `per_org[that_org]`,
  - all-orgs-combined → `min(per_org.values())`,
  - per-org-split → per row.
  This stage adds the supporting `Store` method (e.g.
  `latest_fetch_runs_per_org`) since Phase 1 did not need it.
- **Spot-check fixture harness** under
  `crates/dp-reports/tests/fixtures/`: at least three checked-in
  fixtures, each one JSON payload + one test. Cases:
  single-user-single-org (sanity), co-authored-commit-spanning-two-orgs
  (the §11.4 + §0.2 regression), home-org-split-on-shared-org
  (the cross-company executive use case from SCOPE §7).

## Out of scope

- HTTP route handlers, OpenAPI annotations, `with_principal`
  wrapping, `audit_log` writes — Phase 4 (`dp-rest`).
- MCP `Tool` impls — Phase 5 (`dp-mcp`).
- CLI commands for report queries — Phase 6 (`dp-cli`).
- Frontend rendering of the three-lens toggle, trend chart, TZ
  anchor selector — Phase 7.
- Materialised `event_actor_facts` table — TODO Phase 1 explicitly
  defers to "first 10k-event load test"; not this phase.
- Anything that touches `crates/starter-*` or `packages/`. If the
  work seems to require it, stop and write it up. R-boundary §0.6
  is enforced by CI.
- Re-opening any §0 decision from TODO.md or any decision locked
  in Phase 2's SCOPE.md. They are inputs.
- Lines-changed / "developer score" / leaderboard metrics — SCOPE
  §4 design constraint, not a v1 metric.
- Changes to ingestion behaviour. If a report reveals a fetcher
  bug, write it up; do not fix it here.

## Hard rules (load-bearing)

These are inherited from `dev-pulse/TODO.md` §0 and SCOPE; restated
so the runner re-reads them every stage.

- **R-boundary (§0.6)** — Zero `starter_*` imports in `dp-reports`.
  `scripts/check-boundaries.sh` enforces in CI; this phase must
  keep it green.
- **R-events (§0.2)** — One `activity_events` row per real GitHub
  event; `(user_id, role)` rows in `event_actors` per human
  attached. Reports join `event_actors` and filter by role per
  metric. De-dup on the all-orgs-combined lens operates on
  `(user_id, event_id)` pairs, **not** event rows alone.
- **R-window-server-side (§0.4)** — The frontend never resolves
  "last week" itself. The server takes `(label, tz, anchor)`,
  produces UTC `[start, end)`, and echoes the resolved window
  back so the UI can label it unambiguously.
- **R-no-means (SCOPE §6)** — Duration metrics use percentiles
  (p50 / p90 / p95) via `percentile_cont`. Means are forbidden
  because of long-tail distortion. Grep-guarded in the smoke
  tests.
- **R-no-leaderboard (SCOPE §4)** — The aggregation layer offers
  no single-score affordance. Comparisons require explicit group
  selection. This is enforced by *not building* the affordance,
  not by a runtime check.
- **R-data-as-of (§0.3, §11.7)** — Every report response carries
  the `DataAsOf` envelope. A response without it is incomplete;
  Phase 4 cannot wire a handler that drops it.
- **R-no-starter-edit** — Inherited from TODO §0.6. The boundary
  script runs in the per-stage closing trio's `checks` todo, not
  only in CI.

## Constraints

- Percentiles are computed in SQL (`percentile_cont`) inside one
  helper in `dp-store-pg` exposed through a typed `Store` method,
  not by pulling raw durations into Rust and sorting in memory —
  at expected scale (~10k events/day, growing) the round-trip
  matters.
- The role→metric mapping is a `const` table in `dp-reports`,
  not a config knob. v1 metric definitions are fixed; new metrics
  ship as code changes, not runtime configuration.
- Trend bucket granularity is fixed per window length (day ≤ 31d,
  week 32–183d, month > 183d). The UI does not pick; the report
  layer does. Re-anchor to the window TZ for labelling, but
  truncate in UTC for grouping.
- Sample-size guard for percentiles is `n >= 5`. Below that,
  return `None`. The UI renders `—` (this is documented behaviour
  for Phase 7, not enforced by the report layer beyond returning
  `None`).
- The store method `list_event_actor_rows_in_window` already
  exists (Phase 1); the report layer uses it as-is. If a new
  projection is genuinely needed (e.g. for percentile sources),
  extend `Store` through `dp-domain` — never reach into
  `dp-store-pg` from `dp-reports`.

## Deliverables

- `dp-reports::envelope`: `ReportRequest`, `WindowSpec`,
  `ScopeMode`, `GroupBy`, `ActivityType`, and `resolve_window`.
- `dp-reports::lenses`: three pure functions (one per lens) over
  `Vec<EventActorRow>` returning the lens-shaped rows.
- `dp-reports::aggregate`: counts + percentiles + group-by + the
  role→metric `const` table.
- `dp-reports::freshness`: `DataAsOf` + one new `Store` method
  for per-org / per-kind freshness.
- Three checked-in fixtures + tests under
  `crates/dp-reports/tests/fixtures/`.
- Six Phase-3 smoke tests in CI (see §"Smoke tests" below).

## Open questions (resolve in stage 1)

The §0 decisions in TODO.md are **inputs**, not open questions
for this phase. The remaining four are Phase-3-specific:

1. **Envelope shape final for v1?** Bias: yes — `orgs`, `users`,
   `teams`, `window_spec`, `scope_mode`, `group_by`,
   `activity_types`, `actor_roles`. Phase 4 handlers and Phase 5
   MCP tools mirror this verbatim; changing it later means
   coordinated edits across three crates.
2. **Role→metric mapping table.** Bias: lock the v1 metric set
   (commits authored, PRs opened, PRs merged, PRs closed, reviews
   given, review comments, issues opened, issues commented,
   review turnaround, time-to-first-review, time-to-merge) and
   the exact `ActorRole` filter for each. New metrics ship as
   code in later phases.
3. **Trend bucket granularity.** Bias: day ≤ 31 days, week
   32–183 days, month > 183 days. UTC-truncated for grouping,
   re-anchored to the window TZ for labelling.
4. **Percentile sample-size guard.** Bias: `n >= 5` or return
   `None`. Below 5 the noise overwhelms the signal and the UI
   renders `—`.

Record decisions in this file under "Decisions" before stage 3
(the first code stage) begins.

## Decisions

(populated in stage 1)

## Smoke tests (Phase-3 merge gate)

- **resolved-window-echoes-back-with-anchor-preserved** —
  `ReportRequest` with `(label="last_week", tz="Australia/Sydney",
  anchor=Org)` resolves to a UTC `[start, end)` and the response
  echoes `tz`, `anchor`, `label` unchanged.
- **three-lens-numbers-correct-on-co-author-fixture** — single
  fixture, one co-authored commit spanning org-A and org-B; the
  three lenses produce three distinct, correct row sets, and the
  all-orgs-combined de-dup on `(user_id, event_id)` holds.
- **percentile_cont-returns-none-when-sample-under-five** —
  fixture with four review-turnaround samples; p50/p90/p95 all
  return `None`. Same fixture with five → all three return
  concrete values.
- **percentiles-match-expected-on-recorded-fixture** — the
  recorded-fixture harness runs against
  `tests/fixtures/recorded-*.json` and asserts numbers within
  tolerance (SCOPE §11.4 trust gate).
- **data_as_of-per-org-and-combined-match-fetch_runs** — seed
  `fetch_runs` with per-org reconciler ticks; the response's
  `DataAsOf.per_org` matches; `min(per_org.values())` matches
  what the combined lens would render.
- **boundary-check-still-green** —
  `scripts/check-boundaries.sh` reports zero `starter_*` imports
  in `dp-reports`.

A seventh grep-guard runs in the per-stage closing trio:
`grep -rn 'avg\|mean' crates/dp-reports/src | grep -v '// not used'`
must yield no hits in metric code (R-no-means).

## Cross-cutting checks the runner must keep honest

- The role→metric mapping is **one** `const` table. If two
  modules define their own role filter for the same metric,
  numbers will diverge silently — refactor to one source.
- Percentiles go through **one** SQL helper. A second
  implementation in Rust means two sources of truth for the same
  number. Grep-guard:
  `grep -rn 'percentile_cont' crates/dp-store-pg/src` should
  yield exactly one hit.
- The Window type is owned by `dp-domain::window` and re-used
  here. `dp-reports` does not redefine it.
- The boundary script runs in the per-stage closing trio's
  `checks` todo. Pushing a stage that breaks the boundary is
  wasted work — catch it locally.
- No new `starter_*` import lands in any commit. If a stage feels
  like it needs one, stop and write it up.
