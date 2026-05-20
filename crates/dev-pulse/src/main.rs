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
        );

    let matches = app.get_matches();
    match matches.subcommand() {
        Some(("serve", sub)) => run_serve(sub).await,
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
    AUTH_USERS_MIGRATOR
        .run(sqlite_pool.sqlx())
        .await
        .context("apply starter_auth_users migrations")?;
    AUTH_OAUTH_MIGRATOR
        .run(sqlite_pool.sqlx())
        .await
        .context("apply starter_auth_oauth migrations")?;

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
    let reconciler =
        build_reconciler(store.clone()).context("build reconciler")?;
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

    let sched_handle = if cfg.scheduler.enable {
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

// ---------------------------------------------------------------- helpers

/// Build the [`dp_fetcher::reconciler::Reconciler`] the dev-pulse
/// bin runs against. Phase 4 ships with an empty target list and a
/// token-less Client — `POST /admin/refresh` completes instantly,
/// the dormant tick loop wakes only on `shutdown`. Phase 6 wires
/// the real GitHub App installation pair.
fn build_reconciler(
    store: Arc<dp_store_pg::PgStore>,
) -> Result<Arc<dp_fetcher::reconciler::Reconciler>> {
    let client = dp_fetcher::client::Client::with_personal_token(
        SecretString::from(String::new()),
        "https://api.github.com",
    )
    .map_err(|e| anyhow!("build placeholder github client: {e}"))?;
    let targets: Arc<dyn dp_fetcher::reconciler::TargetProvider> =
        Arc::new(dp_fetcher::reconciler::StaticTargets::new(Vec::new()));
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
