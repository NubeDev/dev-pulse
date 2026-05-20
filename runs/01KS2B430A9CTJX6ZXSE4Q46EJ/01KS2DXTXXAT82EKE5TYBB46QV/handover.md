## Done

- New `crates/dp-reports/src/my_standing.rs` module with `MyStandingEnvelope`, `ResolvedMyStandingEnvelope`, `MyStandingResponse`, `MyStandingError`, plus `resolve_my_standing_envelope`, `validate_my_standing_permission`, `effective_neighbor_radius`, `build_my_standing_sql`, `anonymise_neighbour_row`, `compute_visible_headline`.
- Permission check (§6.9) enforces `principal == envelope.viewer_subject_id`; runs before any other 4xx so a probe can't differential-leak "is this viewer_id real".
- SQL builder wraps `build_leaderboard_sql` in `WITH ranked AS (... rank() OVER (§6.1 tie-break) ...), me AS (...) SELECT ... WHERE row_rank BETWEEN me.me_rank - $8 AND me.me_rank + $8` — no LIMIT (radius bounds the slice).
- Bind order locked in `MY_STANDING_BIND_ORDER` (8 slots: base 6 + viewer_subject_id + neighbor_radius).
- Anonymisation sentinels are fixed (`__anonymised__` / `—`), constant across rows so they can't be used as a per-row salt for correlation.
- Visible-set headline (§6.9) counts viewer + neighbours only, never the population.
- Subject axis restricted to `user` / `team` (org / home_org_label rejected as `SubjectKindUnsupported`).
- Re-exports wired in `crates/dp-reports/src/lib.rs`.
- 26 new tests; `cargo test -p dp-reports` 145/145 green (was 119). Build clean modulo the pre-existing missing-docs warning.
- Committed as `stage 9: …` on `codeless/org-leaderboard` (commit e470146).

## Next

- Stage 10: presumably the REST + MCP + frontend wiring off the same envelopes — the SQL primitives, validators, and response types are now complete for both `leaderboard` and `my_standing`.

## What you need to know

- `MyStandingError` is intentionally a distinct enum from `LeaderboardError` (separate permission surface per §15.12) but carries `LeaderboardError` via `#[from] LeaderboardError` for shared rules (subject/scope, orgs cardinality, also_compute cap). REST/MCP can match on the distinct variants for precise 4xx mapping.
- `anonymise_neighbour_row` preserves `rank`, `primary`, `context`, `sparkline`, `active_orgs`, and `subject_org` — these convey position, not identity. Only `subject_id` and `subject_label` are sentinel-replaced. If a future field could fingerprint a user (e.g. a per-user salt), it must be considered when added.
- The SQL emits a `RANK()` `OVER (ORDER BY ...)` clause whose tie-break must mirror `LEADERBOARD_TIE_BREAK_ORDER_BY_CLAUSE` — copy-pasted because the over-clause spelling differs syntactically from the trailing `ORDER BY`. `sql_wraps_base_in_rank_cte_and_slice` asserts the exact ordering string.
- `viewer_row` on the response is `Option<LeaderboardRow>` so a viewer with no events in the window doesn't break the shape; the visible-set headline still summarises whatever rows the slice contains.
- `neighbor_radius` has both a default (3) and a hard cap (10) — beyond 10 the request approximates a leaderboard slice and should use the leaderboard endpoint instead.

## Open questions

- Should `MyStandingResponse` carry a `LeaderboardFooter` for §6.2/§6.4 reconciliation? Stage 9 omits it because the visible-set framing makes reconciliation against the full population meaningless; if stage 10 needs a "bots near me" or "unattributed events in my slice" counter, the footer shape may need a self-view variant.
- Stage 11's SCOPE.md promotion still owes §6.9 the "permission denial is order-zero" footnote — the order-of-checks test (`resolve_rejects_principal_mismatch_before_anything_else`) encodes a stricter contract than the prose.
- The `__anonymised__` sentinel collides if a real subject_id ever equals that literal — extremely unlikely for UUIDs but possible for `home_org_label`. Not relevant in stage 9 (home_org_label rejected) but worth a guard if future subject kinds are added.
