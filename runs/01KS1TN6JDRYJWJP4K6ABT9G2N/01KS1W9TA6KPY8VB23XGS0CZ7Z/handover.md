## Done

- Added three new report pages mirroring the user-report shell: `frontend/src/reports/team-report-page.tsx` (two-step org→team selector populated from `GET /orgs` + `GET /teams`, per-activity-kind fanout), `frontend/src/reports/org-report-page.tsx` (single org dropdown from `GET /orgs`, per-kind fanout), `frontend/src/reports/home-org-split-report-page.tsx` (two parallel `GET /reports/home-org-split` queries — totals + day-bucketed trend, rolled up by home-org with a totals + share % + sparkline table).
- Each page has the three-lens toggle (`LensTabs`), `WindowPicker`, `DataAsOfBanner`, and a SCOPE §11.5 headline + table + trend.
- Updated `frontend/src/routes.ts` with `ReportTab` + `reportTabOf(route)` to parse `#/reports/{user,team,org,home-org-split}[/:id]`.
- Updated `frontend/src/app.tsx`: `Router` now dispatches to a `ReportsSection` that renders a reports sub-nav above the active pane (`ReportsPane` switch). Plain anchors so the hash route stays the source of truth.
- All four pages respect `VITE_USE_MOCK_REPORTS=1` for the offline smoke; deterministic fixtures render the full page without dp-server.
- `pnpm typecheck` + `pnpm build` green; committed as `7e1ee7e` on `codeless/phase-7-frontend`.

## Next

- (none — next stage picks up from a fresh session per workflow)

## What you need to know

- No leaderboard ordering gesture anywhere: home-org-split table sorts by raw count and renders share % alongside (not a ranking); user/team/org tables sort by label or total only.
- Home-org-split trend fanout: dp-rest's `HomeOrgSplitRow` is `{user_id, org_id, count}` — the bucketed query reuses `user_id` to encode the RFC3339 day key in the mock, and the `rollup()` reducer detects that with a regex so a real dp-rest response that fans out per (user × org × day) aggregates per org cleanly. If the real shape differs, only the `rollup` reducer needs adjusting.
- Team page resets the selected team when the org changes (no cross-org team carryover).
- Frontend `dist/` is tracked in the repo (prior stage's pattern); the build outputs are committed alongside the source changes.

## Open questions

- (none)
