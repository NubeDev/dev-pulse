## Done

- `src/main.tsx`: wraps app in `<ThemeProvider defaultTheme="system">` from `@/theme` (resolved via the `@/` Vite + tsconfig alias because starter-ui-kit doesn't export `./theme` as a subpath). `QueryClient` defaults bumped to `retry: 1` for queries (mutations stay at 0).
- `src/components/theme-toggle.tsx` (new): icon-only button cycling light → dark → system, persisted by `ThemeProvider` (localStorage `starter:theme`).
- `src/components/error-boundary.tsx` (new): class boundary with Retry + Reload-page actions; auto-resets on `resetKey` change so route navigation clears a previous crash.
- `src/components/not-found.tsx` (new): 404 card with deep-links to `#/reports` and `#/directory`.
- `src/components/skeleton.tsx` (new): local Skeleton primitive (pulsing styled div). Upstream `@nube/starter-ui-kit/components/skeleton` and `…/empty` use React 19 implicit-global typings that clash with this app's React 18 — that's why I didn't reuse them.
- `src/layout/app-shell.tsx`: responsive — at viewport `<= 768px` the sidebar collapses into a horizontal scroll-strip below the header; email/role chip hidden on mobile to save space. ThemeToggle added to the header.
- `src/app.tsx`: every section pane is now wrapped in `<ErrorBoundary scope=… resetKey={route}>`. Unknown root segments (`isKnownRoute(route) === false`) render `<NotFoundPage />` inside the shell.
- `src/routes.ts`: new `isKnownRoute(route)` helper.
- `src/reports/activity-table.tsx`: per-row Skeletons for the Total + Trend columns while a kind's query is in flight; empty-state row when every kind has loaded with `total === 0`.
- `src/globals.css`: `@keyframes dp-pulse` (Skeleton animation) + `@media (max-width: 768px)` rules — horizontal-scrolling tables, wrap-on-narrow sub-nav, word-break headings.
- `pnpm typecheck` + `pnpm build` both pass; `dist/` rebuilt and committed (matches prior stages' convention).
- Committed as `polish + responsive + dark mode — …` (829b234).

## Next

- (none) — stage 10 picks up in a fresh session per job instructions.

## What you need to know

- `@nube/starter-ui-kit` only exports `./styles.css`, `./theme-editor`, `./components/*`, `./lib/utils`. There's no `./theme` subpath, and importing from the barrel pulls every component into the typecheck graph (which trips React-version mismatches on `skeleton.tsx`/`empty.tsx`). Resolve via `import … from "@/theme"` (the `@/*` → `starter-ui-kit/src/*` alias is wired in both `vite.config.ts` and `tsconfig.json`).
- Upstream `starter-ui-kit/src/components/ui/{skeleton,empty}.tsx` rely on the implicit global `React` namespace and don't compile under React 18's `@types/react`. Don't import them directly; use the local `src/components/skeleton.tsx` (and add an Empty if needed later) instead. Fixing upstream is out of scope for the dev-pulse worktree.
- The hash router still defaults unknown sub-paths to the section default (e.g. `#/reports/zzz` → user report). Only unknown *root* segments (`#/foo/bar`) hit the 404. That's the intentional "typo tolerance for known sections" behaviour from earlier stages.
- The ErrorBoundary uses `resetKey={route}`, so navigating away from a crashed page silently clears the error — operators won't see a stale error card after they click somewhere else.
- Theme dark-mode class is toggled on `document.documentElement.classList` ("dark"), so all starter-ui-kit components inherit the right tokens automatically.

## Open questions

- (none)
