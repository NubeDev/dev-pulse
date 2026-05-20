## Done

- reviewed stage-1 investigation note (.codeless/jobs/org-leaderboard/STAGE-1-COMPOSABILITY.md) and the stage-0/1 commits 13d85a3 + 6f4419c
- confirmed diff scope is docs-only under .codeless/jobs/ — zero crate, transport, schema, or auth edits
- confirmed §6.3 escape hatch is cheap: `also_compute` is a field add (widened `actor_roles` predicate + N pure reducers on the same `Vec<EventActorRow>`), and `compute_percentiles(&[i64])` is already reusable for §6.8 home_org_label pooling
- no new refactor stage needs to be inserted before stage 3

## Next

- stage 3 (scaffold leaderboard engine: subject=user, single-org mode, count metrics) can proceed against the existing `dp-reports::aggregate` reducers without touching `aggregate.rs`

## What you need to know

- duration-metric store path (`list_duration_samples_in_window`) is still missing — flagged as Phase-3 follow-up, not a leaderboard blocker; leaderboard ships against count metrics first and inherits duration metrics when that fetch lands
- the widened-predicate `union_of_default_roles(rank_by, also_compute)` call lives in the leaderboard engine, not in `METRIC_ROLE_MAP`
- page boundaries must be derived from `primary_counts` (sorted by `rank_by`) only — `also_compute` results ride in `row.context` and must not influence the sort key (§6.3)
- working tree is clean; this REVIEW gate does not produce a commit of its own (per WORKFLOW.md: gates pause the next stage, they do not introduce new commits)

## Open questions

- (none)

PASS: stage-1 docs-only investigation closes SCOPE Q1+Q2 with file:line evidence that §6.3 `also_compute` is a field add over existing composable reducers, so no metric-layer refactor stage is needed and Layer-1 invariants (R1/R2/R4/R5, wire formats) are trivially intact.
