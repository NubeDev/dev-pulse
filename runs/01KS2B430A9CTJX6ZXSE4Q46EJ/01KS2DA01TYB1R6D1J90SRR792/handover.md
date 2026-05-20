## Done

- Added `LEADERBOARD_ALSO_COMPUTE_CAP = 5` and `validate_also_compute()`; new `LeaderboardError::AlsoComputeTooLarge { len, cap }` variant (`#[non_exhaustive]` enum, additive).
- `LeaderboardEnvelope.also_compute: Vec<MetricId>` (`#[serde(default, skip_serializing_if = Vec::is_empty)]`); echoed back in `ResolvedLeaderboardEnvelope.also_compute`.
- `resolve_leaderboard_envelope` now calls `validate_also_compute(&env.also_compute)` before window resolution; cap violations surface as a typed 400-class error.
- Extras continue to ride inside `LeaderboardContext.extras` (the existing `serde_json::Map<String, Value>`); no shape change to `LeaderboardRow`. `build_paginated_leaderboard_sql` signature unchanged — pagination/cursor stays single-metric on `rank_by`.
- 11 new tests added; key invariant proved: `build_next_cursor` is byte-identical whether or not rows carry extras, and a cursor minted without `also_compute` validates against an envelope that now has it (same `resolved_window.end`).
- `cargo test -p dp-reports leaderboard`: 62/62 green (was 51). `cargo build --workspace` clean.
- Re-exports added to `crates/dp-reports/src/lib.rs`: `validate_also_compute`, `LEADERBOARD_ALSO_COMPUTE_CAP`.
- Committed as `stage 7: …` on `codeless/org-leaderboard` (commit 0203beb).

## Next

- Stage 8: `subject_ids` filter (§6.10) — the ≤50-subject "compare these users" companion path that pairs naturally with `also_compute`; add an envelope field + `SubjectIdsTooLarge { len, cap: 50 }` error, fan-out into the SQL builder as a `WHERE subject_id = ANY(...)` predicate that lives outside the GROUP BY.

## What you need to know

- Extras shape stays loose (`serde_json::Map<String, Value>`); ORG-REPORTS §4 shows two payload variants per extra (`{"value": N}` and `{"value": X, "n": M}`), so a stricter Rust type would have to be a tagged enum. Defer until §15.7 lands `DurationMetric` (STAGE-1-COMPOSABILITY §3) — that's the same fetch that justifies typed extras.
- The §6.3 single-metric pagination rule is now load-bearing: any future change to `build_paginated_leaderboard_sql` that lets `also_compute` reach the SQL would silently break `page_boundary_cursor_is_invariant_under_also_compute_changes`. That test is the canary.
- `also_compute` is echoed in `ResolvedLeaderboardEnvelope` so REST/MCP caches can key on (rank_by + extras + scope + window) — stage 9's `my_standing` and stage 10's frontend wiring should consume the echo, not the request, when rendering "available extras".

## Open questions

- Should `also_compute` deduplicate or reject duplicates? Current code is purely cardinality-based; the SQL layer (stage 8+) can decide based on whether extras-by-key dedup happens upstream. Leaving as cardinality-only is the cheapest stance.
- Stage 11 promotion still owes ORG-REPORTS §6.2/§6.5 the "identity holds across full result set, not per page" footnote flagged in stage 6.
