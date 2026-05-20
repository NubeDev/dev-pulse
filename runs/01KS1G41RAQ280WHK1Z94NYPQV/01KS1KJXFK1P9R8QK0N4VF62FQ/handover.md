## Done

- reviewed the dp-rest tree (`admin.rs`, `audit.rs`, `directory.rs`, `error.rs`, `openapi.rs`, `reports.rs`, `state.rs`, `lib.rs`) plus the `openapi.snapshot.json` test
- ran `scripts/check-boundaries.sh` (clean) and `cargo test -p dp-rest` (21 unit + 1 snapshot tests pass)
- confirmed R1/R2/R4/R5 + wire-format invariants hold for the stage-7 gate

## Next

- (none) — stage 8 (composition into `dp-server`) is the next session's job

## What you need to know

- dp-rest is the only edge crate touched; dp-domain/dp-reports/dp-fetcher remain free of starter_* imports
- `audit::record` is wired on every mutation + admin route (`HOME_ORG_SET`, `ADMIN_REFRESH`, `RUNS_LIST`, `USER_ANONYMISE`, `USER_EXPORT`); `REPORT_READ` is defined but not yet emitted from the `/reports/*` handlers — note for stage 9 if the composition layer doesn't inject it via middleware
- `with_principal` is not applied here; the report handlers do not yet take `Extension<Principal>`. Stage 8 composition is responsible for wrapping the merged router
- OpenAPI snapshot lives at `crates/dp-rest/tests/openapi.snapshot.json`; regenerate with `cargo test -p dp-rest -- --update-openapi-snapshot`

## Open questions

- whether `report.read` audit emission belongs on each report handler or in a shared post-response layer at composition — flagged but out of scope for this review

PASS: dp-rest dependency graph, transport choice, trust boundary, and Phase 3 wire formats are all intact; tests + boundary script are green.
