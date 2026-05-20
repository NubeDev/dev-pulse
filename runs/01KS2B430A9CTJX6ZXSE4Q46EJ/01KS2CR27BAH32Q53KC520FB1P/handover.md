## Done

- §6.5 pinned-cursor pagination implemented in `crates/dp-reports/src/leaderboard.rs`: `PageRequest`, `PageCursor` (with `encode`/`decode`), `LeaderboardPage`, `effective_page_size`, `validate_page_request`, `build_next_cursor`, `build_paginated_leaderboard_sql`, plus `LEADERBOARD_PAGE_SIZE_DEFAULT/MAX` and the 7-slot / 9-slot bind-order constants.
- New `LeaderboardError` variants on the `#[non_exhaustive]` enum: `CursorWindowMismatch { cursor_window_end, resolved_window_end }`, `CursorDecode(String)`, `PageSizeOutOfRange { size, max }`.
- 17 new tests covering: page-size default/cap, cursor encode/decode + parse error, **§6.5 stale-cursor honored** case, **§6.5 re-resolve 400 mismatch** case, bidirectional drift rejection, SQL shape (with + without cursor), bind-order contracts, wire-form of `page` + `has_more`, and backwards-compat envelope deserialisation.
- `cargo test -p dp-reports leaderboard`: 51/51 green (was 34). `cargo build --workspace` clean. `scripts/check-boundaries.sh` OK.
- Handover.md updated for stage 7; committed as `stage 6: …` on `codeless/org-leaderboard`.

## Next

- Stage 7: `also_compute` multi-metric rows (§6.3, cap 5) — extend `LeaderboardContext.extras`, accept `also_compute: Vec<MetricId>` on the envelope with an `AlsoComputeTooLarge { len, cap: 5 }` validation error, and thread the extras through §15.7 primitives without touching §6.1 rank order.

## What you need to know

- §6.2/§6.5 interaction is resolved as **option (b)** from the stage-5 menu: `headline` + `footer` carry full-result totals; the reconciliation identity holds across the union of all pages, not per page. The existing `check_reconciliation_identity` signature already matches this — call it only with the complete row set. Needs a one-line footnote in ORG-REPORTS §6.2/§6.5 during stage 11 promotion.
- The opaque cursor wire form is plain JSON (no base64 crate in the workspace). `PageCursor::encode`/`decode` are the format boundary — changing them is a breaking change for any cached cursor still on the wire.
- The pagination wrapper uses a strict tuple predicate (`<`, not `<=`) so the cursor row itself is never re-emitted on the next page. The §6.1 tie-break clause is re-emitted on the outer query so future inner-SQL rewrites cannot drift the order.
- `LeaderboardError` stays `#[non_exhaustive]`; stage 7 can add `AlsoComputeTooLarge` without a breaking change for REST/MCP match arms.

## Open questions

- Stage 7 needs to decide: keep `LeaderboardContext.extras` as a loose `serde_json::Map` or lift to a typed `MetricId → value` map (count vs duration variants). The §4 wire form is loose JSON; a stricter Rust type stays compatible if it serialises identically.
- SCOPE Q3 (`my_standing` permission constant) still owned by stage 9. SCOPE Q4 (frontend dashboard-01 leaderboard block) still owned by frontend wiring stage.
