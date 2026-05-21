## Done

- Added `gh_list_org_projectv2(org)` to `dp_fetcher::client` (verbatim GraphQL envelope).
- New `crates/dp-rest/src/board_links.rs` module — `OrgProjectsPickerBackend` trait + Unconfigured + OctocrabOrgProjectsPicker, picker DTOs (`OrgProjectPickerDto` / `BoardPickerDto` / `DateFieldDto`) normalised server-side (closed boards filtered, non-date fields dropped), `BoardLinkDto` + `CreateBoardLinkRequest`, and the four §7.3 handlers wired on `(projects, read|write)` with §9.2 `created_by`/`lead_user_id` elevation enforced on DELETE.
- Audit verbs `PROJECT_BOARD_LINK` / `PROJECT_BOARD_UNLINK` recorded on accepted mutations.
- `BoardLinkDto` surfaces `last_mirror_at` / `last_mirror_error` straight off the store row.
- Mirror rewire in `patch_issue_dates`: now `get_project_for_issue` → `list_board_links` → one spawn per link → `record_board_item_result`, which transactionally rolls per-item outcome up to the aggregate `dp_project_board_links` columns. `OctocrabProjectV2Mirror::mirror_dates` signature preserved; the handler synthesises a `RepoProjectLink` per `BoardLink`.
- `AppState`/`BuildConfig` grew `org_projects_picker`; `dev-pulse` main shares the budget-scoped fetcher client.
- Router mounted in `dp-server::build`; OpenAPI snapshot regenerated.
- Replaced two issue-dates mirror tests with project + board-link rigs that exercise the new path (success persists item + clears aggregate error; failure records on per-link aggregate without failing the response).
- `cargo test --workspace --no-fail-fast` green; pg-integration tests remain `ignored` without Docker, matching pre-existing baseline.

## Next

- Stage 9 (next session) — frontend §6.3 link block + §6.4 link-a-board dialog consuming `/orgs/{org_id}/projects-v2` and the link CRUD; and the §7.4 `207 Multi-Status` response shape on `PATCH /issues/{id}/dates` (per-board outcomes for `SyncStatus`).

## What you need to know

- The mirror call still uses the legacy `mirror_dates(&RepoProjectLink, ...)` signature; on the new fan-out path we synthesise a `RepoProjectLink` from each `BoardLink` (`repo_id` is meaningless on this path and the backend implementation never reads it). If you reshape the trait later, audit callers — only `patch_issue_dates` constructs one.
- `dp_issue_dates.mirror_node_id` / `mirror_synced_at` / `mirror_error` columns remain in the schema (per migration 0023's deferred-drop note) but the new path no longer writes them via `record_issue_dates_mirror_result`. The §7.4 207 response stage is the natural place to retire those columns.
- The §9.2 elevation in `delete_board_link` is the lead/creator check only; a broader admin-lane override is deferred to the elevation refactor noted in `projects.rs`.
- Picker errors map to `400 upstream_unavailable` / `400 github_validation_failed` (consistent with the §3.10 repo picker handler); the §6.4 dialog renders the `[Open GitHub project settings]` hint on those codes.

## Open questions

- The §7.4 `207 Multi-Status` response shape is not yet wired through `PATCH /issues/{id}/dates`; v1 still returns plain `IssueDatesDto` and the per-link outcome is only observable via a subsequent `GET /projects/{id}/board-links` read. That's intentional for stage 8 (per-link `last_mirror_*` is what stage 8 surfaces), but the frontend `SyncStatus` aggregate (§6.5) will want the 207 body — flag for the next stage.
