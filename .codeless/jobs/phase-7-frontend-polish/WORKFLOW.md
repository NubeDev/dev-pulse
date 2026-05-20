# Workflow — phase-7-frontend-polish

## Sequencing

- Stage 1 is the audit + plan. Write `frontend/POLISH-PLAN.md` listing every inline-style occurrence and its target. No code changes.
- Stage 2 fixes the shell + login first (highest-visibility, sets the pattern).
- Stage 3 fixes reports (the user-visible bulk).
- Stage 4 fixes directory + admin (mutations + tables).
- Stage 5 is the polish + empty/loading/error sweep + theme toggle.
- Stage 6 is the REVIEW gate — visual walkthrough light + dark.
- Stage 7 is the verification gate — tests + the inline-style count check.

## Per-stage discipline

- Before any code change in a stage:
  - Re-read `frontend/POLISH-PLAN.md` to confirm the target for the files in scope.
  - Re-read the shadcn component source in `/home/user/code/rust/starter/packages/starter-ui-kit/src/components/ui/` for the components you are about to use — the API + `data-slot` attributes are stable.
- Touch only the files the stage names. No drive-by refactors.
- After every stage:
  - `pnpm typecheck` clean.
  - `pnpm build` succeeds.
  - `pnpm test:e2e` all green.
  - `scripts/check-boundaries.sh` green.
  - Visual spot-check: open `pnpm dev` with `VITE_USE_MOCK_REPORTS=1`, navigate to the touched pages, confirm no obvious regression.

## REVIEW gates

One:
- **After stage 6** — full visual walkthrough of every page in light + dark mode before the verification + commit gate.

## Anti-patterns

- Adding a custom CSS class instead of using a Tailwind utility (the global CSS file should not grow).
- Leaving an inline `style={{...}}` without a comment explaining why. The final audit script will fail on bare survivors.
- Reaching into shadcn component internals or copy-pasting their source into `frontend/src/`. Consume the exports.
- Rewriting business logic. This job is pure presentation.
- Changing the route shape, the URL hash conventions, or the query-key namespaces. Those are Phase 7 contracts that the Playwright tests depend on.
- Touching `src/api/` or the OpenAPI snapshot. Out of scope.
- Editing `crates/starter-*` or `packages/`. Boundary rule.

## Closing trio — the last three todos of every stage

Every stage's todo checklist ends with the same three items, in order.

1. `checks` — `pnpm typecheck`, `pnpm build`, `pnpm test:e2e`, `scripts/check-boundaries.sh`. Every step must pass.
2. `docs` — update `handover.md` for the next stage, and update `frontend/POLISH-PLAN.md` ticking off the files done.
3. `git` — stage, commit `stage N: <one-line title>`, push to `codeless/phase-7-frontend-polish`.

A stage is not "done" until all three are green and the push succeeds. Never `--force`, never `--no-verify`.
