## Done

- Added `LeaderboardEnvelope.subject_ids: Vec<String>` (and echoed it on `ResolvedLeaderboardEnvelope.subject_ids`); both fields use `#[serde(default, skip_serializing_if = Vec::is_empty)]` so the stage-3 wire shape is unchanged when the small-N path is unused.
- New `LEADERBOARD_SUBJECT_IDS_CAP = 50` const + `validate_subject_ids()`; over-cap requests surface `LeaderboardError::SubjectIdsTooLarge { len, cap }` at envelope resolution.
- `validate_page_request` now rejects a cursor or non-default `page.size` when `subject_ids` is non-empty, via the new `LeaderboardError::PaginationDisabledForSubjectIds { subject_ids_len }` variant — making §6.10's "all rows in one response" rule a typed 400-class error.
- New `build_subject_ids_leaderboard_sql(subject, scope_mode)` wraps each per-variant base SQL in `SELECT * FROM (...) sub WHERE sub.subject_id = ANY($7::text[]) ORDER BY …`, predicate sits outside the GROUP BY (so §6.1 tie-break stays authoritative). No LIMIT — pagination is disabled in this mode.
- `LEADERBOARD_BIND_ORDER_SUBJECT_IDS` (7 slots) documents the bind order; mirrors the paginated bind constants' style.
- Re-exports updated in `crates/dp-reports/src/lib.rs`: `validate_subject_ids`, `build_subject_ids_leaderboard_sql`, `LEADERBOARD_SUBJECT_IDS_CAP`, `LEADERBOARD_BIND_ORDER_SUBJECT_IDS`.
- 12 new tests added; `cargo test -p dp-reports leaderboard` 75/75 green (was 62). `cargo build --workspace` clean (the one pre-existing missing-docs warning on `build_paginated_leaderboard_sql` is unrelated).
- Committed as `stage 8: …` on `codeless/org-leaderboard` (commit a694f04).

## Next

- Stage 9: `my_standing` endpoint (§6.9) — the IC self-view companion that pairs with the small-N leaderboard. Likely a separate envelope (different permissioning surface per §6.9 / §15.12) sharing the resolve/echo machinery and `also_compute` cap. Consider whether `my_standing` reuses `build_subject_ids_leaderboard_sql` with a singleton `subject_ids = [me]` (cheap) or warrants a dedicated rank-of-me SQL that also returns total population context.

## What you need to know

- `PaginationDisabledForSubjectIds` is intentionally stricter than the ORG-REPORTS prose: the spec says "pagination is disabled" without saying what happens if a cursor is sent. Surfacing the conflict as a typed 400 keeps REST/MCP loud rather than quietly ignoring `page.cursor`. Stage 11's promotion into SCOPE.md §15.15 should mention this stance so a later reader doesn't soften it.
- The §6.10 SQL wrap matches the §6.5 paginated SQL wrap structurally — they're independent (you can't use both — `subject_ids` mode has no LIMIT and pagination mode has no `subject_ids` predicate). If stage 9's `my_standing` needs both *populated rank* and *bounded compare*, it should compose `build_subject_ids_leaderboard_sql` for the compare slice and a separate rank-of-me query for the position, not try to mash the two wraps together.
- `subject_ids` values are typed as `Vec<String>` (not `Vec<Uuid>`) on purpose: `home_org_label` subjects have opaque-string ids (possibly `__unlabeled__`), so a UUID-typed list would have closed off that subject kind. The §6.10 SQL binds the list as `text[]` to match.
- The §6.3 single-metric pagination invariant test (`page_boundary_cursor_is_invariant_under_also_compute_changes`) still passes — `subject_ids` doesn't touch the cursor minting path. Pair invariant for §6.10 ("cursor mode and subject_ids mode are mutually exclusive") is now enforced by `validate_page_request_rejects_cursor_in_subject_ids_mode` rather than by a structural test.

## Open questions

- Should `subject_ids` dedupe or reject duplicates? Current code is purely cardinality-based; the SQL `ANY(...)` naturally collapses duplicates so it's harmless, but a strict-mode REST/MCP might want to reject. Leaving as cardinality-only for parity with `validate_also_compute`.
- Stage 11 promotion still owes ORG-REPORTS §6.2/§6.5 the "identity holds across full result set, not per page" footnote flagged in stage 6 — and now also owes §6.10 the "pagination_disabled_for_subject_ids is a typed 400" stance.
