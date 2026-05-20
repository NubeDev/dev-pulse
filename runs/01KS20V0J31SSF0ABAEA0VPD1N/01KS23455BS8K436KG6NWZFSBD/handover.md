## Done

- pnpm typecheck: clean
- pnpm build: 485 KiB raw / 141.63 KiB gzipped JS + 6.22 KiB gzipped CSS — total dist well under the 2 MB stage-11 gate
- pnpm test:e2e: 9/9 passing (login, user-report lens+window+data-as-of, admin refresh, freshness dashboard, directory sub-nav, no-leaderboard grep, Rust boundary check, dist size gate, **new** visual-regression smoke)
- New visual-regression smoke in frontend/tests/e2e/smoke.spec.ts ("every report page renders with shadcn Card + Tabs primitives") — navigates user/team/org/home-org-split via reports sub-nav and asserts ≥1 `[data-slot="card"]` and ≥1 `[data-slot="tabs-list"]` on each; freshness is checked Card-only with an inline comment explaining why (single-view dashboard, no lens)
- Collapsed the 3 residual hand-rolled sub-nav strips in src/app.tsx into a typed shared `<SubNav>` helper using Tailwind utilities (bg-muted/p-1/rounded-md + bg-primary text-primary-foreground for the active anchor) — dropped the last 9 actual `style={{}}` occurrences flagged by stage 6
- Inline-style audit: `grep -rn "style={{" frontend/src/ | wc -l` reports **2** survivors, both pure doc-comment mentions in app-shell.tsx + login-page.tsx ("No inline `style={{}}` remain in this file"). Well under the <15 target
- scripts/check-boundaries.sh: OK (no backend changes)
- Committed as `cb9f538` on `codeless/phase-7-frontend-polish` with message starting "stage 7: tests + verification —"

## Next

- (none) — final stage of phase 7

## What you need to know

- The new visual-regression smoke is browser-driven (Playwright), not source-grep, so it actually verifies the rendered DOM carries the shadcn data-slots. The check uses `[data-slot="card"]` and `[data-slot="tabs-list"]` (the attrs the kit's Card/Tabs primitives emit at packages/starter-ui-kit/src/components/ui/{card,tabs}.tsx)
- Freshness is intentionally checked Card-only; the SCOPE doesn't give it a lens toggle and adding Tabs there would be a new feature (forbidden by the brief). The test comment documents this scope
- The shared `<SubNav>` component in src/app.tsx is a generic `<T extends string>` so the three sections (Reports / Directory / Admin) keep their distinct `ReportTab` / `DirectoryTab` / `AdminTab` type-safety
- All `data-testid` attrs on the sub-navs (`reports-subnav`, `directory-subnav`, `admin-subnav`) preserved — Playwright still locates them
- Stage 6 had reported 11 residual style={{} } occurrences (9 actual + 2 doc-comment); now down to 2 (both doc-comment), beating the original ≤4 plan target

## Open questions

- (none)
