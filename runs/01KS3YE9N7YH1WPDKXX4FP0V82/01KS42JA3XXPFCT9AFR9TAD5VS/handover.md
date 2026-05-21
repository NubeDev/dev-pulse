## Done

- Added zod schemas + API client methods for `GET /orgs/{org_id}/projects-v2` (org-scoped board picker, null on `upstream_unavailable`), `GET /projects/{id}`, and `POST/GET/DELETE /projects/{id}/board-links` (frontend/src/api/client.ts).
- Added react-query hooks `useProject`, `useBoardLinks`, `useOrgBoardPicker`, `useCreateBoardLink`, `useDeleteBoardLink` (frontend/src/projects/use-projects-data.ts).
- Added `projectDetailIdOf` / `projectDetailRoute` to routes.ts and renamed admin tab `projects` → `project-sync` (with legacy `#/admin/projects` alias).
- Wrote `LinkBoardDialog` (org-scoped board dropdown, Start/Due field mapping, no node-id paste field on the primary path; falls through to `[Open GitHub project settings]` hint when picker unavailable).
- Wrote `ProjectDetailPage` with header, meta block (Start/Due/Issues/Boards), and a `Linked GitHub boards` card rendering per-link mirror status (`✓ synced HH:mm:ss` / `✕ failed — <msg>` / `· pending`) plus an Unlink button.
- Wired the detail route into app.tsx and renamed the sidebar entry to `Admin ▸ Project sync` (icon unchanged), updated `ADMIN_TITLE` map.
- Updated `frontend/src/admin/projects-page.tsx` docstring + page heading to mark it the §9.4 escape hatch.
- `pnpm --filter dev-pulse-frontend typecheck` passes.
- Committed as `stage 9: frontend slice B — Link-a-board dialog replacing the admin page`.

## Next

- Stage 10/11 should plumb the workflow-detail-pane `SyncStatus` (§6.5 — per-issue per-board fan-out from the §7.4 `207 Multi-Status` response) and complete slice A bits the spec lists (list page, bulk add from triage, `Add to project` chip in the detail pane). The detail page here is intentionally minimal — header + meta + board-links only; the §6.3 issue list inside it is still TODO.

## What you need to know

- Backend already exposes everything this stage consumes (stage 7/8 shipped `/orgs/{org_id}/projects-v2`, `/projects/{id}/board-links` CRUD, mirror fan-out).
- The Link-a-board dialog branches on `useOrgBoardPicker(...).data === null` to detect "transport unavailable" — the api client maps `code === "upstream_unavailable"` / `"github_validation_failed"` to `null` for ergonomic UI handling.
- The detail route uses a UUID-shape guard so `#/projects/active` etc. still land on the list page; the route `#/projects/{uuid}` lands on the new detail page.
- The §6.5 in-context `[+ Add to project]` chip in the workflow detail pane is NOT in this stage — only the project detail page surfaces the linking + mirror status per the stage 9 scope.
- Admin sidebar entry now reads `Project sync`; the old route `#/admin/projects` still resolves (alias) so any chat-sourced links continue to work.

## Open questions

- (none)
