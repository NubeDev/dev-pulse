## Done

- routes.ts: added `AdminTab` + `adminTabOf` parser (defaults to `runs`).
- frontend/src/admin/mocks.ts: deterministic MOCK_RUNS (32 rows, all 4 status branches), MOCK_ORGS, MOCK_USERS, mockRefresh, mockUserExport, gated by `VITE_USE_MOCK_REPORTS=1`.
- frontend/src/admin/runs-page.tsx: `GET /admin/runs` paginated table (25/page), derived status badge (running/partial/failed/clean), 15s `refetchInterval` (paused when tab hidden), manual "Refresh now" + prev/next pagination, error and empty states.
- frontend/src/admin/refresh-page.tsx: org-scope Select (with "All orgs" sentinel), `POST /admin/refresh` mutation with loading state, last-result panel (items/errors/partial badges, "no-op" branch when `ran: false`), invalidates `admin-runs` + `report-freshness` queries on success.
- frontend/src/admin/users-page.tsx: user Select, "Export user data" button — direct `client.fetch(/admin/users/:id/export)` → Blob → hidden anchor download (bypasses zod wrapper, filename `dev-pulse-user-<login>-export.json`), "Anonymise user…" opens AlertDialog requiring typed-login confirmation before `POST /admin/users/:id/anonymise`.
- app.tsx: replaced AdminHome placeholder with AdminSection/AdminPane following the same plain-anchor sub-nav idiom as Reports/Directory.
- `pnpm typecheck` and `pnpm build` both pass.
- Committed as `admin pages — Runs log, Refresh trigger, User GDPR controls` (2402040).

## Next

- (none) — stage 9 picks up in a fresh session per job instructions.

## What you need to know

- `api.exportUser` zod-parses the whole envelope, which is the wrong shape for a download. The users page deliberately bypasses it and calls `api.client.fetch` directly to get a Blob.
- Refresh "All orgs" uses the `__all__` sentinel because Radix `<SelectItem>` refuses empty string values.
- `dist/` artefacts are checked in and were rebuilt (consistent with prior stages' commits).
- Smoke harness flag is shared (`VITE_USE_MOCK_REPORTS=1`) — same env switch as reports/directory.

## Open questions

- (none)
