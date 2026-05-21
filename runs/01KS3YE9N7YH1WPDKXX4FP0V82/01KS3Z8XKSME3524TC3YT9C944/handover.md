## Done

- Reviewed stage-1 storage diff (migration 0022, dp-domain project module, Store trait extensions, PgStore impls, integration tests) against linear-projects-v2.md §4 / §5 / §8.2 / §14.4 and Layer-1 rulebook invariants

## Next

- Stage 3 (next session): REST surface per §7.1–§7.2 — `GET/POST /projects`, `PATCH /projects/{id}`, `POST /projects/{id}/archive`, membership endpoints, `GET /issues/{id}/project`, new `(projects, read|write)` permission pair, OpenAPI snapshot regen

## What you need to know

- §14.4 (multi-project per issue) is resolved by the spec's 2026-05-21 peer-review pass: v1 ships one project per issue via `UNIQUE (issue_id)` on the join table; relaxation later is a non-destructive `DROP CONSTRAINT`
- Migration slots 0023 (board links + items, drop legacy `dp_issue_dates.mirror_*`) and 0024 (rename `dp_repo_project_link` → `_deprecated_*`) are reserved for slice B — do not consume in slice A
- Archive is a status value, not a soft-delete column; partial-unique name index excludes archived rows so names can be recycled
- PASS: schema, version/CAS columns, index coverage, and the §14.4 one-project-per-issue invariant all match the spec; storage-only diff leaves R1 direction, R2 single transport, R4/R5 trust boundary, and wire formats untouched

## Open questions

- (none)

PASS: storage-only stage matches §5 schema, §8.2 CAS via version BIGINT, §6.2 + reverse-lookup indexes, §4/§14.4 one-project-per-issue via UNIQUE(issue_id), with no transport or wire-format changes and clean dp-store-pg → dp-domain dependency direction.
