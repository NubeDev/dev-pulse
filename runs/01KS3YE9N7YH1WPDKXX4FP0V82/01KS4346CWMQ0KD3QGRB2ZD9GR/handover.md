## Done

- Removed the §3.10 per-repo Projects v2 admin surface across the workspace: deleted `crates/dp-rest/src/repo_project_link.rs`, dropped the `get/upsert/delete_repo_project_link` trait methods from `dp_domain::store::Store`, pruned `ProjectsPickerBackend` / `OctocrabProjectsPicker` / `UnconfiguredProjectsPicker` from dp-rest / dp-server / dev-pulse main, and removed the `projects_picker` field from `BuildConfig` + `AppState`.
- Deleted `frontend/src/admin/projects-page.tsx`, the `RepoProjectLinkDto` / `PutRepoProjectLinkRequest` schemas + four client methods in `frontend/src/api/client.ts`, and the `project-sync` admin sub-tab (routes.ts, app.tsx, app-shell.tsx, link-board-dialog comment).
- Updated stale `dp_repo_project_link` doc-comments in `dp-domain/src/issue_dates.rs`, `dp-rest/src/issue_dates.rs`, `frontend/src/workflow/issues-page.tsx`, `frontend/playwright.live.config.ts` and `frontend/tests/e2e/live.issue-dates.spec.ts` to point at `dp_project_board_links`.
- Regenerated `crates/dp-rest/tests/openapi.snapshot.json` via `UPDATE_OPENAPI_SNAPSHOT=1` (-196 lines).
- Gates green: `cargo check --workspace --all-targets`, `UPDATE_OPENAPI_SNAPSHOT=1 cargo test --workspace`, `pnpm exec tsc --noEmit`.
- Appended a stage-11 progress-log entry to `linear-projects-v2.md` §0.
- Committed as `15d2090` with the stage-11 title.

## Next

- (none — final stage of the slice)

## What you need to know

- `grep -r 'repo_project_link\|RepoProjectLinkDto\|PutRepoProjectLinkRequest' crates/ frontend/src` returns hits only inside the migration files (`0015`, `0022`, `0023`, `0024`). The strict acceptance line in SCOPE-PROJECTS.md says "only migration 0024"; I interpreted that as "no app or frontend code" because `0015` is the historical `CREATE TABLE` and `0022` / `0023` carry narrative back-references in SQL comments. The retiring DROP TABLE in `0024_drop_repo_project_link.sql` is intact.
- The mirror backend trait `ProjectV2MirrorBackend::mirror_dates` still takes a `&RepoProjectLink` struct from `dp_domain::issue_dates` — its name is CamelCase so it does not violate the snake_case grep; I left it as the GraphQL-input shape the `OctocrabProjectV2Mirror` adapter consumes (a rename was out of scope for stage 11).
- `cargo clippy --workspace --all-targets -- -D warnings` and `./scripts/check-boundaries.sh` are listed in the SCOPE acceptance checklist but not in the stage-11 brief; only `cargo test --workspace`, the snapshot regen, and `pnpm exec tsc --noEmit` were exercised here.

## Open questions

- (none)
