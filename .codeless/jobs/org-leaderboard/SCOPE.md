# Scope — org-leaderboard

> Full design rationale lives in
> [`ORG-REPORTS.md`](../../../ORG-REPORTS.md) at the repo root. This
> file is the trimmed brief; do not duplicate the design — point at it.

## Goal

Ship the `leaderboard` report kind from ORG-REPORTS.md as the first
cross-cutting "rank subjects against each other" primitive, plus a
separate `my_standing` endpoint for IC self-view. One SQL shape
must serve "rank everyone in this org," "rank across all orgs,"
and "compare these N users" — without leaking distributional info
to non-admins, without composite scores, and without diverging
behaviour between REST, MCP, and the frontend.

## In scope

- `LeaderboardEnvelope` type and response shape exactly as in
  ORG-REPORTS.md §3 and §4 (incl. `envelope.resolved_at`,
  `resolved_window`, and `subject_org` in per-org-split rows).
- All four `SubjectKind` values (`user`, `team`, `org`,
  `home_org_label`) and all three `OrgScope` modes from SCOPE.md
  §8.1.
- The ten decisions §6.1–§6.10:
  - §6.1 tie-break (`rank_by DESC → active_days DESC → subject_id ASC`).
  - §6.2 reconciliation identity + duration-metric exemption.
  - §6.3 `also_compute` multi-metric rows (cap 5).
  - §6.4 split bot footer (`bots_suppressed`, `bots_suppressed_events`).
  - §6.5 pinned-cursor pagination + `cursor_window_mismatch`.
  - §6.6 duration-metric NULL handling (defers to SCOPE.md §15.9
    sufficiency threshold; no leaderboard-local threshold).
  - §6.7 no composite score (use §6.3 escape hatch only).
  - §6.8 `home_org_label` aggregation + `__unlabeled__` bucket.
  - §6.9 `my_standing` as a *separate* endpoint, not a projection
    of the leaderboard.
  - §6.10 `subject_ids` cap 50 + pagination-disabled small-N mode.
- REST + MCP + frontend wiring off the same envelope.
- Promotion into SCOPE.md §8.2 (sections 1–5) and §15.15
  (decisions §6.1–§6.10) per the §8 promotion path.

## Out of scope

- Composite / weighted "productivity scores" — ORG-REPORTS.md §7.
- A backend pairwise diff endpoint — compare-users is a UI on top
  of `subject_ids` + `also_compute`.
- Anomaly / outlier callouts — separate report, separate envelope.
- Custom metric definitions — the SCOPE.md §15.7 map stays the
  universe; new metrics are §15.7 rows, not leaderboard-only
  extensions.
- Cross-org team comparison — `subject = team` is org-scoped by
  definition.
- Backend re-sort across metrics within one paginated result.

## Constraints

- Reuse SCOPE.md primitives: envelope (§15.6), role→metric map
  (§15.7), trend buckets (§15.8), percentile semantics (§15.9),
  three org-scope modes (§8.1), permission boundary (§15.12).
  Do **not** invent parallel versions.
- Every number in a response must trace back to a §15.7 row
  (SCOPE.md §9 transparency, §11.4 trust).
- REST, MCP, and frontend must compute identical outputs for
  identical envelopes — divergence is a §11.4 trust violation.
- The §6.2 reconciliation identity is enforced as a debug-build
  assertion for count metrics; release builds may skip it but
  the tests must verify it.
- `my_standing` (§6.9) is a separate endpoint with its own
  envelope and permission gate. The "same SQL, only projection
  changes" framing was explicitly rejected — `total_subjects` and
  page boundaries are themselves information leaks.
- Stage 1 must answer the §15.7 composability open question
  *before* any code lands; if the metric layer needs a refactor
  to support `also_compute` cheaply, that refactor lands as its
  own stage ahead of the leaderboard work.

## Open questions

1. ~~Are §15.7 metric aggregates already composable (so
   `also_compute` is a projection-list field add) or do they
   need to be lifted out of the per-user query path? Stage 1.~~
   **Resolved (Stage 1, see `STAGE-1-COMPOSABILITY.md`):** yes,
   already composable; `also_compute` is a field add — no
   refactor stage required ahead of stage 3.
2. ~~Is SCOPE.md §15.9's percentile aggregator already a reusable
   function callable from the `home_org_label` aggregation path
   (§6.8), or is it inlined? Stage 1.~~
   **Resolved (Stage 1, see `STAGE-1-COMPOSABILITY.md`):** yes,
   `compute_percentiles(&[i64])` is a free function with the
   `n < 5` floor internalised; directly callable from per-team,
   per-org, and per-`home_org_label` paths (incl.
   `__unlabeled__`). `percentile_cont_sql(column)` is the
   matching SQL fragment helper.
3. Where does `my_standing`'s permission gate live —
   `require_permission` on the same `with_principal` middleware
   used by the leaderboard, or a new permission constant? Stage 9.
4. Does the frontend dashboard-01 surface already have a
   leaderboard-shaped block, or does this job ship a new one?
   Out of scope to decide here; flagged for the frontend wiring
   stage.
