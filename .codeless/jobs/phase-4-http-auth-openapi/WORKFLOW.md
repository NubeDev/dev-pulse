# Workflow — phase-4-http-auth-openapi

How to drive this job. The shape is "lock the auth boundary +
audit vocabulary + OpenAPI shape first because every handler
shapes around them, then land the report / directory / admin
handlers (each pure-axum + utoipa, fast to test against in-memory
`Store` fakes + a stub principal), then aggregate OpenAPI, then
REVIEW the dp-rest surface, then compose into `dp-server::build`,
then wire the bin, then prove it with seven smoke tests."

## Sequencing

- Stage 1 is **prose-only**. Lock the four Phase-4-specific open
  questions in [SCOPE.md](./SCOPE.md), record under "Decisions",
  commit. No code.
- Stage 3 (report handlers) lands first because they exercise
  the Phase 3 envelope + `DataAsOf` + resolved-window contract
  end-to-end through axum — getting these right validates the
  shape every later stage mirrors.
- Stage 4 (directory + home-org) lands next because it's the
  smallest mutating surface and pins down the `audit_log`
  helper that admin handlers reuse in stage 5.
- Stage 5 (admin handlers) extends the Phase 2 `POST
  /admin/refresh` with audit emission and adds the three new
  routes. The streaming export is the riskiest piece; pin it
  early.
- Stage 6 (OpenAPI aggregation) lands once every utoipa
  annotation exists. The snapshot test pins drift detection
  before composition.
- Stage 7 is the **REVIEW gate** on the dp-rest surface. Phase 5
  MCP and Phase 7 frontend both consume this OpenAPI doc; a
  rewind from those phases is much more expensive than catching
  shape bugs now.
- Stage 8 (`dp-server::build`) composes everything: routers
  merged, OpenAPI handed in, metrics + principal wrapped, the
  webhook router merged *outside* `with_principal`. The
  `starter-auth-oauth` GitHub router is also merged outside
  `with_principal` (it authenticates on the callback).
- Stage 9 (GitHub OAuth + authz wiring) layers the
  `github_orgs` attribute stamper, the `starter-authz`
  `StaticRbacEngine` + policy file, and the
  `require_permission(...)` decorations on every protected
  route. The out-of-org gate is proven here against a stubbed
  GitHub provider so stage 10 is not the first time the flow
  runs end-to-end.
- Stage 10 (bin wiring) is the smallest stage but proves the
  whole thing boots: `dev-pulse serve` against a test Postgres,
  `curl /openapi.json` returns the document, `curl
  /auth/oauth/github/login` 302s to GitHub, the stubbed
  callback mints a session with `oauth.github_orgs` populated,
  an in-org request returns a populated `ReportResponse` and an
  out-of-org request returns `403 awaiting_access`.
- Stage 11 (smoke tests in CI) is the merge gate.

## Per-stage discipline

- Before any code change in a stage:
  - `git log -20 --oneline` for the surrounding history.
  - Re-read the rule numbers in [SCOPE.md](./SCOPE.md) that the
    stage touches. R-boundary, R-window-server-side,
    R-data-as-of, R-hmac-only-webhook, R-audit, R-one-openapi,
    R-pseudonymise-not-delete, R-github-org-gate, and
    R-no-bespoke-oauth are the load-bearing ones for Phase 4.
  - Re-read the relevant §0 decision in
    [`../../../TODO.md`](../../../TODO.md): §0.4 (TZ / windows)
    for every report handler, §0.3 (cursors / freshness) for the
    `DataAsOf` echo, §0.5 (deletion model) for the anonymise +
    export handlers, §0.6 (boundary) for every stage.
  - For any stage touching the Store trait, read
    `crates/dp-domain/src/store.rs` first. New methods go through
    `dp-domain` then `dp-store-pg`. `dp-rest` does not import
    `dp-store-pg` directly — it takes `Arc<dyn Store>`.
  - For any stage touching auth or composition, read
    `/home/user/code/rust/starter/DOCS/howto/using-starter-as-a-library.md`
    consumer rules §4 and §6 first. The composition rules are
    not negotiable; a workaround is a rewrite-later cost.
- Touch only what the stage names. No drive-by refactors.
- Verify before commit:
  - **Boundary check first**: run
    `scripts/check-boundaries.sh`. A failure here is the cheapest
    signal the change is shaped wrong. Do not silence the script.
  - **Rust**: `cargo check -p dp-rest -p dp-server -p dev-pulse`,
    then `cargo test -p dp-rest -p dp-server`, then
    `cargo clippy --workspace --all-targets -- -D warnings`.
  - **OpenAPI snapshot**: stages 3–6 must finish with a green
    `cargo test openapi_snapshot` (regenerate via
    `--update-openapi-snapshot` only when the change is
    intentional).
  - **Stage-specific smoke**: every stage's Done column below
    lists the smoke subset it must pass. The full sweep gates
    stage 10; per-stage passes gate per-stage merges.
- Commit only if green. One logical batch per commit; commit
  message stage-tagged: `stage N: <one-line title>`.

## REVIEW gates

Two:

- **After stage 1** — decisions sign-off before any code lands.
  The five Phase-4-specific questions (GitHub OAuth via
  starter-auth-oauth, starter-authz allow-list gate,
  `with_principal` + `require_permission` boundary, audit
  vocabulary, one-OpenAPI-doc) ripple through every later stage
  and into Phases 5 + 7.
- **After stage 7** — dp-rest surface end-to-end before
  composition. Phase 5 MCP and Phase 7 frontend mirror this
  surface verbatim; gating here costs less than rewinding from
  those phases with a handler-shape bug.

Write a one-line summary into the handover at each gate. Do not
proceed.

## What "done" looks like per stage

| Stage | Done when |
|---|---|
| 1 | SCOPE.md "Decisions" section filled in for all four open questions; no code changed; boundary check green (trivially). |
| 3 | Five report handlers in `dp-rest::reports` (`GET /reports/user/:id`, `/team/:id`, `/org/:id`, `/home-org-split`, `/freshness`); each takes the Phase 3 `ReportRequest` envelope and returns `ReportResponse { resolved_window, rows, data_as_of }`; utoipa-annotated; unit tests against an in-memory `Store` assert the window echo + `DataAsOf` presence; `report-handler-echoes-resolved-window-and-data_as_of` smoke passes; boundary check green. |
| 4 | Four directory handlers in `dp-rest::directory` (`GET /users`, `GET /orgs`, `GET /teams`, `POST /home-org`); home-org mutation is atomic (only one `home_org = true` per user); `dp-rest::audit::record()` helper exists and writes one row per call; pinned action vocabulary is a `const` enum; unit tests cover the audit row and the atomic flip; boundary check green. |
| 5 | `GET /admin/runs` (paginated), `POST /admin/users/:id/anonymise`, `GET /admin/users/:id/export` live in `dp-rest::admin`; the Phase 2 `POST /admin/refresh` now also emits an `audit_log` row; user export streams via `axum::body::Body::from_stream` and pins a memory-budget upper bound in tests; pseudonymisation cascade unit-tested end-to-end; `admin-user-export-streams-without-OOM` smoke passes; boundary check green. |
| 6 | `#[derive(OpenApi)] DevPulseApi` in `dp-rest::openapi` lists every annotated handler + every schema; `tests/openapi.snapshot.json` checked in; regenerator flag `--update-openapi-snapshot` documented; `openapi-snapshot-stable` smoke passes; boundary check green. |
| 8 | `dp-server::build(config) -> Result<Router, BuildError>` returns one composed `Router`; `ServerBuilder::new(state).merge_router(…).with_openapi(…).with_metrics(…).with_principal(auth, &[protected])` chain wired; webhook router merged outside `with_principal`; auth session routes from `starter_auth_users::routes::session_router` merged in; `starter_auth_oauth::routes::github_router` merged in outside `with_principal`; `with_principal-covers-every-non-webhook-non-auth-route` smoke passes; boundary check green. |
| 9 | GitHub OAuth provider mounted (client_id/client_secret resolved from dp-config `[auth.github]` via starter-secrets-file); `github_orgs` attribute stamper writes `Principal.extra.oauth.github_orgs` on session mint via one octocrab `GET /user/orgs` call cached on the session row; `starter-authz::StaticRbacEngine` loaded from `crates/dp-server/policy/dev-pulse.toml` with one allow rule keyed on `oauth.github_orgs intersects auth.github.allow_orgs`; every protected route in dp-rest decorated with `require_permission(...)`; out-of-org user signs in but is denied with `403 awaiting_access` + `auth.denied_org` audit row; `github-oauth-callback-mints-session-and-stamps-orgs`, `in-org-github-user-can-read-reports`, `out-of-org-github-user-signs-in-but-cannot-read-reports`, and `require_permission-covers-every-protected-route` smokes pass; boundary check green. |
| 10 | `dev-pulse serve` calls `dp_server::build(config).await?` and mounts on the address from `dp-config`; cooperative shutdown hooks into Phase 2 worker + scheduler join handles; `curl /openapi.json` returns the DevPulseApi document; `curl /auth/oauth/github/login` 302s to GitHub; stubbed callback mints a session with `oauth.github_orgs`; in-org authenticated `curl /reports/user/:id` returns a populated `ReportResponse`; out-of-org returns 403 awaiting_access; boundary check green. |
| 11 | All Phase-4 smoke tests green in CI: github-oauth-callback-mints-session-and-stamps-orgs, out-of-org-github-user-signs-in-but-cannot-read-reports, in-org-github-user-can-read-reports, report-handler-echoes-resolved-window-and-data_as_of, webhooks-github-not-principal-wrapped-but-rejects-bad-hmac, audit_log-row-written-per-protected-handler, openapi-snapshot-stable, with_principal-covers-every-non-webhook-non-auth-route, require_permission-covers-every-protected-route, boundary-check-still-green, admin-user-export-streams-without-OOM. |

## Anti-patterns

- A second `audit_log` writer. R-audit — one
  `dp_rest::audit::record()` helper; every handler routes
  through it. A second writer means the schema drifts silently
  per-handler.
- Per-module OpenAPI documents. R-one-openapi + consumer-rules
  §6.7 — one `DevPulseApi`. Phase 7 generates one TS client off
  one doc; fragmenting the doc fragments the client.
- A `with_principal` exclusion for anything other than `POST
  /webhooks/github`, the `starter-auth-users` session routes,
  and the `starter-auth-oauth` GitHub login / callback routes.
  R-hmac-only-webhook — every other route is principal-wrapped.
  A new exclusion is a SCOPE update, not a stage shortcut.
- A hand-rolled GitHub OAuth flow. R-no-bespoke-oauth — use
  `starter-auth-oauth`. If a hook is missing, write it up; do
  not fork or edit the starter crate.
- An in-handler `principal.extra.oauth.github_orgs.contains(…)`
  check. R-github-org-gate — the gate is one
  `starter-authz` policy rule plus `require_permission(…)`
  decorations. A handler that reads the org list directly
  duplicates the policy and drifts.
- Adding the allow-list to the policy file instead of
  `dp-config`. R-github-org-gate — the policy file references
  the config-driven list via the condition; adding `"BetaCo"`
  is a config edit, not a code edit.
- A handler that returns a `ReportResponse` without
  `resolved_window` or `data_as_of`. R-window-server-side +
  R-data-as-of — both fields are mandatory on every report
  shape. The smoke covers every shape for a reason.
- Resolving "last week" in the handler. R-window-server-side —
  the handler hands the `WindowSpec` to `dp-reports`; the
  resolution happens once, server-side, in the report layer.
- A handler that imports `dp-store-pg` directly. R-boundary +
  the §0.6 boundary rule for `dp-rest`'s allowlist. New
  projections go through `dp-domain::store::Store`.
- Hard-deleting a user from a handler. R-pseudonymise-not-delete
  + §0.5 — the v1 surface is pseudonymisation. Hard-delete is a
  separate admin operation documented but not wired for v1.
- A user export that materialises the full event list in memory.
  The smoke pins a memory budget; loosening it is the wrong fix.
  Stream via `Body::from_stream`.
- Editing `crates/starter-*` to add an Authenticator hook the
  operator-OAuth layer needs. R-no-starter-edit — layer in
  `dp-server` around the existing surface; if you cannot, write
  it up.
- Loosening the OpenAPI snapshot to make a failing test pass.
  R-one-openapi — the snapshot is the contract Phase 5 + Phase
  7 read against. Regenerate deliberately via
  `--update-openapi-snapshot` only when the schema change is
  intentional.
- A protected-path list that lives in two places (the handler's
  `#[utoipa::path]` and the `with_principal` array in
  `dp-server::build`). One source of truth — the array. The
  smoke that hits every route catches drift.
- Trying to land Phase 5 (MCP) or Phase 6 (CLI) work "while
  we're here." Out of scope; each is its own job.

## Closing trio — the last three todos of every stage

Every stage's todo checklist ends with the same three items, in
order. The user watches these tick over in the `Stages` overview;
they are how the user confirms a long-running stage actually
landed instead of just looking like it did. Do **not** rename or
reorder them.

1. `checks` — run the stage's verify list, **including**
   `scripts/check-boundaries.sh` before any cargo invocation,
   **and** the OpenAPI snapshot test for any stage touching
   `dp-rest/src`. Every step must pass. On failure: stop, fix,
   re-run; do not advance to `docs`.
2. `docs` — update `handover.md` for the next stage and the active
   session doc, in the same worktree, so the fresh agent that opens
   the next stage has the context it needs.
3. `git` — stage the changes, commit with the message
   `stage N: <one-line title from template.yaml>`, and push to the
   job's branch (`codeless/phase-4-http-auth-openapi`).

A stage is not "done" until all three are green and the push
succeeds. Never `--force`, never `--no-verify`; if a hook fails,
fix the cause.
