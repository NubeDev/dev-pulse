## Done

- Locked Phase 3 open questions in `SCOPE.md` as new Decisions §15.6–§15.9:
- §15.6 Report envelope shape (`orgs`, `users`, `teams`, `window`, `scope_mode`, `group_by`, `activity_types`, `actor_roles`) — frozen for v1 so Phase 4 REST and Phase 5 MCP mirror it verbatim.
- §15.7 Role→metric mapping table — every count metric has exactly one default `actor_roles` filter; `commits authored` is the only default-union (`author + co_author`) per SCOPE §6 co-author credit.
- §15.8 Trend bucket granularity — day ≤ 31d, week 32–183d, month > 183d; `date_trunc` in window TZ, bucket-start converted back to UTC; response carries `trend.bucket`; empty buckets emitted as zeros.
- §15.9 Percentile semantics — `percentile_cont` for p50/p90/p95 over duration_seconds; all three nulled (with `sample_n` exposed) when n < 5; no means.
- Each decision lists "Revisit triggers" and what it resolves.
- Committed on branch `codeless/phase-3-reports` as `d002034`, message starts with `stage 1:`.

## Next

- Stage 2 picks up implementation against these locked decisions — likely the `dp-reports` envelope types + `Window` resolver in `crates/dp-reports`, with zero `starter_*` imports (§0.6 boundary).

## What you need to know

- TODO §0 decisions remain read-only inputs (already in §15.5); the new sections only cover Phase 3 questions.
- `ScopeMode` variants used in §15.6 are `SingleOrg | AllOrgsCombined | PerOrgSplit` (matches SCOPE §8.1 lens names).
- `group_by` is treated as an ordered list (first = row key, rest = sub-keys) — note this when typing the enum/list in code.
- Bot suppression is explicitly *not* part of role mapping — it's a `users.is_bot = false` predicate handled separately so Phase 3 row-mapping queries stay role-pure.
- Durations are stored/returned as `int` seconds; rendering is a frontend concern.

## Open questions

- (none) — the four stage-1 questions are all locked; anything else stays in SCOPE §12.
