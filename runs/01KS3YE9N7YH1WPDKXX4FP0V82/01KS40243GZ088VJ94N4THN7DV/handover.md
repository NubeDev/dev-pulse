## Done

- New `dp-rest::project_issues` module implementing the four §7.2 routes: `GET/POST /projects/{id}/issues`, `DELETE /projects/{id}/issues/{issue_id}?expected_version=`, and `GET /issues/{id}/project`.
- `BulkAddResult` / `BulkAddSkipDto` minted at the REST boundary mirroring SCOPE-PROJECTS.md §7 (`added: [uuid]`, `skipped: [{ issue_id, reason, existing_project_id? }]`). Closed-vocabulary `reason` values: `already_in_project` (carries `existing_project_id`), `unknown_issue`, `cross_org`.
- CAS via `expected_version` on the project row; bulk `issue_ids` capped at `BULK_ADD_ISSUE_CAP = 100`. Audits `project.issue.add` per accepted row and `project.issue.remove` per detach (skipped rows never audit).
- Widened `issues_read::attach_repo_slugs` to `pub(crate)` so the new list handler reuses the same per-page slug join.
- Registered new paths + DTOs on `DevPulseApi`; merged `project_issues_router` into `dp-server::build`. OpenAPI snapshot regenerated.
- 13 new unit tests covering per-row outcome matrix, CAS misses, cap rejection, empty no-op, idempotence on detach, list filtering, and null/owning-project resolution. `cargo build --workspace` and `cargo test -p dp-rest` (108 lib + 1 snapshot) green.

## Next

- Stage 5: org-scoped GitHub Projects v2 board picker + link CRUD (§7.3) — `GET /orgs/{org_id}/projects-v2`, `GET/POST/DELETE /projects/{id}/board-links`, and the §7.4 mirror fan-out response contract (`207 Multi-Status`).

## What you need to know

- v1 `GET /projects/{id}/issues` resolves membership in-memory (point-lookup `get_issue` per id) since project sizes target ≤ 100 issues. The natural SQL push-down lands when `IssueListFilter` grows a `project_id` field — flagged in the module doc comment.
- Default `state` filter on `GET /projects/{id}/issues` is `all` (not `open` like `/issues`) because a project detail surface wants both open and closed work in view.
- Authz uses the existing `(projects, read|write)` pairs registered in stage 2; no policy-engine changes needed.
- `project.issue.add` / `project.issue.remove` audit verbs were already pinned in `dp-rest::audit` ahead of this stage.

## Open questions

- (none)
