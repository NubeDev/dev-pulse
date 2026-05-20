## Done

- crates/dev-pulse/src/main.rs: `serve --config <path>` subcommand loads a TOML config, validates `[auth.github]` before build, opens a Postgres pool for dp-data (PgStore) and a sidecar SQLite pool for starter-auth-users/oauth row families (migrations applied via `sqlx::migrate!` against `../../../starter/crates/starter-auth-{users,oauth}/migrations`), wires the `GithubOrgsStamper` over `OAuthPrincipalExtras` + `CachedGithubOrgsSource` (`StaticGithubOrgsSource` inner placeholder until Phase 6), constructs `AuthAuthenticator::new(...).with_principal_extras(stamper)`, builds the `GitHubProvider` + `OAuthRoutesState` + `AuthState`, loads `crates/dp-server/policy/dev-pulse.toml` via `load_static_engine` + `register_dev_pulse_resources`, wires `StaticSecrets::single` from a `resolve_secret`-resolved handle for the webhook HMAC source, builds an empty-target `Reconciler`+`Scheduler` (so `POST /admin/refresh` has a handle without a real GitHub App; tick loop dormant by default), registers `StandardMetrics` on a shared `prometheus::Registry`, calls `dp_server::build`, and runs `axum::serve(...).with_graceful_shutdown(ctrl_c)` with a `tokio::sync::watch` channel that also stops the optional scheduler-run task.
- crates/dev-pulse/Cargo.toml: adds dp-server / dp-fetcher / dp-store-pg / starter-{server,spi,store-postgres,store-sqlite,auth-users (sqlite),auth-oauth (sqlite),authz} / secrecy / sqlx / toml; starter-store-sqlite uses `features = ["testing"]` because starter-auth-oauth's `pub mod testing` references `starter_store_sqlite::testing::ephemeral` under the `sqlite` gate.
- root Cargo.toml: registers the `starter-store-sqlite` path dep in `[workspace.dependencies]`.
- crates/dev-pulse/config.example.toml: documents the TOML shape (server / postgres / auth_sqlite / webhook / auth.github / scheduler).
- Four unit tests in main.rs pin the seams: `resolve_secret_*` for the three secret-handle shapes (`secret://NAME` env, `file:/path` trimmed, literal) and `validate_runs_on_deserialised_github_config` regression for the boot-time `[auth.github].validate()` invariant. `cargo test --workspace` green; `scripts/check-boundaries.sh` reports OK.
- Committed as `1e3ff23` on `codeless/phase-4-http-auth-openapi`; title starts with `stage 10: …`.

## Next

- Stage 11 of 11 — the final Phase 4 gate (job SCOPE / WORKFLOW name it; the stage-10 description does not).

## What you need to know

- Operator login is GitHub OAuth via starter-auth-oauth; first-callback auto-provisions the user row (signup_enabled = true) and the `AwaitingAccessEngine` rewrites the deny reason so out-of-org users see `403 awaiting_access`. Local email+password signup stays `SignupMode::Disabled` (`AuthState::new` default).
- Phase 6 owns the rest of the clap registry (`migrate`, `fetch-now`, `backfill`, `claim`, `<registry>`) plus the real GitHub App-installation Client + TargetProvider that lets the scheduler tick loop run usefully. The Phase 4 bin ships with `scheduler.enable = false` so the dormant tick loop does not hammer the API with a token-less Client.
- `resolve_secret` is a temporary shim until `starter-secrets-file` (age-based) is wired in Phase 6: `secret://NAME` reads env var `NAME` (upper-cased, `/` → `_`); `file:/path` reads + trims; anything else is a literal.
- starter-store-sqlite is forced to its `testing` feature because starter-auth-oauth's testing module is unconditionally compiled when `sqlite` is enabled. If starter ever splits the testing helpers out, the dev-pulse `features = ["testing"]` line can be dropped.
- Smoke targets (`dev-pulse serve --config test.toml`, `curl /openapi.json`, `curl /auth/oauth/github/login`, in-org session → ReportResponse, out-of-org → 403 awaiting_access) are end-to-end and need a real Postgres + a real GitHub OAuth App — they are operator-run against staging, not in CI.

## Open questions

- (none)
