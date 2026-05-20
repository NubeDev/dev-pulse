# Workflow — phase-7-frontend

## Sequencing

- Stage 1 scaffolds the app and resolves the three open questions (chart lib, router, date lib). Commit the skeleton that builds.
- Stage 2 generates the typed API client — this is the contract every page depends on.
- Stage 3 wires auth + layout shell — after this, you can log in and see the nav.
- Stages 4–8 are the meat: one page group per stage, each self-contained.
- Stage 9 is polish (responsive, dark mode, loading states, error boundaries).
- Stage 10 is the REVIEW gate before integration tests.
- Stage 11 is Playwright smoke tests — the merge gate.

## Per-stage discipline

- Before any code change:
  - Re-read the SCOPE §11 success criteria (headline + table + trend, three-lens, data_as_of).
  - Re-read SCOPE §4 (no leaderboard, no single-score).
  - Check `crates/dp-rest/tests/openapi.snapshot.json` for the exact API contract.
- After every stage:
  - `pnpm typecheck` must pass (no TS errors).
  - `pnpm build` must produce a working dist/.
  - `scripts/check-boundaries.sh` must stay green.
- Touch only frontend code. No backend changes.

## REVIEW gates

One:
- **After stage 10** — full walkthrough before integration tests. All pages render, lenses toggle, data_as_of shows, no leaderboard anywhere.

## Closing trio — the last three todos of every stage

Every stage's todo checklist ends with the same three items, in order.

1. `checks` — run the stage's verify list: `pnpm typecheck`, `pnpm build`, `scripts/check-boundaries.sh`. Every step must pass.
2. `docs` — update `handover.md` for the next stage.
3. `git` — stage the changes, commit with `stage N: <one-line title from template.yaml>`, push to `codeless/phase-7-frontend`.

A stage is not "done" until all three are green and the push succeeds. Never `--force`, never `--no-verify`.

## Anti-patterns

- A leaderboard or "top N users" view. SCOPE §4 forbids it.
- A report page without the three-lens toggle. Every report has all three.
- A report page without "Data as of". §11.7 is non-negotiable.
- A report page that is just a table (no headline, no trend). §11.5 requires the triptych.
- Calling GitHub directly from the frontend. The frontend talks to the Rust server only.
- Editing backend crates. The Phase 4 surface is the contract.
- Using npm or yarn instead of pnpm.
- Putting API types in separate files from the generated client (drift risk).
