## Done

- Added `crates/dp-reports/tests/phase3_smoke.rs` with seven `#[test]`s, one per Phase 3 stage-9 checklist bullet: resolved-window-echoes-back-with-anchor-preserved, three-lens-numbers-correct-on-co-author-fixture, percentile_cont-returns-none-when-sample-under-five, percentiles-match-expected-on-recorded-fixture, data_as_of-per-org-and-combined-match-fetch_runs, boundary-check-still-green, no-means-anywhere.
- All 7 smoke tests pass; existing dp-reports unit + spot-check tests still pass.
- Committed as `550e465` on branch `codeless/phase-3-reports` with message starting "stage 9: Phase 3 smoke tests pass in CI".

## Next

- (none) — Phase 3 stage 9 is the final stage of this job.

## What you need to know

- `boundary_check_still_green` shells out to `scripts/check-boundaries.sh` via `bash` with `current_dir = repo_root` (computed from `CARGO_MANIFEST_DIR`'s grandparent). It needs `bash` on PATH and the repo layout to keep `scripts/` two levels above the crate.
- `no_means_anywhere` walks `crates/dp-reports/src/**/*.rs` with a tiny inline `walk_rs` (no extra deps), skips `//`/`*` comment lines and the `// not used` exemption from the checklist, and uses whole-word matching so prose like "None means …" or "long-tail … means" in doc comments does not trip the guard — only real identifier hits in code do.
- Percentile expectations in `percentiles_match_expected_on_recorded_fixture` are derived from Postgres `percentile_cont` semantics (linear interpolation, rank = p*(n-1)) on the recorded sample; values 5400 / 47520 / 66960 should be regenerated if the sample changes.
- `three_lens_numbers_correct_on_co_author_fixture` constructs rows inline rather than re-reading the fixture JSON, so it's an in-test redundancy with `spot_check_co_authored_commit_spanning_two_orgs` — intentional, because the smoke test pins the public lens API directly.

## Open questions

- (none)
