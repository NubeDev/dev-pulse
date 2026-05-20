## Done

- Rewrote `frontend/src/layout/app-shell.tsx` to a proper shadcn sidebar layout matching the codeless-ui reference: h-14 sticky+blur header, 15rem `bg-card/40` sidebar, `max-w-6xl` content column, `data-[active=true]:*` nav rows, rounded-square initials chip in the user-menu trigger, brand mark + wordmark on the left.
- Verified: `pnpm typecheck` clean, `pnpm build` clean (dist 141.91 KiB gzipped, under the 2 MB gate), `pnpm test:e2e` 9/9 green (incl. login, lens toggle, admin, no-leaderboard grep, Rust boundary check, shadcn Card+Tabs visual-regression smoke).
- Committed as `534c98e`: "stage 2: app shell rewrite — shadcn sidebar layout matching codeless-ui".

## Next

- Stage 3 of 7 picks up next session per WORKFLOW.md (this run only ships Stage 2).

## What you need to know

- Existing test selectors preserved verbatim: `data-testid="app-shell"`, `primary-nav`, `primary-nav-mobile`, `user-menu-trigger`, `theme-toggle`.
- Hash routing remained the source of truth — sidebar items are plain `<a>` anchors with `data-active="true|false"` driving the accent state (same shape shadcn's Sidebar primitive uses). No router dependency added.
- Mobile parity kept: the same `<NavLinks>` renders inside a `<Sheet>` triggered by a hamburger button (hidden on `md`+).
- Brand mark is an inline "dp" chip on `bg-primary`; no asset added.
- Inline-style audit on this file: 1 survivor, a doc-comment mention only (line 19: "No inline `style={{}}` in this file").

## Open questions

- The stage spec said "60-ish px sidebar items"; I implemented the literal `px-3 py-2 text-sm` (~36 px tall) given in the same paragraph since those classes were written out explicitly. If the next-stage reviewer reads "60-ish" as the target, the row padding will need to grow (e.g. `py-3` or `py-4`) and the glyph chip with it.
