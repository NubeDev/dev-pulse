## Done

- New `crates/dp-rest/src/projects.rs` with five handlers (`list_projects`, `get_project`, `create_project`, `patch_project`, `archive_project`), DTOs (`ProjectDto`, `ProjectStatusDto`, `ProjectListResponse`, `CreateProjectRequest`, `PatchProjectRequest`, `ArchiveProjectRequest`), `ListProjectsQuery`, and `projects_router` with `(projects, read|write)` `with_permission` lanes.
- CAS via `expected_version` on PATCH and POST `/archive` → 409 `stale_project_version` on miss; archive is idempotent (no version bump, no audit row on re-archive).
- `?count_only=1` returns `{ total, limit: 0, offset, rows: [] }` for the §6.1 sidebar.
- `ProjectDto.board_link_count` carried now (always `0`) so the wire shape is stable for slice B.
- Audit verbs added in `crates/dp-rest/src/audit.rs`: `PROJECT_CREATE`, `PROJECT_UPDATE`, `PROJECT_ARCHIVE` plus the four reserved verbs for §7.2/§7.3 (`PROJECT_ISSUE_ADD|REMOVE`, `PROJECT_BOARD_LINK|UNLINK`).
- `projects` resource (`read`, `write`) registered in `dp_server::auth::policy::register_dev_pulse_resources`.
- OpenAPI: five handlers + six schemas registered in `DevPulseApi`, new `projects` tag, `openapi.snapshot.json` regenerated.
- `dp-server::build` mounts `projects_router` alongside `pins`/`tags`.
- 12 unit tests in `projects::tests` pass; `cargo test --workspace` is green.
- Committed as `562a9f2`.

## Next

- Stage 4: membership endpoints from §7.2 (`GET/POST /projects/{id}/issues`, `DELETE /projects/{id}/issues/{issue_id}`, `GET /issues/{id}/project`) — wire `PROJECT_ISSUE_ADD|REMOVE` and the `BulkAddResult` shape into the new module or a sibling `projects_issues.rs`.

## What you need to know

- `PatchProjectRequest` uses `Option<Option<T>>` for nullable fields (`description`, `lead_user_id`, `start_at`, `due_at`) so callers can distinguish "leave alone" from "clear". This shape is opaque in serde JSON (`null` vs missing); document carefully when wiring the frontend.
- §9.2 elevated checks (lead-or-author for `archive` and for `lead_user_id` mutation) are NOT yet enforced in this stage — the spec marks them elevated but the implementation gates only on `(projects, write)`. The lane is reserved with a code comment; a follow-up stage that also handles project lead resolution should add the gate. Tests pass without it because the in-memory store / test harness uses NoopPolicyEngine.
- `validate_name` caps at 200 chars; spec doesn't pin a length so this is a reasonable defensive bound — adjust if migration 0022 set a `VARCHAR(N)` value I missed.
- `board_link_count` is hard-coded to `0` in `ProjectDto::from(Project)`. Slice B needs to replace this with a real count (likely an added denormalised column on `dp_projects` or a join helper).

## Open questions

- §9.2 elevation gating: which stage adds the lead-or-author check, and does it want a new `(projects, elevate)` action pair or an inline check inside the handlers using `principal` + the project row's `created_by`/`lead_user_id`?
- The OpenAPI snapshot regeneration command `cargo test -p dp-rest -- --update-openapi-snapshot` per the module doc didn't work for me; only `UPDATE_OPENAPI_SNAPSHOT=1 cargo test -p dp-rest --test openapi_snapshot` did. Worth fixing the doc or the test-binary arg parsing in a separate stage.
