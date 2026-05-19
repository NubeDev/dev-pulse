## Done

- Added `crates/dp-reports/src/aggregate.rs` (790 LOC) implementing the Phase 3 aggregation layer.
- `METRIC_ROLE_MAP` const materialises the SCOPE §15.7 role→metric table; 13 `CountMetric` variants (skipped `issues.assigned` and `pull_request_review_requested` — their `EventKind` variants don't exist yet in `dp-domain`).
- `filter_rows_for_metric(rows, metric, actor_roles_override)` narrows by `(kind, role∈roles)`, honouring the §15.6 envelope override.
- `compute_percentiles(&[i64]) -> Percentiles { p50, p90, p95: Option<f64>, sample_n }` — pure-Rust `percentile_cont` matching Postgres interpolation. Below `MIN_PERCENTILE_SAMPLE_N = 5` all three percentiles are `None`, `sample_n` is always reported. `percentile_cont_sql("col")` emits the SQL fragment for `dp-store-pg` to embed.
- `DurationMetric` enum (`TimeToFirstReview`, `TimeToMerge`, `ReviewTurnaround`) reserved for the store-side duration query.
- `pick_trend_bucket(window)` implements SCOPE §15.8 (≤31d Day, 32–183d Week, >183d Month, both boundaries inclusive on the day side). `truncate_to_bucket(ts, bucket, tz)` truncates in the window TZ and returns UTC, mirroring `date_trunc(…, ts AT TIME ZONE tz)`.
- Group-by reducers: `count_by_user`, `count_by_repo`, `count_by_org`, `count_by_bucket`, `count_by_team(rows, |uid| Option<team_id>)`. All return `BTreeMap` for stable iteration; total-conservation tested.
- Wired the new module + key items into `crates/dp-reports/src/lib.rs`.
- `cargo test -p dp-reports` → 37 passed (21 prior + 16 new). `scripts/check-boundaries.sh` → OK.
- Committed as `c9c9e45` on `codeless/phase-3-reports` with commit message starting `stage 5: aggregation layer in dp-reports::aggregate`.

## Next

- Stage 6 (next session). Likely the response-shaping layer that ties envelope + lens + aggregate together into a single `run_report(...)` plus the `data_as_of` object (SCOPE §0.3). The spot-check fixture harness (SCOPE §11.4) is still outstanding for Phase 3.

## What you need to know

- `EventActorRow` has no `team_id`; `count_by_team` accepts a `FnMut(Uuid) -> Option<Uuid>` resolver. Users with no team are skipped silently (the "unaffiliated" bucket is a frontend concern).
- `Percentiles::p50/p90/p95` are `Option<f64>` *not* `Option<i64>`; `percentile_cont` interpolates between ranked values so the result is fractional seconds. Wire format keeps the keys with `null` (not omitted) at n<5.
- `percentile_cont_sql` interpolates the column name as-is; pass a hard-coded column, never user input. The n<5 guard is applied row-side (in `dp-store-pg`'s mapper using `Percentiles::too_small`), not in SQL.
- `pick_trend_bucket` uses `(end-start).num_days()`; for a DST fall-back week this yields 7 (because `+1h`) and for spring-forward 6 — neither crosses the 31/183 boundaries in practice. If that ever matters, switch to `ceil(seconds/86400)`.
- `truncate_to_bucket` handles ambiguous (fall-back) local times by taking the earliest; skipped local times fall back to UTC midnight (very rare — only midnight-DST zones).
- The `METRIC_ROLE_MAP` const is order-preserved by SCOPE §15.7. Adding metrics goes at the bottom, never re-order.
- `CountMetric::role_entry()` panics on a missing row — that's deliberate (catches "added enum variant without const row"). The `metric_role_map_covers_every_count_metric_variant` test enforces it.

## Open questions

- SCOPE §15.7 lists `issues assigned` (kind `issues.assigned`) and `review requests received` (kind `pull_request_review_requested`) but `dp_domain::event::EventKind` doesn't model those yet. Stage 6 or a follow-on Phase 2 ingestion task should decide whether to extend `EventKind` (then add the matching `CountMetric` rows) or leave both metrics out of v1.
