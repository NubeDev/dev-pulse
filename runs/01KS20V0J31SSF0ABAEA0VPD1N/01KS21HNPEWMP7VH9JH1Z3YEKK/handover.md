## Done

- Refactored every report file to drop inline `style={{…}}`:
- `lens-tabs.tsx`: explicit `orientation="horizontal"`, `grid gap-3`, hint as `text-muted-foreground`.
- `data-as-of.tsx`: Tailwind muted pill + shadcn `Badge` for the source/delta tag.
- `window-picker.tsx`: card-bordered grid wrapper with `grid grid-cols-[repeat(auto-fit,minmax(10rem,1fr))]` + `grid gap-1.5` field wrappers.
- `activity-table.tsx`: semantic `<table>` with shared `HEADER_CLASS / CELL_CLASS / NUM_CLASS` constants; sort buttons use shadcn Button ghost variant via className.
- `user-report-page.tsx`, `team-report-page.tsx`, `org-report-page.tsx`: flex-header lockup (`text-2xl font-semibold tracking-tight` title + muted description), `CardContent` is `grid gap-4`, selectors use `grid gap-1.5`, headline uses `text-base text-foreground`.
- `home-org-split-report-page.tsx`: same template; share bar replaced with shadcn `Progress`; "No leaderboard" phrasing preserved (static-check still passes).
- `freshness-page.tsx`: `BAND_STYLE` (oklch inline) replaced with `BAND_CLASSES` (Tailwind className groups: emerald/amber/red/neutral with dark-mode variants). Headline rendered via shadcn `Alert` + `AlertDescription`. Per-org card uses Badge for the status pill. Error banner uses `Alert variant="destructive"`.
- `pnpm typecheck` and `pnpm build` both pass.
- Reports tree `grep -n 'style={{'` → 0 occurrences.
- All `data-testid` hooks (`activity-table`, `headline`, `data-as-of`, `freshness-headline`, `freshness-grid`, `freshness-card`, `home-org-split-table`, `user-select`, `team-org-select`, `team-select`, `org-select`, `activity-skel-*`, `activity-table-empty`) preserved on their original DOM nodes.
- Committed as `6ff8827` with subject starting with "stage 3:".

## Next

- (none — fresh session picks up stage 4)

## What you need to know

- The kit ships no `Table` component (verified `frontend/node_modules/@nube/starter-ui-kit/src/components/ui/`); used semantic `<table>` with className constants — matches the POLISH-PLAN floor.
- Local `components/skeleton.tsx` still uses inline `style` internally (it owns the `dp-pulse` keyframe). Callers now pass Tailwind `className` for sizing; widths/heights flow through because the local skeleton's inline style doesn't pin them. Border-radius defaults to `0.5rem` from the local component — Tailwind `rounded-*` is ignored, which matches prior behaviour.
- The "renders vertically" complaint was traced in the POLISH-PLAN to parent containers, not to lens-tabs itself. The kit's `Tabs` root uses `data-horizontal:flex-col` which stacks the list above the content (intended); the triggers themselves are inline-flex in a horizontal `TabsList`. Adding `orientation="horizontal"` explicitly + giving the root `gap-3` keeps the triggers reading left-to-right.
- AlertDescription's baked-in `text-muted-foreground` is overridden via twMerge by passing `text-current` in the headline alert; band-colour text-foreground bleeds through correctly.
- Static-check guard: kept the exact phrase "No leaderboard — totals + share only" so the regex (`>\s*Leaderboard\s*<`) doesn't match (the trailing em-dash blocks it).

## Open questions

- (none)
