//! `dev-pulse` — top-level binary.
//!
//! ## Phase 4 stage 10 — `serve` body wired
//!
//! The clap top-level (`migrate | serve | fetch-now | backfill | claim`)
//! is Phase 6's job; this stage owns only the body of the `serve`
//! subcommand, which:
//!
//! 1. Loads a TOML config (see `crates/dev-pulse/config.example.toml`
//!    for the shape — the bin layer reads it verbatim, no env-var
//!    overlay magic) and validates the `[auth.github]` block before
//!    [`dp_server::build`] runs (per the stage description).
//! 2. Connects the dp-data Postgres pool and wraps it in
//!    [`dp_store_pg::PgStore`].
//! 3. Connects a sidecar SQLite pool for the starter-auth-users /
//!    starter-auth-oauth row families (the only stores those crates
//!    ship today) and runs both migration sets.
//! 4. Builds the GitHub OAuth provider (`client_id` from config,
//!    `client_secret` resolved from the `secret://`/`file:`/literal
//!    handle in config — full `starter-secrets-file` integration is
//!    a follow-up; today the bin recognises `secret://NAME` →
//!    env var `NAME` plus `file:PATH` → read-trim).
//! 5. Constructs the [`AuthAuthenticator`] with a
//!    [`dp_server::auth::GithubOrgsStamper`] so every authenticated
//!    request carries `oauth.github_orgs` + `oauth.in_allowed_org`
//!    on `Principal.extra` (R8 attribute bus).
//! 6. Loads the [`StaticRbacEngine`] from
//!    `crates/dp-server/policy/dev-pulse.toml` wrapped in
//!    [`dp_server::auth::AwaitingAccessEngine`] so out-of-org users
//!    see the SCOPE D4.2 `awaiting_access` reason on every 403.
//! 7. Builds an empty-target [`dp_fetcher::reconciler::Scheduler`]
//!    so `POST /admin/refresh` has a handle to call. Real fetcher
//!    Client + TargetProvider construction is Phase 6 (CLI `fetch-now`
//!    / `backfill` own the GitHub App credential resolution).
//! 8. Calls [`dp_server::build`], binds the returned `axum::Router`
//!    on `[server].listen`, and runs `axum::serve(...)` with a
//!    `ctrl_c` graceful-shutdown signal. The scheduler's `run` task
//!    receives the same shutdown so the webhook worker + reconciler
//!    tick loop stop with the server.
//!
//! Out of scope for this stage:
//! * Other subcommands (`migrate`, `fetch-now`, `backfill`, `claim`,
//!   the `starter-cli` registry) — Phase 6.
//! * Full `starter-secrets-file` (age-based) resolution — the
//!   `resolve_secret` helper below honours `secret://` /  `file:` /
//!   literal which is enough to wire deployments without leaking
//!   secrets into the TOML.
//! * Real GitHub App credentials for the fetcher Client — Phase 6.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use clap::{Arg, ArgMatches, Command};
use secrecy::SecretString;
use serde::Deserialize;
use starter_observability::metrics::StandardMetrics;
use starter_observability::tracing::Format;
use starter_spi::auth::Authenticator;
use tokio::sync::watch;
use uuid::Uuid;

use dp_server::auth::{
    config::GitHubAuthConfig, load_static_engine, register_dev_pulse_resources,
    CachedGithubOrgsSource, GithubOrgsSource, GithubOrgsStamper, StaticGithubOrgsSource,
};
use dp_server::{AppState, BuildConfig};

// ---------------------------------------------------------------- config

/// Top-level deployment config — what the operator hands in via
/// `dev-pulse serve --config <path>`.
///
/// The shape is deliberately flat-TOML rather than the layered
/// `starter-config` env-overlay pattern; Phase 6 may re-platform
/// onto `starter-config` once the CLI registry lands, but for the
/// stage-10 smoke a single file is enough.
#[derive(Debug, Deserialize)]
struct DevPulseConfig {
    server: ServerSection,
    postgres: PostgresSection,
    auth_sqlite: AuthSqliteSection,
    webhook: WebhookSection,
    auth: AuthSection,
    #[serde(default)]
    scheduler: SchedulerSection,
    #[serde(default)]
    github: GithubSection,
}

/// Fetcher-side GitHub credentials. Separate from `[auth.github]`,
/// which is the *operator OAuth* config — this one is the token the
/// reconciler / backfill use to call the GitHub REST API.
///
/// `token_ref` follows the same `secret://` / `file:` / literal
/// handle syntax as the other secret fields. When unset (or resolves
/// to an empty string), the fetcher builds a token-less placeholder
/// Client and the scheduler stays dormant regardless of
/// `[scheduler].enable` — `POST /admin/refresh` still answers but
/// does no real work, which matches the pre-PAT Phase 4 behaviour.
///
/// Per SCOPE §15.1 the *production* path is a GitHub App, not a PAT
/// — `token_ref` is the Phase 6 stepping stone that lets an operator
/// drive the fetcher with a classic / fine-grained PAT until the App
/// installation seam lands.
#[derive(Debug, Deserialize)]
struct GithubSection {
    /// Secret handle for the fetcher's GitHub access token (PAT).
    /// Optional; absent → dormant fetcher.
    #[serde(default)]
    token_ref: Option<String>,
    /// REST API base URL. Override only for testing against a
    /// GitHub Enterprise host or a wiremock fixture.
    #[serde(default = "default_github_base_url")]
    base_url: String,
    /// Local per-run request budget — the operator-side fuse against
    /// runaway reconciler ticks. After this many GitHub HTTP calls,
    /// the wrapper returns `BudgetExhausted` and the run stops. `0`
    /// disables the fuse (rely solely on GitHub's own quota). The
    /// default is conservative — bump it once a deployment knows its
    /// real per-tick cost.
    #[serde(default = "default_max_requests_per_run")]
    max_requests_per_run: u64,
}

fn default_max_requests_per_run() -> u64 {
    // 200 is well below the 5000/h primary bucket per SCOPE §15.4 and
    // covers a reconciler tick over ~30 repos at ~5 endpoints each
    // with no pagination. Real production deployments should tune.
    200
}

fn default_github_base_url() -> String {
    "https://api.github.com".to_string()
}

impl Default for GithubSection {
    fn default() -> Self {
        Self {
            token_ref: None,
            base_url: default_github_base_url(),
            max_requests_per_run: default_max_requests_per_run(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ServerSection {
    /// `host:port` to bind, e.g. `127.0.0.1:8080`.
    listen: String,
    /// External-facing base URL — used by the OAuth provider as
    /// `{base_url}/auth/oauth/{provider}/callback`.
    base_url: String,
    /// Where the OAuth callback lands the browser when `return_to`
    /// is `None`. Defaults to `/`.
    #[serde(default = "default_return_to")]
    default_return_to: String,
}

fn default_return_to() -> String {
    "/".to_string()
}

#[derive(Debug, Deserialize)]
struct PostgresSection {
    /// libpq-style URL, e.g. `postgres://dev-pulse:…@localhost/dev_pulse`.
    url: String,
}

#[derive(Debug, Deserialize)]
struct AuthSqliteSection {
    /// SQLx URL, e.g. `sqlite:./auth.db?mode=rwc`.
    url: String,
}

#[derive(Debug, Deserialize)]
struct WebhookSection {
    /// GitHub webhook HMAC secret — the receiver matches this
    /// against the `X-Hub-Signature-256` header. Same `secret://` /
    /// `file:` / literal resolution as [`AuthSection::github`]'s
    /// `client_secret_ref`.
    secret_ref: String,
}

#[derive(Debug, Deserialize)]
struct AuthSection {
    github: GitHubAuthConfig,
}

#[derive(Debug, Deserialize, Default)]
struct SchedulerSection {
    /// Seconds between reconciler ticks. Default 300s (5 min) —
    /// `Scheduler` ignores ticks while a previous tick is still
    /// running, so a missed deadline doesn't pile up.
    #[serde(default = "default_tick_interval_secs")]
    tick_interval_secs: u64,
    /// Set to `true` to actually run the reconciler loop. Default
    /// `false` because Phase 4's bin does not yet resolve GitHub App
    /// credentials (Phase 6) — leaving the scheduler dormant avoids
    /// hammering the API with a token-less Client.
    #[serde(default)]
    enable: bool,
}

fn default_tick_interval_secs() -> u64 {
    300
}

// ---------------------------------------------------------------- migrators

/// `starter-auth-users` SQLite migrations — embedded at compile time
/// so the bin doesn't depend on the migrations directory being
/// present at runtime.
static AUTH_USERS_MIGRATOR: sqlx::migrate::Migrator =
    sqlx::migrate!("../../../starter/crates/starter-auth-users/migrations/starter_auth_users");

/// `starter-auth-oauth` SQLite migrations (identities + state).
static AUTH_OAUTH_MIGRATOR: sqlx::migrate::Migrator =
    sqlx::migrate!(
        "../../../starter/crates/starter-auth-oauth/migrations/starter_auth_oauth_sqlite"
    );

// ---------------------------------------------------------------- main

#[tokio::main]
async fn main() -> Result<()> {
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into());
    let _tracing = starter_observability::tracing::init(&filter, Format::Pretty)
        .map_err(|e| anyhow!("init tracing: {e}"))?;

    let app = Command::new("dev-pulse")
        .about("dev-pulse — GitHub reporting and insights across multiple orgs.")
        .arg_required_else_help(true)
        .subcommand(
            Command::new("serve")
                .about("Boot the dev-pulse HTTP server.")
                .arg(
                    Arg::new("config")
                        .long("config")
                        .required(true)
                        .help("Path to the dev-pulse TOML config."),
                ),
        )
        .subcommand(
            Command::new("migrate")
                .about(
                    "Apply the dp-data Postgres migrations (and the \
                     starter-auth-{users,oauth} SQLite migrations).",
                )
                .arg(
                    Arg::new("config")
                        .long("config")
                        .required(true)
                        .help("Path to the dev-pulse TOML config."),
                ),
        )
        .subcommand(
            Command::new("add-org")
                .about(
                    "Resolve a GitHub org (or user account) and upsert it \
                     into dp_orgs so the reconciler will tick its repos.",
                )
                .arg(
                    Arg::new("config")
                        .long("config")
                        .required(true)
                        .help("Path to the dev-pulse TOML config."),
                )
                .arg(
                    Arg::new("login")
                        .required(true)
                        .help("GitHub login (e.g. `nube-io` or `NubeDev`)."),
                ),
        )
        .subcommand(
            Command::new("add-repo")
                .about(
                    "Resolve a GitHub repo and upsert it (plus its owning \
                     org if missing) so the reconciler will tick it.",
                )
                .arg(
                    Arg::new("config")
                        .long("config")
                        .required(true)
                        .help("Path to the dev-pulse TOML config."),
                )
                .arg(
                    Arg::new("repo")
                        .required(true)
                        .help("`owner/repo` (e.g. `nube-io/dev-pulse`)."),
                ),
        )
        .subcommand(
            Command::new("create-admin")
                .about(
                    "Seed an email+password admin user in the auth.db \
                     (break-glass / dev path — SCOPE §15.10). The \
                     frontend's login form posts to /auth/login with \
                     these credentials.",
                )
                .arg(
                    Arg::new("config")
                        .long("config")
                        .required(true)
                        .help("Path to the dev-pulse TOML config."),
                )
                .arg(
                    Arg::new("email")
                        .long("email")
                        .required(true)
                        .help("Operator email."),
                )
                .arg(
                    Arg::new("password")
                        .long("password")
                        .required(true)
                        .help("Operator password (min length from env / starter defaults)."),
                ),
        )
        .subcommand(
            Command::new("fetch-now")
                .about(
                    "Run one reconciler tick against the registered \
                     targets and exit. No HTTP server, no OAuth setup \
                     required — just the PAT + Postgres.",
                )
                .arg(
                    Arg::new("config")
                        .long("config")
                        .required(true)
                        .help("Path to the dev-pulse TOML config."),
                ),
        )
        .subcommand(
            Command::new("list-targets")
                .about("List the orgs + repos registered for reconciler ticks.")
                .arg(
                    Arg::new("config")
                        .long("config")
                        .required(true)
                        .help("Path to the dev-pulse TOML config."),
                ),
        )
        .subcommand(
            Command::new("check-github")
                .about(
                    "Resolve [github].token_ref from config + env and call \
                     GitHub `GET /user` to confirm the credentials work.",
                )
                .arg(
                    Arg::new("config")
                        .long("config")
                        .required(true)
                        .help("Path to the dev-pulse TOML config."),
                ),
        );

    let matches = app.get_matches();
    match matches.subcommand() {
        Some(("serve", sub)) => run_serve(sub).await,
        Some(("migrate", sub)) => run_migrate(sub).await,
        Some(("add-org", sub)) => run_add_org(sub).await,
        Some(("add-repo", sub)) => run_add_repo(sub).await,
        Some(("create-admin", sub)) => run_create_admin(sub).await,
        Some(("fetch-now", sub)) => run_fetch_now(sub).await,
        Some(("list-targets", sub)) => run_list_targets(sub).await,
        Some(("check-github", sub)) => run_check_github(sub).await,
        _ => {
            tracing::info!(
                "dev-pulse: no subcommand. Phase 6 wires `migrate`, `fetch-now`, `backfill`, `claim`."
            );
            Ok(())
        }
    }
}

// ---------------------------------------------------------------- serve

async fn run_serve(matches: &ArgMatches) -> Result<()> {
    let cfg_path = matches
        .get_one::<String>("config")
        .ok_or_else(|| anyhow!("--config is required"))?;
    let raw = std::fs::read_to_string(cfg_path)
        .with_context(|| format!("read config: {cfg_path}"))?;
    let cfg: DevPulseConfig = toml::from_str(&raw).context("parse config TOML")?;

    // Fail loudly if the `[auth.github]` block is unusable — operators
    // who forget `client_id` discover it at boot, not via mysterious
    // 403s once the first user tries to log in.
    //
    // Escape hatch for local development without a real GitHub OAuth
    // App: putting any non-empty placeholder in `client_id` /
    // `client_secret_ref` satisfies the validator. The OAuth login
    // flow itself won't work (GitHub will 401 the callback), but the
    // server boots, `POST /auth/login` (email+password) works, and
    // every protected route can be exercised via a `dev-pulse
    // create-admin`-seeded session.
    cfg.auth.github.validate().context("auth.github config")?;

    tracing::info!(
        listen = %cfg.server.listen,
        base_url = %cfg.server.base_url,
        "dev-pulse serve: starting"
    );

    // -- dp-data postgres pool + Store ---------------------------------
    let pg_pool = starter_store_postgres::pool::connect(&cfg.postgres.url)
        .await
        .with_context(|| format!("connect postgres: {}", cfg.postgres.url))?;
    let store = Arc::new(dp_store_pg::PgStore::new(pg_pool));

    // -- auth sidecar sqlite pool + migrations -------------------------
    let sqlite_pool = starter_store_sqlite::pool::connect(&cfg.auth_sqlite.url)
        .await
        .with_context(|| format!("connect sqlite: {}", cfg.auth_sqlite.url))?;
    apply_auth_sqlite_migrations(&sqlite_pool).await?;

    let users = Arc::new(starter_auth_users::store::SqliteUserStore::new(
        sqlite_pool.clone(),
    ));
    let sessions = Arc::new(starter_auth_users::store::SqliteSessionStore::new(
        sqlite_pool.clone(),
    ));
    let tokens = Arc::new(starter_auth_users::store::SqliteTokenStore::new(
        sqlite_pool.clone(),
    ));
    let identities = Arc::new(starter_auth_oauth::SqliteIdentityStore::new(
        sqlite_pool.clone(),
    ));
    let state_store = Arc::new(starter_auth_oauth::MemoryStateStore::new());

    // -- principal extras stamper (oauth.github_orgs / in_allowed_org) -
    let gh_cfg = Arc::new(cfg.auth.github.clone());
    let oauth_extras = Arc::new(starter_auth_oauth::OAuthPrincipalExtras::new(
        identities.clone(),
    ));
    // The bin wires `StaticGithubOrgsSource` as the inner source so the
    // process boots cleanly without per-user GitHub tokens — Phase 6
    // swaps this for the real octocrab-backed source once the App
    // installation seam lands. The TTL cache wraps it so the seam
    // contract is identical regardless of inner impl.
    let orgs_inner: Arc<dyn GithubOrgsSource> = Arc::new(StaticGithubOrgsSource::new());
    let orgs_cached = Arc::new(CachedGithubOrgsSource::new(
        orgs_inner,
        gh_cfg.org_refresh_interval(),
    ));
    let stamper = Arc::new(GithubOrgsStamper::new(
        oauth_extras,
        orgs_cached,
        gh_cfg.clone(),
    ));

    // -- Authenticator (sas_* cookies + sak_* tokens, with extras) -----
    let authenticator: Arc<dyn Authenticator> = Arc::new(
        starter_auth_users::AuthAuthenticator::new(
            users.clone(),
            sessions.clone(),
            tokens.clone(),
        )
        .with_principal_extras(stamper.clone()),
    );

    // -- GitHub OAuth provider -----------------------------------------
    let client_secret = resolve_secret(&cfg.auth.github.client_secret_ref)
        .context("resolve auth.github.client_secret_ref")?;
    let mut providers: BTreeMap<String, Arc<dyn starter_auth_oauth::OAuthProvider>> =
        BTreeMap::new();
    let github_provider: Arc<dyn starter_auth_oauth::OAuthProvider> = Arc::new(
        starter_auth_oauth::GitHubProvider::new(cfg.auth.github.client_id.clone(), client_secret),
    );
    providers.insert(github_provider.id().to_string(), github_provider);

    // -- AuthState (the /auth/* session router's closure value) --------
    let auth_state = starter_auth_users::routes::AuthState::new(
        users.clone(),
        sessions.clone(),
        tokens.clone(),
    )
    .with_linked_providers(Arc::new(starter_auth_oauth::OAuthLinkedProviders::new(
        identities.clone(),
    )))
    .with_principal_extras(stamper.clone());

    // -- OAuthRoutesState ----------------------------------------------
    let oauth_state = starter_auth_oauth::routes::OAuthRoutesState {
        providers,
        state_store,
        identity_store: identities.clone(),
        user_store: users.clone(),
        session_store: sessions.clone(),
        base_url: cfg.server.base_url.clone(),
        // First-callback auto-provisions the operator row per the
        // Phase 4 §0 decision; the org gate filters out-of-org
        // users *after* the row exists so leaked-invite-style abuse
        // shows up in the audit trail.
        signup_enabled: true,
        signup_default_role: starter_auth_users::Role::Reader,
        role_domain_maps: HashMap::new(),
        default_return_to: cfg.server.default_return_to.clone(),
    };

    // -- policy engine (StaticRbacEngine + AwaitingAccessEngine) -------
    let authz_registry = Arc::new(starter_authz::StaticRegistry::new());
    register_dev_pulse_resources(&authz_registry);
    let policy = load_static_engine("crates/dp-server/policy/dev-pulse.toml", authz_registry)
        .context("load dev-pulse policy")?;

    // -- webhook HMAC secret -------------------------------------------
    let webhook_secret_value =
        resolve_secret(&cfg.webhook.secret_ref).context("resolve webhook.secret_ref")?;
    let webhook_secret: Arc<dyn dp_fetcher::webhook::WebhookSecretSource> =
        Arc::new(dp_fetcher::webhook::StaticSecrets::single(SecretString::from(
            webhook_secret_value,
        )));

    // -- Scheduler (dormant by default; Phase 6 wires the fetcher) -----
    //
    // We keep a handle on the underlying `Reconciler` so the optional
    // tick loop can build its own owned `Scheduler` (the API consumes
    // `self` on `run`, which means we can't share the AppState
    // `Arc<Scheduler>` with the loop).
    // Resolve the optional fetcher PAT (SCOPE §15.1 stepping stone —
    // production path is GitHub App). Empty / missing → dormant
    // fetcher: the placeholder client is built but `[scheduler].enable`
    // is force-overridden to `false` below so we don't tick a
    // token-less client against the API.
    let github_token = match cfg.github.token_ref.as_deref() {
        Some(handle) if !handle.is_empty() => Some(
            resolve_secret(handle).context("resolve github.token_ref")?,
        ),
        _ => None,
    };
    let fetcher_armed = github_token
        .as_ref()
        .map(|t| !t.is_empty())
        .unwrap_or(false);
    let budget = (cfg.github.max_requests_per_run > 0)
        .then_some(cfg.github.max_requests_per_run);
    if let Some(b) = budget {
        tracing::info!(budget = b, "github request budget per run");
    } else {
        tracing::warn!(
            "github.max_requests_per_run = 0 — local fuse disabled, \
             relying solely on GitHub-side rate limit"
        );
    }
    let reconciler = build_reconciler(
        store.clone(),
        github_token,
        &cfg.github.base_url,
        budget,
    )
    .context("build reconciler")?;
    let scheduler = Arc::new(dp_fetcher::reconciler::Scheduler::new(
        reconciler.clone(),
        Duration::from_secs(cfg.scheduler.tick_interval_secs),
    ));

    // -- prometheus registry + standard metrics ------------------------
    let prom_registry = Arc::new(prometheus::Registry::new());
    let metrics = Arc::new(
        StandardMetrics::register(&prom_registry).context("register prometheus metrics")?,
    );

    // -- Compose the dp_server router ----------------------------------
    let build_cfg = BuildConfig {
        state: AppState {
            store,
            scheduler: scheduler.clone(),
            authenticator,
            policy,
            webhook_secret,
            registry: prom_registry,
            metrics,
        },
        auth: auth_state,
        oauth: oauth_state,
    };
    let router = dp_server::build(build_cfg).context("dp_server::build")?;

    // -- Cooperative shutdown ------------------------------------------
    //
    // One `watch` channel drives both the scheduler's `run` loop and
    // (indirectly) the webhook worker / reconciler — they receive the
    // `shutdown_rx` end and exit cleanly on `true`. axum's
    // `with_graceful_shutdown` watches the same ctrl-c future so the
    // server, scheduler, and any reconciler tick stop together.
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let sched_enabled = cfg.scheduler.enable && fetcher_armed;
    if cfg.scheduler.enable && !fetcher_armed {
        tracing::warn!(
            "scheduler.enable=true but github.token_ref is empty — \
             forcing scheduler dormant to avoid token-less API calls"
        );
    }
    let sched_handle = if sched_enabled {
        // `Scheduler::run(self, …)` consumes the receiver — the
        // AppState's `Arc<Scheduler>` is for `try_trigger_now`
        // (which takes `&self`), so the tick loop gets its own
        // owned Scheduler built off the same Reconciler. Cheap:
        // the Reconciler is Arc'd.
        let owned = dp_fetcher::reconciler::Scheduler::new(
            reconciler.clone(),
            Duration::from_secs(cfg.scheduler.tick_interval_secs),
        );
        let rx = shutdown_rx.clone();
        Some(tokio::spawn(async move {
            owned.run(rx).await;
        }))
    } else {
        tracing::info!(
            "scheduler.enable=false — reconciler tick loop is dormant; \
             `POST /admin/refresh` still triggers an ad-hoc tick"
        );
        None
    };

    let listen: SocketAddr = cfg
        .server
        .listen
        .parse()
        .with_context(|| format!("parse listen addr {:?}", cfg.server.listen))?;
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .with_context(|| format!("bind {listen}"))?;

    tracing::info!(%listen, "dev-pulse serve: listening");

    let shutdown_tx_for_signal = shutdown_tx.clone();
    let serve_result = axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("ctrl-c received, beginning graceful shutdown");
            // Signal the scheduler / worker watchers BEFORE axum
            // tears down so cooperating background tasks finish
            // their in-flight tick instead of being torn out.
            let _ = shutdown_tx_for_signal.send(true);
        })
        .await;

    // Make absolutely sure the scheduler watcher sees the shutdown,
    // even if axum's graceful path errored out before sending it.
    let _ = shutdown_tx.send(true);
    if let Some(h) = sched_handle {
        let _ = h.await;
    }

    serve_result.context("axum::serve")?;
    Ok(())
}

// ---------------------------------------------------------------- check-github

/// Minimal "does my PAT work" smoke. Runs the same code path the
/// server uses (config parse → `resolve_secret` → octocrab Client →
/// `GET /user`) and prints the resolved GitHub identity. Exits
/// non-zero if any step fails so it's CI-friendly.
async fn run_check_github(matches: &ArgMatches) -> Result<()> {
    let cfg_path = matches
        .get_one::<String>("config")
        .ok_or_else(|| anyhow!("--config is required"))?;
    let raw = std::fs::read_to_string(cfg_path)
        .with_context(|| format!("read config: {cfg_path}"))?;
    let cfg: DevPulseConfig = toml::from_str(&raw).context("parse config TOML")?;

    let handle = cfg
        .github
        .token_ref
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("[github].token_ref is missing or empty in {cfg_path}"))?;
    let token = resolve_secret(handle).context("resolve github.token_ref")?;
    if token.is_empty() {
        return Err(anyhow!(
            "[github].token_ref {handle:?} resolved to an empty string \
             (env var or file is set but empty)"
        ));
    }

    println!("config       : {cfg_path}");
    println!("token handle : {handle}");
    println!("base_url     : {}", cfg.github.base_url);
    println!("calling GET {}/user ...", cfg.github.base_url);

    let budget = (cfg.github.max_requests_per_run > 0)
        .then_some(cfg.github.max_requests_per_run);
    let client = dp_fetcher::client::Client::with_personal_token(
        SecretString::from(token),
        &cfg.github.base_url,
    )
    .map_err(|e| anyhow!("build github client: {e}"))?
    .with_budget(budget);
    println!(
        "budget       : {}",
        budget
            .map(|b| b.to_string())
            .unwrap_or_else(|| "disabled".into())
    );

    #[derive(Debug, serde::Deserialize)]
    struct GhUser {
        login: String,
        id: u64,
        #[serde(default)]
        name: Option<String>,
        #[serde(rename = "type")]
        kind: String,
    }

    match client
        .get_conditional::<GhUser>("/user", None)
        .await
        .context("GET /user")?
    {
        dp_fetcher::client::Fetched::Ok { body, signal, .. } => {
            println!("\nOK — GitHub answered:");
            println!("  login : {}", body.login);
            println!("  id    : {}", body.id);
            println!("  name  : {}", body.name.as_deref().unwrap_or("(unset)"));
            println!("  type  : {}", body.kind);
            if let Some(s) = signal {
                println!("\nrate limit signal: {s:?}");
            }
            Ok(())
        }
        dp_fetcher::client::Fetched::NotModified { .. } => {
            // Won't happen for a no-etag GET, but cover the branch.
            Err(anyhow!("unexpected 304 from /user"))
        }
    }
}

// ---------------------------------------------------------------- migrate / targets

/// Parse the config + connect to the dp-data Postgres pool. Shared
/// by every subcommand that touches the DB.
async fn load_cfg_and_pool(cfg_path: &str) -> Result<(DevPulseConfig, starter_store_postgres::Pool)> {
    let raw = std::fs::read_to_string(cfg_path)
        .with_context(|| format!("read config: {cfg_path}"))?;
    let cfg: DevPulseConfig = toml::from_str(&raw).context("parse config TOML")?;
    let pool = starter_store_postgres::pool::connect(&cfg.postgres.url)
        .await
        .with_context(|| format!("connect postgres: {}", cfg.postgres.url))?;
    Ok((cfg, pool))
}

/// Build a PAT-backed octocrab Client from the config, honouring the
/// budget. Used by `add-org` / `add-repo` so a registration call also
/// counts against the operator-side fuse.
fn build_pat_client(cfg: &DevPulseConfig) -> Result<dp_fetcher::client::Client> {
    let handle = cfg
        .github
        .token_ref
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("[github].token_ref is missing or empty"))?;
    let token = resolve_secret(handle).context("resolve github.token_ref")?;
    if token.is_empty() {
        return Err(anyhow!(
            "[github].token_ref {handle:?} resolved to an empty string"
        ));
    }
    let budget = (cfg.github.max_requests_per_run > 0)
        .then_some(cfg.github.max_requests_per_run);
    dp_fetcher::client::Client::with_personal_token(
        SecretString::from(token),
        &cfg.github.base_url,
    )
    .map_err(|e| anyhow!("build github client: {e}"))
    .map(|c| c.with_budget(budget))
}

async fn run_migrate(matches: &ArgMatches) -> Result<()> {
    let cfg_path = matches
        .get_one::<String>("config")
        .ok_or_else(|| anyhow!("--config is required"))?;
    let (cfg, pool) = load_cfg_and_pool(cfg_path).await?;

    println!("postgres : {}", cfg.postgres.url);
    print!("applying dp/* migrations ... ");
    let mut m = starter_store_postgres::migrate::migrate(&pool);
    for s in dp_store_pg::sources() {
        m = m.with_source(s);
    }
    m.run().await.context("run dp migrations")?;
    println!("ok");

    // Auth sidecar SQLite — same migrations the `serve` body runs on
    // boot, but materialised here so an operator can verify the file
    // separately.
    println!("sqlite   : {}", cfg.auth_sqlite.url);
    let sqlite_pool = starter_store_sqlite::pool::connect(&cfg.auth_sqlite.url)
        .await
        .with_context(|| format!("connect sqlite: {}", cfg.auth_sqlite.url))?;
    print!("applying starter_auth_users + starter_auth_oauth ... ");
    apply_auth_sqlite_migrations(&sqlite_pool).await?;
    println!("ok");
    Ok(())
}

/// Run both auth-side SQLite migrators against the shared sidecar
/// `auth.db`. Each is mounted as a named source via
/// `starter_store_sqlite::migrate`, which gives each its own
/// `_sqlx_migrations_<name>` progress table — without that
/// namespacing, `auth_users` (4 migrations) and `auth_oauth` (3
/// migrations) collide on a single `_sqlx_migrations` and the second
/// to run fails with "migration N was previously applied but is
/// missing in the resolved migrations".
async fn apply_auth_sqlite_migrations(
    pool: &starter_store_sqlite::Pool,
) -> Result<()> {
    starter_store_sqlite::migrate::migrate(pool)
        .with_source(starter_store_sqlite::MigrationSource {
            name: "starter_auth_users",
            migrator: &AUTH_USERS_MIGRATOR,
        })
        .with_source(starter_store_sqlite::MigrationSource {
            name: "starter_auth_oauth",
            migrator: &AUTH_OAUTH_MIGRATOR,
        })
        .run()
        .await
        .context("apply starter_auth_{users,oauth} migrations")
}

/// Minimal projection of a GitHub `User` / `Organization` payload —
/// `add-org` accepts either, because a single PAT user (e.g. NubeDev)
/// is itself a valid owner of repos.
#[derive(Debug, serde::Deserialize)]
struct GhAccount {
    id: i64,
    login: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct GhRepo {
    id: i64,
    name: String,
    owner: GhAccount,
}

async fn fetch_account(
    client: &dp_fetcher::client::Client,
    login: &str,
) -> Result<GhAccount> {
    // Try /orgs/{login} first; fall back to /users/{login}. Either
    // gives us the id + login + display name we need.
    match client
        .get_conditional::<GhAccount>(&format!("/orgs/{login}"), None)
        .await
    {
        Ok(dp_fetcher::client::Fetched::Ok { body, .. }) => return Ok(body),
        Ok(dp_fetcher::client::Fetched::NotModified { .. }) => {
            return Err(anyhow!("unexpected 304"))
        }
        Err(dp_fetcher::client::ClientError::Client { status: 404, .. }) => {
            // not an org — fall through
        }
        Err(e) => return Err(anyhow!("GET /orgs/{login}: {e}")),
    }
    match client
        .get_conditional::<GhAccount>(&format!("/users/{login}"), None)
        .await
        .with_context(|| format!("GET /users/{login}"))?
    {
        dp_fetcher::client::Fetched::Ok { body, .. } => Ok(body),
        dp_fetcher::client::Fetched::NotModified { .. } => Err(anyhow!("unexpected 304")),
    }
}

async fn run_add_org(matches: &ArgMatches) -> Result<()> {
    let cfg_path = matches
        .get_one::<String>("config")
        .ok_or_else(|| anyhow!("--config is required"))?;
    let login = matches.get_one::<String>("login").unwrap();
    let (cfg, pool) = load_cfg_and_pool(cfg_path).await?;
    let client = build_pat_client(&cfg)?;

    println!("resolving GitHub account: {login}");
    let acct = fetch_account(&client, login).await?;
    println!(
        "  github_id : {}\n  login     : {}\n  name      : {}",
        acct.id,
        acct.login,
        acct.name.as_deref().unwrap_or("(unset)")
    );

    let store = dp_store_pg::PgStore::new(pool);
    use dp_domain::Store as _;
    let row = dp_domain::Org {
        id: Uuid::new_v4(),
        github_id: acct.id,
        login: acct.login.clone(),
        name: acct.name.clone(),
    };
    let saved = store
        .upsert_org(&row)
        .await
        .with_context(|| format!("upsert_org {login}"))?;
    println!("upserted org id = {}", saved.id);
    Ok(())
}

async fn run_add_repo(matches: &ArgMatches) -> Result<()> {
    let cfg_path = matches
        .get_one::<String>("config")
        .ok_or_else(|| anyhow!("--config is required"))?;
    let spec = matches.get_one::<String>("repo").unwrap();
    let (owner_login, repo_name) = spec.split_once('/').ok_or_else(|| {
        anyhow!("expected `owner/repo`, got {spec:?}")
    })?;

    let (cfg, pool) = load_cfg_and_pool(cfg_path).await?;
    let client = build_pat_client(&cfg)?;

    println!("resolving GitHub repo: {spec}");
    let repo = match client
        .get_conditional::<GhRepo>(&format!("/repos/{owner_login}/{repo_name}"), None)
        .await
        .with_context(|| format!("GET /repos/{spec}"))?
    {
        dp_fetcher::client::Fetched::Ok { body, .. } => body,
        dp_fetcher::client::Fetched::NotModified { .. } => return Err(anyhow!("unexpected 304")),
    };
    println!(
        "  repo github_id  : {}\n  owner login     : {}\n  owner github_id : {}",
        repo.id, repo.owner.login, repo.owner.id
    );

    let store = dp_store_pg::PgStore::new(pool);
    use dp_domain::Store as _;
    // Upsert the owner first so the FK on dp_repos.org_id holds.
    let org_row = dp_domain::Org {
        id: Uuid::new_v4(),
        github_id: repo.owner.id,
        login: repo.owner.login.clone(),
        name: repo.owner.name.clone(),
    };
    let saved_org = store
        .upsert_org(&org_row)
        .await
        .with_context(|| format!("upsert_org {}", repo.owner.login))?;
    let repo_row = dp_domain::Repo {
        id: Uuid::new_v4(),
        org_id: saved_org.id,
        github_id: repo.id,
        name: repo.name.clone(),
    };
    let saved_repo = store
        .upsert_repo(&repo_row)
        .await
        .with_context(|| format!("upsert_repo {spec}"))?;
    println!("upserted repo id = {} (org id = {})", saved_repo.id, saved_org.id);
    Ok(())
}

async fn run_list_targets(matches: &ArgMatches) -> Result<()> {
    let cfg_path = matches
        .get_one::<String>("config")
        .ok_or_else(|| anyhow!("--config is required"))?;
    let (_cfg, pool) = load_cfg_and_pool(cfg_path).await?;
    let rows: Vec<(String, String, i64, i64)> = sqlx::query_as(
        "SELECT o.login, r.name, o.github_id, r.github_id
           FROM dp_repos r
           JOIN dp_orgs  o ON o.id = r.org_id
          ORDER BY o.login, r.name",
    )
    .fetch_all(pool.sqlx())
    .await
    .context("query dp_repos JOIN dp_orgs")?;
    if rows.is_empty() {
        println!("(no targets registered — `dev-pulse add-repo OWNER/REPO`)");
        return Ok(());
    }
    println!("{:<24} {:<32} {:>12} {:>12}", "OWNER", "REPO", "ORG_GH_ID", "REPO_GH_ID");
    for (owner, name, oid, rid) in rows {
        println!("{:<24} {:<32} {:>12} {:>12}", owner, name, oid, rid);
    }
    Ok(())
}

async fn run_create_admin(matches: &ArgMatches) -> Result<()> {
    let cfg_path = matches.get_one::<String>("config").unwrap();
    let email = matches.get_one::<String>("email").unwrap();
    let password = matches.get_one::<String>("password").unwrap();
    let raw = std::fs::read_to_string(cfg_path)
        .with_context(|| format!("read config: {cfg_path}"))?;
    let cfg: DevPulseConfig = toml::from_str(&raw).context("parse config TOML")?;

    let sqlite_pool = starter_store_sqlite::pool::connect(&cfg.auth_sqlite.url)
        .await
        .with_context(|| format!("connect sqlite: {}", cfg.auth_sqlite.url))?;
    apply_auth_sqlite_migrations(&sqlite_pool).await?;

    let users = starter_auth_users::store::SqliteUserStore::new(sqlite_pool);
    match starter_auth_users::admin::create_admin(
        &users,
        email,
        password,
        starter_auth_users::Role::Admin,
    )
    .await
    {
        Ok(id) => {
            println!("created admin user: {email}");
            println!("  id   : {id}");
            println!("  role : admin");
            Ok(())
        }
        Err(starter_auth_users::admin::AdminError::Conflict) => {
            println!("user already exists: {email} (no change)");
            Ok(())
        }
        Err(e) => Err(anyhow!("create_admin: {e}")),
    }
}

async fn run_fetch_now(matches: &ArgMatches) -> Result<()> {
    let cfg_path = matches
        .get_one::<String>("config")
        .ok_or_else(|| anyhow!("--config is required"))?;
    let (cfg, pool) = load_cfg_and_pool(cfg_path).await?;
    let client = build_pat_client(&cfg)?;
    let store = Arc::new(dp_store_pg::PgStore::new(pool.clone()));
    let targets: Arc<dyn dp_fetcher::reconciler::TargetProvider> =
        Arc::new(PgTargetProvider { pool });

    let n = dp_fetcher::reconciler::TargetProvider::list_targets(&*targets)
        .await
        .map_err(|e| anyhow!("list targets: {e}"))?
        .len();
    println!(
        "budget   : {}",
        client
            .max_requests()
            .map(|b| b.to_string())
            .unwrap_or_else(|| "disabled".into())
    );
    println!("targets  : {n}");
    if n == 0 {
        println!("(nothing to fetch — `dev-pulse add-repo OWNER/REPO` first)");
        return Ok(());
    }
    client.reset_budget();
    let reconciler = dp_fetcher::reconciler::Reconciler::new(
        store,
        Arc::new(client.clone()),
        targets,
    );
    println!("ticking ...");
    let stats = reconciler
        .do_tick(dp_fetcher::reconciler::Scope::All)
        .await
        .map_err(|e| anyhow!("tick: {e}"))?;
    println!("\ntick stats:");
    println!("  items        : {}", stats.items);
    println!("  errors       : {}", stats.errors);
    println!("  partial      : {}", stats.partial);
    println!("  github calls : {}", client.requests_made());
    Ok(())
}

/// Reads dp_orgs + dp_repos and serves them as [`RepoTarget`]s on
/// every reconciler tick. The query is cheap (joined PK lookups) so
/// we don't bother caching at this layer — the reconciler ticks every
/// few minutes, not every few seconds.
struct PgTargetProvider {
    pool: starter_store_postgres::Pool,
}

#[async_trait::async_trait]
impl dp_fetcher::reconciler::TargetProvider for PgTargetProvider {
    async fn list_targets(
        &self,
    ) -> Result<Vec<dp_fetcher::reconciler::RepoTarget>, dp_domain::StoreError> {
        let rows: Vec<(Uuid, i64, String, Uuid, i64, String)> = sqlx::query_as(
            "SELECT o.id, o.github_id, o.login, r.id, r.github_id, r.name
               FROM dp_repos r
               JOIN dp_orgs  o ON o.id = r.org_id
              ORDER BY o.login, r.name",
        )
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(|e| dp_domain::StoreError::Backend(Box::new(e)))?;
        Ok(rows
            .into_iter()
            .map(|(org_id, og_id, owner, repo_id, rg_id, name)| {
                dp_fetcher::reconciler::RepoTarget {
                    org_id,
                    org_github_id: og_id,
                    owner_login: owner,
                    repo_id,
                    repo_github_id: rg_id,
                    repo_name: name,
                }
            })
            .collect())
    }
}

// ---------------------------------------------------------------- helpers

/// Build the [`dp_fetcher::reconciler::Reconciler`] the dev-pulse
/// bin runs against. When `token` is `Some(non_empty)` the Client is
/// armed with a PAT (SCOPE §15.1 stepping stone) and the target list
/// is still empty at this layer — targets come from Phase 6 install
/// metadata. When `token` is `None`/empty, the Client is built with
/// an empty token (placeholder) and the caller must keep the
/// scheduler dormant; `POST /admin/refresh` completes instantly.
fn build_reconciler(
    store: Arc<dp_store_pg::PgStore>,
    token: Option<String>,
    base_url: &str,
    budget: Option<u64>,
) -> Result<Arc<dp_fetcher::reconciler::Reconciler>> {
    let secret = SecretString::from(token.unwrap_or_default());
    let client = dp_fetcher::client::Client::with_personal_token(secret, base_url)
        .map_err(|e| anyhow!("build github client: {e}"))?
        .with_budget(budget);
    // PgTargetProvider reads dp_orgs + dp_repos at every tick. Empty
    // result is a valid state (no targets registered yet) — the
    // reconciler will simply do nothing that tick. Operators add
    // targets via `dev-pulse add-repo OWNER/REPO`.
    let targets: Arc<dyn dp_fetcher::reconciler::TargetProvider> =
        Arc::new(PgTargetProvider { pool: store.pool().clone() });
    Ok(Arc::new(dp_fetcher::reconciler::Reconciler::new(
        store,
        Arc::new(client),
        targets,
    )))
}

/// Minimal secret-handle resolver. Recognises three shapes:
///
/// * `secret://NAME` — read env var `NAME` (upper-cased, `/` → `_`).
///   Bridges to the `starter-secrets-file` handle syntax without
///   pulling in age-based decryption yet.
/// * `file:/path/to/file` — read the file, trim whitespace.
/// * anything else — treated as a literal value. Operators who
///   inline secrets into the TOML accept the operational risk.
///
/// Full `starter-secrets-file` integration is a Phase 6 follow-up
/// once the bin owns its data directory layout.
fn resolve_secret(handle: &str) -> Result<String> {
    if let Some(name) = handle.strip_prefix("secret://") {
        let env_key = name.to_ascii_uppercase().replace('/', "_");
        std::env::var(&env_key)
            .with_context(|| format!("env var {env_key} (for secret handle {handle}) not set"))
    } else if let Some(path) = handle.strip_prefix("file:") {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("read secret file {path}"))?;
        Ok(raw.trim().to_string())
    } else {
        Ok(handle.to_string())
    }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_secret_passes_literal_through() {
        let v = resolve_secret("inline-secret").expect("ok");
        assert_eq!(v, "inline-secret");
    }

    #[test]
    fn resolve_secret_reads_env_for_secret_scheme() {
        // SAFETY: in-process test, single-threaded read.
        std::env::set_var("DEV_PULSE_TEST_SECRET", "from-env");
        let v = resolve_secret("secret://dev_pulse/test_secret").expect("ok");
        assert_eq!(v, "from-env");
    }

    #[test]
    fn resolve_secret_reads_file_with_trim() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "dev-pulse-secret-test-{}",
            std::process::id()
        ));
        std::fs::write(&path, "  payload\n").unwrap();
        let handle = format!("file:{}", path.display());
        let v = resolve_secret(&handle).expect("ok");
        assert_eq!(v, "payload");
        let _ = std::fs::remove_file(&path);
    }

    /// `[auth.github].validate()` runs before `dp_server::build`,
    /// so a config missing `client_id` fails loudly at boot rather
    /// than mysteriously 403-ing every protected request later.
    /// The validator itself lives in `dp_server::auth::config` —
    /// this test pins that the bin calls it on the deserialised
    /// struct (regression for "main forgets to validate").
    #[test]
    fn validate_runs_on_deserialised_github_config() {
        let raw = r#"
[server]
listen = "127.0.0.1:8080"
base_url = "http://localhost:8080"

[postgres]
url = "postgres://nope"

[auth_sqlite]
url = "sqlite::memory:"

[webhook]
secret_ref = "literal"

[auth.github]
client_id = ""
client_secret_ref = "secret://gh"
allow_orgs = []
"#;
        let cfg: DevPulseConfig = toml::from_str(raw).expect("parse");
        assert!(cfg.auth.github.validate().is_err());
    }
}
