## Done

- Rewrote `frontend/src/layout/app-shell.tsx` on shadcn + Tailwind: `Sheet` for mobile nav (hamburger trigger, `md:hidden`), fixed desktop sidebar (`md:flex md:flex-col`), `Separator`, ghost `Button` nav items with active-state via `bg-primary text-primary-foreground`, `DropdownMenu` user menu (email + role + Logout), and a route-derived `Breadcrumb` in the header. `useIsMobile()` deleted; responsiveness driven entirely by `md:` utilities. Zero inline `style={{}}` left in the file.
- Rewrote `frontend/src/auth/login-page.tsx` as a centred `Card` with `grid gap-4` form spacing, `Label` + `Input` + `Button`, and `<Alert variant="destructive">` for the error banner. Zero inline `style={{}}` left.
- Typecheck clean; all 8 Playwright smokes (incl. no-leaderboard grep, mock-mode walkthrough) still pass.
- Committed as `30b1f95` on `codeless/phase-7-frontend-polish` with title starting `stage 2: app shell + layout rewrite`.

## Next

- (none) — next session picks up stage 3 per the phase plan.

## What you need to know

- Desktop sidebar nav keeps `data-testid="primary-nav"`; the mobile Sheet's nav uses `data-testid="primary-nav-mobile"` to keep testids unique when both are mounted (Radix Dialog only renders content while open, but kept the rename anyway for clarity). No test currently asserts on either testid, only on `app-shell`.
- `<a href="#/admin">` anchors used by `smoke.spec.ts` are still in the desktop sidebar at default Playwright viewport (~1280px), so `locator(...).first().click()` still resolves.
- Logout is now inside `DropdownMenu` (trigger has `data-testid="user-menu-trigger"`); no test currently invokes it, but accessible via menu role.
- Breadcrumb derivation lives in this file (`SECTION_LABEL`, `REPORT_TAB_LABEL`, `DIRECTORY_TAB_LABEL`, `ADMIN_TAB_LABEL`, `crumbsFor`) — uses the existing `reportTabOf` / `directoryTabOf` / `adminTabOf` parsers from `routes.ts`.
- `ThemeToggle` still has inline styles; not in stage 2 scope (touches `components/theme-toggle.tsx`) but flagged for a later stage.

## Open questions

- (none)
