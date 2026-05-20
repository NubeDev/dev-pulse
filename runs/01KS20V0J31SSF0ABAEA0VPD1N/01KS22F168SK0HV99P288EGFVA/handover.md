## Done

- Stage 5 ("theme + polish pass") implemented and committed as `d01dec6` on `codeless/phase-7-frontend-polish`.
- `frontend/src/globals.css`: dropped the redundant `body { font-family ... }` block and the `dp-pulse` `@keyframes`; kept the `@import "@nube/starter-ui-kit/styles.css"` line and the table-mobile-scroll rule; added explicit `@source "../src"` for Tailwind v4 consumer-side class detection.
- `frontend/index.html`: `<body class="font-sans antialiased bg-background text-foreground">`.
- `frontend/src/components/theme-toggle.tsx`: rewritten to use shadcn `DropdownMenu` with explicit Light/Dark/System items (replaces the cycle-on-click button); test id preserved (`theme-toggle`) plus new per-option ids `theme-toggle-{light,dark,system}`.
- `frontend/src/components/error-boundary.tsx`: now renders shadcn `Alert` (destructive variant) + retry/reload `Button`s instead of a `Card`. `data-testid` and `error-boundary-retry` preserved.
- `frontend/src/components/not-found.tsx`: centred `Card` with the missing route in an inline shadcn-style `<code>` block.
- `frontend/src/components/skeleton.tsx`: local primitive now uses Tailwind `animate-pulse rounded-md bg-muted` via `cn(className)` (no inline styles, no custom keyframe).
- New `frontend/src/components/empty.tsx`: local mirror of shadcn `Empty/EmptyHeader/EmptyTitle/EmptyDescription/EmptyContent` (upstream trips the React 18/19 typing mismatch, same rationale as the local Skeleton).
- Wired `Empty` into `orgs-page.tsx` and `teams-page.tsx` empty states (testids `orgs-empty` / `teams-empty` preserved).
- `frontend/src/auth/protected-route.tsx`: loading shim uses Tailwind utilities (`grid min-h-[100dvh] place-items-center text-muted-foreground`).
- `pnpm typecheck` clean. `pnpm build` clean (264 modules, 28.26 kB CSS, 486.40 kB JS).
- Audit `grep -rn "style={{" frontend/src/ | wc -l` reports **11** (target < 15).

## Next

- Stage 6 of `.codeless/jobs/phase-7-frontend-polish` (next session picks up).

## What you need to know

- Local Empty/Skeleton primitives use `cn` from `@nube/starter-ui-kit/lib/utils` and the same class strings as upstream — swap to upstream imports later when the React 18→19 typing issue is fixed.
- Remaining 11 `style={{` occurrences are all in `frontend/src/app.tsx` (the three sub-nav strips for Reports / Directory / Admin) and two doc-comment mentions in `layout/app-shell.tsx` / `auth/login-page.tsx`. Refactoring the sub-navs to shadcn primitives looks like the next obvious target if a future stage wants to push the count further down.
- Playwright tests don't reference theme toggle, empty-state copy, or the 404 page, so the behaviour-preservation guarantee holds for the smoke suites.
- ThemeToggle's click semantics changed (was cycle-on-click, now opens a menu). If any future test does a `click()` on `[data-testid="theme-toggle"]` expecting a theme switch, it'll need to additionally click `[data-testid="theme-toggle-dark"]` etc.

## Open questions

- (none)
