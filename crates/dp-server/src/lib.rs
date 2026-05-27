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
    admin_router, app_permissions_router, board_links_router, directory_router, inbox_router,
    issue_dates_router, issues_read_router, issues_write_router, me_identities_router,
    pins_router, project_exec_summary_blob_router, project_exec_summary_router,
    project_issues_router, project_milestones_router, project_repos_router,
    project_views_router,
    projects_router, repos_router,
    reports_router, settings_router,
    tags_router, AdminState, AppState as RestAppState, DevPulseApi,
};

// Re-export so the bin layer (which doesn't depend on dp-rest
// directly) can name the GitHub App config type for
// SCOPE-PROJECTS §13.6 `[github.app]`.
pub use dp_rest::{FetcherIssueWriter, GitHubAppConfig, IssueWriteBackend};
pub use dp_rest::{FetcherMilestoneWriter, MilestoneWriteBackend};
pub use dp_rest::{
    OctocrabOrgProjectsPicker, OctocrabProjectV2Mirror, OrgProjectsPickerBackend,
    ProjectV2MirrorBackend,
};
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

/// Map the dp-domain operator role onto the starter SPI role the
/// policy engine consumes. The starter Admin tier matches the
/// dp-pulse Admin; Writer and Reader both map to `Role::Reader` on
/// the starter side because the upstream engine has no Writer
/// notion of its own — the Writer-tier checks happen in dp-rest's
/// own `(<resource>, write)` decorations rather than via the SPI
/// role enum. The mapping is intentionally narrow; the source of
/// truth is the dp-pulse `dp_users.role` column and the
/// `with_permission(...)` decorations on each route.
fn dp_role_to_spi_role(role: dp_domain::user::Role) -> starter_spi::auth::Role {
    use starter_spi::auth::Role as SpiRole;
    match role {
        dp_domain::user::Role::Admin => SpiRole::Admin,
        // Writer and Reader both fall under the starter Reader tier:
        // the writer-vs-reader split is enforced via the per-resource
        // `(.., write)` permission lanes inside dp-rest, not via the
        // SPI Role enum.
        _ => SpiRole::Reader,
    }
}

/// State the principal-role middleware closes over so it can look
/// up the persisted operator role per request. Held in an `Arc` so
/// the layer is cheap to apply.
#[derive(Clone)]
struct PrincipalRoleState {
    store: Arc<dyn Store>,
}

/// Middleware: overrides `Principal.role` from the `dp_users.role`
/// column (DOCS/SCOPE-AUTHZ-USERS.md §3 / §6). Runs after
/// `with_principal` has populated the SPI `Principal` so we can
/// observe the actor's subject UUID, then rewrites the in-request
/// extension with a new `Principal` whose role reflects the
/// operator-controlled tier.
///
/// On any lookup failure (subject not a UUID, row missing, store
/// error) the principal is left untouched — fail-open here is
/// acceptable because the downstream `require_permission` layer is
/// still the authority, and a stale Reader role is the safe default.
async fn principal_role_override(
    axum::extract::State(state): axum::extract::State<PrincipalRoleState>,
    mut req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if let Some(spi_principal) = req.extensions().get::<Principal>().cloned() {
        if let Ok(uuid) = Uuid::parse_str(&spi_principal.subject) {
            if let Ok(user) = state.store.get_user(uuid).await {
                let mut new_principal = spi_principal;
                new_principal.role = dp_role_to_spi_role(user.role);
                req.extensions_mut().insert(new_principal);
            }
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
    /// Optional §8 issue-write backend. `None` keeps the dp-rest
    /// default ([`dp_rest::UnconfiguredIssueWriter`]) which refuses
    /// every call — used when the deployment has no GitHub token
    /// armed. The bin layer hands a
    /// [`dp_rest::FetcherIssueWriter`] in PAT mode.
    pub issue_writer: Option<Arc<dyn IssueWriteBackend>>,
    /// Optional milestone-write backend. `None` keeps the dp-rest
    /// default ([`dp_rest::UnconfiguredMilestoneWriter`]) which
    /// refuses every call. The bin layer hands a
    /// [`dp_rest::FetcherMilestoneWriter`] in PAT mode so the
    /// `POST /projects/{id}/milestones` two-way-sync handler can
    /// reach GitHub.
    pub milestone_writer: Option<Arc<dyn MilestoneWriteBackend>>,
    /// Optional Projects v2 mirror backend. `None` leaves the
    /// dp-rest default ([`dp_rest::UnconfiguredProjectV2Mirror`])
    /// in place — mirroring is skipped entirely and only the
    /// local `dp_issue_dates` row is updated. PAT mode wires the
    /// [`dp_rest::OctocrabProjectV2Mirror`] adapter so the date
    /// editor lands cards on the linked Projects v2 board.
    pub projectv2_mirror: Option<Arc<dyn ProjectV2MirrorBackend>>,
    /// Optional org-scoped Projects v2 picker backend used by the
    /// §6.4 link-a-board dialog (`GET /orgs/{org_id}/projects-v2`).
    /// `None` leaves the dp-rest default in place; the route then
    /// returns the `upstream_unavailable` 400 so the dialog can
    /// render the `[Open GitHub project settings]` hint.
    pub org_projects_picker: Option<Arc<dyn OrgProjectsPickerBackend>>,
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
        issue_writer,
        milestone_writer,
        projectv2_mirror,
        org_projects_picker,
    } = state;

    // -----------------------------------------------------------------
    // dp-rest routers — protected fragment.
    //
    // Each fragment has its own per-router `State<…>` set inside the
    // dp-rest constructor; the resulting `Router` is `Router<()>` and
    // composes into the `ServerBuilder<()>` accumulator below.
    // -----------------------------------------------------------------
    //
    // Borrow the OAuth identity store *before* `oauth` is moved
    // into `oauth_router` below so the `/me/identities` handler
    // (§3.0 / §10) reads from the same row family the OAuth
    // callback writes to. Cheap `Arc` clone — no fan-out cost.
    let identity_store = oauth.identity_store.clone();

    // Default in-process blob store for the project Executive
    // Summary upload + proxy routes. Production binaries swap to
    // `starter-blob-fs` (single-node) or `starter-blob-garage`
    // (cluster) via a follow-up; the trait surface lets that be
    // a one-line wiring change per the storage-scope §"Swap test".
    let blob_store: Arc<dyn starter_spi::blob::BlobStore> =
        Arc::new(starter_blob_memory::MemoryBlobStore::new());

    let rest_state = Arc::new({
        let mut s = RestAppState::new(store.clone())
            .with_github_app(github_app.clone())
            .with_scheduler(scheduler.clone())
            .with_identity_store(identity_store)
            .with_blob_store(blob_store);
        if let Some(w) = issue_writer {
            s = s.with_issue_writer(w);
        }
        if let Some(w) = milestone_writer {
            s = s.with_milestone_writer(w);
        }
        if let Some(m) = projectv2_mirror {
            s = s.with_projectv2_mirror(m);
        }
        if let Some(p) = org_projects_picker {
            s = s.with_org_projects_picker(p);
        }
        s
    });
    let admin_state = Arc::new(AdminState::new(scheduler.clone(), store.clone()));

    let reports = reports_router(rest_state.clone());
    let directory = directory_router(rest_state.clone());
    let pins = pins_router(rest_state.clone());
    // First-class Projects v2 CRUD (linear-projects-v2.md §7.1).
    // Slice A: list / get / create / patch / archive. Membership
    // (§7.2) and board picker (§7.3) land in later stages.
    let projects = projects_router(rest_state.clone());
    // First-class Projects ↔ issues membership (linear-projects-v2.md
    // §7.2): bulk add, single delete, list, and "what's this issue's
    // project?". Routes ride the same `(projects, read|write)` lanes
    // as the §7.1 CRUD spine.
    let project_issues = project_issues_router(rest_state.clone());
    // Project ↔ repo soft scoping — used by the §6.3 issue
    // picker to narrow candidates to repos the operator has
    // associated with the project.
    let project_repos = project_repos_router(rest_state.clone());
    // Per-(project, user) saved views (PROJECT-VIEW.md §6.1 /
    // §7.1, Slice 4). Same `(projects, read|write)` lanes as the
    // §7.1 CRUD spine; the store layer scopes every read by
    // `owner_user_id`, so cross-user view access is invisible.
    let project_views = project_views_router(rest_state.clone());
    // Active milestones across linked repos (PROJECT-VIEW.md
    // §5.5, Slice 1). Read-only; `(projects, read)` lane. Adopt-
    // as-primary lands in Slice 5.
    let project_milestones = project_milestones_router(rest_state.clone());
    // Per-project Executive Summary (DOCS/SCOPE-PROJECT-EXECUTIVE-SUMMARY.md):
    // tabbed form (Summary / Scope / Requirements / Hardware / Commercial /
    // Documents / Approval / Change Log) with a `draft → in_review →
    // approved` state machine. Reads ride `(projects, read)`; writes ride
    // `(projects, write)`; approve / revert add a per-handler
    // project-lead check (E2). Image and document upload presign +
    // confirm routes are deferred until the starter-blob wiring lands;
    // the list / patch / delete routes already work end-to-end against
    // any `BlobRef`s a future upload path produces.
    let project_exec_summary = project_exec_summary_router(rest_state.clone());
    // Proxy GET surface for exec-summary blob attachments
    // (`/blobs/exec-summary/{kind}/{row_id}`). Auth-checked per
    // request under `(projects, read)` so anyone who can read the
    // project can fetch the bytes; the URL is the same one the
    // image / document DTOs carry on `url`.
    let project_exec_summary_blob = project_exec_summary_blob_router(rest_state.clone());
    // First-class Project ↔ GitHub Projects v2 board picker + link
    // CRUD (linear-projects-v2.md §7.3). Replaces the retired
    // per-repo admin surface on the primary path; the §6.4 dialog
    // reads the picker and the §6.3 row reads the link list.
    let board_links = board_links_router(rest_state.clone());
    let tags = tags_router(rest_state.clone());
    let repos = repos_router(rest_state.clone());
    let issues_read = issues_read_router(rest_state.clone());
    // SCOPE §18 / SCOPE-PROJECTS §8 — the issue write surface.
    // Gated on `(issues, write)` inside the router fragment; the
    // default `UnconfiguredIssueWriter` on `AppState` refuses every
    // call until the bin layer wires an octocrab-backed backend.
    let issues_write = issues_write_router(rest_state.clone());
    // §3.10 — start / due date upsert + best-effort Projects v2
    // mirror. Gated on `(issues, write)`; the local upsert is
    // synchronous and the mirror task is spawned and recorded
    // out-of-band on `dp_issue_dates.mirror_error`.
    let issue_dates = issue_dates_router(rest_state.clone());
    let inbox = inbox_router(rest_state.clone());
    // §3.0 / §10 — `GET /me/identities`. Reads the same
    // `IdentityStore` `starter_auth_oauth` writes to on link /
    // callback; gated on `(identities, read)` so the linked-account
    // surface is locked behind its own authz pair (narrower than
    // `users.read`).
    let me_identities = me_identities_router(rest_state.clone());
    // Per-user K/V settings (Account → Settings page). Gated on
    // `(settings, read|write)`. Pinned key catalogue lives in
    // `dp_rest::settings::KEYS` so new settings ship without a
    // migration.
    let settings = settings_router(rest_state.clone());
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
        .merge(projects)
        .merge(project_issues)
        .merge(project_repos)
        .merge(project_views)
        .merge(project_milestones)
        .merge(project_exec_summary)
        .merge(project_exec_summary_blob)
        .merge(board_links)
        .merge(tags)
        .merge(repos)
        .merge(issues_read)
        .merge(issues_write)
        .merge(issue_dates)
        .merge(inbox)
        .merge(me_identities)
        .merge(settings)
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
    // Override `Principal.role` from `dp_users.role` (SCOPE-AUTHZ-USERS
    // §3). Attached *outside* `bridge_principal` so the SPI Principal
    // it rewrites is the same one `bridge_principal` later reads; both
    // run after `with_principal` has populated the SPI Principal. The
    // override is a no-op on routes the user doesn't yet have a
    // `dp_users` row for (fail-open — the downstream `require_permission`
    // remains the authority).
    let role_state = PrincipalRoleState {
        store: store.clone(),
    };
    let protected = protected.layer(axum::middleware::from_fn_with_state(
        role_state,
        principal_role_override,
    ));
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
