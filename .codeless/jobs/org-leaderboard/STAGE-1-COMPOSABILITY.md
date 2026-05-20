# Stage 1 — §15.7 metric-layer composability investigation

> Pinned note for the `org-leaderboard` job. Answers SCOPE open
> questions 1 and 2 and decides whether ORG-REPORTS.md §6.3
> `also_compute` is a field add or a metric-layer refactor.

## TL;DR

**`also_compute` is a field add, not a metric-layer refactor.** The
existing `dp-reports::aggregate` layer is already composable along
every axis §6.3 + §6.8 need. No new stage is required ahead of
stage 3. Stage 7 (`also_compute`) can land as plain projection +
fan-out over the rows the leaderboard already pulls.

One caveat — duration-metric storage isn't wired yet (see §3) —
but that's a Phase-3 follow-up, **not** a leaderboard blocker;
`also_compute` with duration metrics ships when the duration
fetch path ships, and the percentile aggregator we'd reuse is
already in shape.

## 1. Are §15.7 count aggregates additively composable?

**Yes, trivially.** The whole aggregation pipeline operates on
`Vec<EventActorRow>` returned by
`Store::list_event_actor_rows_in_window`
(`crates/dp-store-pg/src/store.rs:614`). The store does one SQL
fetch with optional `kind` / `role` filters applied via
`cardinality($N) = 0 OR … = ANY($N)` predicates — no per-metric
SQL.

The metric layer (`crates/dp-reports/src/aggregate.rs`) then
splits work cleanly:

- `CountMetric` + `METRIC_ROLE_MAP` (lines 94–213) — pure data,
  one const row per §15.7 metric.
- `filter_rows_for_metric(rows, metric, override)` (line 257) —
  pure `EventKind` + `actor_roles` predicate over already-fetched
  rows.
- `count_by_user / _team / _org / _repo / _bucket` (lines
  488–549) — pure reducers, `BTreeMap<Uuid, u64>` outputs.

That means computing N metrics for the same `(window, scope,
subject)` set costs **one** store fetch (widened to the union of
`(kind, role)` predicates across the N metrics) followed by N
calls to `filter_rows_for_metric → count_by_<subject>`. Each
extra metric is O(rows) in memory, no extra round trip.

Concretely, the `also_compute` (cap 5) path is:

```rust
let rows = store.list_event_actor_rows_in_window(
    window,
    orgs, repos, users,
    union_of_default_roles(rank_by_metric, also_compute), // widened
).await?;

let primary_counts = count_by_user(&filter_rows_for_metric(&rows, rank_by, roles_override));
let extras: BTreeMap<CountMetric, BTreeMap<Uuid, u64>> = also_compute
    .iter()
    .map(|m| (*m, count_by_user(&filter_rows_for_metric(&rows, *m, None))))
    .collect();
```

Page boundaries stay deterministic because they're computed from
`primary_counts` alone (sorted by `rank_by` per §6.1); `extras`
just rides along inside `row.context` per ORG-REPORTS.md §3 and
never feeds the sort key — exactly the §6.3 guarantee that
`also_compute` cannot drift page boundaries.

**No refactor required.** The widened-predicate move
(`union_of_default_roles`) is a few lines in the leaderboard
engine; it does not touch `METRIC_ROLE_MAP`, the const table, or
the reducers.

## 2. Is the §15.9 percentile aggregator already reusable for §6.8 `home_org_label`?

**Yes.** `compute_percentiles(&[i64]) -> Percentiles`
(`aggregate.rs:340`) is already a free function over a flat
duration slice, with the `n < 5 → None` floor baked in
(`MIN_PERCENTILE_SAMPLE_N`, line 296). It takes a `&[i64]`, not
a per-user-grouped map, so it's directly callable from any
aggregation path:

- per-user — pass that user's durations.
- per-team — pass the union of member durations.
- per-org — likewise.
- per-`home_org_label` — pass the union of durations across all
  users whose `home_org_label = "<bucket>"`, including the
  `__unlabeled__` bucket.

This is exactly the §6.8 "no averaging-of-averages" rule:
percentiles are computed once over the pooled member sample, not
averaged across per-user percentiles. The function signature
already encodes that — there is no per-user-pre-aggregated input
it would tempt callers to misuse.

The SQL companion `percentile_cont_sql("duration_seconds")`
(line 392) is likewise reusable; the store layer will embed it
inside whichever GROUP BY (`user_id` / `team_id` / `home_org_label`)
the leaderboard subject demands. One helper, one `(p50, p90, p95,
sample_n)` projection list everywhere — meets the §11.4 trust /
§9 transparency bar without further extraction.

**No refactor required.**

## 3. Caveat — duration-metric store path is not wired yet

`DurationMetric` (line 282) and `compute_percentiles` are defined
but there is no `Store::list_duration_samples_in_window` (or
similar) today. The comment at line 277 acknowledges this: "the
actual computation … lives in `dp-store-pg`; this enum only names
the metrics and serves as the contract that future store methods
must satisfy." Grepping confirms no `duration_seconds` SQL
exists.

Impact on the leaderboard job:

- Count-metric leaderboards (the §15.7 default set) are
  unblocked. Stages 3–9 can land as scoped.
- A `rank_by = TimeToMerge` (or any other `DurationMetric`)
  leaderboard request will need a `list_duration_samples_in_window`
  store method first. That is a Phase-3 follow-up the existing
  TODO already tracks; it is **not** an `also_compute` problem
  and **not** a metric-layer refactor — it's a missing fetch
  endpoint.
- Recommended sequencing: implement leaderboard against count
  metrics first; flip duration metrics on once the store fetch
  lands. Document this in the stage-3 handover so it isn't lost.

This is **not** grounds to insert a new stage ahead of stage 3.
Stage 3's brief explicitly scopes "subject = user single-org mode
only" — count metrics are sufficient to validate the envelope
shape, the §6.1 tie-break, the §6.5 cursor, and the §6.2
reconciliation identity. Duration metrics inherit the same
plumbing the moment the store fetch exists.

## 4. SCOPE open-question answers (per WORKFLOW.md REVIEW gate)

**Q1 — Are §15.7 metric aggregates already composable (so
`also_compute` is a projection-list field add) or do they need to
be lifted out of the per-user query path?**

Already composable. The store returns raw `EventActorRow`s for
one (window, scope) tuple; the metric layer is a stack of pure
functions over that slice. `also_compute` is one widened
`actor_roles` predicate at fetch time plus N
`filter_rows_for_metric → count_by_<subject>` calls in memory.
Field add only.

**Q2 — Is SCOPE.md §15.9's percentile aggregator already a
reusable function callable from the `home_org_label` aggregation
path (§6.8), or is it inlined?**

Already reusable. `compute_percentiles(&[i64])` is a free
function with the `n < 5 → None` guard internalised; it doesn't
care whether the input slice was pooled per-user, per-team,
per-org, or per-`home_org_label`. `percentile_cont_sql(column)`
is the matching SQL fragment helper for the store side. No
extraction work needed for §6.8.

## 5. Decision

- **`also_compute` is a field add.** No new stage inserted
  before stage 3.
- **Duration-metric store fetch is a separate, pre-existing
  Phase-3 gap**, flagged in stage 3's handover but not gating
  the leaderboard work itself; the leaderboard ships against
  count metrics first.
- **No edits to `crates/dp-reports/src/aggregate.rs`** are
  required for stages 3–9. The leaderboard engine that lands in
  stage 3 calls the existing reducers; stages 7 (`also_compute`)
  and 8 (`subject_ids`) layer on top without touching this file.

References:
- `crates/dp-reports/src/aggregate.rs` lines 94–213
  (`METRIC_ROLE_MAP`), 257 (`filter_rows_for_metric`), 340
  (`compute_percentiles`), 392 (`percentile_cont_sql`),
  488–549 (count reducers).
- `crates/dp-store-pg/src/store.rs` line 614
  (`list_event_actor_rows_in_window`).
- `ORG-REPORTS.md` §6.3, §6.7, §6.8.
- `SCOPE.md` §15.7, §15.9.
