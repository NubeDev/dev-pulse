## Done

- Added backend GET /issues/{id}/dates (issues,read) + OpenAPI snapshot regen; all dp-rest tests pass.
- Frontend API + hooks: getIssueDates / patchIssueDates, useIssueDates, useUpdateIssueDates, useIssueDatesBatch.
- IssueEditCard now embeds a §3.10 start/due picker with validation, clear, and mirror-error surface.
- Triage page: `g d` Due-column toggle (localStorage), Due-week + Overdue smart views, tag-saved views in the rail with visible_link_count badges, pointer-drag splitters for rail+peek persisted to localStorage (`dp:triage:*`), pin labels joined against GET /repos.
- TriageView extended (`due_week`, `overdue`, `tag:<uuid>`).
- Verified `cargo test -p dp-rest`, `pnpm typecheck`, `make build` all green. Committed on `codeless/triage-slice-2` (7cc1db4).

## Next

- Stage 12 (final) of 12 — a fresh session will pick it up per the workflow.

## What you need to know

- dp-fetcher has 3 preexisting failing tests on the base branch unrelated to this stage (reconciler::tests::not_modified_keeps_since_and_etag_and_writes_no_events, reconciler::tests::pr_list_synthesises_deliveries_that_flow_through_apply_path, phase2_smoke::missed_webhook_detected_by_reconciler). Confirmed by stashing this stage's diff and re-running.
- Date-driven smart views and the Due column rely on per-row useIssueDatesBatch (`useQueries`); bounded by PAGE_SIZE=100 so it’s acceptable but is an N+1 — a future slice should add `due_at`/`start_at` to the IssueListItem wire shape to drop those round-trips.
- Tag-saved views land in the rail but the list query does not yet narrow by tag — clicking a tag-saved view currently surfaces the same /me/queue rows; full narrowing is gated on `GET /issues` accepting a `tag_ids` predicate.
- Mock harness gained an in-memory `mockIssueDates` keyed by issue id so storybook/smoke exercises the picker without a backend.
- OpenAPI snapshot regen procedure: `UPDATE_OPENAPI_SNAPSHOT=1 cargo test -p dp-rest --test openapi_snapshot`.

## Open questions

- Should the `tag:<uuid>` saved view also clear the `repo_id` filter, or compose with it? Left composing for now.
- Splitter min/max widths (rail 160–420, peek 320–720) were picked without UX input; tweak when design lands.
