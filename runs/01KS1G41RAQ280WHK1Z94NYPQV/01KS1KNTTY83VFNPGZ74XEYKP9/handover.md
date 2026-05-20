## Done

- Added `crates/dp-server/src/lib.rs` defining `AppState` (Store/Scheduler/Authenticator/PolicyEngine/WebhookSecretSource/Registry/StandardMetrics), `BuildConfig`, `BuildError`, and `build(cfg)`. The function merges `dp_rest::{reports,directory,admin}_router` into one fragment wrapped by `starter_server::auth::with_principal` (via a `BoxedAuthenticator` newtype to satisfy `A: Sized`), and merges `dp_fetcher::webhook::router` + `starter_auth_users::routes::auth_router::<()>` + `starter_auth_oauth::routes::oauth_router::<()>` outside that wrap. Final assembly via `ServerBuilder::<()>::new(()).…with_openapi(DevPulseApi::openapi()).with_metrics(registry, metrics).build()`.
- Added `starter-auth-oauth` and `starter-authz` to the workspace `[workspace.dependencies]` and to `crates/dp-server/Cargo.toml`. dp-server now depends on `starter-server`, `starter-auth-users`, `starter-auth-oauth`, `starter-authz`, `starter-spi`, `starter-observability`; no other dp-* crate gained starter-* deps.
- Added `crates/dp-server/src/tests.rs` with two unit tests: `boxed_authenticator_forwards_verify_to_inner` (verifies the dyn→Sized wrap calls the inner authenticator exactly once and passes the credential through) and `build_error_wraps_prometheus_error` (pins the `From<prometheus::Error>` conversion the `?` on webhook-metric registration depends on). Both pass.
- `cargo build --workspace` clean, `cargo test -p dp-server` green (2/2), `scripts/check-boundaries.sh` green (zero starter_* imports leaked into dp-domain / dp-fetcher / dp-reports).
- Commit `5765bec` on branch `codeless/phase-4-http-auth-openapi`.

## Next

- (none — stage 9 is a fresh-session pickup per the job contract)

## What you need to know

- `ServerBuilder` is parameterised `<()>`, not `<AppState>`, because every dp-rest fragment already carries its own per-router `State<…>` via `Router::with_state(...)`. `AppState` in dp-server is a value-level dependency bundle, not axum state — it threads things into `build()` so the bin layer hands in one struct.
- `with_principal` from starter is a Router-level layer (no path filter). The "filter" the TODO §Phase-4 stage-8 prose describes is implemented set-theoretically: protected ⊇ {reports/*, users, orgs, teams, home-org, admin/*} (these merge into the wrapped fragment); unprotected ⊇ {webhooks/github, auth/*, auth/oauth/*} (these merge outside the wrap).
- The webhook receipt-to-200 histogram is registered against the shared `Arc<Registry>` inside `build()` — it is the only side-effect `build()` has on the registry. Calling `build()` twice with the same registry fails with `BuildError::Metrics` (a name collision); the test above pins that behaviour.
- Stage-8 deliberately does not exercise a live `/health` over a `TestApp::spawn` because that requires real `UserStore` / `SessionStore` / `TokenStore` / `IdentityStore` / `OAuthStateStore` / `OAuthProvider` impls (sqlite-only in starter). That coverage belongs to the bin stage (9+).
- The `auth_router` / `oauth_router` functions in starter are referred to as `session_router` / `github_router` in the stage description — the names differ but the behaviour is what TODO §Phase-4 stage 8 specifies; the dp-server lib doc comment notes the alias.

## Open questions

- The stage description references a `StaticRbacEngine` policy file at `crates/dp-server/policy/dev-pulse.toml`. dp-server accepts `Arc<dyn PolicyEngine>` as an input on `AppState` but does not load the policy file itself — that wiring is left for the bin stage (and the policy file is not in-tree yet). Confirm in stage 9 that the bin loads it via `AuthzConfig::from_path(...)` + `StaticRbacEngine::from_config(...)`.
- `require_permission(resource, action)` is mentioned in the TODO header as a per-route guard but is *not* invoked from this stage — every protected handler currently relies on `with_principal` plus its own audit row. The per-handler `require_permission` decoration is the natural follow-up if a stage 9–11 spec calls for it; otherwise the `PolicyEngine` lives only at the future `/authz/check` debug surface.
