## Done

- Added `MetricId::is_count()` / `is_duration()` classifiers in `crates/dp-reports/src/leaderboard.rs`.
- Implemented `check_reconciliation_identity(metric, headline, rows, footer)` enforcing the §6.2 identity for count metrics; duration metrics short-circuit per the §6.2 exemption.
- Added `debug_assert_reconciliation_identity(...)` that panics in debug builds and no-ops in release per the SCOPE.md constraint.
- Added `LeaderboardError::ReconciliationViolation { events_total, rows_sum, unattributed_metric, bots_suppressed_events, delta }` to the shared `#[non_exhaustive]` error enum.
- Documented the §6.4 split-bot footer's role in making the §6.2 identity client-checkable.
- Re-exported the new functions from `crates/dp-reports/src/lib.rs`.
- Wrote +13 unit tests (34/34 green) covering happy path, under/over-count detection, empty-rows edge, duration-exemption gate, `catch_unwind` debug-assert proof, and bot-footer wire round-trip.
- Updated `.codeless/jobs/org-leaderboard/handover.md` and committed/pushed on `codeless/org-leaderboard` (commit `515c25d`).

## Next

- Stage 6: pinned-cursor pagination (§6.5). Extend the SQL builders with a `WHERE (primary_value, subject_id) < ($cursor)` predicate, wire `PageRequest` / `next_cursor`, and define how the §6.2 identity behaves across paginated responses (decide between options a/b/c documented in `handover.md`).

## What you need to know

- `cargo build --workspace`, `cargo test -p dp-reports leaderboard` (34/34), and `scripts/check-boundaries.sh` all clean.
- The identity is a per-response contract today — stages that paginate must pick whether to enforce it on page 1, re-pin totals into the cursor, or define a stronger per-page identity. See handover for the three options.
- Reuse the `count_row(...)` test helper now in the tests module for stage 6/7/8 fixtures.

## Open questions

- (none from this stage — duration-variant of `MetricId` still parked behind the missing `list_duration_samples_in_window` store fetch, surfaced to stage 6 as a §6.2-pagination contract decision.)
