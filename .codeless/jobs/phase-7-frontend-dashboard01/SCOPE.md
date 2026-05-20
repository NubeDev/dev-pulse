# Scope — phase-7-frontend-dashboard01

## Goal

Rebuild the dev-pulse frontend layout around the official shadcn
`dashboard-01` block. Install it via `pnpm dlx shadcn@latest add
dashboard-01`, then adopt the block's components (`app-sidebar`,
`site-header`, `section-cards`, `chart-area-interactive`,
`data-table`) as the chrome and surface for every dev-pulse page.
Map our content (reports, directory, admin) onto those primitives.

Pure presentation overhaul. Zero behaviour change. Zero backend
changes. Existing routes, query keys, RPCs, mock-mode, and
Playwright contracts stay intact.

## In scope

- Run `pnpm dlx shadcn@latest add dashboard-01 --overwrite` from
  `frontend/` (init shadcn first if `components.json` missing).
- Adopt the block's `AppSidebar` + `SidebarProvider` +
  `SidebarInset` + `SiteHeader` as the new app shell.
- Map dev-pulse nav into `NavMain` (Reports, Directory, Admin as
  top-level groups with collapsible sub-items per route).
- Wire `/auth/me` into the block's `NavUser` footer block.
- Restructure every report page as: heading → filter Card →
  freshness Alert → `SectionCards` (3-4 KPI cards from the
  headline + sparkline data) → `ChartAreaInteractive` (using the
  existing trend series) → `DataTable` (existing breakdown rows)
  with the three-lens toggle living in the DataTable's `Tabs`
  toolbar.
- Directory + admin pages use the block's `DataTable` and
  `SectionCards` patterns.
- Keep our globals.css token block (codeless-ui oklch lifted in
  the previous run). Accept whatever new primitives the block
  drops into `src/components/ui/`.

## Out of scope

- Any new RPCs, query keys, or backend changes.
- Touching `src/api/` or the OpenAPI snapshot.
- Editing `crates/starter-*`, `packages/`, or any Rust crate.
- Adding new reports or new business logic. The chart/cards/table
  are fed from data we already fetch.
- Replacing react-query or the existing route shapes.
- Restyling shadcn primitives — accept the block's choices.

## Hard rules

- **dashboard-01 block is the visual contract.** A reviewer
  opening the shadcn dashboard-01 demo and a dev-pulse report
  page side-by-side should immediately see the same family —
  sidebar treatment, header chrome, SectionCards rhythm,
  DataTable density.
- **No leaderboard, no single-score affordance** (SCOPE §4).
- **Three-lens toggle lives in the DataTable's `Tabs` toolbar**,
  horizontal, default segmented style.
- **`grep -rn "style={{" frontend/src/ | wc -l` < 10**, survivors
  carry a justifying comment.
- **All Playwright smokes still pass** + new visual-regression
  smoke asserting `[data-slot="sidebar"]` presence and at least
  one `[data-slot="section-cards"]` (or block-equivalent) per
  report page.
- **`scripts/check-boundaries.sh` green** — only `frontend/`
  touched.
- **mock-mode still works** (VITE_USE_MOCK_REPORTS=1 +
  mockAuthPlugin).
- **No `--force`, no `--no-verify`**.

## Reference

- Block source: `pnpm dlx shadcn@latest add dashboard-01` (it
  prints every file it adds; read each one before mapping content
  onto it).
- Block demo: https://ui.shadcn.com/blocks (dashboard-01).
- Token system (already in place from previous run):
  `frontend/src/globals.css`.
- codeless-ui parallel reference (same shadcn aesthetic, same
  tokens): `/home/user/code/rust/codeless-workspace/codeless/ui/codeless-ui/src/`.

## Smoke tests (merge gate)

- `pnpm typecheck` clean.
- `pnpm build` produces dist < 2MB gzipped.
- `pnpm test:e2e` all green.
- Visual-regression smoke: every report page renders
  `[data-slot="sidebar"]` + at least one `[data-slot="section-cards"]`
  (or block-equivalent) + at least one `[data-slot="tabs"]`.
- `grep -rn "style={{" frontend/src/ | wc -l` < 10.
- `scripts/check-boundaries.sh` green.

## Open questions (resolve in stage 1)

1. **Chart data feed for `ChartAreaInteractive`.** Bias: derive
   from the existing per-activity-type trend series we already
   fetch for sparklines; sum across activity types per date
   bucket to get the "all activity over time" line the chart
   wants. No new RPC.
2. **SectionCards content per report.** Bias: pick the top 3-4
   activity counts from the report headline (e.g. PRs opened,
   PRs merged, Commits, Comments) and use the existing
   sparkline + previous-window delta to feed the card''s trend
   badge.
3. **Per-org-split lens inside the DataTable Tabs toolbar.**
   Bias: each lens is a `TabsTrigger` whose `value` swaps the
   table's columns/rows. State stays in URL search params, same
   as today.
