# Handover — after stage 1 (composability investigation)

Stage 1 is done. Next agent picks up **stage 2** (REVIEW gate, no
code) and then **stage 3** (scaffold the `LeaderboardEnvelope`
type + thin SQL builder for `subject = user` single-org mode).

## What the investigation concluded

See `STAGE-1-COMPOSABILITY.md` in this dir for the full note. The
short version:

- **§6.3 `also_compute` is a field add, not a refactor.** The
  `dp-reports::aggregate` layer (`CountMetric`, `METRIC_ROLE_MAP`,
  `filter_rows_for_metric`, `count_by_*` reducers) is already
  composable over `Vec<EventActorRow>` returned by one store
  fetch. N metrics = one widened-predicate fetch + N pure
  reducers in memory.
- **§6.8 `home_org_label` percentile aggregation reuses
  `compute_percentiles(&[i64])`** — it's already a free
  function with the §15.9 `n < 5 → None` floor baked in.
- **No new stage is inserted before stage 3.**

## Caveat the next stage must respect

`DurationMetric` is defined in `aggregate.rs` but the matching
store fetch (`list_duration_samples_in_window` or equivalent) is
not implemented yet. Stages 3–9 scope themselves to count metrics
first; duration metrics inherit the leaderboard plumbing the
moment the store fetch lands. Flag this explicitly in the stage-3
handover so it isn't lost.

## Files touched this stage

- `.codeless/jobs/org-leaderboard/STAGE-1-COMPOSABILITY.md` —
  new, the pinned investigation note.
- `.codeless/jobs/org-leaderboard/SCOPE.md` — open questions 1
  and 2 marked resolved, pointing at the note.
- `.codeless/jobs/org-leaderboard/handover.md` — this file.

No code under `crates/` was changed. No tests need to run for this
stage; the `checks` step is "the note is on disk and SCOPE.md
points at it" — both true.

## For the REVIEW gate (stage 2)

WORKFLOW.md requires the stage-1 handover to include:
- A one-paragraph answer to SCOPE open questions 1 and 2 —
  done, both in SCOPE.md and in STAGE-1-COMPOSABILITY.md §4.
- The decision on whether `also_compute` is a field add or a
  refactor — **field add**, see STAGE-1-COMPOSABILITY.md §5.
- If a refactor: a proposed new stage to insert before stage 3 —
  not applicable, no refactor.

The REVIEW gate should also note the duration-metric store-fetch
caveat (above) so stage 3 inherits it.
