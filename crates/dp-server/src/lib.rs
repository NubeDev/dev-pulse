//! `dp-server` — composition root for dev-pulse.
//!
//! This crate is the **only** dev-pulse crate that imports the
//! starter composition pieces (`starter-server`, `starter-auth-users`,
//! `starter-auth-oauth`, `starter-authz`, `starter-observability`).
//! `dp-rest` stays "edge-allowed but impl-agnostic" — it operates on
//! the `dp_domain::Store` trait, an opaque `Principal` re-export, and
//! its own per-router state. `dp-server` picks the concrete
//! `Authenticator` (`AuthAuthenticator` bridging `sas_*` session
//! cookies and `sak_*` API tokens), the concrete `PolicyEngine`
//! (`StaticRbacEngine` loaded from `crates/dp-server/policy/dev-pulse.toml`
//! at the bin layer), and the concrete `WebhookSecretSource`
//! (file-backed in production via `starter-secrets-file`,
//! `StaticSecrets` in tests). dp-domain / dp-fetcher / dp-reports
//! remain free of starter-* imports per the §0.6 R-boundary.
//!
//! ## What [`build`] does, in order
//!
//! 1. Build each dp-rest router fragment from the supplied [`AppState`]
//!    (`reports_router`, `directory_router`, `admin_router`).
//! 2. Build the webhook receiver fragment from
//!    [`dp_fetcher::webhook::router`] — this is the **only** route
//!    intentionally outside [`with_principal`]; authentication is the
//!    HMAC the receiver verifies itself.
//! 3. Build the auth session router (`starter_auth_users::routes::auth_router`,
//!    aliased as the "session router" in TODO §Phase-4 stage 8) and
//!    the OAuth router (`starter_auth_oauth::routes::oauth_router`,
//!    the "github_router" of the same description). These authenticate
//!    themselves — `/auth/login`, `/auth/oauth/github/callback`, etc.
//!    cannot be behind `with_principal` because the user is *acquiring*
//!    a credential, not presenting one.
//! 4. Wrap **only** the report + directory + admin fragments in
//!    [`with_principal`]. The path-pattern filter the stage description
//!    references is implicit in *which* sub-router the layer wraps —
//!    starter's `with_principal` is a `Router`-level layer, so the
//!    filter is set-theoretic: protected ⊃ {reports/*, users, orgs,
//!    teams, home-org, admin/*}; unprotected ⊃ {webhooks/github,
//!    auth/*}.
//! 5. Hand everything to [`ServerBuilder`], which adds `/health`,
//!    `/metrics`, `/openapi.json`, request-id, latency, and CORS, and
//!    materialises the final `axum::Router`.
//!
//! Per-route audit (`audit_log` per SCOPE §9) is written by the
//! handlers themselves through `dp_rest::audit::record` — adding it at
//! the composition layer would require a typed-extractor middleware
//! and the per-handler approach has one decision point per route.
//!
//! ## Why `ServerBuilder<()>`
//!
//! Every dp-rest router fragment carries its own per-router state
//! (`Arc<AppState>`, `Arc<AdminState>`, `Arc<WebhookState>`) via
//! `Router::with_state(...)`, so each fragment is already a fully-
//! resolved `Router<()>` by the time it reaches the builder. The
//! `S` type parameter the starter builder is generic over carries no
//! information here, so `ServerBuilder::<()>::new(())` keeps the
//! types honest. The [`AppState`] struct this crate exposes is a
//! *value-level* dependency bundle (not axum state) — it threads the
//! `Store`, `Scheduler`, `Authenticator`, `PolicyEngine`,
//! `WebhookSecretSource`, prometheus `Registry`, and standard metrics
//! handle through `build()` so the bin layer has a single struct to
//! hand in.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod auth;

use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use prometheus::Registry;
use starter_observability::metrics::StandardMetrics;
use starter_server::auth::with_principal;
use starter_server::ServerBuilder;
use starter_spi::auth::{Authenticator, Principal};
use starter_spi::authz::PolicyEngine;
use starter_spi::Result as SpiResult;
use thiserror::Error;

use dp_domain::store::Store;
use dp_fetcher::reconciler::Scheduler;
use dp_fetcher::webhook::{self, WebhookMetrics, WebhookSecretSource, WebhookState};
use dp_rest::{
    admin_router, app_permissions_router, directory_router, inbox_router, issues_read_router,
    issues_write_router, pins_router, repos_router, reports_router,
    tags_router, AdminState, AppState as RestAppState, DevPulseApi,
};

// Re-export so the bin layer (which doesn't depend on dp-rest
// directly) can name the GitHub App config type for
// SCOPE-PROJECTS §13.6 `[github.app]`.
pub use dp_rest::GitHubAppConfig;
use utoipa::OpenApi;
use uuid::Uuid;

/// Middleware: bridges `starter_spi::auth::Principal` (string id)
/// into `dp_rest::audit::Principal` (Uuid id) so the handlers in
/// `dp-rest` can read the actor from request extensions.
async fn bridge_principal(
    mut req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if let Some(spi_principal) = req.extensions().get::<Principal>().cloned() {
        if let Ok(uuid) = Uuid::parse_str(&spi_principal.subject) {
            req.extensions_mut()
                .insert(dp_rest::Principal { actor_user_id: uuid });
        }
    }
    next.run(req).await
}

/// Value-level dependency bundle handed into [`build`].
///
/// Held by the bin layer between construction and `build()`. Every
/// field is an `Arc`-equivalent so `Clone` is cheap (the struct itself
/// is `Clone`-able if a future wiring step wants to keep a handle
/// after handing the build off).
#[derive(Clone)]
pub struct AppState {
    /// Persistence — every dp-rest handler reads through it; the
    /// webhook receiver enqueues through it; the reconciler writes
    /// through it.
    pub store: Arc<dyn Store>,
    /// Reconciler scheduler. `POST /admin/refresh` calls
    /// [`Scheduler::try_trigger_now`].
    pub scheduler: Arc<Scheduler>,
    /// Authenticator bridging both the `sas_*` session cookie path
    /// (minted by `starter-auth-users` on login *and* by
    /// `starter-auth-oauth` on first callback — Phase 4 §0 decision)
    /// and the `sak_*` API-token path. Concrete impl is
    /// `starter_auth_users::AuthAuthenticator` in production; tests
    /// wire a stub. Held as `Arc<dyn Authenticator>` so this struct
    /// stays impl-agnostic, then wrapped by [`BoxedAuthenticator`]
    /// at the [`with_principal`] seam (which requires `A: Sized`).
    pub authenticator: Arc<dyn Authenticator>,
    /// Authorisation engine. In production this is
    /// `starter_authz::StaticRbacEngine` loaded from
    /// `crates/dp-server/policy/dev-pulse.toml` with the one allow
    /// rule `oauth.github_orgs intersects auth.github.allow_orgs`
    /// over `resource = "*", actions = ["*"]`. Held as `dyn` so the
    /// composition root stays free to swap engines for tests.
    pub policy: Arc<dyn PolicyEngine>,
    /// Rotation-aware webhook secret source. The bin layer wires
    /// this over `starter-secrets-file`; tests use
    /// `dp_fetcher::webhook::StaticSecrets`.
    pub webhook_secret: Arc<dyn WebhookSecretSource>,
    /// Shared prometheus registry — the webhook receiver registers
    /// its receipt-to-200 histogram against it here, and the builder
    /// mounts `/metrics` from it.
    pub registry: Arc<Registry>,
    /// The standard request-count / request-duration / in-flight
    /// metrics. Required by `ServerBuilder::with_metrics` to drive
    /// the latency middleware.
    pub metrics: Arc<StandardMetrics>,
    /// GitHub App-side configuration carrying the SCOPE-PROJECTS
    /// §13.6 `request_issues_write` `dp-config` flag (and the App
    /// slug used to render the §13.6 migration banner's
    /// admin-copyable text + per-install deep-link). The §8.4
    /// write-gate (`dp_rest::require_issues_write`) and the
    /// `GET /me/app-install-banner` handler both read through it.
    /// Held as `Arc` so cloning [`AppState`] (the bin layer
    /// constructs it once and the build path moves it in) stays
    /// cheap.
    pub github_app: Arc<GitHubAppConfig>,
}

/// All the inputs [`build`] needs. Bundles [`AppState`] with the
/// fully-constructed `AuthState` and `OAuthRoutesState` the auth
/// session and GitHub-OAuth routers close over.
///
/// The bin layer is responsible for constructing the auth / oauth
/// states because they need access to `starter-auth-users::store`
/// implementations (a sqlite or postgres `UserStore` / `SessionStore`
/// / `TokenStore`) that are deployment-shaped, not composition-shaped.
pub struct BuildConfig {
    /// The shared application state.
    pub state: AppState,
    /// Auth session-router state (login / logout / me / signup). Per
    /// the Phase-4 §0 decision, `signup` is left at
    /// `SignupMode::Disabled` — the only way in is OAuth.
    pub auth: starter_auth_users::routes::AuthState,
    /// OAuth router state. Phase 4 ships the `github` provider
    /// behind `signup_enabled = true` so the first callback for a
    /// previously-unknown GitHub identity auto-provisions the
    /// operator row in `starter-auth-users` and mints a `sas_*`
    /// session — the same cookie a local-login would mint. Out-of-
    /// org users get a row + audit row but every protected request
    /// returns `403 awaiting_access` (the `oauth.github_orgs
    /// intersects auth.github.allow_orgs` rule fails).
    pub oauth: starter_auth_oauth::routes::OAuthRoutesState,
}

/// Failure modes that surface before the server starts.
#[derive(Debug, Error)]
pub enum BuildError {
    /// Prometheus rejected one of the metric handles — typically a
    /// name collision because [`AppState::registry`] was already
    /// passed to another caller that registered the webhook
    /// histogram on it.
    #[error("metric registration failed: {0}")]
    Metrics(#[from] prometheus::Error),
}

/// Build the dev-pulse `axum::Router`.
///
/// See module docs for the step-by-step. Returns a fully-assembled
/// router the bin layer hands to `axum::serve(listener, router)`.
pub fn build(cfg: BuildConfig) -> Result<Router, BuildError> {
    let BuildConfig { state, auth, oauth } = cfg;
    let AppState {
        store,
        scheduler,
        authenticator,
        // The policy engine handle is shared as an axum
        // Extension on the protected fragment below (per
        // `starter_authz::require_permission`'s middleware
        // contract — it pulls `Arc<dyn PolicyEngine>` out of
        // request extensions). dp-rest's per-route
        // `require_permission(<resource>, <action>)` layer is
        // what *invokes* the engine; this layer just makes it
        // visible.
        policy,
        webhook_secret,
        registry,
        metrics,
        github_app,
    } = state;

    // -----------------------------------------------------------------
    // dp-rest routers — protected fragment.
    //
    // Each fragment has its own per-router `State<…>` set inside the
    // dp-rest constructor; the resulting `Router` is `Router<()>` and
    // composes into the `ServerBuilder<()>` accumulator below.
    // -----------------------------------------------------------------
    let rest_state = Arc::new(
        RestAppState::new(store.clone()).with_github_app(github_app.clone()),
    );
    let admin_state = Arc::new(AdminState::new(scheduler.clone(), store.clone()));

    let reports = reports_router(rest_state.clone());
    let directory = directory_router(rest_state.clone());
    let pins = pins_router(rest_state.clone());
    let tags = tags_router(rest_state.clone());
    let repos = repos_router(rest_state.clone());
    let issues_read = issues_read_router(rest_state.clone());
    // SCOPE §18 / SCOPE-PROJECTS §8 — the issue write surface.
    // Gated on `(issues, write)` inside the router fragment; the
    // default `UnconfiguredIssueWriter` on `AppState` refuses every
    // call until the bin layer wires an octocrab-backed backend.
    let issues_write = issues_write_router(rest_state.clone());
    let inbox = inbox_router(rest_state.clone());
    // SCOPE-PROJECTS §13.6 — banner + write-gate live in the same
    // dp-rest module; the router fragment registers
    // `(github_app, read)` so the §15.11 access gate is the only
    // visibility check (out-of-org users get the empty `orgs`
    // list, not a 403).
    let github_app_routes = app_permissions_router(rest_state);
    let admin = admin_router(admin_state);

    let protected = Router::new()
        .merge(reports)
        .merge(directory)
        .merge(pins)
        .merge(tags)
        .merge(repos)
        .merge(issues_read)
        .merge(issues_write)
        .merge(inbox)
        .merge(github_app_routes)
        .merge(admin);

    // Hand the policy engine down via Extension. The per-route
    // `require_permission(...)` layers in dp-rest pull this out
    // of request extensions; without it every protected request
    // would 403 with `engine_missing` (per
    // `starter_authz::middleware::gate`'s fail-closed branch).
    let protected = protected.layer(axum::Extension(policy.clone()));

    // `with_principal` wants an `Arc<A: Authenticator + Sized>` — the
    // dyn-trait handle becomes a `BoxedAuthenticator` newtype.
    // Layer order matters: `with_principal` MUST wrap *outside*
    // the policy-extension layer so the SPI `Principal` it
    // attaches is visible to `require_permission` (which reads
    // both the principal and the engine from extensions).
    let boxed_auth: Arc<BoxedAuthenticator> =
        Arc::new(BoxedAuthenticator(authenticator.clone()));
    // Bridge: starter-server's `with_principal` attaches a
    // `starter_spi::auth::Principal` (string `subject`). dp-rest
    // handlers read `dp_rest::audit::Principal` (Uuid
    // `actor_user_id`). Without this layer every protected request
    // 500s with "Missing request extension". The bridge is a thin
    // axum middleware that reads the upstream Principal, parses
    // `subject` as Uuid, and attaches the downstream Principal.
    //
    // Layer order matters: bridge must run *inside* `with_principal`
    // (i.e. attached to the inner router before wrapping), so by the
    // time bridge runs, the SPI Principal is already in extensions.
    let protected = protected.layer(axum::middleware::from_fn(bridge_principal));
    let protected = with_principal(protected, boxed_auth);

    // -----------------------------------------------------------------
    // Webhook receiver — outside `with_principal`.
    //
    // Auth on `POST /webhooks/github` is the HMAC the receiver checks
    // itself. The receipt-to-200 histogram is registered onto the
    // shared registry here (the only side-effect of `build()` on the
    // registry).
    // -----------------------------------------------------------------
    let webhook_metrics = WebhookMetrics::register(&registry)?;
    let webhook_state = Arc::new(WebhookState::new(
        store.clone(),
        webhook_secret,
        webhook_metrics,
    ));
    let webhook_router = webhook::router(webhook_state);

    // -----------------------------------------------------------------
    // Auth + OAuth routers — also outside `with_principal`.
    //
    // The handlers under `/auth/*` and `/auth/oauth/*` authenticate
    // themselves: a login handler is *issuing* a credential, not
    // checking one. `auth_router` and `oauth_router` are generic over
    // the parent state type; with `()` the merged result is
    // `Router<()>` and composes into the `ServerBuilder<()>` below.
    //
    // Naming note: the stage description calls these `session_router`
    // and `github_router`. The starter functions are named
    // `auth_router` and `oauth_router`; the aliases here keep the
    // dev-pulse-side narrative consistent without renaming starter.
    // -----------------------------------------------------------------
    let session_router = starter_auth_users::routes::auth_router::<()>(auth);
    let github_router = starter_auth_oauth::routes::oauth_router::<()>(oauth);

    // -----------------------------------------------------------------
    // Assembly.
    //
    // `with_openapi` hands the utoipa document built in
    // `dp_rest::openapi::DevPulseApi` to the starter-owned
    // `/openapi.json` route — dp-rest owns the document, dp-server
    // owns the surface (consumer-rules §6.7).
    // -----------------------------------------------------------------
    let router = ServerBuilder::<()>::new(())
        .merge_router(protected)
        .merge_router(webhook_router)
        .merge_router(session_router)
        .merge_router(github_router)
        .with_openapi(DevPulseApi::openapi())
        .with_metrics(registry, metrics)
        .build();

    Ok(router)
}

/// Newtype wrapping `Arc<dyn Authenticator>` so it satisfies the
/// `A: Authenticator + Sized` bound `with_principal` /
/// `McpHttpOptions::with_auth` / `router_with_auth` impose. Mirrors
/// the `BoxedAuthenticator` in `starter/examples/notes/src/server.rs`.
struct BoxedAuthenticator(Arc<dyn Authenticator>);

#[async_trait]
impl Authenticator for BoxedAuthenticator {
    async fn verify(&self, credential: &str) -> SpiResult<Principal> {
        self.0.verify(credential).await
    }
}

#[cfg(test)]
mod tests;

// Stage-8 test coverage scope. The handler-level behaviour (audit
// rows, resolved-window echoes, freshness shape, GDPR export
// streaming, etc.) is already pinned in dp-rest's per-handler unit
// tests (stages 2–4). Webhook receipt / replay / HMAC table is pinned
// in dp-fetcher::webhook (Phase 2 stage 4). What composition *adds*
// — and therefore what this crate's tests pin — is:
//
// * `BoxedAuthenticator` correctly forwards to the inner `Arc<dyn
//   Authenticator>` (the wrap-newtype dance for `with_principal`'s
//   `A: Sized` bound).
// * `build(...)` assembles a `Router` without panic-on-merge, the
//   shared registry survives the webhook-histogram registration, and
//   the OpenAPI document `dp_rest::DevPulseApi::openapi()` rides the
//   `/openapi.json` route the starter builder mounts.
//
// End-to-end "hit `/health` over a real listener" is the bin layer's
// job (stage 9+) — once the bin wires a real `AuthAuthenticator` and
// `StaticRbacEngine`, a `TestApp::spawn` style test covers the live
// request path. Forcing it in this crate would require hand-rolling
// stub `UserStore` / `SessionStore` / `TokenStore` / `IdentityStore`
// / `OAuthStateStore` / `OAuthProvider` impls; they exist in starter
// for SQLite only and stage 8's job is *wiring*, not re-implementing
// storage seams.
