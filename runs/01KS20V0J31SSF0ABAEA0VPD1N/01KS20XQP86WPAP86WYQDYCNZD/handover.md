## Done

- Wrote frontend/POLISH-PLAN.md (~500 lines): per-file inventory of all 181 `style={{…}}` occurrences with target Tailwind utility + shadcn primitive replacements; CSS-var-to-Tailwind cheatsheet; four worked examples (error-boundary.tsx, app-shell.tsx, login-page.tsx, freshness-page.tsx) and a report-page template (team-report-page.tsx); list of legitimate residual inline-style exceptions (`display:contents`, dynamic computed widths); verification gates.
- Committed as `audit pass — frontend POLISH-PLAN.md` on branch `codeless/phase-7-frontend-polish` (1f71cea).

## Next

- Stage 2 picks up the code rewrite, starting with the four worked-example files as the template, then propagating the patterns to the remaining files per the inventory.

## What you need to know

- Starter UI kit components live at `/home/user/.codeless/worktrees/starter/packages/starter-ui-kit/src/components/ui/` — verified the full shadcn set is available (Card, Tabs, Table, Select, Dialog, Alert, Badge, Skeleton, Sheet, DropdownMenu, Breadcrumb, Tooltip, Progress, …). Imports use `@nube/starter-ui-kit/components/<name>` per the package's `exports` map; `cn` helper lives at `@nube/starter-ui-kit/lib/utils`.
- Tailwind v4 colour tokens (`bg-background`, `text-muted-foreground`, `border-border`, `bg-primary`, `text-destructive`, etc.) are wired by the kit's `styles.css` (already imported via `globals.css`) — the plan maps CSS-var inline styles onto them directly rather than keeping `var(--…)` references.
- The "lens-tabs renders vertically" complaint is **not** an inline-style bug. The shadcn `Tabs` root has `data-horizontal:flex-col` by design (stacks list above content). Flagged for stage-6 visual QA, not stage-2 cleanup.
- Hand-rolled tables in `runs-page.tsx`, `orgs/users/teams-page.tsx`, `activity-table.tsx`, `home-org-split-report-page.tsx` use a CSS-grid + `display:contents` pattern. The plan recommends swapping them for shadcn `Table` primitives where available; the className-only fallback is documented as the floor.
- `data-testid` attributes are the Playwright test surface; the plan preserves every one on the same DOM nodes after refactor.

## Open questions

- App.tsx sub-nav refactor has two options (A: keep anchors, just className-ify; B: shadcn `Tabs` with `asChild` anchors). Stage 2 needs to pick — option A is the safe floor; B is the more idiomatic move.
- `src/components/skeleton.tsx` keeps its bespoke `dp-pulse` keyframe; whether to delete it and switch callers to `@nube/starter-ui-kit/components/skeleton` is left to stage 6.
