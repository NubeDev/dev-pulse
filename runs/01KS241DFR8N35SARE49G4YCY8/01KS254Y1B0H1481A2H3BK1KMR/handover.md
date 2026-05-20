## Done

- Directory pages (users, orgs, teams, home-org) rewritten with `PageHeading` lockup + filter `Card` + results `Card` pattern; results render through the local shadcn-shaped `Table` primitive so rows pick up `hover:bg-muted/50 transition-colors`.
- Users-page search uses `InputGroup` + leading magnifier addon (preserved from prior stage); home-org Dialog form uses `grid gap-4` rhythm.
- Admin runs page: shadcn `Table` + Badge variants (default / secondary / destructive) per status, with subtle tonal accents kept on top.
- Admin refresh page: single always-mounted `Alert` walks idle → loading (+ shadcn `Spinner`) → success (+ inline check icon) → destructive; existing `data-testid="refresh-result"` preserved on success branch.
- Admin users page: PageHeading added; `Progress` bar moved below the action row.
- Login page: gained `CardFooter` housing the submit button; form switched to `display: contents` so Card owns the layout rhythm.
- `pnpm typecheck` and `pnpm build` both clean. Committed as `fe23290` on branch `codeless/phase-7-frontend-apple`.

## Next

- Stage 5 of 7 (per SCOPE-driven visual overhaul) — pick up from this branch in a fresh session.

## What you need to know

- Local table primitive lives at `frontend/src/components/table.tsx`; the kit doesn't export Table, so directory + runs pages import from there.
- `data-testid` hooks preserved verbatim: `users-table`, `orgs-table`, `teams-table`, `runs-table`, `run-status-badge`, `refresh-result`, `refresh-error`, `refresh-status` (new for idle/loading), `home-org-confirm`, `anonymise-confirm`, `admin-export-progress`, etc.
- Refresh page now emits two new test-ids — `refresh-status` with `data-kind="idle"` or `"loading"` — these don't conflict with existing smoke tests (which only check `refresh-result`).
- Run-status Badge keeps colour cues via a `STATUS_META` className map; spec asked for success/secondary/destructive variants, mapped: clean→default+emerald tint, running→secondary+blue, partial→secondary+amber, failed→destructive.
- Login form uses `<form className="contents">` wrapping the Card so the Button in `CardFooter` still submits the form (plus `form="login-form"` for safety).

## Open questions

- (none)
