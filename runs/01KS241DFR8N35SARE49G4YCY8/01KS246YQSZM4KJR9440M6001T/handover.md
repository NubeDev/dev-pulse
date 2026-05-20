## Done

- Replaced `frontend/src/globals.css` with the codeless-ui blueprint verbatim: `:root` + `.dark` oklch token blocks (background, foreground, card, primary, secondary, muted, accent, destructive, border, input, ring, chart-1..5, sidebar-*), `@theme inline` exposing them to Tailwind v4, the same radius ladder (`--radius: 0.625rem` with `--radius-sm/md/lg/xl/2xl/3xl/4xl`), `--font-sans: "Inter Variable", sans-serif;`, and the same `@layer base` rules (`border-border outline-ring/50` on `*`, `bg-background text-foreground font-sans` on `body`, `height: 100%` on `html, body, #root`).
- Added `@fontsource-variable/inter` and `tw-animate-css` to `frontend/package.json` (the former for Inter Variable, the latter because we now own the Tailwind imports directly instead of inheriting them via the kit stylesheet).
- Dropped the old `@import "@nube/starter-ui-kit/styles.css"` so dev-pulse owns its tokens (the kit components still resolve their utility classes against this app's `@theme inline`).
- Kept the existing small-screen responsive table tweaks at the bottom.
- Verified `pnpm typecheck` and `pnpm build` both succeed; Vite emits the latin/latin-ext/cyrillic/greek/vietnamese Inter woff2 subsets.
- Read and internalised the reference layout sensibility from `JobsDashboard.tsx`, `JobRow.tsx`, `JobDetail.tsx`, `Header.tsx`, and `App.tsx` in codeless-ui — Card density, header bar treatment, sidebar nav rows, Sheet for details, Skeleton shapes.
- Committed as `3e356f2` on branch `codeless/phase-7-frontend-apple`.

## Next

- Stage 2 picks up the component-level overhaul against these foundations (header bar, sidebar treatment, Cards, Tables, Sheet for details, Skeleton rhythm). The stage will land in a fresh session per the job model.

## What you need to know

- The kit stylesheet import was intentionally removed; tokens are now defined in `frontend/src/globals.css` directly. Any future stage that wants kit-only utilities should add them here rather than re-importing the kit's globals (re-importing would double-emit `tailwindcss` and the token blocks).
- `tw-animate-css` had to be added explicitly — codeless-ui imports it directly, and it was previously transitive via the kit's stylesheet. Without it the Vite build fails to resolve the import in `globals.css`.
- The reference globals.css contained desktop-shell-only blocks (xterm.js scrollbar overrides, CodeMirror scrollbar overrides, `data-chrome="borderless"` window chrome) that don't apply to dev-pulse (a pure browser SPA), so they were not lifted.
- `index.html` already had `class="font-sans antialiased bg-background text-foreground"` on `<body>`, so font-sans is doubly enforced (base layer + inline class) — leaving both is fine and matches the codeless-ui pattern.
- `frontend/POLISH-PLAN.md` is the prior Phase 7 polish ground-rules doc; the rules there (keep `data-testid` verbatim, use shadcn primitives only, no behaviour change) still apply to the upcoming visual stages.
- Recent commits live on branch `codeless/phase-7-frontend-apple`; the prior Phase 7 + Phase 7 Polish work already landed on main via PR #6.

## Open questions

- (none)
