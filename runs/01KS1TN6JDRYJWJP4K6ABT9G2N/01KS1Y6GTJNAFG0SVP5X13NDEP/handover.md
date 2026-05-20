## Done

- Reviewed phase-7-frontend stages 0–8 (commits 4253a87..c66dff5) against Layer-1 invariants.
- Confirmed zero changes under crates/ or starter/ — frontend-only job.
- Verified Vite proxy + client.ts use REST only; no second transport.
- Verified auth path goes through session cookies + CSRF, no bypass of with_principal / require_permission.
- Verified OpenAPI snapshot and wire formats are unmodified; zod schemas are consumer-side mirrors.

## Next

- Stage 11 (integration testing) — wire dp-server against a real Postgres + test users, smoke-test each frontend page end-to-end, confirm three-lens toggle + window picker + "Data as of" banner all render on real data, exercise admin refresh + anonymise + export.

## What you need to know

- PASS: frontend job is REST-only over the existing dp-rest surface with no crate, transport, trust-boundary, or wire-format change.
- The review gate is the only thing this stage does; no patches, no commit.
- Frontend currently has VITE_USE_MOCK_REPORTS=1 short-circuit fixtures — make sure stage 11 disables it for the live walkthrough.
- starter-ui-kit is source-aliased via Vite (`@` -> `starter/packages/starter-ui-kit/src`) so tsc and Vite resolve identically; remember this when running typecheck outside `pnpm`.

## Open questions

- (none)
