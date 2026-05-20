# Scope — phase-7-frontend

## Goal

Ship a working React frontend under `frontend/` that gives operators
a usable UI over the Phase 4 REST surface. Operators can log in,
view activity reports across all three org-scope lenses, manage
directory/home-org assignments, trigger admin operations, and monitor
system freshness — all from a browser. The app follows the SCOPE §11
success criteria: headline + table + trend on every report, three-lens
toggle, "Data as of" timestamps, no leaderboard.

## In scope

- Vite + React + TypeScript + Tailwind v4 app under `frontend/`
- pnpm workspace linking to `@nube/starter-client-ts`, `@nube/starter-ui-kit`, `@nube/starter-ui-core`
- Session-based auth (login with email/password against `starter-auth-users`)
- Typed API client generated from the OpenAPI snapshot
- Report pages: user, team, org, home-org-split, freshness
- Three org-scope lenses per report (§8.1 toggle)
- Window picker: last week / last month / last quarter / custom
- Headline + table + trend chart shape on every report (§11.5)
- "Data as of <timestamp>" displayed per §0.3
- Directory pages: users, orgs, teams with search/filter
- Home-org assignment UI (the cross-company mapping)
- Admin pages: fetch runs log, manual refresh trigger, user anonymise, user export download
- Dark mode via starter-ui-kit theme toggle
- Responsive layout (mobile-friendly)
- Vite proxy to Rust server in dev; static file serving in prod
- Playwright smoke tests for the critical paths

## Out of scope

- Phase 5 (MCP) — deferred
- Phase 6 (CLI) — deferred
- GitHub OAuth login in the frontend (backend has it; frontend uses simple session for now)
- Real-time WebSocket updates (polling with react-query is fine for v1)
- Editing `crates/starter-*` or `packages/` (boundary rule)
- Any backend code changes (the Phase 4 surface is the contract)
- SSR / Next.js — plain Vite SPA

## Hard rules

- **No leaderboard, no single-score affordance** (SCOPE §4). Report pages never rank users by a single metric.
- **Three-lens toggle on every report** — single-org, all-orgs-combined, per-org-split (§8.1). Never one-lens-only.
- **"Data as of" on every report** — always visible, never hidden (§11.7).
- **Headline + table + trend** — every report follows this shape (§11.5). Not just a table.
- **No writes to GitHub** — frontend is read-only against GitHub; mutations go through the admin surface only.
- **`scripts/check-boundaries.sh` still green** — frontend is a new directory; backend crates untouched.

## Constraints

- Follow the `starter/examples/notes/frontend/` pattern for structure
- Use `@tanstack/react-query` for data fetching (caching, refetch)
- Use the OpenAPI snapshot as the API contract (types must match)
- Vite dev proxy targets the Rust server (default localhost:3000)
- pnpm (not npm/yarn) — consistent with the starter workspace

## Open questions (resolve in stage 1)

1. **Chart library** — recharts, chart.js, or lightweight sparklines? Bias: recharts (React-native, composable, handles time-series well).
2. **Routing** — react-router or TanStack Router? Bias: react-router v6 (simpler, well-known, good enough for this scale).
3. **Date/time formatting** — date-fns or dayjs? The window picker needs TZ-aware display. Bias: date-fns (tree-shakeable, no moment legacy).
