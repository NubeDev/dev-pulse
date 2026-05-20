## Done

- Added Playwright + `@playwright/test@1.60.0` as dev dep and wired `pnpm test:e2e` / `test:e2e:headed` scripts.
- `frontend/playwright.config.ts` boots `pnpm dev` with `VITE_USE_MOCK_REPORTS=1` on :5173, chromium project, retain-on-failure traces.
- `frontend/tests/e2e/helpers.ts` stubs `/auth/login`, `/auth/me`, `/auth/logout` via `page.route()` so no Rust backend is needed; exposes `signIn(page)` helper.
- `frontend/tests/e2e/smoke.spec.ts` covers: login flow lands on reports, user report renders headline + activity-table + data-as-of, three-lens toggle (Tabs `data-state="active"`), window picker switches to "Last 30 days", admin/refresh triggers a `data-ran="true"` result, freshness page renders headline + grid with one `fresh` and one `stale` card, directory sub-nav navigates.
- `frontend/tests/e2e/static-checks.spec.ts` enforces the no-leaderboard rule via JSX-text regex sweep of `frontend/src`, shells out to `scripts/check-boundaries.sh` (green), and re-runs `pnpm build` then asserts total `gzipSync(file)` size of `dist/` is under 2 MiB (currently ~134 KiB).
- Excluded `tests/`, `dist/`, `playwright.config.ts` from the main `tsconfig.json` so `pnpm typecheck` ignores them; Playwright's own ts loader handles the e2e suite.
- Untracked `frontend/dist/` and added `frontend/test-results`, `playwright-report`, `.playwright` to `.gitignore`.
- Committed as `fcd766d` on `codeless/phase-7-frontend`.

## Next

- (none — last stage of the frontend job)

## What you need to know

- Run with `pnpm --filter dev-pulse-frontend test:e2e` (or `cd frontend && pnpm test:e2e`). Chromium 1223 was already cached under `~/.cache/ms-playwright`; no `playwright install` was needed.
- `signIn(page)` does a real `page.goto("/#/login")` + form submit; subsequent hash navigations must be done via in-app anchor clicks (`page.locator('a[href="#/..."]').click()`) — `page.goto()` on a hash-only change does not refire the SPA router.
- The freshness mock keys are `aaaa…`, `bbbb…`, `cccc…`, `dddd…` org UUIDs (freshness-page.tsx `mockOrgs()`), distinct from `admin/mocks.ts` `MOCK_ORGS` and `directory/mocks.ts`. Because react-query dedupes by `["orgs"]`, the freshness test runs in its own page (fresh `signIn`) so the local `mockOrgs()` populates the cache first — otherwise all cards collapse to the `pending` band.
- The static "no-leaderboard" sweep matches rendered strings (JSX text nodes + aria/title/placeholder attrs) for `Leaderboard`, `Top performers`, `Ranking`, `Score`. Doc-comments that mention "leaderboard" while disavowing it (e.g. `home-org-split-report-page.tsx`) intentionally pass.
- Build size budget is the **sum of `gzipSync(file)` over every file in `dist/`** — covers JS, CSS, HTML, and any future assets. Current footprint leaves >15× headroom.

## Open questions

- (none)
