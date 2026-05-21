## Done

- Migration `crates/dp-store-pg/migrations/dp/0022_projects.sql`: `dp_projects` with status CHECK, dates CHECK, version CAS, denormalised `issue_count`/`closed_issue_count` (sanity-checked by CHECK), partial-unique `(org_id, lower(name))` excluding archived; `dp_project_issues` with `UNIQUE (issue_id)` per v1 §4 and ON DELETE CASCADE
- New `crates/dp-domain/src/project.rs` module: `Project`, `ProjectStatus` (with wire round-trip + serde lowercase unit tests), `ProjectUpsert`, `ProjectListFilter`, `ProjectIssueAddOutcome`, `ProjectIssueAddSkip`; re-exported from `lib.rs`
- `Store` trait extended with `list_projects`, `count_projects`, `get_project`, `create_project`, `update_project` (CAS), `archive_project` (idempotent CAS), `add_issues_to_project` (transactional FOR UPDATE + per-row skip taxonomy), `remove_issue_from_project` (CAS, recompute counts), `get_project_for_issue`, `list_issue_ids_for_project`. Defaults keep existing fakes green
- `PgStore` implementations for all methods; CAS misses use a `disambiguate_project_miss` helper to pick NotFound vs Conflict; bulk add resolves `(org, existing project)` per row in one query, recomputes denormalised counts inside the same transaction, and only bumps `version` when at least one row was added
- Integration tests in `crates/dp-store-pg/tests/integration.rs` (`projects_crud_round_trip`, `project_issue_membership_outcomes`) — verified passing against a real Postgres container (`cargo test -p dp-store-pg --test integration -- --ignored projects_*`). `cargo build --workspace` and `cargo test --workspace` green; lib unit tests in dp-domain include the enum round-trip
- Committed as `f452b12` on `codeless/projects-v2`

## Next

- Stage 2 (next session): REST surface — `GET/POST /projects`, `PATCH /projects/{id}`, `POST /projects/{id}/archive`, membership endpoints under `/projects/{id}/issues`, and `GET /issues/{id}/project` per §7.1–§7.2. New `(projects, read|write)` permission pair in the policy engine. OpenAPI snapshot regen.

## What you need to know

- Migration slot `0023_*` is reserved for slice B (`dp_project_board_links` + `dp_project_board_items` + drop of legacy `dp_issue_dates.mirror_*` columns), `0024_*` for the rename of `dp_repo_project_link` to `_deprecated_*`. Do not consume those numbers in slice A
- The `invalid()` helper in `dp-store-pg` is misnamed — it actually wraps into `StoreError::Backend`, not `StoreError::Invalid`. Existing code (e.g. `upsert_issue_dates`) follows this convention; project code does too
- Bulk add treats "already in *this* project" the same as "already in another project" — both surface `reason = "already_in_project"` with `existing_project_id` set. Detail-pane idempotent re-add is intentionally a skip, not an add
- `archive_project` is idempotent: if already archived, returns the row unchanged with no version bump regardless of `expected_version`. This matches the §9.2 wording but means callers must re-read after a successful archive to learn the canonical version
- `update_project` ignores `org_id` in the payload (projects do not move between orgs per §4); `created_by` is also untouched on update
- v1 `UNIQUE (issue_id)` ships on `dp_project_issues` exactly as §4 specifies; relaxing later is a single `ALTER TABLE … DROP CONSTRAINT`

## Open questions

- (none)
