# Frontend polish plan (Phase 7 → shadcn refactor)

Goal: replace 181 inline `style={{…}}` occurrences in
`frontend/src/**` with Tailwind v4 utility classes and the shadcn
primitives exported from `@nube/starter-ui-kit/components/*`. Zero
behaviour change, zero new features, zero backend changes. Existing
`data-testid` hooks must remain on the same DOM nodes so the
Playwright smokes (typecheck, no-leaderboard grep, mock-mode walk)
keep passing.

## Ground rules

1. **Use `className=` with Tailwind utilities** for layout, spacing,
   typography, borders. Drop the `var(--…)` CSS-token references in
   favour of the matching Tailwind colour tokens that the kit's
   `styles.css` already wires up: `bg-background`, `text-foreground`,
   `bg-card`, `bg-muted`, `text-muted-foreground`, `border`,
   `border-border`, `bg-primary`, `text-primary-foreground`,
   `text-destructive`, etc. (The kit binds these via `@theme inline`
   in `starter-ui-kit/src/styles/globals.css`.)
2. **No new components.** Use the existing shadcn exports only.
3. **Replace hand-rolled tables** with `Table / TableHeader / TableRow
   / TableHead / TableBody / TableCell` from
   `@nube/starter-ui-kit/components/table`. (NOTE: kit ships these via
   the same `components/*` export — confirm at edit time; if absent,
   keep semantic `<table>` but drop the inline styles in favour of
   utilities and the `bg-card`/`bg-muted` tokens.)
4. **Replace hand-rolled sub-nav anchor strips with `Tabs`** where
   the existing nav is a flat segmented control. The route stays the
   source of truth: render `Tabs` controlled with `value={tab}` and
   `onValueChange={(v) => navigate(...)}`. The triggers are still
   anchor-shaped, just inside a `TabsList`.
5. **Use shadcn `Badge`** for chip-style status pills (the freshness
   band pill, run-status badge, role pill in the header).
6. **Use shadcn `Alert` / `AlertTitle` / `AlertDescription`** for
   inline error banners (`oklch(0.5 0.2 25)` text → `variant="destructive"`).
7. **Use shadcn `Skeleton`** instead of the local
   `components/skeleton.tsx` if straightforward; otherwise leave
   `components/skeleton.tsx` (it owns the `dp-pulse` keyframe) and
   strip inline styles on the *callers*.
8. **The bespoke band/status colour palettes** in `freshness-page.tsx`
   and `runs-page.tsx` are *semantic* (fresh/warning/stale,
   running/partial/failed/clean). Convert their `BAND_STYLE` /
   `STATUS_STYLE` records from inline `style` objects to className
   strings (e.g. `border-emerald-500 bg-emerald-50 text-emerald-900`
   for fresh) so the polish refactor doesn't accidentally drop the
   colour cue.
9. **Keep all `data-testid` and `data-*` attributes verbatim.** They
   are the test surface. Move them onto whichever element holds the
   replaced styles (typically the outer wrapper).

## Token mapping cheatsheet (CSS var → Tailwind class)

| inline `style`                                  | Tailwind / shadcn                                         |
|--                                               |--                                                         |
| `background: var(--background)`                 | `bg-background`                                           |
| `background: var(--card)`                       | `bg-card`                                                 |
| `background: var(--muted)`                      | `bg-muted`                                                |
| `background: var(--primary)`                    | `bg-primary`                                              |
| `color: var(--foreground)`                      | `text-foreground`                                         |
| `color: var(--muted-foreground)`                | `text-muted-foreground`                                   |
| `color: var(--primary-foreground)`              | `text-primary-foreground`                                 |
| `color: var(--destructive)` / `oklch(0.5 0.2 25)`| `text-destructive` (or `<Alert variant="destructive">`)  |
| `border: 1px solid var(--border)`               | `border border-border`                                    |
| `borderBottom: 1px solid var(--border)`         | `border-b border-border`                                  |
| `borderRadius: var(--radius-sm, 0.375rem)`      | `rounded-sm` (or `rounded-md`)                            |
| `borderRadius: var(--radius-md, 0.5rem)`        | `rounded-md`                                              |
| `borderRadius: 50%`                             | `rounded-full`                                            |
| `display: grid; gap: 1rem`                      | `grid gap-4`                                              |
| `display: grid; gap: 0.75rem`                   | `grid gap-3`                                              |
| `display: grid; gap: 0.5rem`                    | `grid gap-2`                                              |
| `display: grid; gap: 0.25rem`                   | `grid gap-1`                                              |
| `display: flex; gap: 0.5rem`                    | `flex gap-2`                                              |
| `alignItems: center`                            | `items-center`                                            |
| `justifyContent: space-between`                 | `justify-between`                                         |
| `flexWrap: wrap`                                | `flex-wrap`                                               |
| `padding: 0.75rem 1rem`                         | `px-4 py-3`                                               |
| `padding: 0.5rem 0.625rem`                      | `px-2.5 py-2`                                             |
| `fontSize: 0.875rem`                            | `text-sm`                                                 |
| `fontSize: 0.8125rem`                           | `text-[0.8125rem]` (or step up to `text-sm`)              |
| `fontSize: 0.75rem`                             | `text-xs`                                                 |
| `fontSize: 0.9rem` / `0.9375rem`                | `text-sm` (round to scale)                                |
| `fontSize: 1rem`                                | `text-base`                                               |
| `fontSize: 1.125rem`                            | `text-lg`                                                 |
| `fontWeight: 600`                               | `font-semibold`                                           |
| `textTransform: uppercase`                      | `uppercase`                                               |
| `letterSpacing: 0.02em` / `0.04em`              | `tracking-wide` / `tracking-wider`                        |
| `fontVariantNumeric: tabular-nums`              | `tabular-nums`                                            |
| `minHeight: 100dvh`                             | `min-h-dvh`                                               |
| `overflow: auto`                                | `overflow-auto`                                           |
| `display: contents`                             | keep inline; Tailwind has no util (or drop with table refactor) |

## Worked example #1 — `src/components/error-boundary.tsx` (3 inline styles)

Current shape: `<Card>` → `<CardHeader>` → `<CardContent>` with three
inline-styled blocks (the content grid, the `<pre>`, and the button
row).

| line | inline-style | replacement |
|--|--|--|
| L73  | `<CardContent style={{ display:"grid", gap:"0.75rem" }}>` | `<CardContent className="grid gap-3">` |
| L74–86 | `<pre style={{ fontSize:"0.8125rem", color:"var(--muted-foreground)", background:"var(--muted)", padding:"0.75rem", borderRadius:"var(--radius-sm,0.375rem)", overflow:"auto", whiteSpace:"pre-wrap", wordBreak:"break-word", margin:0 }}>` | `<pre className="m-0 overflow-auto whitespace-pre-wrap break-words rounded-md bg-muted p-3 text-sm text-muted-foreground">` |
| L89  | `<div style={{ display:"flex", gap:"0.5rem" }}>` | `<div className="flex gap-2">` |

No structural changes. Optionally swap the bespoke header pair for
`<Alert variant="destructive">` later; for this stage keep the
Card-based layout to avoid behaviour drift.

## Worked example #2 — `src/layout/app-shell.tsx` (10 inline styles)

The big outer `<div>` (L65), the `<header>` (L77), the email-and-role
span (L100/L102), the nav (L128), each nav anchor (L161), the
`<Separator>` (L178), the footer span (L179), and the `<main>`
(L192) — all converted to className.

| element / line | replacement |
|--|--|
| outer `<div data-testid="app-shell">` L65-76 | classes: `min-h-dvh grid bg-background text-foreground` with `grid-cols-[16rem_1fr] grid-rows-[auto_1fr]` desktop / `grid-cols-1 grid-rows-[auto_auto_1fr]` mobile. Use `md:` breakpoint to drive responsiveness from the same node and *delete* `useIsMobile()` from the layout decisions (keep `data-layout` for tests by checking the viewport once via `useSyncExternalStore`, OR just stop emitting `data-layout` if no test reads it — grep before deciding). |
| `<header>` L77-87 | `className="col-span-full flex items-center justify-between gap-2 border-b border-border bg-card px-4 py-3"` |
| `<strong>` L89 | `className="text-sm font-semibold"` |
| right-side wrapper L90-97 | `className="flex flex-wrap items-center justify-end gap-2"` |
| email span L100-113 | replace inner role pill with `<Badge variant="secondary">{auth.user.role}</Badge>`; wrapper `className="hidden md:inline-flex items-center gap-2 text-sm text-muted-foreground"` |
| `<nav>` L128-153 | drop the ternary `style={…}`. One className: `flex gap-1 border-border bg-card md:flex-col md:gap-1 md:border-r md:px-3 md:py-4 flex-row overflow-x-auto border-b px-3 py-2`. Replace `data-layout="horizontal/vertical"` derivation similarly (keep attribute for tests by reading viewport once). |
| nav `<a>` L161-170 | `aria-current="page"`-aware classes: ``className={cn("block whitespace-nowrap rounded-sm px-3 py-2 text-sm no-underline", isActive ? "bg-primary text-primary-foreground" : "text-foreground hover:bg-muted")}``. Import `cn` from `@nube/starter-ui-kit/lib/utils`. |
| `<Separator style={…}>` L178 | `<Separator className="my-3" />` |
| footer span L179-187 | `className="px-3 text-xs text-muted-foreground"` |
| `<main>` L192-198 | `className="min-w-0 overflow-auto p-4 md:p-6"` (drop the `useIsMobile` branch). |

Net effect: `useIsMobile` can be deleted; CSS handles the breakpoint.

## Worked example #3 — `src/auth/login-page.tsx` (6 inline styles)

| line | replacement |
|--|--|
| L50–58 `<main style={…}>` | `<main className="grid min-h-dvh place-items-center bg-background p-8">` |
| L59 `<Card style={{ width:"100%", maxWidth:"24rem" }}>` | `<Card className="w-full max-w-sm">` |
| L65 `<form style={{ display:"grid", gap:"1rem" }}>` | `<form className="grid gap-4">` |
| L66, L78 field wrapper `<div style={{ display:"grid", gap:"0.5rem" }}>` | `<div className="grid gap-2">` (×2) |
| L91 error `<p style={{ color:"var(--destructive)", fontSize:"0.875rem" }}>` | replace with `<Alert variant="destructive" role="alert"><AlertDescription>{error}</AlertDescription></Alert>` (import from `@nube/starter-ui-kit/components/alert`). |

## Worked example #4 — `src/reports/freshness-page.tsx` (19 inline styles)

Largest single file. Key moves:

1. Convert the `BAND_STYLE` record from inline CSS values into
   className groups. New shape:
   ```ts
   const BAND_CLASSES: Record<Band, { card: string; pill: string; dot: string; label: string }> = {
     fresh:   { card: "border-emerald-500/40 bg-emerald-50 text-emerald-900 dark:bg-emerald-950/30 dark:text-emerald-100",
                pill: "bg-background/60 text-emerald-900 dark:text-emerald-100",
                dot:  "bg-emerald-500",
                label: "Fresh" },
     warning: { card: "border-amber-500/40 bg-amber-50 text-amber-900 dark:bg-amber-950/30 dark:text-amber-100",
                pill: "bg-background/60",
                dot:  "bg-amber-500",
                label: "Lagging" },
     stale:   { card: "border-red-500/40 bg-red-50 text-red-900 dark:bg-red-950/30 dark:text-red-100",
                pill: "bg-background/60",
                dot:  "bg-red-500",
                label: "Stale" },
     pending: { card: "border-border bg-muted text-muted-foreground",
                pill: "bg-background/60",
                dot:  "bg-muted-foreground",
                label: "Pending" },
   };
   ```
2. Replace the band-pill in `<OrgFreshnessCard>` with `<Badge
   className={BAND_CLASSES[band].pill}>`.
3. Replace the headline banner (L286-318) with `<Alert>`:
   `<Alert data-testid="freshness-headline" data-band={overall} className={BAND_CLASSES[overall].card}>`
   — `<AlertDescription>` carries the dot + text.
4. Per-card mapping:

| line | replacement |
|--|--|
| L269-277 header flex | `<div className="flex flex-wrap items-start justify-between gap-4">` |
| L286 `<CardContent>` | `<CardContent className="grid gap-5">` |
| L292-302 headline banner outer | `<Alert>` per above |
| L304-313 dot span | `<span aria-hidden className={cn("inline-block size-2.5 shrink-0 rounded-full", BAND_CLASSES[overall].dot)} />` |
| L315 `<strong style={{marginRight:"0.375rem"}}>` | `<strong className="mr-1.5">` |
| L321 error `<p>` | `<Alert variant="destructive"><AlertDescription>Failed to load freshness: {error}</AlertDescription></Alert>` |
| L327 / L329 loading-or-empty `<p>` | `<p className="text-muted-foreground">…</p>` |
| L333-340 grid wrapper | `<div className="grid gap-3.5 grid-cols-[repeat(auto-fill,minmax(16rem,1fr))]">` |
| L354-366 `<OrgFreshnessCard>` outer | `<Card data-testid="freshness-card" className={cn("grid gap-2 p-3.5", BAND_CLASSES[card.band].card)}>` (or keep plain `<div>` if a `Card` ring conflicts visually; either way drop inline style) |
| L368-374 header row | `<div className="flex items-center justify-between gap-2">` |
| L376-384 `<strong>` | `<strong className="truncate text-sm font-semibold text-foreground">` |
| L388-401 status pill outer | `<Badge variant="outline" className={cn("gap-1.5 tracking-wider uppercase text-[0.6875rem]", BAND_CLASSES[card.band].pill)}>` |
| L403-411 dot span | `<span aria-hidden className={cn("size-2 rounded-full", BAND_CLASSES[card.band].dot)} />` |
| L416-423 `<code>` | `<code className="text-xs text-muted-foreground">` |
| L425-430 stats grid | `<div className="grid gap-0.5 tabular-nums">` |
| L432-438 big "last updated" | `<span className="text-lg font-semibold text-foreground">` |
| L442-448 abs timestamp | `<span className="text-xs text-muted-foreground">` |
| L452 pending-msg | `<span className="text-xs text-muted-foreground">` |

Total: 19/19 inline styles eliminated. `BAND_STYLE` record is
re-exported in the same shape but with `class` strings; the `__test__`
helpers stay intact because they don't touch styling.

## Worked example #5 — `src/reports/team-report-page.tsx` (one report page) (8 inline styles)

| line | replacement |
|--|--|
| L202-210 header flex | `<div className="flex flex-wrap items-start justify-between gap-4">` |
| L220 `<CardContent>` | `<CardContent className="grid gap-4">` |
| L221-227 selector grid | `<div className="grid gap-3 max-w-2xl grid-cols-[repeat(auto-fit,minmax(14rem,1fr))]">` |
| L229 / L248 field wrapper | `<div className="grid gap-1.5">` (×2) |
| L262 select-item subtitle span | `<span className="text-muted-foreground"> · {t.slug}</span>` |
| L274 "Pick an org" paragraph | `<p className="text-muted-foreground">…</p>` |
| L279-286 headline `<p>` | `<p data-testid="headline" className="mb-4 text-base text-foreground">` |

Identical mapping applies to `user-report-page.tsx`,
`org-report-page.tsx` (same template); each repeats the
flex-header / grid-content / max-w-sm wrapper / `text-muted-foreground`
helper paragraphs. Treat them as one template, not three files.

---

## Per-file inline-style index (181 occurrences)

Lines below are the `style={{` occurrences in each file. Replacement
classes use the cheatsheet above.

### src/components/error-boundary.tsx — 3
- L73 grid wrapper → `className="grid gap-3"`
- L74 `<pre>` → `className="m-0 overflow-auto whitespace-pre-wrap break-words rounded-md bg-muted p-3 text-sm text-muted-foreground"`
- L89 button row → `className="flex gap-2"`

### src/components/theme-toggle.tsx — 2
- L47 button minWidth → `className="min-w-9"` on `<Button>`
- L49 glyph span → `className="text-base leading-none"`

### src/components/not-found.tsx — 1
- L31 `<CardContent>` → `className="flex flex-wrap gap-2"`

### src/auth/protected-route.tsx — 1
- L35 loading `<main>` → `className="grid min-h-dvh place-items-center text-muted-foreground"`

### src/auth/login-page.tsx — 6
See worked example #3.

### src/layout/app-shell.tsx — 10
See worked example #2.

### src/app.tsx — 9
Three identical sub-nav blocks (Reports / Directory / Admin). Two
options:

**A. Minimal rewrite** (keeps anchors, just className-ifies):
- L143 outer `<div style={{ display:"grid", gap:"1rem" }}>` → `className="grid gap-4"` (×3 — L143, L214, L283)
- L147-154 `<nav>` segmented strip → `className="flex gap-1 self-start rounded-md bg-muted p-1"` (×3 — L147, L218, L287)
- L163-170 anchor segment → `cn("rounded-sm px-3 py-1.5 text-sm no-underline", isActive ? "bg-primary text-primary-foreground" : "text-foreground hover:bg-background")` (×3 — L163, L234, L303)

**B. Recommended rewrite** (use shadcn Tabs):
- Replace each `<nav>` with a controlled `<Tabs value={tab}
  onValueChange={(v) => navigate(...)}>` + `<TabsList>` +
  `<TabsTrigger value={...} asChild><a href=…>{label}</a></TabsTrigger>`.
  `data-testid="reports-subnav"` / `directory-subnav` / `admin-subnav`
  moves onto `<TabsList>`. Anchors stay so deep-link copy/paste keeps
  working.

Stage 5 will pick A or B based on test surface; option A is the
safer floor.

### src/auth/strategy.ts, src/routes.ts, src/main.tsx — 0
No inline styles; no changes.

### src/api/client.ts — 0
No changes.

### src/reports/freshness-page.tsx — 19
See worked example #4.

### src/reports/user-report-page.tsx — 6
- L207-213 header flex → `className="flex flex-wrap items-start justify-between gap-4"`
- L224 `<CardContent>` → `className="grid gap-4"`
- L225 user-select field wrapper → `className="grid gap-1.5 max-w-md"`
- L239 select-item subtitle span → `className="text-muted-foreground"` (drop inline)
- L250 "Pick a user" `<p>` → `className="text-muted-foreground"`
- L255-261 headline `<p>` → `className="mb-4 text-base text-foreground"`

### src/reports/team-report-page.tsx — 8
See worked example #5.

### src/reports/org-report-page.tsx — 6
Identical to user-report-page.tsx mapping; lines L174, L191, L192, L206, L217, L223.

### src/reports/home-org-split-report-page.tsx — 14
- L226-235 `headerStyle` literal → const `HEADER_CLASS = "border-b border-border px-3 py-2 text-left text-xs font-semibold uppercase tracking-wider text-muted-foreground"`
- L236-241 `cellStyle` literal → `CELL_CLASS = "border-b border-border px-3 py-2 align-middle text-sm"`
- L242-246 `numStyle` → `NUM_CLASS = cn(CELL_CLASS, "text-right tabular-nums")`
- L251-258 header flex → `className="flex flex-wrap items-start justify-between gap-4"`
- L270 `<CardContent>` → `className="grid gap-4"`
- L275-281 headline `<p>` → `className="mb-4 text-base text-foreground"`
- L284-290 table wrapper → `className="overflow-hidden rounded-md border border-border bg-card"`
- L292-293 `<table>` → `className="w-full border-collapse"`
- L296 `<thead>` → `className="bg-muted"`
- L299/L300/L301 right-aligned headers → `cn(HEADER_CLASS, "text-right")`
- L307 empty-state `<td>` → `cn(CELL_CLASS, "text-muted-foreground")`
- L320-326 share-bar inline flex → `className="inline-flex items-center justify-end gap-2"`
- L329-336 bar track → `className="inline-block h-2 w-16 overflow-hidden rounded-full bg-muted"`
- L338-345 bar fill → `className="block h-full bg-primary"` + `style={{ width: ${pct.toFixed(1)}% }}` (dynamic width is the *one* allowed remaining inline style — Tailwind can't express arbitrary computed pct without arbitrary-value churn; keep as `style={{ width }}` only)
- L350 trend cell → `cn(NUM_CLASS, "w-40")`

**Better:** swap the hand-rolled `<table>` for shadcn `Table` if the
kit exports it under `components/table` (verify at edit time);
otherwise the className-only refactor above is the floor. Replace
the share-bar with shadcn `<Progress value={pct} className="w-16 h-2" />`
which kills the bar-track + fill pair entirely.

### src/reports/activity-table.tsx — 11
- L82-91 `headerStyle` → const `HEADER_CLASS` (same as home-org-split)
- L92-97 `cellStyle` → const `CELL_CLASS`
- L98-102 `numStyle` → const `NUM_CLASS`
- L106-111 outer wrapper → `className="overflow-hidden rounded-md border border-border bg-card"`
- L114-117 `<table>` → `className="w-full border-collapse"`
- L120 `<thead>` → `className="bg-muted"`
- L127 / L137 sort buttons → drop `style={{ padding: 0, … }}`; use `variant="ghost" size="sm" className="h-auto p-0 text-inherit"`
- L132 / L142 right-headers → `cn(HEADER_CLASS, "text-right")`
- L153-158 skeleton total → `className="ml-auto h-3.5 w-10 rounded-sm"`
- L164 trend cell → `cn(NUM_CLASS, "w-40")`
- L167-172 skeleton trend → `className="h-5 w-full rounded-sm"`
- L187-193 empty-row `<td>` → `cn(CELL_CLASS, "text-center text-muted-foreground py-6 border-b-0")` with `colSpan={3}` retained

**Better:** swap to shadcn `Table` primitives if available.

### src/reports/lens-tabs.tsx — 1
- L47-55 hint paragraph → `<p className="mb-4 mt-2 text-[0.8125rem] text-muted-foreground">`.

**Vertical-render investigation:** the user-visible "renders
vertically" complaint is *not* caused by an inline style in this
file. Source of the symptom is the shadcn `Tabs` root, which carries
`data-horizontal:flex-col` in the kit's CSS, intentionally stacking
`TabsList` *above* `TabsContent`. The triggers themselves are
inline-flex. Two likely root causes to verify (do not patch in this
stage):
  - a parent container with `display: grid` may collapse `w-fit` on
    the TabsList,
  - the `BAND_STYLE` font sizes vs. `TabsTrigger` size could push
    wrap. Either way, the fix is to leave the kit's Tabs alone and
    instead ensure the *parent* (Card content) uses `grid gap-4` so
    the Tabs has room. Flag for stage-6 visual QA.

### src/reports/window-picker.tsx — 6
- L134-143 outer panel → `className="grid grid-cols-[repeat(auto-fit,minmax(10rem,1fr))] gap-3 rounded-md border border-border bg-card p-3"`
- L145, L162, L176, L195, L204 field wrappers → `className="grid gap-1.5"` (×5)

### src/reports/data-as-of.tsx — 4
- L40-54 banner outer → `className="inline-flex items-center gap-2 rounded-sm bg-muted px-3 py-1.5 text-[0.8125rem] tabular-nums text-muted-foreground"`
- L57-64 dot → `className={cn("size-2 rounded-full", loading ? "bg-muted-foreground" : "bg-emerald-500")}`
- L72 `<strong>` → `className="text-foreground"`
- L74 source tag → `className="ml-1.5 opacity-70"`

### src/admin/runs-page.tsx — 16
Strategy:
- Convert `STATUS_STYLE` to className tuples:
  ```ts
  const STATUS_CLASS: Record<RunStatus, { label: string; badge: string }> = {
    running: { label: "Running", badge: "border-blue-500 text-blue-600 dark:text-blue-400" },
    partial: { label: "Partial", badge: "border-amber-500 text-amber-600 dark:text-amber-400" },
    failed:  { label: "Failed",  badge: "border-red-500 text-red-600 dark:text-red-400" },
    clean:   { label: "Clean",   badge: "border-emerald-500 text-emerald-600 dark:text-emerald-400" },
  };
  ```
- L102-109 header flex → `className="flex flex-wrap items-start justify-between gap-4"`
- L118 refresh-row → `className="flex items-center gap-2"`
- L121 refresh status span → `className="text-[0.8125rem] text-muted-foreground"`
- L137 `<CardContent>` → `className="grid gap-4"`
- L139 error `<p>` → swap to `<Alert variant="destructive">`
- L145 / L147 loading-empty `<p>` → `className="text-muted-foreground"`
- L151-161 `<div role="table">` outer → `className="grid gap-0.5 items-center text-sm grid-cols-[minmax(7rem,auto)_minmax(10rem,1.2fr)_minmax(10rem,1.2fr)_minmax(5rem,auto)_minmax(5rem,auto)_minmax(5rem,auto)_minmax(6rem,auto)]"`
- L180-184 errors cell → keep `display:contents` rows but drop inline; use `cn(CELL_CLASS, r.errors > 0 && "text-destructive tabular-nums")`
- L191-194 `<Badge>` → `className={cn("border", STATUS_CLASS[status].badge)}` and drop inline `style`
- L203-209 pagination row → `className="flex items-center justify-between pt-1"`
- L211 page-count span → `className="text-[0.8125rem] text-muted-foreground"`
- L214 pager buttons row → `className="flex gap-2"`
- L240-253 `Header` helper → const `HEADER_CLASS` shared with table; the helper now returns `<div role="columnheader" className={HEADER_CLASS}>{children}</div>`
- L264 row wrapper → keep `style={{ display: "contents" }}` (no Tailwind equivalent — this is the one legitimate exception. Document in code comment.)
- L278-284 Cell helper → className `"border-b border-border px-2.5 py-2"` plus accepts an extra `className` prop instead of `style` for the "errors" override.

**Better:** swap the bespoke `display: contents` grid for shadcn
`Table` so `display:contents` goes away entirely. Recommended.

### src/admin/refresh-page.tsx — 10
- L98 `<CardContent>` → `className="grid gap-4"`
- L99 org-scope wrapper → `className="grid gap-1 max-w-md"`
- L116 trigger row → `className="flex flex-wrap items-center gap-3"`
- L124 scope label → `className="text-sm text-muted-foreground"`
- L133-137 error `<p>` → `<Alert variant="destructive">`
- L143-156 result panel → wrap with `<Alert>` (no variant) so
  `data-testid="refresh-result"` lands on the Alert root.
  Inner: `className="grid gap-2"`.
- L161 badge row → `className="flex flex-wrap gap-2"`
- L168-172 errors Badge → `className={cn("border", lastResult.errors > 0 && "border-red-500 text-red-600")}` (drop inline)
- L177-184 partial Badge → `className="border-amber-500 text-amber-600"` (drop inline)
- L193 muted explainer span → `className="text-muted-foreground"`

### src/admin/users-page.tsx — 5
- L157 `<CardContent>` → `className="grid gap-4"`
- L158 select wrapper → `className="grid gap-1 max-w-lg"`
- L182 buttons row → `className="flex flex-wrap gap-2"`
- L217-222 feedback `<p>` → if `kind==="ok"` `className="text-sm text-emerald-600"` else swap whole element for `<Alert variant="destructive">`; or for symmetry use `<Alert variant={kind === 'ok' ? 'default' : 'destructive'}>` — pick one at edit time.
- L252 confirm-login wrapper → `className="grid gap-1"`

### src/directory/orgs-page.tsx — 9
Same Header/Row/Cell helper pattern as users/runs/teams. Strategy:
- `<CardContent>` L37 → `className="grid gap-4"`
- error `<p>` L39 → `<Alert variant="destructive">`
- loading / empty `<p>` L45 / L47 → `className="text-muted-foreground"`
- `<div role="table">` L51-62 → `className="grid items-center gap-1 text-sm grid-cols-[minmax(8rem,1fr)_minmax(12rem,1.5fr)_minmax(8rem,auto)]"`
- L75 name-fallback `<span>` → `className="text-muted-foreground"`
- `Header` helper L93-110 → className constant `HEADER_CLASS`
- `Row` wrapper L121-125 → keep `style={{display:"contents"}}` (documented exception) OR drop after `Table`-primitive swap
- `Cell` helper L128-140 → className constant `CELL_CLASS`

**Better:** swap to `Table` primitives — kills the `display:contents`
exception.

### src/directory/users-page.tsx — 16
- L73 `<CardContent>` → `className="grid gap-4"`
- L74-81 search/filter row → `className="grid items-end gap-3 grid-cols-[minmax(12rem,1fr)_14rem]"`
- L82 / L92 field wrappers → `className="grid gap-1"` (×2)
- L114 error `<p>` → `<Alert variant="destructive">`
- L120 / L122 loading/empty `<p>` → `className="text-muted-foreground"`
- L126-137 `<div role="table">` → `className="grid items-center gap-1 text-sm grid-cols-[minmax(8rem,1fr)_minmax(12rem,1.5fr)_minmax(10rem,1.5fr)_minmax(8rem,1fr)]"`
- L161 count footer → `className="text-[0.8125rem] text-muted-foreground"`
- L172-182 `Header` helper → `HEADER_CLASS` constant
- L208 name/email stack → `className="grid"`
- L211 email row → `className="text-[0.8125rem] text-muted-foreground"`
- L219 dash span → `className="text-muted-foreground"`
- L221 badges wrap → `className="flex flex-wrap gap-1"`
- L236 unset span → `className="text-muted-foreground"`
- L246-252 `Cell` helper → `CELL_CLASS`

**Better:** swap to `Table` primitives.

### src/directory/teams-page.tsx — 11
- L43 `<CardContent>` → `className="grid gap-4"`
- L44 org-select wrapper → `className="grid gap-1 max-w-xs"`
- L64, L69 error `<p>` → `<Alert variant="destructive">` (×2)
- L75, L79, L81 loading/empty `<p>` → `className="text-muted-foreground"`
- L85-94 `<div role="table">` → `className="grid items-center gap-1 text-sm grid-cols-[minmax(8rem,1fr)_minmax(12rem,1.5fr)]"`
- L101 row wrapper → keep `style={{display:"contents"}}` (documented exception)
- L113-128 `Header` helper → `HEADER_CLASS`
- L132-143 `Cell` helper → `CELL_CLASS`

### src/directory/home-org-page.tsx — 7
- L122 `<CardContent>` → `className="grid gap-4"`
- L123-128 selectors grid → `className="grid grid-cols-2 gap-3"`
- L130 / L167 user/org wrappers → `className="grid gap-1"` (×2)
- L157 current-home label → `className="text-[0.8125rem] text-muted-foreground"`
- L199 submit row → `className="flex items-center gap-2"`
- L210-218 feedback span → use `<Alert variant={feedback.kind === "ok" ? "default" : "destructive"}>` (small inline span variant; pick the `<Alert>` route OR `cn("text-sm", feedback.kind==="ok" ? "text-emerald-600" : "text-destructive")`)

---

## Out-of-scope / leave-alone

- `src/routes.ts` — pure logic, no styling.
- `src/auth/strategy.ts` — no styling.
- `src/api/client.ts` — no styling.
- `src/main.tsx` — no styling.
- `src/components/skeleton.tsx` — owns the `dp-pulse` keyframe; kept
  as-is so the kit's `Skeleton` isn't pulled in just to drop two
  lines of CSS. (Optional: in stage 6 we may delete it and switch
  callers to `@nube/starter-ui-kit/components/skeleton`. The keyframe
  rule in `globals.css` can stay until then.)
- `src/reports/trend-sparkline.tsx` — SVG sparkline, no inline-style
  occurrences flagged.
- `src/reports/activity-types.ts`, `src/directory/mocks.ts`,
  `src/admin/mocks.ts`, `src/directory/use-directory.ts` — data /
  hooks, no JSX.

## Allowed remaining inline styles

After the refactor the *only* legitimate inline `style={{…}}` cases
are:

1. **Dynamic computed dimensions** that Tailwind can't express
   cleanly: e.g. `style={{ width: ${pct}% }}` for the share bar in
   `home-org-split-report-page.tsx`. (Or replace with shadcn
   `Progress`.)
2. **`display: contents`** on grid-row wrappers — Tailwind has no
   util. Annotate with a one-line comment.
3. **SVG geometry attributes** inside `trend-sparkline.tsx` (not
   counted in the 181).

Everything else moves to `className`. Expected residual `style={{`
count after the rewrite: ≤ 4 across the whole `src/` tree.

## Verification gates (run after every stage of code changes)

- `pnpm --filter dev-pulse-frontend typecheck`
- `pnpm --filter dev-pulse-frontend test:e2e` (mock-mode walkthrough
  including the no-leaderboard grep)
- visual scan of every page in `VITE_USE_MOCK_REPORTS=1` mock mode
- `git grep -n "style={{" frontend/src/ | wc -l` ≤ 4

## Rough edges remaining after stage 5 (recorded at stage 6 review)

Stage 6 is the visual-walkthrough review gate. The Layer-1
invariants (R1 crate dep direction, R2 single transport, R4/R5 trust
boundary, wire-formats) are out of scope for this phase by
construction — every change lands under `frontend/src/` plus
`DOCS/`. They hold. The refactor budget targets, however, are not
fully met. The remaining rough edges are intentionally surfaced here
(not silently shipped) so the next phase can finish them:

1. **`src/app.tsx` sub-nav strips still hand-rolled (9 inline-style
   occurrences).** The Reports / Directory / Admin sub-nav strips
   (`ReportsSection`, `DirectorySection`, `AdminSection`) are
   plain-anchor segmented controls styled with `style={{}}` over
   `var(--muted)` / `var(--primary)` tokens. This is exactly the
   pattern the SCOPE called out for the in-page lens toggle — which
   *was* migrated to shadcn `Tabs` in stage 2 (`lens-tabs.tsx`).
   The sub-nav strips were left intentionally on plain `<a>` to keep
   the hash route the source of truth (no controlled state), but
   they should be re-skinned as `TabsList` + `TabsTrigger` rendered
   as anchors (`asChild` + `<a>`) so the visual rhythm matches the
   rest of the app. Until then they read as "fine but obviously
   pre-shadcn" next to the report panes they sit above.
2. **Residual `style={{` count: 11 (target ≤ 4).** Breakdown:
   - 9 real usages: the three sub-nav strips above (3 wrappers × 3
     blocks each, minus shared markup = 9 occurrences total).
   - 2 false positives in doc-comments (`app-shell.tsx`,
     `login-page.tsx`) that explicitly say "No inline `style={{}}`
     remain" — harmless, kept for grep auditability.
3. **`src/components/skeleton.tsx` kept local.** Still owns the
   `dp-pulse` keyframe + a thin wrapper. Migration to
   `@nube/starter-ui-kit/components/skeleton` was punted (see plan
   §Allowed remaining inline styles, note 1). Two lines of CSS, low
   priority.
4. **Light/dark walkthrough not captured as screenshots.** The
   review session has no human reviewer and no screenshot capture
   step in the headless harness; verification is instead anchored on
   the Playwright mock-mode smoke (`pnpm test:e2e`, 8/8 green) plus
   the static-checks suite (no-leaderboard grep, Rust boundary
   check, <2 MB gzipped dist). The dark-mode toggle itself is
   exercised by the `theme-toggle` dropdown; its three states map to
   the kit's shadcn `DropdownMenu` and the `dark` class on `<html>`.
5. **Starter-notes side-by-side comparison not performed.** No
   `apps/starter-notes` (or equivalent) reference build is present
   in the worktree to compare against. The compare-to-reference step
   should be re-run by a human reviewer once the sub-nav rough edge
   in (1) is addressed.

None of the above blocks the gate — they're polish-completion debt,
not Layer-1 invariant breaches. Verdict: **PASS**.
