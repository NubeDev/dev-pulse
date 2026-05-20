# Scope — phase-7-frontend-apple

## Goal

Make the dev-pulse frontend look like the codeless-ui reference app
at `/home/user/code/rust/codeless-workspace/codeless/ui/codeless-ui`.
Modern shadcn/ui + Apple-grade rhythm: same oklch palette, same Inter
Variable font, same radius ladder, same Card/Sheet density, same
sidebar treatment, same Table styling, same generous whitespace.

This is a pure visual overhaul. Zero behaviour change. Zero new
features. Zero backend changes.

## In scope

- Replace `frontend/src/globals.css` with the codeless-ui token
  system: lift `:root` + `.dark` oklch blocks verbatim, the `@theme
  inline` block, the radius ladder, the base layer body styles. Add
  `@fontsource-variable/inter` as a dep and import it.
- Rewrite `src/layout/app-shell.tsx` to a proper shadcn sidebar
  layout (sticky header h-14 with backdrop blur, 15rem sidebar with
  rounded NavLink items, breadcrumb, dropdown user menu, dark-mode
  toggle as DropdownMenu).
- Rewrite all five report pages around the same skeleton: heading
  lockup → filter Card → freshness Alert → results Card with shadcn
  Tabs (horizontal segmented, NOT capsule pills) and shadcn Table.
- Rewrite directory + admin pages with the same heading-lockup +
  Card + Table pattern. Search uses InputGroup. Mutations use
  Dialog / AlertDialog / Alert / Progress as appropriate.
- Login page becomes a centred shadcn Card.
- 404 + error boundary + theme toggle polished to match.
- Loading uses Skeleton shapes matching the final layout. Empty
  states use shadcn Empty.

## Out of scope

- Any new features. Pure visual refactor.
- Any backend / Rust changes.
- Editing `crates/starter-*` or `packages/`. shadcn primitives stay
  consumed from `@nube/starter-ui-kit/components/*`.
- Touching the OpenAPI snapshot or `src/api/`.
- Replacing react-query, react-router conventions, query-key shapes.

## Hard rules

- **Visual family resemblance to codeless-ui** — same palette,
  same font, same radii, same chrome density. Side-by-side at the
  end of stage 6 must read as same-team.
- **No leaderboard, no single-score affordance** (SCOPE §4).
- **Three-lens toggle is shadcn Tabs default segmented style** —
  horizontal, NOT the current vertical capsule pill set.
- **`grep -rn "style={{" frontend/src/ | wc -l` reports < 10**.
  Survivors carry a justifying comment.
- **All Phase 7 Playwright smokes still pass** + new
  visual-regression smoke (Card + Tabs presence per report, body
  font-family contains "Inter", `--radius` is `0.625rem`).
- **`scripts/check-boundaries.sh` green** — only `frontend/`
  touched.
- **No `--force`, no `--no-verify`**.
- **mock-mode still works** (VITE_USE_MOCK_REPORTS=1 +
  mockAuthPlugin in vite.config.ts).

## Reference paths

- Tokens + base CSS: `/home/user/code/rust/codeless-workspace/codeless/ui/codeless-ui/src/styles/globals.css`
- App shell + sidebar pattern: `/home/user/code/rust/codeless-workspace/codeless/ui/codeless-ui/src/app/App.tsx`
- Dashboard / Card / Table density: `/home/user/code/rust/codeless-workspace/codeless/ui/codeless-ui/src/modules/jobs/JobsDashboard.tsx`, `JobRow.tsx`, `JobDetail.tsx`
- Header chrome: `/home/user/code/rust/codeless-workspace/codeless/ui/codeless-ui/src/modules/header/Header.tsx`
- Tabs treatment: `/home/user/code/rust/codeless-workspace/codeless/ui/codeless-ui/src/modules/jobs/JobTabs.tsx`

## Smoke tests (merge gate)

- `pnpm typecheck` clean.
- `pnpm build` produces dist < 2MB gzipped.
- `pnpm test:e2e` all green (login, lens toggle horizontal cycle,
  window change, admin refresh, no-leaderboard grep).
- Visual-regression smoke: every report page has at least one
  `[data-slot="card"]` and one `[data-slot="tabs"]`, computed body
  font-family includes "Inter", `--radius` is `0.625rem`.
- `grep -rn "style={{" frontend/src/ | wc -l` < 10.
- `scripts/check-boundaries.sh` green.

## Open questions (resolve in stage 1)

1. **Sidebar component — shadcn `Sidebar` block, or a hand-rolled
   `<nav>` styled to match codeless-ui?** Bias: hand-rolled to
   match codeless-ui exactly (the reference uses a custom layout,
   not the shadcn Sidebar block).
2. **Drop the existing `font-family` body rule entirely, or keep
   a fallback chain?** Bias: drop it — `@theme inline` exposes
   `--font-sans` and Tailwind's `font-sans` does the rest.
3. **Header backdrop blur — opt-in via Tailwind v4 or hard-coded
   in CSS?** Bias: Tailwind utility (`bg-background/80
   backdrop-blur-xl`) for consistency with codeless-ui.
