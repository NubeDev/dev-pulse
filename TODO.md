# dev-pulse — Build plan (consuming `starter` as a library)

> Reference: [SCOPE.md](SCOPE.md) for product scope, and
> `/home/user/code/rust/starter/DOCS/howto/using-starter-as-a-library.md`
> for the consumer rules. The canonical example to mimic is
> `starter/examples/notes/`.

The hard rule: **we never edit `crates/starter-*`**. Every surface
(REST, MCP, CLI, auth, storage, migrations, observability, client) is
composed from public `starter-*` APIs. If we need a feature starter
doesn't expose, we build it in `dev-pulse` — not inside starter.

---

## 0. Decisions to lock BEFORE Phase 1

These ripple through schema, ingestion architecture, and report
correctness. Settling them after scaffolding means painful retrofits.

### 0.1 Ingestion: webhooks primary, scheduler reconciles

The original "scheduled fetcher only" model (SCOPE §10) does not
survive contact with ~1000 repos. Polling PRs / reviews / issues /
workflow_runs / commits every 4h burns rate-limit budget on
near-empty diffs *and* leaves a 4h freshness lag that undercuts
SCOPE §11.7's "data as of" trust story.

Architecture:

- **Primary** — GitHub App webhooks (`pull_request`, `pull_request_review`,
  `pull_request_review_comment`, `issues`, `issue_comment`,
  `push`, `workflow_run`, `deployment`, `release`, `member`,
  `membership`, `team`). Webhook handler validates HMAC, enqueues
  to an inbox table, returns 200 fast.
- **Worker** — drains the inbox, applies idempotent upserts, marks
  `received_at` and `processed_at`. Replay-safe (GitHub redelivers
  on failure).
- **Reconciler (the old scheduler)** — runs every 4h, but now only
  does (a) diff-detection against the local store to catch missed
  webhooks, (b) backfill of resources GitHub doesn't webhook (e.g.
  historical commits), (c) membership/team drift. Same idempotent
  upserts as the webhook path.
- **Backfill** — separate one-shot job at install time, bounded
  window (default 90 days, configurable), paced against rate limits.

This keeps `data_as_of` close to real-time for the live feed while
still satisfying SCOPE §10's "page loads never call GitHub" rule.

### 0.2 Schema: multi-actor events from day one

SCOPE §6 explicitly calls out co-authored commits, multi-reviewer
PRs, and squash-merge author/committer split. A
`activity_events(user_id, ...)` mono-attribution column forces us
to either drop co-authors or duplicate rows (double-counting).
Neither is acceptable for SCOPE §11.4 trust.

Decision: split actor out of the event row.

- `activity_events(id, org_id, repo_id, kind, ts, payload jsonb,
  external_id)` — one row per real GitHub event.
- `event_actors(event_id, user_id, role)` — many rows per event.
  `role` ∈ `author`, `co_author`, `committer`, `merger`,
  `reviewer`, `commenter`, `assignee`, `requester`, `closer`.
- Reports `JOIN event_actors` and filter by role(s) per metric
  (e.g. "commits authored" filters `role IN (author, co_author)`).
- De-dup in the "all orgs combined" lens (SCOPE §8.1) operates on
  `(user_id, event_id)` pairs, not on events alone.

### 0.3 Resumable cursors: per (org, repo, resource_kind)

A single global cursor on `fetch_runs` forces full re-pulls on any
failure. Replace with:

- `fetch_cursors(org_id, repo_id, resource_kind, since, etag,
  last_event_id, updated_at)`.
- Webhook worker is cursor-less (event-driven). Reconciler and
  backfill both read/write here. `etag` lets us hit GitHub's
  conditional GETs (cheap on no-change polls).
- `fetch_runs` remains a run log (SCOPE §10 ops requirement) but
  is not the cursor.

### 0.4 Time-zone semantics for windows

"Last week" is ambiguous across audiences. Lock it in the window
contract, not the frontend.

- All event timestamps stored as `timestamptz` in UTC.
- Window contract: `{ start: tstz, end: tstz, label: string,
  tz: IANA, anchor: "viewer" | "org" | "utc" }`.
- Reports compute the start/end server-side from `(label, tz,
  anchor)`. "Last week" with `anchor: viewer` means
  Mon 00:00 → Sun 23:59:59 in the viewer's IANA TZ, converted to
  UTC for the query.
- Default `anchor` per audience: managers → `org` (their primary
  org's TZ), individuals → `viewer`, execs → `utc` (cross-company).
- The report response echoes the resolved UTC window so the UI
  can label it unambiguously.

### 0.5 GDPR deletion model

SCOPE §9 mandates per-user erasure. Decide the cascade before any
FK is written.

- `users.deleted_at timestamptz NULL` — soft-delete by default.
- On deletion request: pseudonymise the `users` row (replace
  login/email/name with `deleted-user-<hash>`), keep the id stable
  so historical reports don't break referential integrity.
- `event_actors` rows are **kept** but the user is effectively
  anonymised via the users-row pseudonymisation. Aggregate counts
  remain correct; per-person identification is gone.
- `audit_log.actor_user_id` keeps the id (legal-defensibility);
  rendering layer resolves it through `users` so a deleted user
  shows as anonymised in the UI.
- Hard-delete is a separate admin operation (legal hold escape
  hatch), removes `event_actors` rows for the user and rewrites
  affected aggregates. Documented but not in the v1 UI.
- A `data_export(user_id)` endpoint joins users → memberships →
  event_actors → events to produce a JSON dump (SCOPE §9 access
  right).

### 0.6 Boundary rule, made enforceable

The previous "only edge crates import `starter_*::`" rule
contradicted itself because `dp-store-pg` must build
`starter_spi::MigrationSource` values to feed `migrate(&pool)`.

Refined rule:

- `dp-domain`, `dp-fetcher`, `dp-reports` — **zero** `starter_*`
  imports. Enforced by `scripts/check-boundaries.sh` (CI):
  ```
  ! git grep -nE '^\s*use\s+starter_' \
      crates/dp-domain crates/dp-fetcher crates/dp-reports
  ```
- `dp-store-pg` — may import **only** `starter_spi::MigrationSource`
  and `starter_spi`'s zero-dep contract types. Anything else is a
  CI failure. Same script greps with an allowlist.
- `dp-server`, `dp-rest`, `dp-mcp`, `dp-cli`, `dev-pulse` bin —
  unrestricted.

This is checked in CI from Phase 0, not aspirational prose in the
DoD.

---

## 1. Dependency selection

Pick once, up-front, and don't mix:

- **Store** — `starter-store-postgres`.
  - Why: SCOPE §13 scale (~10k events/day, ~20 orgs, ~1000 repos,
    multi-user concurrent reads), plus webhook inbox throughput
    (§0.1), is past sqlite comfort. Postgres also the path of
    least resistance for hosted deployment (SCOPE §12).
- **Auth** — `starter-auth-users` (sessions + tokens + admin
  CLI) plus `starter-auth-oauth` (GitHub provider) plus
  `starter-authz` (policy engine, allow-list gate).
  - Why: SCOPE §7 has three distinct audiences — token-only
    (single owner) doesn't fit. Audit-log "who-viewed-what" (SCOPE
    §9) needs a real user identity per request. Operator login
    is GitHub OAuth (`starter-auth-oauth` auto-provisions the
    user row on first callback and mints the same `sas_*`
    session local login mints, so the rest of the stack is
    unchanged). Email+password signup stays
    `SIGNUP_MODE=disabled`; the CLI-seeded admin from
    `starter-auth-users::admin::create-admin` is the
    break-glass path. Access is gated by `starter-authz` with
    a single allow rule keyed on `oauth.github_orgs intersects
    auth.github.allow_orgs` (e.g. `["NubeIO", "ACME"]`),
    configured in `dp-config` not the policy file so adding an
    org is a config bump not a code change. Out-of-org users
    get a row + `auth.denied_org` audit entry but every
    protected request returns `403 awaiting_access`.
- **Secrets** — `starter-secrets-file` (age-encrypted file).
  - Why: GitHub App private key + webhook secret + DB URL belong
    in a secrets backend; file is portable across self-hosted and
    SaaS (SCOPE §12 hosting open question).
- **Config** — `starter-config`.
  - Why: schedule interval, backfill window, TZ defaults (§0.4),
    rate-limit caps all live here. Referenced from Phase 2.
- **MCP** — `starter-mcp` with `http` feature.
  - Why: lets an LLM agent answer "what did X do last week"
    without re-implementing the report query layer.
- **Observability** — `starter-observability` (tracing + metrics).
  - Why: SCOPE §10 requires per-run logs and freshness timestamps;
    we get tracing + Prometheus for free.
- **CLI** — `starter-cli` (we build our own `main.rs`, register
  commands into `CommandRegistry`).
- **Client** — `starter-client-rs` for internal tooling;
  `@nube/starter-client-ts` + `@nube/starter-ui-kit` +
  `@nube/starter-ui-core` for the frontend.

Explicitly **not used** from starter: gRPC (none shipped),
background job runner (none shipped — see §5).

---

## 2. Crate / workspace layout

A small Cargo workspace inside `dev-pulse/`:

- `crates/dp-domain` — entities and the `Store` trait. **Plain
  Rust, zero `starter-*` imports** (§0.6). Models: `User`, `Org`,
  `Team`, `Membership`, `Repo`, `ActivityEvent`, `EventActor`,
  `FetchRun`, `FetchCursor`, `WebhookDelivery`, `Window`.
- `crates/dp-store-pg` — Postgres impl of the `Store` trait. Owns
  `PgPool`, queries, SQL under `migrations/dp/`. May import
  `starter_spi::MigrationSource` and nothing else from starter
  (§0.6).
- `crates/dp-fetcher` — ingestion. Subdivides into:
  - `webhook` — HMAC validation, inbox enqueue, worker drain.
  - `reconciler` — 4h diff against local store + cursor-driven
    backfill of non-webhooked resources.
  - `backfill` — one-shot, bounded-window, rate-paced.
  - Octocrab client wrapper with rate-limit pacing in **one**
    place.
- `crates/dp-reports` — report query layer. Takes `Window`
  (§0.4) + scope + group_by + agg, returns rows. Implements the
  three org-scope lenses (SCOPE §8.1) with `event_actors`-aware
  de-dup (§0.2).
- `crates/dp-rest` — axum `Router<AppState>`. utoipa-annotated
  handlers. Generic over `S: Clone + Send + Sync` per starter.
- `crates/dp-mcp` — `impl Tool` per query: `user_activity`,
  `team_activity`, `home_org_split`, `freshness`. Registered into
  a `ToolRegistry`.
- `crates/dp-cli` — `impl Command` for: `fetch-now`, `backfill`,
  `home-org set/list/clear`, `user list`, `org list`. Thin HTTP
  clients against our own server.
- `crates/dp-server` — composes everything: `ServerBuilder`,
  `merge_router`, `with_openapi`, `with_metrics`,
  `with_principal`, `mcp_router`.
- `dev-pulse` (bin) — `main.rs` only. clap top-level with
  `migrate`, `serve`, `fetch-now`, `backfill`, `claim`, plus
  `registry.subcommands()`.
- `frontend/` — Vite + React app using `StarterClient`,
  `<AuthProvider>`, `tokenStrategy`, `@nube/starter-ui-kit`.

Boundary enforcement: `scripts/check-boundaries.sh` (§0.6) wired
into CI from Phase 0.

---

## 3. Phased build plan

Honest estimates. Webhook + multi-actor + cursor + TZ +
deletion-cascade all roll into Phases 1–2 and inflate them.

### Phase 0 — bootstrap (1–2 days)

- [x] Add `dev-pulse` to a workspace `Cargo.toml`. Pin `starter-*`
      via `path = "../starter/crates/<name>"`.
- [x] Copy `examples/notes/Cargo.toml` as a starting point; swap
      `sqlite` → `postgres`, `auth-token` → `auth-users`, drop
      tonic/prost/build.rs.
- [x] Scaffold the crates listed in §2 (empty `lib.rs` each).
- [x] Wire `starter_observability::tracing::init` in `main.rs`.
- [x] Land `scripts/check-boundaries.sh` and a CI job that runs it
      (§0.6).

### Phase 1 — domain + store + migrations (4–5 days)

- [x] Write `dp-domain`: entity types and the `Store` trait
      (`upsert_user`, `record_event`, `add_event_actors`,
      `list_event_actor_rows_in_window`, `set_home_org`,
      `pseudonymise_user`, `get_cursor`, `put_cursor`,
      `enqueue_webhook`, `claim_webhooks`, …). Zero `starter-*`
      imports. Mirror
      [domain.rs](file:///home/user/code/rust/starter/examples/notes/src/domain.rs).
- [x] Write `dp-store-pg`: implement `Store` against a `PgPool`.
- [x] Schema (SCOPE §5/§6/§4.1 + §0.2/§0.3/§0.5):
      - `users(id, github_id, login, email, name, deleted_at)` —
        soft-delete + pseudonymisation per §0.5.
      - `orgs`, `teams`, `repos`.
      - `memberships(user_id, org_id, role, home_org, joined_at)`.
      - `activity_events(id, org_id, repo_id, kind, ts timestamptz,
        external_id, payload jsonb)` — **no** `user_id` column.
      - `event_actors(event_id, user_id, role)` — multi-actor
        attribution (§0.2). Composite PK `(event_id, user_id,
        role)`.
      - `issues(... CRUD fields per SCOPE §4.1)`.
      - `webhook_inbox(id, delivery_id unique, event, payload jsonb,
        received_at, processed_at, error)`.
      - `fetch_cursors(org_id, repo_id, resource_kind, since, etag,
        last_event_id, updated_at)` — composite PK
        `(org_id, repo_id, resource_kind)` (§0.3).
      - `fetch_runs(id, kind, started, finished, items, errors,
        partial)` — `kind ∈ webhook_worker, reconciler, backfill`.
      - `audit_log(actor_user_id, action, target, at)`.
- [x] Mandatory indexes (medium issue, called out explicitly):
      - `event_actors(user_id, event_id)`
      - `activity_events(org_id, ts DESC)`
      - `activity_events(repo_id, ts DESC)`
      - `activity_events(kind, ts DESC)`
      - `event_actors_event_actors_ts` covering index via join:
        materialised as `event_actor_facts(user_id, org_id, repo_id,
        kind, ts, role)` if `EXPLAIN` shows the join is too hot.
        Decide at first 10k-event load test, not up-front.
      - `webhook_inbox(processed_at) WHERE processed_at IS NULL`
        (partial index for the worker drain).
- [x] Migrations under `crates/dp-store-pg/migrations/dp/`. Build
      a `sources()` returning `[starter_auth_users, dp]` per the
      starter migrations namespacing rule (consumer rules §4.5).

### Phase 2 — ingestion (2–3 weeks)

Was 3–5 days. Honest estimate after §0.1/§0.3 is much larger.

- [ ] Octocrab client wrapper with rate-limit pacing in **one**
      place. Respects `X-RateLimit-*` and conditional GETs (etag).
- [ ] **Webhook receiver** — axum route, HMAC validation against
      `starter-secrets-file` secret, enqueue to `webhook_inbox`,
      return 200 in under 100ms.
- [ ] **Webhook worker** — drains `webhook_inbox`, applies
      idempotent upserts via `external_id`, fans out multi-actor
      events into `event_actors` rows.
- [ ] Co-author / squash-merge / bot / unattributed handling per
      SCOPE §6 caveats — explicit unit tests for each, against
      recorded GitHub fixture payloads.
- [ ] **Reconciler** — 4h-default tokio interval, configurable via
      `starter-config`. Per-(org, repo, resource_kind) cursor
      pagination. Detects events missing from local store
      (missed webhooks) and fills them in.
- [ ] Scheduler: `Mutex<Option<JoinHandle>>` for coalescing;
      `fetch-now` CLI + HTTP trigger reuses the same path.
- [ ] **Backfill** — one-shot per org, bounded historical window
      (default 90 days, configurable), separately paced.
- [ ] Run-log entries written for every webhook batch, reconciler
      tick, backfill chunk.

### Phase 3 — reports (4–6 days)

Inflated from 2–3 days because de-dup is now `event_actors`-aware
(§0.2) and windows go through the TZ contract (§0.4).

- [ ] `dp-reports`: every report accepts the same envelope:
      `(orgs, users, teams, window: Window, scope_mode, group_by,
      activity_types, actor_roles)`.
- [ ] Resolve `Window` → UTC `(start, end)` server-side per §0.4.
      Echo the resolved window back in the response.
- [ ] Implement the **three org-scope lenses** (SCOPE §8.1) with
      correct de-dup on "all orgs combined". De-dup operates on
      `(user_id, event_id)`, **not** event row alone — tested
      against fixtures with co-authored commits across two orgs.
- [ ] Aggregation: counts for events (filtered by `actor_roles`),
      p50/p90/p95 for durations (`percentile_cont`). No means.
- [ ] `data_as_of` semantics: per-response object
      `{ webhook_latest: tstz, reconciler_latest: tstz,
         per_org: { org_id: tstz } }`. UI picks which to render
      per lens (single-org → that org's latest; all-orgs-combined
      → `min(per_org)`; per-org split → per row).
- [ ] Spot-check harness: fixture comparing our numbers vs a
      recorded GitHub response (SCOPE §11.4 trust requirement).

### Phase 4 — HTTP + auth + OpenAPI (2–3 days)

- [ ] `dp-rest`: handlers for `GET /reports/...`, `GET /users`,
      `GET /orgs`, `POST /home-org`, `POST /admin/refresh`,
      `POST /webhooks/github`, `GET /admin/runs`,
      `POST /admin/users/:id/anonymise`,
      `GET /admin/users/:id/export`.
- [ ] utoipa annotations + `#[derive(OpenApi)] DevPulseApi`. Hand
      to `ServerBuilder::with_openapi` (rule §6.7 — consumer owns
      the doc).
- [ ] `dp-server::build()`:
      `ServerBuilder::new(AppState).merge_router(dp_rest::router())
       .merge_router(starter_auth_users::routes::session_router(...))
       .merge_router(starter_auth_oauth::routes::github_router(...))
       .with_openapi(...).with_metrics(...)`.
- [ ] GitHub OAuth operator login via `starter-auth-oauth`
      (first-callback auto-provisions the user row + mints the
      standard `sas_*` session). Local email+password signup
      stays `SIGNUP_MODE=disabled`; CLI admin is the break-glass
      path.
- [ ] `github_orgs` attribute stamper writes
      `Principal.extra.oauth.github_orgs` on session mint via
      one octocrab `GET /user/orgs` call (cached on the session,
      refreshed per `auth.github.org_refresh_interval`).
- [ ] `starter-authz` `StaticRbacEngine` loaded from
      `crates/dp-server/policy/dev-pulse.toml` with one allow
      rule keyed on `oauth.github_orgs intersects
      auth.github.allow_orgs`. The allow-list (`["NubeIO",
      "ACME"]` etc.) lives in `dp-config`. Every protected route
      decorated with `require_permission(<resource>,
      <action>)`. Out-of-org users get a row + `auth.denied_org`
      audit entry but every protected request returns `403
      awaiting_access`.
- [ ] Wrap protected routes with
      `starter_server::auth::with_principal` (auth) +
      `require_permission` (authz) and the
      `Arc<dyn Authenticator>` from `starter-auth-users`.
      `/webhooks/github` is **not** principal-wrapped — it
      authenticates via HMAC. The OAuth login / callback +
      `starter-auth-users` session routes are also outside
      `with_principal` (they authenticate themselves).
- [ ] Every report response includes the `data_as_of` object
      (SCOPE §11.7, §0.3 of this doc).
- [ ] Every protected handler writes to `audit_log` (SCOPE §9)
      via one `dp_rest::audit::record()` helper; v1 action
      vocabulary `{report.read, home_org.set, admin.refresh,
      user.anonymise, user.export, runs.list, auth.signed_in,
      auth.denied_org}`.

### Phase 5 — MCP (1–2 days)

- [ ] `dp-mcp`: `impl Tool` for `user_activity`, `team_activity`,
      `home_org_split`, `freshness`. JSON schemas reflect the
      report envelope from Phase 3 (including `Window` shape).
- [ ] Mount via `mcp_router::<AppState>(tools,
      McpHttpOptions::new().with_auth(auth))` — same
      `Authenticator` as HTTP (rule §6.8).

### Phase 6 — CLI (1–2 days)

- [ ] `dp-cli`: `impl Command` for `fetch-now`, `backfill`,
      `home-org set`, `home-org list`, `user list`, `org list`,
      `user anonymise`, `user export`.
- [ ] In `main.rs`:
      `CommandRegistry::new().register_starter_defaults().register(...)`.
      Top-level clap with `migrate | serve | claim | <registry>`.

### Phase 7 — frontend (5–7 days)

- [ ] Vite + React app under `frontend/`. Reuse `StarterClient`,
      `<AuthProvider>`, `tokenStrategy`, `@nube/starter-ui-kit`.
- [ ] Pages: per-user, per-team, per-org, home-org split,
      freshness, run-log admin.
- [ ] Every report page renders the **headline + table + trend**
      shape (SCOPE §11.5) and the three-lens toggle (§8.1).
- [ ] TZ anchor selector (§0.4) on the window picker.
- [ ] "Data as of <ts>" surfaced per-lens per §0.3 of this doc.
- [ ] No leaderboard, no single-score affordance (SCOPE §4
      design constraint).

### Phase 8 — E2E + ops (3–4 days)

- [ ] `tests/e2e.rs` using `starter-server`'s `testing` feature.
      Fixtures: 3 orgs, overlapping users, **co-authored commits
      spanning two orgs**, validate de-dup correctness on all
      three lenses.
- [ ] Webhook replay test: send the same `delivery_id` twice,
      assert exactly one upsert.
- [ ] Prometheus scrape via `with_metrics`. Dashboards:
      webhook lag (received → processed), reconciler tick health,
      rate-limit headroom, fetch-run errors.
- [ ] Run-log endpoint + UI surface for last N runs of each kind.
- [ ] Privacy hooks: anonymise + export endpoints, with audit
      log entries.

---

## 4. Mapping `starter` extension points to dev-pulse

| Starter point (consumer rules §4)         | Where we use it                                       |
|-------------------------------------------|-------------------------------------------------------|
| `ServerBuilder::merge_router`             | `dp-server` merges `dp_rest::router` + auth routes    |
| `with_openapi`                            | `dp-server` passes `DevPulseApi::openapi()`           |
| `with_metrics`                            | `dp-server` mounts Prometheus registry                |
| `with_principal` / `Authenticator`        | wraps all `/reports`, `/admin`, `/home-org`           |
| `Tool` + `ToolRegistry` + `mcp_router`    | `dp-mcp`                                              |
| `Command` + `CommandRegistry`             | `dp-cli`, dispatched from `main.rs`                   |
| `MigrationSource` + `migrate(&pool)`      | `dp-store-pg::sources()` (only starter import here)   |
| `starter-client-rs` / `starter-client-ts` | `dp-cli` over HTTP; frontend                          |
| `starter-observability::tracing::init`    | `main.rs` first line                                  |
| `starter-config`                          | schedule, backfill window, TZ defaults                |
| `starter-secrets-file`                    | GitHub App key, webhook HMAC secret, DB URL           |

If a row above ever needs a new starter API, that is a signal to
either (a) compose around it in dev-pulse, or (b) raise it as a
starter change in a separate PR against the starter repo —
**never** inline-edit from here.

---

## 5. Things starter does **not** give us (build in dev-pulse)

Per consumer rules §5 and SCOPE §10:

- **Scheduler.** Starter has no job runner. We build the
  reconciler interval in `dp-fetcher::reconciler` using
  `tokio::time` + cancellation + a `Mutex<Option<JoinHandle>>`
  for coalescing.
- **Webhook receiver / worker.** Starter has no opinion on
  inbound integrations. `dp-fetcher::webhook` owns it.
- **GitHub client.** Bring `octocrab` at consumer level.
- **Domain.** All entities in SCOPE §5 are ours.
- **Report query language.** Lives in `dp-reports`.

---

## 6. Risks / open questions to resolve before Phase 1

Pulled forward from SCOPE §12 (and the new §0 decisions):

- [ ] **Auth choice** — confirm `starter-auth-users` +
      `starter-auth-oauth` (GitHub) + `starter-authz` covers the
      operator login + allow-list gate (Phase 4 SCOPE locks the
      bias). If a needed hook is missing from the starter
      crates, layer around it in `dp-server`; still no starter
      edit.
- [ ] **GitHub App vs PAT** — affects webhook availability (§0.1),
      rate-limit budget, and per-org install model. App is the
      working assumption.
- [ ] **Backfill bound** — default 90 days; confirm with first
      target deployment.
- [ ] **Per-org TZ source** — do we let admins set it, or infer
      from GitHub org profile / repo activity? Affects §0.4.
- [ ] **Materialised facts table** — decide after first load test
      whether the `event_actor_facts` table in §Phase 1 is needed.
- [ ] **Data residency** — Postgres region for SaaS deployment
      must satisfy SCOPE §9 jurisdictions.

---

## 7. Definition of done (v1)

Tracks SCOPE §11:

- [ ] Manager flow under 30s: open app → pick user → see
      last-week report across all orgs.
- [ ] All three org-scope lenses on every report, toggleable
      without re-running.
- [ ] Cross-company split on shared org as a single report.
- [ ] Co-authored commits across two orgs are de-duplicated
      correctly in the all-orgs-combined lens (regression test
      in `tests/e2e.rs`).
- [ ] Spot-checked numbers within tolerance vs GitHub UI.
- [ ] Every report follows headline + table + trend.
- [ ] Zero GitHub calls on page load — all data from local store.
- [ ] `data_as_of` rendered per lens per §0.3 of this doc.
- [ ] Webhook lag p95 under 30s in steady state.
- [ ] GDPR anonymise + export endpoints exercised end-to-end.
- [ ] Legal sign-off recorded before first prod deploy in any
      affected jurisdiction.
- [ ] **Zero edits to `crates/starter-*` or `packages/`** across
      the whole repo's git log. Enforced by
      `scripts/check-boundaries.sh` in CI from Phase 0 (§0.6) —
      not an aspirational invariant.
