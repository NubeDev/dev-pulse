## Done

- Created `frontend/src/reports/` with seven files implementing the SCOPE §11.5 user-activity report:
- `user-report-page.tsx` — main page with user dropdown (from `GET /users`), window picker, three-lens tabs, headline sentence, table, and trend.
- `window-picker.tsx` — preset selector (last 7 days / last week / last month / last quarter→`last_90_days` / last 30 days / custom) + IANA TZ select + anchor (`utc`/`viewer`/`org`); `windowStateToParams` folds the state into `ReportParams`.
- `lens-tabs.tsx` — three-lens toggle (SingleOrg / AllOrgsCombined / PerOrgSplit) keyed on `ScopeMode`.
- `data-as-of.tsx` — prominent "Data as of …" banner picking `headline`→`reconciler_latest`→`webhook_latest` per §11.7.
- `activity-table.tsx` — sortable table (Activity / Total / Trend) with row-level loading skeletons; `buildActivityRows` maps per-kind queries to display rows.
- `trend-sparkline.tsx` — dep-free SVG polyline sparkline.
- `activity-types.ts` — canonical 12-kind list mirroring `dp_domain::EventKind` in snake_case.
- Per-kind fanout: `useQueries` fires one `getReportUser` per `EventKind` with `group_by=day`, so the trend column is bucketed data.
- Headline sentence summarises top 3 non-zero kinds and names the active lens.
- `VITE_USE_MOCK_REPORTS=1` short-circuits queries to a deterministic mock (matches the 2026-05-20 harness clock); production wiring is the same code path.
- `app.tsx` updated: the reports section now renders `<UserReportPage />` instead of the stage-3 placeholder.
- `pnpm typecheck` and `pnpm build` both pass; committed as `stage 4: user activity report page — …`.

## Next

- Stage 5 picks up the next report page (likely team or org); the components in `reports/` (lens-tabs, window-picker, data-as-of, activity-table, sparkline) are designed to be reused.

## What you need to know

- "Last quarter" is mapped to `WindowLabel = "last_90_days"` because dp-rest has no `last_quarter` variant — the UI label still says "Last quarter (90d)".
- The dp-rest server has no `activity_type` group_by, so per-kind breakdown is implemented as N parallel queries (one per `EventKind`) each filtered with `activity_types=[kind]`. This is 12 queries per render — fine for an operator UI, but if it ever shows up in a perf budget, the right move is a server-side aggregation, not a client refactor.
- The route shape `#/reports/user/:user_id` is honored on first load (deep links) and the user dropdown pushes new selections back into the hash via `navigate()`. The dropdown remains the source of truth in-session.
- `frontend/dist/` is tracked in git (consistent with prior stages); the build artifacts are part of the commit.
- Smoke verification: typecheck + Vite build both succeed; the page is not exercised against a live dp-server in this stage — that's an operator smoke once the bin is running.

## Open questions

- The dp-rest `GET /reports/user/:user_id` default `group_by` is `user` (returns a single CountRow with total). Stage description said "table of CountRows grouped by activity type" — interpreted as a client-side per-kind fanout. If the intent was a single call rendering N rows where `key` = activity_type, a server-side `GroupBy::ActivityType` variant would need to land first.
- TZ list in the picker is a hand-picked subset; if operators need full IANA coverage, swap the `Select` for a typeahead `Combobox` from starter-ui-kit.
