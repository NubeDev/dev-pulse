---
name: verify
description: Build, launch, and drive dev-pulse — the SPA in a real browser, and the Rust API over a real socket — to observe a change working end-to-end.
---

# Verifying dev-pulse changes

Two surfaces. Pick by what the diff touches:

- **Frontend only** → browser. Drive the SPA with Playwright and stub
  the Rust API with `page.route()`; no backend needed.
- **Anything in `crates/`** (SQL, handlers) → **the socket**. A stubbed
  frontend cannot see a backend bug — stub the endpoint and you are
  testing your stub. Curl the real server (below), then drive the
  browser for the UI half.

## Get a handle

Dev server (Vite) on **:8732**:

```bash
cd frontend
pnpm dev --host 127.0.0.1 --port 8732 --strictPort
```

`make start` from the repo root also launches it on 8732 (backend on
8731). **Check whether :8732 is already up first** (`curl -s
localhost:8732 | grep title`) — a dev server is often already
running and serves your working tree with HMR, so just use it. If
something else holds the port, `--strictPort` fails loudly rather
than silently landing tests on the wrong app.

The repo-wide `playwright.config.ts` targets :5173, which is
frequently another project's Vite. Pin every spec:

```ts
test.use({ baseURL: process.env.DP_FRONTEND_URL ?? "http://localhost:8732" });
```

## Backend surface (crates/)

Postgres is at `postgres://dev-pulse:devpass@localhost/dev_pulse`
(`config.local.toml`); creds also in `docker-compose.yml`. Query it
directly to tell "genuinely empty data" from "handler is 500ing":

```bash
PGPASSWORD=devpass psql -h localhost -U dev-pulse -d dev_pulse -tA -c "select ..."
```

Run the API with the fix:

```bash
set -a; . .env; set +a
cargo run -p dev-pulse -- serve --config config.local.toml   # :8731
```

Do **not** `pkill -f "dev-pulse.*serve"` — the pattern matches the
enclosing shell and kills your own Bash tool call (exit 144). Launch
detached via `run_in_background` and wait with
`until curl -s -o /dev/null localhost:8731/users; do sleep 2; done`.

Most routes sit behind `with_permission(...)` and 401 without a
session. Passwords for existing users are unknown/unguessable — seed
a scratch admin, then log in for a cookie jar:

```bash
cargo run -q -p dev-pulse -- create-admin --config config.local.toml \
  --email verify-scratch@example.com --password VerifyScratch12345
curl -c cj.txt -X POST localhost:8731/auth/login -H 'content-type: application/json' \
  -d '{"email":"verify-scratch@example.com","password":"VerifyScratch12345"}'
curl -b cj.txt "localhost:8731/users?org_id=<uuid>"
```

Clean up after: delete the row from `dp_memberships` + `dp_users`
(login `local:<email>`) and `starter_auth_users_users` in `auth.db`.
Note it also mirrors into `dp_users`, so it inflates org member
counts by 1 while it exists.

**Row-mapper gotcha**: `row_to_user` (`dp-store-pg/src/store/rows.rs`)
`try_get`s every column including `role`; a SELECT that omits one
500s at runtime with `store_error`, not at compile time — sqlx here
is the untyped `sqlx::query` API. Issue #15 was exactly this. If one
scoped variant of an endpoint 500s while another works, diff their
SELECT lists first.

## Drive it

Write a scratch spec in `frontend/tests/e2e/` prefixed with `_`
(matches existing `_pdf-zoom.spec.ts`, `_verify-delete-all.spec.ts`);
delete it when done. Run with `npx playwright test <name>
--reporter=list`. `console.log` in a spec prints to the reporter —
the cheapest way to surface counters (request counts, etc.).

**Copy the stub harness from `tests/e2e/project-views.spec.ts`.** It
is the best template in the repo: `stubAuth(page, { preAuthenticated:
true })` plus a stateful in-memory store behind `page.route()` for
`/projects/**`, modelling real server behaviour including the
`view_name_taken` 409. Stub the app-shell sidecars (`/me/pins`,
`/me/app-install-banner`, `/me/queue*`, `/tags`) or the shell can
bounce you to `/login`; give every per-project sub-collection
(`issues*`, `group-by-options`, `milestones*`, `repos*`,
`board-links*`) a shape-correct empty payload or a zod parse throws
and traps the page in loading.

Injecting faults (500s, 409s, never-succeeds) via the route handler
is how you verify retry/reconcile logic — see
`useCreateProjectViewBatch` in `src/projects/use-projects-data.ts`.

## Gotchas

- **Tab locators**: view tabs render as `G1\n0/0` (name + count
  badge), and a `project-view-tab-drag-<id>` wrapper shares the
  `project-view-tab-` testid prefix. Use
  `page.locator('button[data-testid^="project-view-tab-"]', { hasText: "G1" })`
  — an `^G1$` regex fails on the badge, and omitting `button` is a
  strict-mode violation against the drag wrapper.
- **Toasts**: `<Toaster>` is mounted in `src/main.tsx`. It was missing
  until 2026-07, which made every `toast()` call a silent no-op — if
  toasts stop appearing, check that mount first. Assert on
  `[data-sonner-toast]`.
- Wizard flow: `project-view-new` → `project-view-template-<id>` →
  `project-view-wizard-next` → `project-view-wizard-submit`.
- **Lead picker**: open via `project-detail-lead-label` (the popover
  trigger wraps the label; there is no `-edit` testid), then click
  `project-detail-lead-picker`. Options render under `[role="listbox"]`.
- **`smoke.spec.ts` fails on `main`** (5 of 6, `app-shell` never
  renders) as of 2026-07 — verified against a pristine tree, so don't
  chase it as your regression. `project-views.spec.ts` is the reliable
  regression suite. Don't run `smoke` together with specs that pin
  `baseURL` to :8732 — smoke needs the config's own :5173 webServer
  with `VITE_USE_MOCK_REPORTS=1`.
