## Done

- Stage 3 reports rewrite committed (`f453fe7`) on `codeless/phase-7-frontend-apple`.
- New shared primitives: `frontend/src/components/table.tsx` (shadcn Table family — the kit doesn't ship one) and `frontend/src/components/page-heading.tsx` (h1 + muted description lockup).
- Refactored `data-as-of.tsx` to render as shadcn `Alert` + coloured `Badge` (fresh / lagging / stale / pending) instead of the muted pill div.
- Refactored `window-picker.tsx` to render Label+Select pairs as bare fragment cells (no wrapping bordered card); exports `FILTER_GRID_CLASS` so each report page composes its filter grid identically.
- Refactored `activity-table.tsx` onto the new shadcn Table primitives with `text-sm`, right-aligned `tabular-nums` numeric columns, fixed `h-8 w-24` trend cell, `Skeleton` shapes per cell, and `Empty` for the no-activity state.
- Rewrote user/team/org/home-org-split/freshness report pages to the shared skeleton: PageHeading lockup → filter Card (one grid) → freshness Alert → results Card (TabsList + Table).
- `lens-tabs.tsx` cleaned up to render the kit-default segmented Tabs (active trigger elevated to `bg-background`) with the per-lens hint string beside the TabsList.
- `pnpm typecheck` + `pnpm build` clean (dist 142.5 KiB gzipped). `pnpm test:e2e` 9/9 green including the visual-regression smoke that asserts every report page has `[data-slot="card"]` and `[data-slot="tabs-list"]`.

## Next

- Stage 4 of 7 (per WORKFLOW.md / SCOPE.md). A fresh session should pick it up.

## What you need to know

- The kit (`@nube/starter-ui-kit`) does NOT export a Table primitive — `frontend/src/components/table.tsx` is local. codeless-ui also has no Table primitive, so this is fine; the slot/data-slot attributes match shadcn conventions so a future kit Table can drop in.
- The brief said "copy the JSX shape from `codeless-ui/src/modules/jobs/JobTabs.tsx` if useful" — that reference actually uses underline tabs, not segmented. The brief's primary directive ("shadcn Tabs default style — segmented background, active tab elevated") wins, so I stuck with the kit's default `TabsList`/`TabsTrigger` styling (rounded-full bg-muted with `data-active:bg-background`). LensTabs uses that.
- `DataAsOfBanner` keeps its `data-testid="data-as-of"` and the substring "Data as of" so the existing smoke test selectors still match.
- `WindowPicker` now returns a `<>...</>` of grid cells. The parent must own a grid with `FILTER_GRID_CLASS` (or compatible). All five report pages wire this correctly.
- `Sparkline` accepts `width` / `height` props; I pass `width={96} height={32}` to match the `h-8 w-24` cell footprint exactly.
- Freshness page reuses its own band palette + cards (no lens toggle); only the chrome (PageHeading, Alert+Badge styling, Empty/Skeleton states, rounded-xl cards) changed.

## Open questions

- (none)
