## Done

- Wrote the Stage 1 investigation note at `.codeless/jobs/org-leaderboard/STAGE-1-COMPOSABILITY.md` answering both SCOPE open questions (1 and 2)
- Decision: §6.3 `also_compute` is a field add, not a refactor — no new stage inserted ahead of stage 3
- Updated `.codeless/jobs/org-leaderboard/SCOPE.md` to mark open questions 1 and 2 resolved with a pointer to the note
- Wrote `.codeless/jobs/org-leaderboard/handover.md` for the next stage
- Committed as `stage 1: investigate §15.7 metric layer composability` on branch `codeless/org-leaderboard` (commit `6f4419c`)

## Next

- Stage 2 (REVIEW gate, no code) — confirm the §6.3 escape hatch is cheap given the stage-1 finding; release stage 3 to start
- Stage 3 — scaffold `LeaderboardEnvelope` + thin SQL builder for `subject = user` single-org mode only

## What you need to know

- `dp-reports::aggregate` (`CountMetric`, `METRIC_ROLE_MAP`, `filter_rows_for_metric`, `count_by_*`, `compute_percentiles`, `percentile_cont_sql`) is already composable enough for §6.3 and §6.8 — do NOT refactor it during stages 3–9
- The store path (`Store::list_event_actor_rows_in_window`, `crates/dp-store-pg/src/store.rs:614`) already takes optional `kind`/`role` filters via `cardinality(...) = 0 OR ... = ANY(...)`, so `also_compute` widens those arrays at fetch time, then N reducers run in memory
- Caveat for stage 3: there is no duration-metric store fetch yet (`DurationMetric` is defined but no SQL); leaderboard ships against count metrics first, duration metrics inherit the plumbing later. This caveat must be carried forward in stage 3's handover

## Open questions

- (none) — the two stage-1 open questions are resolved; SCOPE open questions 3 and 4 belong to later stages
