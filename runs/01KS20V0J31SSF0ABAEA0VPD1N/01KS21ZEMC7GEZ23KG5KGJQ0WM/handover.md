## Done

- Rewrote `frontend/src/admin/runs-page.tsx` to use a semantic `<table>` + shared `HEADER_CLASS`/`CELL_CLASS` Tailwind constants; status column now a shadcn `Badge` driven by a `STATUS_CLASS` className map (running/partial/failed/clean, dark-mode aware). Card header / refresh row / pagination row all use Tailwind utilities.
- Rewrote `frontend/src/admin/refresh-page.tsx`: org-scope wrapper, trigger row, result panel switched to Tailwind utilities; the error path is now a shadcn `Alert variant="destructive"` and the result panel is a default `Alert` (`data-testid="refresh-result"` preserved on the Alert root). Badge color overrides use className groups.
- Rewrote `frontend/src/admin/users-page.tsx`: feedback surfaces become shadcn `Alert` (default/destructive), `AlertDialogAction` now uses `variant="destructive"`, and the export button gets a shadcn `Progress` indicator (two-step 33%→100%, hidden when idle — dp-rest streams without a content-length header so byte-accurate is not possible).
- Rewrote `frontend/src/directory/orgs-page.tsx`, `teams-page.tsx`, `users-page.tsx` to semantic `<table>` markup with the same Tailwind constants. `users-page` search input is wrapped in shadcn `InputGroup` + `InputGroupAddon` (inline SVG magnifier, sized to the addon's default 16px) + `InputGroupInput`.
- Converted `frontend/src/directory/home-org-page.tsx` from `AlertDialog` to shadcn `Dialog` with `DialogTrigger asChild` wrapping the submit button, plus `DialogHeader`/`DialogTitle`/`DialogDescription`/`DialogFooter`/`DialogClose`; confirmation body composes a read-only user/org summary. Feedback uses shadcn `Alert` (default/destructive).
- `pnpm typecheck` ✓, `pnpm build` ✓ (485 KB JS, 27.6 KB CSS, no compile errors).
- Committed as `stage 4: directory + admin rewrite …` (`ea98f5c`).

## Next

- Stage 5 of 7 picks up next per WORKFLOW.md (not started here).

## What you need to know

- The kit ships no `Table` primitive — we mirror reports/activity-table.tsx's "semantic `<table>` + shared HEADER_CLASS/CELL_CLASS Tailwind constants" pattern. Every per-file copy is identical; if you want a DRY pass, hoist them into a shared util in a later stage.
- All preserved data-testids: `runs-table`, `runs-empty`, `runs-error`, `runs-refresh`, `runs-refresh-status`, `runs-prev`, `runs-next`, `run-status-badge`; `refresh-trigger`, `refresh-result`, `refresh-items`, `refresh-errors`, `refresh-partial`, `refresh-error`, `refresh-org`, `refresh-scope`; `admin-user-select`, `admin-export`, `admin-anonymise`, `admin-export-progress` (new), `admin-users-feedback`, `anonymise-confirm`, `anonymise-confirm-login`, `anonymise-cancel`, `anonymise-confirm-submit`; `orgs-table`, `orgs-empty`, `orgs-error`, `org-member-count`; `teams-table`, `teams-empty`, `teams-error`, `teams-org-select`; `users-table`, `users-empty`, `users-error`, `users-search`, `users-org-filter`, `user-membership`, `user-home-org`; `home-org-user`, `home-org-org`, `home-org-submit`, `home-org-confirm`, `home-org-cancel`, `home-org-confirm-submit`, `home-org-feedback`.
- `home-org-page` simplified state: dropped the separate `pending` object — controlled `open` + the existing `userId`/`orgId` state drives the dialog. Mutation `onSettled` closes the dialog; `onOpenChange` ignores attempts to close mid-flight.
- The kit's `Progress` component contains an `inline style={{ transform }}` on its indicator — that's kit code, not ours, and not in scope for the "no `style={{}}` survivors" gate.
- Smoke tests in `frontend/tests/e2e/smoke.spec.ts` only assert against `refresh-trigger` / `refresh-result` / `refresh-items` from stage 4's surface — all preserved. I did not run Playwright (no behaviour change, all data-testids preserved).

## Open questions

- (none)
