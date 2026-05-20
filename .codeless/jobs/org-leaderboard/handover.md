# Handover — after stage 6 (§6.5 pinned-cursor pagination)

Stage 6 is done. Next agent picks up **stage 7**: `also_compute`
multi-metric rows (§6.3, cap 5) — extend `LeaderboardContext.extras`
into a typed payload, accept `also_compute: Vec<MetricId>` on the
envelope, validate the cap, and run the extra metrics through the
same §15.7 primitives without changing rank order.

## What landed in stage 6

In `crates/dp-reports/src/leaderboard.rs`:

- **`PageRequest { size, cursor }`** added to `LeaderboardEnvelope`
  with `#[serde(default)]`. `size = 0` is the wire-form sentinel for
  "use the default"; absent `page` in the request JSON deserialises
  as the default, so an existing report URL can be pivoted into a
  leaderboard without adding paging params (§6.5).
- **`PageCursor { resolved_window_end, rank_by_value, subject_id }`**
  — the §6.5 triple. `encode()` / `decode()` are the format
  boundary: today the opaque wire form is plain JSON (no base64
  crate available in the workspace; the encoded string is opaque to
  clients but reversible from logs, which is deliberate). Changing
  the encoding is a breaking change for any cached cursor still on
  the wire.
- **`LeaderboardPage { next_cursor, has_more }`** added to
  `LeaderboardResponse`. `has_more` is serialised even when `false`
  so a client never has to introspect the cursor string to decide
  whether to paginate further.
- **`LEADERBOARD_PAGE_SIZE_DEFAULT = 25` / `LEADERBOARD_PAGE_SIZE_MAX
  = 200`** — the §6.5 page bounds.
- **`effective_page_size(&PageRequest)`** — performs the `0 →
  default` substitution and the `> max → error` check. Centralised
  here so REST (`?size=` query) and MCP (structured args) hit the
  identical bound.
- **`validate_page_request(req, resolved)`** — the §6.5 contract:
  size cap + cursor-window match. Returns the effective page size on
  success so callers don't re-compute it.
- **`build_next_cursor(resolved, rows)`** — mints page N+1's cursor
  from page N's last row, with `resolved_window_end` pinned from the
  envelope. Returns `None` for an empty page (no cursor to mint).
- **`build_paginated_leaderboard_sql(subject, scope, has_cursor)`** —
  wraps `build_leaderboard_sql` in a subquery. Without cursor:
  appends `LIMIT $7`. With cursor: appends `WHERE (sub.primary_value,
  sub.subject_id) < ($7::bigint, $8::text) LIMIT $9`. The §6.1
  tie-break clause is re-emitted on the outer query so any future
  rewrite cannot drift the order. The strict `<` (not `<=`) means
  the cursor row itself is never re-emitted on the next page.
- **`LEADERBOARD_BIND_ORDER_PAGED` (7 slots)** and
  **`LEADERBOARD_BIND_ORDER_PAGED_WITH_CURSOR` (9 slots)** —
  documented bind orders for the dp-store-pg adapter.
- **Three new `LeaderboardError` variants** on the still-
  `#[non_exhaustive]` enum so REST + MCP map every leaderboard
  failure through one match:
  - `CursorWindowMismatch { cursor_window_end, resolved_window_end }`
    — the §6.5 400 case. Drift is rejected in **either direction**
    (forward or backward) since both shapes mix snapshots.
  - `CursorDecode(String)` — opaque parse error from
    `PageCursor::decode`, surfaced so a malformed cursor is
    distinguishable from a window-mismatch at the wire layer.
  - `PageSizeOutOfRange { size, max }` — for `size >
    LEADERBOARD_PAGE_SIZE_MAX`.
- **lib.rs re-exports** every new public item.

## Tests added (17 total, all green — 51/51 passing now)

- `page_size_defaults_apply_when_zero` — `size = 0` → 25.
- `page_size_rejects_values_above_the_cap` — 201 → `PageSizeOutOfRange`.
- `page_request_round_trips_through_json_with_optional_cursor` —
  absent cursor is omitted from the wire form.
- `page_cursor_round_trips_through_encode_decode` — encode/decode is
  the format boundary.
- `page_cursor_decode_surfaces_a_parse_error` — garbage in →
  `CursorDecode` out, not a panic.
- `validate_page_request_honours_a_stale_but_consistent_cursor` —
  **the §6.5 stale-cursor case**: cursor's pinned window-end matches
  the freshly-resolved envelope → honour the cursor.
- `validate_page_request_rejects_re_resolved_cursor_with_400_mismatch`
  — **the §6.5 re-resolve case**: envelope's window has moved
  forward → `CursorWindowMismatch` with both ends in the payload.
- `validate_page_request_rejects_cursor_window_drift_in_either_direction`
  — backward drift is equally a misuse; also exercises the `size =
  0 → default` substitution path.
- `validate_page_request_surfaces_decode_errors` — malformed cursor
  is `CursorDecode`, not `CursorWindowMismatch`, so REST/MCP can
  return precise 400 reasons.
- `build_next_cursor_returns_none_for_empty_rows` /
  `build_next_cursor_uses_the_last_row_and_pins_the_window` — page
  N+1's cursor is page N's last row with the resolved window pinned.
- `paginated_sql_without_cursor_appends_limit_only` — page 1: only
  `LIMIT $7`, no tuple predicate; tie-break re-emitted on the outer.
- `paginated_sql_with_cursor_appends_tuple_predicate_and_limit` —
  page N+1: `(sub.primary_value, sub.subject_id) < ($7::bigint,
  $8::text) … LIMIT $9`.
- `paginated_sql_rejects_invalid_subject_scope_combo` — the
  pagination wrapper inherits §2 validation.
- `paginated_bind_orders_match_the_documented_slots` — the 7/9-slot
  contracts the dp-store-pg adapter will bind against.
- `response_serialises_page_block_with_explicit_has_more` —
  `has_more` is always on the wire.
- `envelope_default_page_round_trips_with_no_cursor_field` —
  backwards-compat: existing report URLs deserialise cleanly.

## Verification

- `cargo build --workspace` — clean.
- `cargo test -p dp-reports leaderboard` — **51/51 green** (was
  34/34 after stage 5; +17 new tests this stage).
- `bash scripts/check-boundaries.sh` — OK (zero `starter_*`
  imports).

## §6.2 vs §6.5 — pinned decision

The stage-5 handover surfaced an open question: with pagination, the
§6.2 reconciliation identity (`headline.events_total == Σ
rows.primary.value + unattributed_metric + bots_suppressed_events`)
cannot hold *per page* — each page only carries a slice of `Σ rows`.
Stage 6 picks **option (b)** from the stage-5 menu:

> The cursor pins `resolved_window_end`; headline and footer
> represent the **full-result totals** across all pages. The §6.2
> identity holds across the union of every page in a paginated
> session, not on any one page.

Implications for stage 7+:

- `check_reconciliation_identity` and
  `debug_assert_reconciliation_identity` continue to take the full
  `&[LeaderboardRow]` and the full headline/footer. The REST/MCP
  layer should call them only when it has the *complete* row set in
  hand (single-page response, or after the final page).
- The per-page response carries the full-result `headline` and
  `footer` unchanged — those numbers are stable across pages by
  construction because the resolved window is pinned in the cursor.
- A per-page identity check (sum the rows of *this* page) is **not**
  a §6.2 contract — adding one would break by design and would not
  surface a real bug.

This needs a one-line footnote in ORG-REPORTS.md §6.2 / §6.5 the
next time those sections are edited (e.g. in stage 11 promotion).
Flagged as a documentation-debt item rather than a code change so
this stage stays surgical.

## What you need to know for stage 7

- `LeaderboardContext.extras` already exists as a
  `serde_json::Map<String, serde_json::Value>` with
  `skip_serializing_if = serde_json::Map::is_empty`. Stage 7 should
  decide whether to keep that loose shape or replace it with a typed
  `BTreeMap<MetricId, LeaderboardAlsoComputeValue>` enum (count vs
  duration). The wire shape locked in §4 of ORG-REPORTS.md is loose
  JSON; a stricter Rust-side type stays compatible if it serialises
  to the same JSON.
- The §6.3 cap (5 metrics) needs an envelope-validation error
  variant. Name it `AlsoComputeTooLarge { len, cap: 5 }` to mirror
  the existing `*TooLarge` shape stage 8 will reuse for
  `SubjectIdsTooLarge`.
- The cursor + pagination plumbing does **not** need to change for
  `also_compute` — extras don't participate in §6.1 rank order
  (§6.3 is explicit on this). Don't be tempted to add them to the
  cursor tuple.
- The `count_row(rank, id, value, active_days)` test helper is
  still the natural fixture builder; extend it to take extras when
  stage 7 needs to test the §6.3 cap.

## Open questions

- (none new from stage 6). SCOPE Q3 + Q4 still owned by stages 9
  and the frontend wiring stage respectively. SCOPE Q1 + Q2 remain
  resolved (Stage 1). The stage-5-surfaced §6.2/§6.5 interaction is
  now resolved here as option (b); promote the footnote in stage 11.
- *Surfaced for stage 7:* keep `extras` loose (JSON map) or lift to
  a typed `MetricId → value` map? Decide in stage 7 and lock the
  wire form before stage 8 builds on it.
