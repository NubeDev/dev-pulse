## Done

- Extended `tests/e2e/smoke.spec.ts` visual-regression smoke: every lens-bearing report page (user/team/org/home-org-split) asserts `[data-slot="card"]` ≥1 AND (`[data-slot="tabs"]` root + `[data-slot="tabs-list"]`) ≥1; freshness checked for Card only.
- Added two design-token invariants asserted once per run: body computed `font-family` matches `/Inter/i` AND `getComputedStyle(document.documentElement).getPropertyValue("--radius")` is exactly `0.625rem`.
- `pnpm typecheck`: clean. `pnpm build`: 142.73 KiB JS gzip + 7.86 KiB CSS gzip, well under the 2 MB gate. `pnpm test:e2e`: 9/9 green (incl. login, lens toggle horizontal cycling, window picker, admin refresh, no-leaderboard grep, Rust boundary check, dist <2MB).
- `scripts/check-boundaries.sh`: OK (no backend changes — all edits live under `frontend/`).
- Inline-style audit: 2 survivors total, both doc-comment mentions only (`src/layout/app-shell.tsx`, `src/auth/login-page.tsx`) explicitly disavowing inline styles — well under the <10 target.
- Committed as `1c9d62e` on `codeless/phase-7-frontend-apple` with subject starting `stage 7: tests + verification`.

## Next

- (none) — final stage of the job

## What you need to know

- The Tabs presence check uses `count(tabs) + count(tabs-list) >= 1`. The starter-ui-kit Tabs primitive emits `data-slot="tabs"` on the root and `data-slot="tabs-list"` on the list, so either slot proves Tabs is mounted — robust to refactors that elide the inner list.
- Token assertions are root-scoped, so they're sampled once after sign-in rather than per-route, which keeps the test fast (~777 ms total).
- The build size budget check in `static-checks.spec.ts` re-runs `pnpm build` itself; combined run still finishes in ~5 s.

## Open questions

- (none)
