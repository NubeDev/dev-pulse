# Workflow — phase-7-frontend-apple

## Sequencing

- Stage 1 sets the foundation (tokens + font). Without this the
  rest of the work has nothing to reference.
- Stage 2 rewrites the shell (highest-visibility surface, sets
  the chrome rhythm for every page).
- Stage 3 rewrites the reports (the user-visible bulk).
- Stage 4 rewrites directory + admin.
- Stage 5 polishes the loose ends + audits the inline-style count.
- Stage 6 is the REVIEW gate — side-by-side screenshots vs
  codeless-ui, light + dark.
- Stage 7 is verification (tests + build + boundary check).

## Per-stage discipline

- Before any code change, re-read the relevant codeless-ui
  reference file from the paths in SCOPE.md. Match the patterns,
  do not invent.
- Re-read the shadcn component source in
  `/home/user/code/rust/starter/packages/starter-ui-kit/src/components/ui/`
  for the components you're about to use (the `data-slot`
  attributes and prop shapes are stable).
- Touch only files the stage names. No drive-by refactors.
- After every stage:
  - `pnpm typecheck` clean.
  - `pnpm build` succeeds.
  - `pnpm test:e2e` all green.
  - `scripts/check-boundaries.sh` green.
  - Spot-check: `pnpm dev` with `VITE_USE_MOCK_REPORTS=1`,
    navigate to touched pages, confirm visual progress.

## REVIEW gate

- **After stage 6** — capture screenshots of every route in light
  + dark, drop them in `frontend/REVIEW-screenshots/` (gitignored),
  compare to codeless-ui equivalents, document any gap in
  handover.md before the verification stage.

## Anti-patterns

- Inventing custom palette values instead of lifting codeless-ui
  oklch tokens verbatim.
- Custom CSS classes instead of Tailwind utilities (globals.css
  should not grow beyond tokens + base layer + any unavoidable
  third-party overrides).
- Leaving `style={{...}}` survivors without a justifying comment.
- Reaching into shadcn component internals or copy-pasting their
  source into `frontend/src/`. Consume the exports.
- Rewriting business logic, query keys, route shapes, or any
  Phase 7 contract.
- Touching `src/api/`, the OpenAPI snapshot, or any backend code.
- Editing `crates/starter-*` or `packages/`. Boundary rule.

## Closing trio — last three todos of every stage

1. `checks` — `pnpm typecheck`, `pnpm build`, `pnpm test:e2e`,
   `scripts/check-boundaries.sh`. Every step must pass.
2. `docs` — update `handover.md` for the next stage.
3. `git` — stage, commit `stage N: <one-line title>`, push to
   `codeless/phase-7-frontend-apple`.

A stage isn't done until all three are green and the push succeeds.
Never `--force`, never `--no-verify`.
