# Scope — phase-7-frontend-polish

## Goal

Refactor the dev-pulse frontend from "functional but visually
embarrassing" to "a proper shadcn + Tailwind v4 app a designer would
not be ashamed of". Zero behaviour change. Zero new features. Zero
backend changes. Replace 181 inline `style={{...}}` occurrences with
Tailwind utility classes and the shadcn primitives that already ship
in `@nube/starter-ui-kit/components/*` but were almost completely
ignored by Phase 7.

## In scope

- Rewrite `src/layout/app-shell.tsx` using shadcn `Sheet`, `Separator`, `Button`, `DropdownMenu`, `Breadcrumb`.
- Rewrite `src/auth/login-page.tsx` as a centred shadcn `Card` with `Label` + `Input` + `Button`.
- Rewrite all five report pages (user/team/org/home-org-split/freshness) using shadcn `Card`, `Tabs` (for the three-lens toggle — currently vertical because it's hand-rolled), `Select` (for window + entity pickers), `Alert` (for "Data as of" banner), `Badge` (for delta indicators), `Table` (for results), and proper typography utilities.
- Rewrite directory pages using shadcn `Card`, `Input`/`InputGroup`, `Table`, `Dialog` (for home-org assignment).
- Rewrite admin pages using shadcn `Card`, `Table`, `Badge` (status), `Alert` (loading/success/error), `AlertDialog` (destructive confirm), `Progress` (export).
- Polish: shadcn `Empty`, `Skeleton`, `Alert` (destructive) for empty/loading/error states. 404 page uses centred Card.
- Drop `body { font-family }` from `globals.css` (shadcn handles it via `--font-sans`).
- Final inline-style survivors < 15, each with a justifying comment (e.g. dynamic colour for a sparkline data point).

## Out of scope

- Any new features. This is a pure refactor.
- Any backend / Rust changes. Phase 4 surface is the contract.
- Editing `crates/starter-*` or `packages/`. shadcn primitives are consumed as-is from `@nube/starter-ui-kit/components/*`.
- Visual-design overhauls beyond using the existing shadcn tokens (no custom colour palette, no custom icon set).
- Replacing react-query, react-router conventions (hash-routing), or any architectural choice.
- Adding tests beyond one visual-regression smoke (shadcn Card + Tabs presence per report page).
- Touching the OpenAPI snapshot or the typed API client (`src/api/`).

## Hard rules

- **No leaderboard, no single-score affordance** (SCOPE §4). Unchanged from Phase 7.
- **Three-lens toggle on every report** — must remain horizontal shadcn `Tabs` post-refactor (the current vertical rendering is the most visible bug).
- **"Data as of" banner on every report** — promoted to shadcn `Alert`, never hidden.
- **Headline + table + trend** — every report still follows this shape (§11.5). Card structure makes the triptych more obvious, not less.
- **`scripts/check-boundaries.sh` still green** — only `frontend/` touched.
- **`grep -rn "style={{" frontend/src/ | wc -l` reports < 15** — measured at the end of the run. Anything that survives must have a justifying comment on the line above.
- **All Phase 7 Playwright smokes still pass** — login, lens toggle, window change, admin refresh, no-leaderboard grep, dist < 2MB gzipped.

## Constraints

- Use shadcn primitives via the package path `@nube/starter-ui-kit/components/<name>` (matches the existing imports in `src/layout/app-shell.tsx`).
- Tailwind utilities over inline styles. Where a dynamic value is unavoidable (sparkline colour, computed width) use CSS custom properties via `style={{ "--x": value }}` and reference them in a Tailwind arbitrary value.
- Keep mock-mode (`VITE_USE_MOCK_REPORTS=1` + the `mockAuthPlugin` in `vite.config.ts`) working — that is how the user demos the app without a backend.
- pnpm only.

## Smoke tests (merge gate)

- `pnpm typecheck` clean.
- `pnpm build` produces dist/ < 2MB gzipped.
- `pnpm test:e2e` all green, including:
  - login + nav walkthrough
  - lens toggle (User report) cycles through SingleOrg / AllOrgsCombined / PerOrgSplit horizontally
  - window picker updates the query
  - admin refresh trigger fires
  - no-leaderboard grep returns zero hits
- New visual-regression smoke: every report page renders at least one shadcn `<Card>` and at least one shadcn `<Tabs>` (DOM query against `[data-slot="card"]` / `[data-slot="tabs"]`).
- `grep -rn "style={{" frontend/src/ | wc -l` < 15.
- `scripts/check-boundaries.sh` still green.

## Open questions (resolve in stage 1)

1. **Tabs vs ToggleGroup for the three-lens toggle.** Bias: `Tabs` — it's the most idiomatic shadcn shape for switching between equivalent views.
2. **Custom date range — `Popover` + Calendar, or a flat `Input[type=date]` pair.** Bias: `Popover` + Calendar (richer, matches shadcn convention).
3. **User / team / org pickers — `Select` for all, or `Command` (search) once a list exceeds 20.** Bias: hybrid — `Select` for small lists, `Command` (combobox pattern) for users (likely large).
