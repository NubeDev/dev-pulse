# Handover — after stage 5 (§6.2 reconciliation identity + §6.4 split bot footer)

Stage 5 is done. Next agent picks up **stage 6**: pinned-cursor
pagination (§6.5) — extend the SQL builders with a cursor predicate
(`WHERE (primary_value, subject_id) < ($cursor)`) and wire
`PageRequest` / `next_cursor` / `cursor_window_mismatch`.

## What landed in stage 5

In `crates/dp-reports/src/leaderboard.rs`:

- **`MetricId::is_count()` / `is_duration()`** — `const fn`
  classifiers locked in. The §6.2 identity consults only these, so
  a future `MetricId::Duration(...)` variant flips the exemption by
  construction without touching the identity body.
- **`check_reconciliation_identity(metric, headline, rows, footer)`** —
  enforces ORG-REPORTS §6.2 for count metrics:
  `events_total == Σ rows.primary.value + unattributed_events_metric
  + bots_suppressed_events`. Returns
  `LeaderboardError::ReconciliationViolation { events_total,
  rows_sum, unattributed_metric, bots_suppressed_events, delta }` on
  drift; the signed `delta` (i128) tells operators under- vs
  over-counting at a glance. Duration metrics short-circuit to
  `Ok(())` per §6.2's exemption (their row values are aggregates,
  not counts).
- **`debug_assert_reconciliation_identity(...)`** — the SCOPE.md
  constraint ("enforced as a debug-build assertion for count
  metrics; release builds may skip it but the tests must verify
  it"). Panics in `cfg(debug_assertions)` builds on violation; a
  `#[cfg(not(debug_assertions))]` arm makes it a no-op in release.
  REST/MCP layers wanting a real-build check call
  `check_reconciliation_identity` directly and propagate the error.
- **`LeaderboardError::ReconciliationViolation`** — new variant on
  the shared `#[non_exhaustive]` enum so REST + MCP map every
  leaderboard failure (including §6.2 drift) through one match.
- **`LeaderboardFooter` doc updated** — explicitly notes the §6.4
  split (`bots_suppressed` vs `bots_suppressed_events`) is what
  makes the §6.2 identity client-checkable.
- **lib.rs re-exports** both new functions.

## Tests added (34 total, all green)

- `metric_id_classifies_count_and_duration` — locks the `is_count`/
  `is_duration` predicates.
- `reconciliation_identity_holds_for_count_metrics` — fixture where
  22 (rows) + 5 (unattributed_metric) + 11 (bot events) = 38
  (events_total); both the `check_…` API and the
  `debug_assert_…` helper succeed on the happy path.
- `reconciliation_identity_detects_under_count` / `…_over_count` —
  asserts the exact term breakdown in the error and the sign of
  `delta` so operators can tell under- from over-counting.
- `reconciliation_identity_treats_empty_rows_as_zero_sum` — guards
  the empty-leaderboard case (footer alone must equal
  `events_total`).
- `duration_exemption_predicate_governs_the_identity_gate` —
  exercises the predicate that gates the exemption. The moment
  `MetricId::Duration(...)` lands, the duration-metric path skips
  the identity by construction without re-touching this code.
- `debug_assert_panics_on_count_identity_violation` — uses
  `std::panic::catch_unwind` to prove the debug-build assertion
  actually panics on drift (the SCOPE.md-mandated check).
- `bot_split_footer_fields_are_both_present_on_the_wire` — both
  §6.4 fields serialise + round-trip independently; a typo in
  either name would silently zero one of the §6.2 terms.

## Verification

- `cargo build --workspace` — clean.
- `cargo test -p dp-reports leaderboard` — **34/34 green** (was
  21/21 after stage 4; +13 new tests this stage).
- `bash scripts/check-boundaries.sh` — OK (zero `starter_*`
  imports).

## What you need to know for stage 6

- The §6.2 identity is enforced **only at the response-construction
  boundary**, not inside the SQL builders. Stage 6 must call
  `debug_assert_reconciliation_identity` (or
  `check_reconciliation_identity` in the REST layer) after a page
  is built — *before* the response leaves the engine — so a
  pagination bug that double-counts a row trips the assert in
  debug builds, not silently in production.
- The cursor predicate stage 6 adds (`WHERE (primary_value,
  subject_id) < ($cursor)`) must not change `Σ rows.primary.value`
  *across the full page set* — the identity is a per-response
  contract today. If you stream pages, the per-page identity will
  break by design (each page sums to less than `events_total`).
  Decide explicitly in stage 6 whether the identity is:
  (a) checked only on page 1 with the full server-side counts,
  (b) re-pinned into the cursor along with `resolved_window_end`
  so every page carries the full-result totals, or
  (c) replaced by a "page sum + suffix events" stronger identity.
  Whichever you pick, ORG-REPORTS §6.2 needs an explicit footnote.
- `LeaderboardError::ReconciliationViolation`'s field layout is
  part of the wire form via thiserror's `Display`; stage 6 can add
  fields (the enum is `#[non_exhaustive]`) but must not rename
  the existing ones — they're how operators triage a drift.
- `MetricId::is_duration()` returns `!is_count()` for forward
  compatibility, but the doc explicitly says future variants
  (ratios?) need an explicit decision before they default in. If
  stage 7's `also_compute` cap-5 work introduces a new family,
  decide its §6.2 status before flipping anything.
- The `count_row(rank, id, value, active_days)` test helper added
  to the tests module is the natural fixture builder for stages
  6/7/8 — reuse it rather than open-coding `LeaderboardRow {..}`.

## Open questions

- (none from stage 5) — SCOPE Q3 + Q4 still owned by stages 9 and
  the frontend wiring stage respectively. SCOPE Q1 + Q2 remain
  resolved (Stage 1).
- *Surfaced for stage 6:* which of (a)/(b)/(c) above is the
  cursor-era §6.2 contract. Pick one in stage 6 and update
  ORG-REPORTS §6.2 + §6.5 in the same commit.
