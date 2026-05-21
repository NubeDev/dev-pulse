## Done

- Added `Project*` zod schemas + types (`ProjectStatusDto`, `ProjectDto`, `ProjectListResponse`, `ListProjectsQuery`) and `api.listProjects(q)` to `frontend/src/api/client.ts`, matching the §7.1 wire contract from `linear-projects-v2.md` (board_link_count carried as `0` for slice B forward-compat).
- New `frontend/src/projects/use-projects-data.ts` with `useProjectCount(status)` + `useProjectList(q)` hooks; cache-keyed under `["projects", …]`.
- New `frontend/src/projects/projects-page.tsx` placeholder so `#/projects` and `#/projects?status=…` no longer 404 ahead of the §6.2 list page.
- `routes.ts`: `Section` gains `"projects"`, added `ProjectStatusRoute`, `projectsStatusOf(route)`, and `projectsRoute(status)`; `projects` registered in `sectionOf` + `isKnownRoute`.
- `layout/app-shell.tsx`: Projects nav entry inserted between Workflow and Directory (icon `IconLayoutKanban`, testid `projects-subnav`) with four sub-items Active / Backlog / Done / Archived. Live counts via three `useProjectCount` probes (Archived stays uncounted per §6.1). Badge testids: `projects-count-{active,backlog,done}`; only renders when `count > 0` so the sidebar never flashes `0`. `activeUrlFor` now preserves the `?status=…` query so the right sub-item highlights. `titleFor` produces `Projects · Active` / `Projects` headers.
- `app.tsx` wires the `projects` case in `SectionPane` → `ProjectsPage`.
- `pnpm typecheck` + `pnpm build` green; committed as `e92580f`.

## Next

- Stage 6: the §6.2 `#/projects` list page — search input, status grouping, `[+ New project]` modal, and the create mutation. Stub at `frontend/src/projects/projects-page.tsx` is ready to be replaced; the `useProjectList(q)` hook already exists and the route already parses `?status=…`. Or — per the previous handover from stage 3 — slice B's org-scoped board picker + link CRUD (`GET /orgs/{org_id}/projects-v2`, `/projects/{id}/board-links`, §7.4 mirror fan-out) may be next; pick from the §12 phasing.

## What you need to know

- The sidebar issues three count-only requests on every shell render. They're cheap (server returns `{ rows: [], total, limit: 0, offset }`) and react-query'd with `staleTime: 30_000`, but the loader hooks fire unconditionally — there's no auth gate beyond what the network layer enforces. The existing inbox badge pattern (`useMyQueue({ limit: 1 })`) does the same thing.
- Counts <= 0 deliberately render *no* badge. This avoids the "Active (0)" flash before the first response lands and matches the spec's "no zero badge" implicit ask. The badge appears as soon as `useProjectCount` resolves with a positive number.
- The Projects sub-items use `?status=…` query-string scoping, not path scoping (unlike Workflow/Reports/etc.). `activeUrlFor` special-cases this so the active highlight tracks the filter.
- Slice A still leaves the Admin → Projects tab in place. Per §11 it gets retired in slice B once the §6.4 board-link dialog ships.
- No e2e test was added this stage — the smoke suite uses the mock layer and there is no `mockListProjects` yet; wiring mocks is best done alongside the actual list-page UI in the next stage to avoid a partial fixture.

## Open questions

- (none)
