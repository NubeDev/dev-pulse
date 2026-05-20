# Scope — phase-4-http-auth-openapi

> Source of truth: [`TODO.md`](../../../TODO.md) §"Phase 4 — HTTP +
> auth + OpenAPI" in the dev-pulse repo, plus
> [`SCOPE.md`](../../../SCOPE.md) for product scope (especially §7
> audiences, §9 GDPR, §11.7 freshness). This file is the per-job
> brief the runner reads before every stage; intentionally short.
> When this file disagrees with TODO.md or SCOPE.md, those win —
> open an issue and update this file.

## Goal

Build the **HTTP surface** in `dp-rest` and the **composition
root** in `dp-server` that turns Phase 3's report layer, Phase 2's
ingestion / webhook path, and the Phase 1 GDPR primitives into a
working multi-user JSON API. Operator login is **GitHub OAuth via
`starter-auth-oauth`** layered on top of the `starter-auth-users`
session + token surface — first-callback auto-provisions the
operator row and mints the same `sas_*` session local login would.
Local email+password signup stays `SIGNUP_MODE=disabled`. Access
is **gated by `starter-authz`** against the operator's GitHub
org memberships: only logins whose `oauth.github_orgs` intersects
the configured allow-list (`auth.github.allow_orgs`, e.g.
`["NubeIO", "ACME"]`) clear the policy; everyone else gets a row
(for audit) but every protected route returns `403
awaiting_access`. Routes are wrapped via `with_principal` for the
auth stage and `require_permission(resource, action)` for the
authz stage; OpenAPI is one `DevPulseApi` derived in `dp-rest`;
metrics ride on `with_metrics`. Every protected handler writes
`audit_log`.

The phase succeeds when, given a configured `AppState`,
`dp_server::build()` returns a single axum `Router` that boots
under `dev-pulse serve`, serves the v1 report / directory / admin
surface, exposes `GET /openapi.json` with the full document,
keeps the Phase 2 webhook receiver reachable (HMAC, *not*
principal-wrapped), accepts `GET /auth/oauth/github/login` →
`GET /auth/oauth/github/callback` for operator sign-in, and
refuses every protected route for a GitHub user outside the
configured `allow_orgs` — and every smoke test in §"Smoke tests"
is green in CI.

## In scope

- **GitHub OAuth operator login** (composition in `dp-server`):
  `starter-auth-oauth` mounted with the GitHub provider;
  first-callback auto-provisions the operator row in the
  `starter-auth-users` users table and mints the standard
  `sas_*` session. Email+password signup stays disabled
  (`SIGNUP_MODE=disabled`); the local-login route from
  `starter-auth-users` stays available for admin-CLI-seeded
  break-glass operators.
- **GitHub org attribute stamping** (`dp-server::auth::github_orgs`):
  on session mint and on a configurable refresh interval (default
  1h), call `GET /user/orgs` via the same octocrab client wrapper
  Phase 2 already paces, and write the resulting org login list
  into `Principal.extra.oauth.github_orgs` per the
  `starter-authz` R8 attribute-bus convention. One call per
  session-mint + per-refresh, cached on the session row, never on
  the request hot path.
- **`starter-authz` policy gate** (composition in `dp-server`,
  policy file in `crates/dp-server/policy/dev-pulse.toml`):
  `StaticRbacEngine` loaded at boot, default policy ships one
  allow rule (`condition = 'oauth.github_orgs intersects
  auth.github.allow_orgs'` over `resource = "*"`, `actions =
  ["*"]`) plus the `starter-authz` built-in role defaults.
  Every protected route in `dp-rest` wraps itself in
  `require_permission(<resource>, <action>)` so a forgotten
  permission decoration trips the R-default-deny smoke. A
  user outside `allow_orgs` is signed in (row + audit) but every
  protected request returns `403 awaiting_access`.
- **Report handlers** (`dp-rest::reports`):
  `GET /reports/user/:user_id`, `GET /reports/team/:team_id`,
  `GET /reports/org/:org_id`, `GET /reports/home-org-split`,
  `GET /reports/freshness`. Each takes the Phase 3
  `ReportRequest` envelope verbatim and returns
  `ReportResponse { resolved_window, rows, data_as_of }`. The
  resolved `Window` echoes back per §0.4; `DataAsOf` is present
  on every shape per §0.3 / §11.7.
- **Directory handlers** (`dp-rest::directory`): `GET /users`,
  `GET /orgs`, `GET /teams`, `POST /home-org`. The home-org
  mutation is atomic — only one `memberships.home_org = true`
  per user. Audit row written per call.
- **Admin handlers** (`dp-rest::admin`): `GET /admin/runs`
  (paginated `fetch_runs` projection), `POST /admin/users/:id/
  anonymise`, `GET /admin/users/:id/export` (streamed JSON dump
  per SCOPE §9 — no full materialisation in memory). The Phase 2
  `POST /admin/refresh` is updated to also emit `audit_log`.
- **OpenAPI aggregation** (`dp-rest::openapi`):
  `#[derive(OpenApi)] struct DevPulseApi` lists every utoipa
  handler + every request/response schema. Snapshot test pinned
  to `tests/openapi.snapshot.json`; regenerate with
  `--update-openapi-snapshot`.
- **Composition root** (`dp-server::build`): one `Router` built
  by `ServerBuilder::new(state).merge_router(…).with_openapi(…)
  .with_metrics(…).with_principal(auth, &[protected_paths])
  .build()`. Webhook router merged outside `with_principal`.
  Auth session routes merged from
  `starter_auth_users::routes::session_router`.
- **Bin wiring** (`dev-pulse::serve`): replaces the Phase 2 stub
  with `dp_server::build(config).await?`, mounts on the address
  from `dp-config`, hooks cooperative shutdown into the existing
  webhook worker / scheduler join handles.
- **`audit_log` writes** in every protected handler, using a
  pinned `action` vocabulary (`report.read`, `home_org.set`,
  `admin.refresh`, `user.anonymise`, `user.export`, `runs.list`).

## Out of scope

- Self-service local signup (`starter-auth-users` signup add-on
  in modes `open` / `invite`) — not needed because operator
  onboarding is GitHub OAuth + allow-list. `SIGNUP_MODE` stays
  `disabled`. If a future deployment wants invite-only local
  accounts as a fallback, it ships as its own job, not here.
- A bespoke pre-OAuth signup gate (rejecting users before the
  `users` row is created). That would require adding an
  `OAuthSignupGate` hook to `starter-auth-oauth`, which violates
  R-no-starter-edit. The post-OAuth `starter-authz` policy gate
  is the in-bounds shape for v1.
- Per-user / per-org policy editing UI. The v1 policy is the
  static `allow_orgs` allow-list plus the built-in role defaults;
  per-user overrides live in `memberships.role` which the
  policy file references. A future phase swaps `StaticRbacEngine`
  for `DbPolicyEngine` if operators need to edit policies without
  a redeploy.
- The MCP tool surface — Phase 5 (`dp-mcp`).
- CLI implementations beyond what already exists from Phase 2
  (`fetch-now`, `backfill`, `migrate`, `serve`, `claim`) — Phase
  6 (`dp-cli`).
- The frontend (any React / TS / `@nube/starter-*` work) — Phase
  7. The OpenAPI doc here is what Phase 7 generates a client off.
- The E2E test harness — Phase 8.
- Schema changes in `dp-store-pg` — Phase 1 owns the schema. If a
  handler needs a projection that does not exist yet, extend
  `dp-domain::store::Store` (and the `dp-store-pg` impl) through
  one Store-trait method; never reach into `dp-store-pg` from
  `dp-rest`.
- Re-opening any §0 decision from TODO.md or any decision locked
  in Phases 2 / 3. They are inputs, not questions.
- Editing anything under `crates/starter-*` or `packages/`. If
  the work seems to require that, stop and write it up; the
  boundary rule is the entire point.
- Building a second OpenAPI document (e.g. per-module
  sub-documents) — one `DevPulseApi` per consumer-rules §6.7.
- Bespoke auth: we use `starter-auth-users`. If the operator
  GitHub OAuth login is not covered (stage 1 decision), we layer
  OAuth *on top of* the existing Authenticator surface inside
  `dp-server`; we do not fork or edit `starter-auth-users`.

## Hard rules (load-bearing)

These are inherited from `dev-pulse/TODO.md` §0 and SCOPE; restated
so the runner re-reads them every stage.

- **R-boundary (§0.6)** — Zero `starter_*` imports in `dp-domain`,
  `dp-fetcher`, `dp-reports`. `dp-store-pg` may import only
  `starter_spi::MigrationSource`. `dp-rest`, `dp-server`,
  `dp-mcp`, `dp-cli`, and the `dev-pulse` bin are edge-allowed
  per §0.6 — those are the only places this phase touches with
  starter imports. `scripts/check-boundaries.sh` enforces in CI.
- **R-window-server-side (§0.4)** — Every report handler resolves
  `(label, tz, anchor)` server-side via `dp-reports` and echoes
  the resolved UTC `Window` back. Handlers never accept a
  pre-resolved start/end without also accepting the spec for the
  echo — the frontend takes labels, not pre-computed ranges.
- **R-data-as-of (§0.3, §11.7)** — Every report response carries
  the `DataAsOf` envelope. A handler that drops it is broken.
  Smoke-tested across every shape.
- **R-hmac-only-webhook** — `POST /webhooks/github` authenticates
  via HMAC SHA-256 (Phase 2) and is the *only* route excluded
  from `with_principal`. If a new public route is needed, it
  goes through `with_principal` with a `RoleRequirement` — not
  another HMAC channel. The OAuth login + callback routes from
  `starter-auth-oauth` are also outside `with_principal`
  (they authenticate themselves on the callback), same as the
  `starter-auth-users` session routes.
- **R-github-org-gate** — Operator access is gated by GitHub org
  membership through `starter-authz`, not by a hand-rolled check
  in any handler. The allow-list lives in `dp-config`
  (`auth.github.allow_orgs`); the policy file in
  `crates/dp-server/policy/dev-pulse.toml` references it via the
  `oauth.github_orgs intersects …` condition. Every protected
  route is wrapped in `require_permission(...)`; no handler
  reads `principal.extra.oauth.github_orgs` directly. A user
  outside the allow-list is signed in (row + audit) but is
  denied at the authz layer with `403 awaiting_access`.
- **R-no-bespoke-oauth** — GitHub OAuth is `starter-auth-oauth`,
  not a fork or a parallel implementation. The org-attribute
  stamping wrapper in `dp-server` calls `starter-auth-oauth`'s
  existing session-mint hook (or, if no such hook exists,
  wraps the callback route to fetch + write the attribute
  before returning the session). It does **not** edit
  `starter-auth-oauth`.
- **R-audit** — Every protected handler writes one `audit_log`
  row per call using the pinned action vocabulary. The writer
  is one helper (`audit::record`) so the schema cannot drift
  per-handler. Smoke test asserts the row lands. A login that
  is denied at the authz gate (out-of-org GitHub user) writes
  `auth.denied_org` against the freshly-created user_id so
  operators can spot leaked invite-style abuse in the audit
  trail.
- **R-one-openapi** — One `#[derive(OpenApi)] DevPulseApi` in
  `dp-rest::openapi` aggregates every annotated handler. No
  per-module sub-documents. Snapshot test pins the JSON.
- **R-pseudonymise-not-delete** — `POST /admin/users/:id/
  anonymise` calls `Store::pseudonymise_user` (Phase 1 §0.5) and
  keeps the user_id stable. Hard-delete is documented but not
  wired into the v1 UI / handlers.
- **R-no-starter-edit** — Inherited from TODO §0.6. The boundary
  script runs in the per-stage closing trio's `checks` todo, not
  only in CI.

## Constraints

- `audit_log` writes use **one** helper
  `dp_rest::audit::record(state, action, target)` so per-handler
  drift is impossible. The action vocabulary is a `const` enum;
  inventing a new action ships as a code change in `dp-rest`.
- The user export streams via `axum::body::Body::from_stream` so
  a 500MB export does not materialise in memory. Smoke test
  pins a memory-budget upper bound.
- The protected-path list for `with_principal` lives in **one**
  array in `dp-server::build`. Forgetting to add a new route
  there is the failure mode the
  `with_principal-covers-every-non-webhook-non-auth-route` smoke
  exists to catch.
- OpenAPI snapshot is JSON-pretty-printed and sorted so diffs
  are reviewable. Regeneration is a deliberate CLI flag, not the
  default.
- `dp-rest` takes `Arc<dyn Store>`, `Arc<dyn Authenticator>`,
  and the rest of `AppState` as generics or trait objects — it
  does not depend on `PgStore` directly. This keeps unit tests
  in `dp-rest` self-contained against in-memory fakes.
- `dp-rest` is allowed to import `starter_server` types it needs
  (`Principal`, `AppState` trait) but not `starter_auth_users`
  internals — the composition wiring lives in `dp-server`.

## Deliverables

- `dp-server::auth`: GitHub OAuth composition wiring
  (`starter-auth-oauth` with the GitHub provider), the
  `github_orgs` attribute stamper (one octocrab call per
  session mint, cached on the session, lazy refresh per
  `auth.github.org_refresh_interval`), and the
  `starter-authz` engine + policy file load.
- `crates/dp-server/policy/dev-pulse.toml`: the v1 policy. One
  allow rule keyed on `oauth.github_orgs intersects
  auth.github.allow_orgs`; the rest is the `starter-authz`
  built-in role defaults via R7.
- `dp-config` additions: `[auth.github]` block with
  `client_id`, `client_secret` (secret://), `allow_orgs:
  Vec<String>`, `org_refresh_interval: Duration` (default 1h).
- `dp-rest::reports`: five report handlers, utoipa-annotated.
- `dp-rest::directory`: four directory handlers (`GET /users`,
  `GET /orgs`, `GET /teams`, `POST /home-org`).
- `dp-rest::admin`: `GET /admin/runs`, `POST /admin/users/:id/
  anonymise`, `GET /admin/users/:id/export`; `POST /admin/refresh`
  extended with audit-log emission.
- `dp-rest::openapi`: `DevPulseApi` + snapshot test +
  `--update-openapi-snapshot` regenerator.
- `dp-rest::audit`: single `record()` helper + pinned action
  vocabulary.
- `dp-server::build`: composition root returning one `Router`.
- `dev-pulse::serve`: bin wiring against `dp_server::build`.
- Seven Phase-4 smoke tests in CI (see §"Smoke tests" below).

## Open questions (resolve in stage 1)

The §0 decisions in TODO.md and the Phase 2 / Phase 3 decisions
are **inputs**, not open questions for this phase. The remaining
four are Phase-4-specific:

1. **Operator login = GitHub OAuth via `starter-auth-oauth`.**
   Bias: yes. `starter-auth-users` provides the user / session /
   token primitives; `starter-auth-oauth` provides the GitHub
   provider that auto-provisions the user row on first callback
   and mints the same `sas_*` session. Local email+password
   signup stays `SIGNUP_MODE=disabled`. The CLI-seeded admin
   from `starter-auth-users::admin::create-admin` is the
   break-glass path; everyone else logs in with GitHub.
2. **Access gate = `starter-authz` allow-list on
   `oauth.github_orgs`.** Bias: yes. One `StaticRbacEngine`
   policy file (`crates/dp-server/policy/dev-pulse.toml`) ships
   with one allow rule keyed on `oauth.github_orgs intersects
   auth.github.allow_orgs` over `resource = "*"`,
   `actions = ["*"]`. Out-of-org users get a row (audit) but
   every protected request returns `403 awaiting_access`. The
   allow-list (`["NubeIO", "ACME"]` etc.) lives in `dp-config`,
   not in the policy file, so adding an org is a config bump
   not a code change.
3. **`with_principal` + `require_permission` boundary.** Bias:
   every route is protected by both except `POST
   /webhooks/github` (HMAC) and the OAuth login / callback +
   `starter-auth-users` session routes (authenticate themselves).
   `require_permission` decorations are mandatory on every other
   route; the R-github-org-gate smoke catches drift.
4. **Audit action vocabulary.** Bias: lock to `report.read`,
   `home_org.set`, `admin.refresh`, `user.anonymise`,
   `user.export`, `runs.list`, plus `auth.signed_in` and
   `auth.denied_org` for the OAuth-gate path. New actions ship
   as code, not config.
5. **One `DevPulseApi` OpenAPI document.** Bias: one
   `#[derive(OpenApi)]` aggregator in `dp-rest::openapi`. Per
   consumer-rules §6.7, the consumer owns the doc; per-module
   sub-documents fragment client generation. The OAuth login +
   callback routes mounted from `starter-auth-oauth` are
   referenced in `DevPulseApi` as `#[utoipa::path]` shims so
   they appear in the published doc even though they live in
   the starter crate.

Record decisions in this file under "Decisions" before stage 3
(the first code stage) begins.

## Decisions

### D4.1 Operator login: GitHub OAuth via `starter-auth-oauth`

- **Decision:** Operators authenticate via GitHub OAuth using
  `starter-auth-oauth` with the GitHub provider. On first callback
  the provider auto-provisions a `users` row and mints the standard
  `sas_sid` + `starter_csrf` session cookies — the same flow local
  login would produce. Local email+password signup stays
  `SIGNUP_MODE=disabled`; the CLI-seeded admin from
  `starter-auth-users::admin::create-admin` is the break-glass path
  for initial deployment bootstrap.
- **Mounting:** `starter_auth_oauth::oauth_router(OAuthRoutesState)`
  merged into `dp-server::build` **outside** `with_principal` (the
  callback authenticates itself via the OAuth code exchange). The
  route prefix is `GET /auth/oauth/github/login` and
  `GET /auth/oauth/github/callback` per the starter crate's defaults.
- **Session minting:** reuses `starter-auth-users`' `SessionStore`;
  `mint_session_headers` produces `sas_sid` (HttpOnly) and
  `starter_csrf` (JS-readable). No custom session logic in
  `dp-server`.
- **`github_orgs` stamping:** a post-callback wrapper in
  `dp-server::auth` calls `GET /user/orgs` via the Phase 2 octocrab
  client wrapper (rate-limit-paced), extracts the org login list, and
  writes it into the session row as
  `Principal.extra.oauth.github_orgs: Vec<String>`. Cached on the
  session row; refreshed lazily per `auth.github.org_refresh_interval`
  (default 1h, configurable in `dp-config`). Never called on the
  request hot path.
- **Why:** `starter-auth-oauth` is the composition-rule-compliant
  shape (R-no-bespoke-oauth). GitHub OAuth surfaces the org
  membership needed for the authz gate (D4.2) without requiring
  org-admin install permissions. The same `Arc<dyn Authenticator>`
  resolves sessions minted by OAuth or by local CLI-seeded admin —
  the stack does not branch on login method downstream.
- **Revisit if:** a deployment environment blocks GitHub OAuth (air-
  gapped) → layer OIDC via a second `starter-auth-oauth` provider
  (Google / generic OIDC); the session + authz surface stays
  unchanged. Or if `starter-auth-oauth` adds a `post_provision_hook`
  that eliminates the need for the callback wrapper in `dp-server` →
  remove the wrapper, move stamping into the hook.

### D4.2 Access gate: `starter-authz` allow-list on `oauth.github_orgs`

- **Decision:** Access to every protected route is gated by
  `starter-authz` (`StaticRbacEngine`) loaded at boot from
  `crates/dp-server/policy/dev-pulse.toml`. The v1 policy ships one
  allow rule:
  ```toml
  [[rules]]
  id = "org-gate"
  role = "*"
  resource = "*"
  actions = ["*"]
  condition = "oauth.github_orgs intersects auth.github.allow_orgs"
  effect = "allow"
  priority = 100
  ```
  Plus `default_policy = true` for the built-in Reader/Writer/Admin
  role defaults (R7).
- **allow-list location:** `auth.github.allow_orgs: Vec<String>` in
  `dp-config` (e.g. `["NubeIO", "ACME"]`). Adding an org is a config
  edit, not a code / policy-file change. The policy condition
  references it via the attribute-bus convention.
- **Out-of-org behaviour:** a GitHub user whose orgs do not intersect
  `allow_orgs` still gets a `users` row (provisioned by
  `starter-auth-oauth` on callback) and a valid session, but every
  `require_permission(...)` check returns `403 awaiting_access`. An
  `auth.denied_org` audit row is written on each denial so operators
  can spot leaked-invite-style abuse.
- **Per-route decoration:** every handler in `dp-rest` that is behind
  `with_principal` also has a `require_permission(<resource>,
  <action>)` wrapper. The resource/action pairs follow the audit
  vocabulary (D4.4). A forgotten decoration trips the
  `require_permission-covers-every-protected-route` smoke.
- **Why:** centralised policy, not per-handler if-checks. The allow-
  list in config (not in the policy file) means operators never edit
  TOML rule syntax. `starter-authz` is the in-bounds shape; a hand-
  rolled `github_orgs.contains(...)` in every handler drifts and
  violates R-github-org-gate.
- **Revisit if:** a deployment wants per-user overrides (e.g. block
  one person who is in an allowed org) → add a deny rule per-user in
  the policy file referencing `principal.email` or `principal.id`;
  no code change needed. Or if operators demand UI-editable policies
  → swap `StaticRbacEngine` for `DbPolicyEngine` in a future phase.

### D4.3 `with_principal` + `require_permission` boundary

- **Decision:** every route is protected by **both** `with_principal`
  (authn — is there a valid session?) **and** `require_permission`
  (authz — does the session's principal satisfy the policy?) except:
  1. `POST /webhooks/github` — HMAC-authenticated per Phase 2; not
     principal-wrapped.
  2. `GET /auth/oauth/github/login` and
     `GET /auth/oauth/github/callback` — the OAuth flow authenticates
     itself via code exchange; mounted outside `with_principal`.
  3. `starter-auth-users` session routes (`POST /auth/session`,
     `DELETE /auth/session`) — authenticate themselves.
- **Protected-path array:** one authoritative list in
  `dp-server::build`: `&["/reports/*", "/users", "/orgs", "/teams",
  "/home-org", "/admin/*"]`. The
  `with_principal-covers-every-non-webhook-non-auth-route` smoke hits
  every non-excluded route without a session and asserts 401.
- **Why:** defense-in-depth. `with_principal` catches unauthenticated
  requests (401); `require_permission` catches authenticated-but-
  unauthorised requests (403). Separating them means the auth gate
  and authz gate can evolve independently (e.g. adding per-resource
  role checks later without touching the auth layer).
- **Revisit if:** a new public route (e.g. a health endpoint) is
  needed → add it to the explicit exclusion set with a SCOPE update,
  not as a stage-level shortcut.

### D4.4 Audit action vocabulary (v1, pinned)

- **Decision:** the v1 `audit_log` action vocabulary is a `const`
  enum in `dp-rest::audit`:
  ```
  report.read      — any report handler invocation
  home_org.set     — POST /home-org
  admin.refresh    — POST /admin/refresh
  user.anonymise   — POST /admin/users/:id/anonymise
  user.export      — GET /admin/users/:id/export
  runs.list        — GET /admin/runs
  auth.signed_in   — successful OAuth callback (session minted)
  auth.denied_org  — authz denial due to out-of-org membership
  ```
- **Schema:** the Phase 1 `audit_log` table
  (`actor_user_id UUID, action TEXT, target TEXT, at TIMESTAMPTZ`)
  is used as-is. `target` is the entity the action operates on
  (user_id for anonymise/export, org_id for refresh with scope,
  report-path for report.read, etc.).
- **Writer:** one helper `dp_rest::audit::record(store, actor_user_id,
  action, target)`. Every protected handler routes through it. No
  second writer in any handler or middleware.
- **New verbs:** adding a new action is a code change in `dp-rest`
  (extend the enum + add a variant → forces the exhaustive match to
  update every site). Not config-driven — intentional drift-
  prevention.
- **Why:** a pinned vocabulary makes the audit trail queryable without
  knowing which handler wrote what. The enum exhaustiveness prevents
  silent schema drift. `auth.denied_org` specifically exists so
  operators can detect leaked invite-style abuse (someone got the
  callback URL but is not in an allowed org).
- **Revisit if:** Phase 5 (MCP) needs a distinct action (e.g.
  `mcp.tool_call`) → extend the enum; the vocabulary grows but never
  repurposes an existing verb.

### D4.5 One `DevPulseApi` OpenAPI document

- **Decision:** one `#[derive(OpenApi)] struct DevPulseApi` in
  `dp-rest::openapi` aggregates every utoipa-annotated handler in
  `dp-rest` plus `#[utoipa::path]` shims for the OAuth login /
  callback and session routes mounted from starter crates.
- **Why:** per consumer-rules §6.7, the consumer owns the OpenAPI
  document and passes it once to `ServerBuilder::with_openapi`. Per-
  module sub-documents would fragment Phase 7's TS client generation
  and Phase 5's MCP tool-schema derivation.
- **Snapshot:** `tests/openapi.snapshot.json` is committed and
  pinned. Accidental drift surfaces as a failing test. Deliberate
  changes regenerate via `cargo test -- --update-openapi-snapshot`.
- **Starter-route shims:** the OAuth and session routes live in
  starter crates and may not have utoipa annotations. `DevPulseApi`
  includes hand-written `#[utoipa::path]` shims (in
  `dp-rest::openapi`) that describe their request/response shapes so
  the published document covers the full API surface. These shims
  are tested for correctness against the actual route responses in
  the smoke tests.
- **Revisit if:** `starter-auth-oauth` or `starter-auth-users` adds
  native utoipa annotations → remove the shims and replace with
  direct path references in `DevPulseApi`.

## Smoke tests (Phase-4 merge gate)

- **github-oauth-callback-mints-session-and-stamps-orgs** —
  stub the GitHub OAuth provider + `GET /user/orgs`, drive the
  callback, assert the response sets the `sas_*` cookie and the
  resulting session's `Principal.extra.oauth.github_orgs`
  matches the stub.
- **out-of-org-github-user-signs-in-but-cannot-read-reports** —
  drive the OAuth callback for a user whose org list does not
  intersect `allow_orgs`; the user row exists, the session is
  minted, but `GET /reports/user/:id` returns `403
  awaiting_access` and an `auth.denied_org` audit row is
  written.
- **in-org-github-user-can-read-reports** — same flow with a
  user whose org list intersects `allow_orgs`; the same request
  returns `200` with a populated `ReportResponse`.
- **report-handler-echoes-resolved-window-and-data_as_of** —
  hit every report handler shape, assert the response carries
  the resolved UTC `Window` (with `tz`, `anchor`, `label`
  preserved) and a non-empty `DataAsOf`.
- **webhooks-github-not-principal-wrapped-but-rejects-bad-hmac**
  — `POST /webhooks/github` with no session cookie is reachable
  (the route is not behind `with_principal`); the same request
  with a wrong / missing HMAC signature returns 401 (Phase 2
  HMAC validator).
- **audit_log-row-written-per-protected-handler** — table-driven
  across the v1 action vocabulary; one call per handler, one
  row per call, action matches the pinned constant.
- **openapi-snapshot-stable** — `cargo test openapi_snapshot`
  passes against the committed `tests/openapi.snapshot.json`.
  A schema change without regenerating the snapshot fails.
- **with_principal-covers-every-non-webhook-non-auth-route** —
  hit every route without a session cookie / token; everything
  except `/webhooks/github` and the OAuth login / callback +
  session routes returns 401.
- **require_permission-covers-every-protected-route** — hit
  every route with a valid session whose
  `oauth.github_orgs` is empty; everything that is supposed to
  be authz-gated returns `403 awaiting_access`. A route that
  forgets `require_permission(...)` returns `200` and trips
  this test.
- **boundary-check-still-green** —
  `scripts/check-boundaries.sh` reports zero new `starter_*`
  imports in `dp-domain` / `dp-fetcher` / `dp-reports`, and
  only `starter_spi::MigrationSource` in `dp-store-pg`.
- **admin-user-export-streams-without-OOM** — seed an in-memory
  store with 100k events for one user, run the export handler
  consuming the response stream chunk-by-chunk, assert peak
  process memory stays under a fixed budget.

## Cross-cutting checks the runner must keep honest

- The protected-path list for `with_principal` lives in **one**
  array in `dp-server::build`. If a stage adds a route without
  also updating that array, the
  `with_principal-covers-every-non-webhook-non-auth-route` smoke
  catches it — do not weaken the smoke to make a new route pass.
- The `audit_log` writer is **one** helper. A second writer in
  any handler means the schema can drift per-call; refactor
  immediately on sight.
- The OpenAPI document is **one** `DevPulseApi`. A second
  `#[derive(OpenApi)]` anywhere in `dp-rest` is a bug; the
  snapshot test will surface it as soon as Phase 7 generates a
  client.
- The boundary script runs in the per-stage closing trio's
  `checks` todo. Pushing a stage that breaks the boundary is
  wasted work — catch it locally.
- `dp-rest` never imports `dp-store-pg` directly. New handler
  projections go through `dp-domain::store::Store` first.
- No `with_principal` exclusion grows beyond `POST
  /webhooks/github` + the `starter-auth-users` session routes.
  A third exclusion is a design decision that needs a SCOPE
  update, not a stage-level shortcut.
