## Done

- Reviewed stages 7–9 (slice B) diff against Layer-1 invariants R1/R2/R4/R5 and wire-format stability.
- Confirmed `LinkBoardDialog` primary path has no node-id paste field (org-scoped dropdown + date field mapping; paste-field fallback only when picker returns `null` via `upstream_unavailable` / `github_validation_failed`).
- Confirmed per-link mirror status (`✓ synced HH:mm:ss to <board>` / `✕ failed` / `· pending`) renders on `ProjectDetailPage` from `last_mirror_at` / `last_mirror_error`.
- No commit produced (review-only stage).

## Next

- Wire §6.5 in-context `[+ Add to project]` chip + per-issue per-board `SyncStatus` into the workflow detail pane so the acceptance scenario ("edit a date in the workflow detail pane, see 'Synced HH:mm:ss to <board name>' within 5s") lands literally on the workflow pane, not just the project detail page.
- Build out the §6.3 issue list inside `ProjectDetailPage` (currently header + meta + board-links only).
- Bulk-add-from-triage UX (slice A completeness).

## What you need to know

- Diff under review: `93524a7..c56b4bd`, ~1048 lines, frontend-only — `frontend/src/{api/client.ts,app.tsx,layout/app-shell.tsx,routes.ts,admin/projects-page.tsx,projects/*}`.
- Zod DTOs (`BoardPickerDto`, `BoardLinkDto`, `OrgProjectPickerDto`, `CreateBoardLinkRequest`) mirror `crates/dp-rest/src/board_links.rs` 1:1 — no wire-format drift introduced.
- Picker fallback logic: api client maps `code === "upstream_unavailable"` / `"github_validation_failed"` to `null` so the dialog can degrade gracefully without exposing a node-id field on the happy path.
- Admin alias `#/admin/projects` still resolves (renamed to `Admin ▸ Project sync`), preserving deep links.

## Open questions

- (none)

PASS: Layer-1 invariants hold — frontend-only diff, all new operations go through DevPulseApi REST, DTOs mirror existing backend shapes, no crate-dependency or trust-boundary changes.
