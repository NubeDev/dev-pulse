# Workflow — phase-7-frontend-dashboard01

## Sequencing

- Stage 1 installs the block and parks the example route at
  `/_blocks/dashboard-01` so we can A/B against the real pages
  during migration.
- Stage 2 swaps the shell to the block's sidebar+header.
- Stage 3 migrates reports onto SectionCards + ChartArea +
  DataTable.
- Stage 4 migrates directory + admin onto DataTable +
  SectionCards.
- Stage 5 sweeps the loose ends (empty / loading / error / 404 /
  theme toggle) and removes the throwaway block demo route.
- Stage 6 is the REVIEW gate: side-by-side screenshots vs the
  shadcn dashboard-01 demo, light + dark.
- Stage 7 is verification (tests + audit + build).

## Per-stage discipline

- Before any code change, re-read the block files the stage
  touches. Match the patterns the block establishes — accept its
  API, don''t invent.
- Re-read the shadcn primitive sources in `src/components/ui/`
  before using them. Stable `data-slot` attributes and prop
  shapes.
- Touch only files the stage names. No drive-by refactors.
- After every stage:
  - `pnpm typecheck` clean.
  - `pnpm build` succeeds.
  - `pnpm test:e2e` all green.
  - `scripts/check-boundaries.sh` green.
  - Spot-check: `pnpm dev` with `VITE_USE_MOCK_REPORTS=1`, walk
    touched pages, confirm visual progress.

## REVIEW gate

- **After stage 6** — screenshots of every route, light + dark,
  in `frontend/REVIEW-screenshots/` (gitignored). Open the
  shadcn dashboard-01 demo in another browser tab and confirm
  family resemblance. Document any remaining gap in
  handover.md before verification.

## Anti-patterns

- Customising the block's components. Accept its choices; map
  content onto its shape.
- Inventing new tokens. The codeless-ui oklch block in
  `globals.css` stays as-is.
- Custom CSS classes instead of Tailwind utilities.
- Leaving `style={{...}}` survivors without a justifying
  comment.
- Rewriting business logic, query keys, or route shapes.
- Touching backend, `src/api/`, OpenAPI snapshot, or
  `crates/starter-*`.
- Adding new RPCs to feed the chart. Derive from existing data.

## Closing trio — last three todos of every stage

1. `checks` — `pnpm typecheck`, `pnpm build`, `pnpm test:e2e`,
   `scripts/check-boundaries.sh`. Every step must pass.
2. `docs` — update `handover.md` for the next stage.
3. `git` — stage, commit `stage N: <one-line title>`, push to
   `codeless/phase-7-frontend-dashboard01`.

A stage isn't done until all three are green and the push
succeeds. Never `--force`, never `--no-verify`.
