## Done

- Added `dp_domain::freshness::DataAsOf {webhook_latest, reconciler_latest, per_org: HashMap<Uuid, DateTime<Utc>>}` with unit tests.
- Extended `dp_domain::store::Store` with `async fn data_as_of(&self) -> Result<DataAsOf, StoreError>` and documented the semantics (latest finished row per kind from `dp_fetch_runs`; per-org max `updated_at` from `dp_fetch_cursors`; absent orgs stay absent).
- Implemented `data_as_of()` on `dp_store_pg::PgStore` via three indexed aggregates (two `MAX(finished)` probes against `dp_fetch_runs` filtered by kind; one `GROUP BY org_id MAX(updated_at)` against `dp_fetch_cursors`).
- Added an integration test `data_as_of_snapshots_freshness_headline_and_per_org` that exercises: empty state → all-None, unfinished webhook run does not mask a finished one, per-org collapses multiple cursors via max, brand-new org with no cursors is absent.
- Added `dp_reports::freshness` module re-exporting `DataAsOf` and providing pure lens helpers: `DataAsOfExt::for_single_org`, `DataAsOfExt::for_all_orgs_combined` (min, ignoring missing), and `pick_freshness_headline(data, scope_mode, orgs)` routing per `ScopeMode`. Six unit tests cover the lens picker.
- Updated three `FakeStore`-style impls in `dp-fetcher` (router + worker) and `dp-rest::admin` to satisfy the new trait method (default / unimplemented for the ones that don't exercise it).
- `cargo test --workspace --lib` green, `cargo clippy -p dp-domain -p dp-reports -p dp-store-pg --tests -- -D warnings` clean, `scripts/check-boundaries.sh` OK.
- Committed as `689cf41` on `codeless/phase-3-reports`.

## Next

- Stage 7 of 9 (next session picks up). See TODO.md / job goal Phase 3 task list — likely the response envelope / wiring or the spot-check fixture harness.

## What you need to know

- The worktree is `/home/user/.codeless/worktrees/job-01KS02C4WR8QJZVGF0KM6JPN7W` — NOT `/home/user/code/rust/dev-pulse`. Earlier in the session I wasted edits on the wrong path before noticing. Always edit under the worktree root.
- Per-org freshness is derived from `dp_fetch_cursors.updated_at` (the reconciler is the writer); `dp_fetch_runs` carries no `org_id`, so headline `reconciler_latest` is global. This was a design call — if a future stage wants per-org reconciler-run granularity, the run log itself needs an `org_id` column.
- `DataAsOfExt::for_all_orgs_combined` deliberately treats missing orgs as unknown (filtered out) rather than min-sentinel, so a brand-new org doesn't yank the combined headline to 1970-01-01. Documented in module docs + a dedicated test.
- Pre-existing clippy warnings in `dp-fetcher` (unrelated `clippy::unnecessary_lazy_evaluations` in `worker` code) remain. Not in scope for this stage.

## Open questions

- (none)
