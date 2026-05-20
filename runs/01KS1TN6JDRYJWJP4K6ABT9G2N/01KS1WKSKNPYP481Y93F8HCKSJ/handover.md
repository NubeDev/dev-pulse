## Done

- Added `frontend/src/reports/freshness-page.tsx`: queries `GET /reports/freshness` + `GET /orgs`, renders a card grid (one per org) with "last updated <relative time>" and green<1h / yellow<4h / red≥4h colour bands, plus an overall system-health headline summarising reconciler/webhook timestamps and per-band org counts.
- Orgs absent from `data_as_of.per_org` render as "pending first reconciler run" and do not tip the overall band (absent ≠ stale per SCOPE).
- Cards sorted by org login (stable order, no leaderboard gesture).
- 30s `refetchInterval` keeps relative-time labels honest; `Date.now()` recomputed each render.
- `VITE_USE_MOCK_REPORTS=1` smoke fixture seeds one org per band (+ a pending org) so the page renders without dp-server.
- Added `#/reports/freshness` to `frontend/src/routes.ts` (`ReportTab` union + `reportTabOf` + route doc-comment).
- Added the "Freshness" tab to `REPORT_TABS` in `frontend/src/app.tsx` and the `ReportsPane` switch.
- Pure helpers (`bandOf`, `buildCards`, `overallBand`, `formatRelative`) re-exported via `__test__` for follow-up unit tests.
- `pnpm typecheck` + `pnpm build` green; committed on `codeless/phase-7-frontend` as `dc1c40c`.

## Next

- (none) — stage 7 will be picked up by a fresh session.

## What you need to know

- Sub-nav now has 5 tabs (User, Team, Org, Home-org split, Freshness); freshness is appended last so existing deep links are untouched.
- The freshness envelope's `rows` field is `null` server-side; all signals live on `data_as_of`. The page does not use the shared `DataAsOfBanner` because it needs the richer per-org breakdown.
- Colours are inline `oklch(...)` tokens (matching `DataAsOfBanner`'s style) rather than Badge variants because shadcn's Badge only has 4 semantic variants and adding green/amber/red would require a kit-level change.
- `dist/` is tracked in this repo (carried over from earlier stages), so each commit churns the hashed bundle filename — not introduced by this stage.

## Open questions

- (none)
