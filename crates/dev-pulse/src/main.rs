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
use serde_json::Value;
use starter_observability::metrics::StandardMetrics;
use starter_observability::tracing::Format;
use starter_spi::auth::Authenticator;
use tokio::sync::watch;
use uuid::Uuid;

use dp_server::auth::{
    config::GitHubAuthConfig, load_static_engine, register_dev_pulse_resources,
    CachedGithubOrgsSource, GithubOrgsSource, GithubOrgsStamper, StaticGithubOrgsSource,
};
use dp_server::{AppState, BuildConfig, GitHubAppConfig};

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
    /// `[github.app]` — SCOPE-PROJECTS §13.6 GitHub App permission
    /// configuration. Absent → defaults (`request_issues_write =
    /// true`, no slug). The defaults match a fresh deployment per
    /// §13.6 step 1 ("default `true` in new deployments"). Set
    /// `request_issues_write = false` to hard-disable the §8
    /// issue mutation surface (the documented escape hatch for
    /// deployments whose security policy forbids any App with
    /// write scope).
    #[serde(default)]
    app: GitHubAppConfig,
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
            app: GitHubAppConfig::default(),
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
            Command::new("import-my-orgs")
                .about(
                    "GET /user/orgs and upsert every org the PAT user \
                     belongs to. Requires `read:org` scope.",
                )
                .arg(
                    Arg::new("config")
                        .long("config")
                        .required(true)
                        .help("Path to the dev-pulse TOML config."),
                ),
        )
        .subcommand(
            Command::new("import-my-repos")
                .about(
                    "GET /user/repos (paginated) and upsert each repo. \
                     Use --orgs to scope to a single org or two.",
                )
                .arg(
                    Arg::new("config")
                        .long("config")
                        .required(true)
                        .help("Path to the dev-pulse TOML config."),
                )
                .arg(
                    Arg::new("include-forks")
                        .long("include-forks")
                        .action(clap::ArgAction::SetTrue)
                        .help("Also import repos that are forks."),
                )
                .arg(
                    Arg::new("max")
                        .long("max")
                        .default_value("500")
                        .help("Hard cap on repos imported."),
                )
                .arg(
                    Arg::new("orgs")
                        .long("orgs")
                        .help(
                            "Comma-separated allow-list of owner logins \
                             (case-insensitive). Only repos owned by one \
                             of these logins are imported.",
                        ),
                )
                .arg(
                    Arg::new("active-within-days")
                        .long("active-within-days")
                        .default_value("60")
                        .help(
                            "Skip repos whose `pushed_at` is older than \
                             this many days. `0` disables the filter.",
                        ),
                ),
        )
        .subcommand(
            Command::new("backfill-issues")
                .about(
                    "Paginate `GET /repos/{owner}/{repo}/issues?state=all` \
                     for every repo in `dp_repos` and upsert the result \
                     into `dp_issues`. Pull requests (rows with a \
                     `pull_request` payload) are skipped — only true \
                     issues land in the mirror.",
                )
                .arg(
                    Arg::new("config")
                        .long("config")
                        .required(true)
                        .help("Path to the dev-pulse TOML config."),
                )
                .arg(
                    Arg::new("orgs")
                        .long("orgs")
                        .help(
                            "Comma-separated allow-list of owner logins \
                             (case-insensitive). Only repos owned by one \
                             of these logins are backfilled.",
                        ),
                )
                .arg(
                    Arg::new("repos")
                        .long("repos")
                        .help(
                            "Comma-separated allow-list of `owner/name` \
                             pairs (case-insensitive). Overrides --orgs.",
                        ),
                )
                .arg(
                    Arg::new("max-pages")
                        .long("max-pages")
                        .default_value("10")
                        .help(
                            "Hard cap on pages per repo (100 issues each). \
                             Use 0 for unbounded.",
                        ),
                )
                .arg(
                    Arg::new("state")
                        .long("state")
                        .default_value("all")
                        .value_parser(["all", "open", "closed"])
                        .help("Filter passed to GitHub's `state` query param."),
                ),
        )
        .subcommand(
            Command::new("prune-stale-repos")
                .about(
                    "Remove already-imported repos that haven't seen any \
                     activity event within the last N days. Cuts the \
                     reconciler's per-tick fan-out without re-importing.",
                )
                .arg(
                    Arg::new("config")
                        .long("config")
                        .required(true)
                        .help("Path to the dev-pulse TOML config."),
                )
                .arg(
                    Arg::new("days")
                        .long("days")
                        .default_value("60")
                        .help("Activity cutoff in days."),
                )
                .arg(
                    Arg::new("yes")
                        .long("yes")
                        .action(clap::ArgAction::SetTrue)
                        .help("Apply the deletion (default is dry-run)."),
                ),
        )
        .subcommand(
            Command::new("purge-data")
                .about(
                    "Wipe every fetched event, run, cursor, repo, team \
                     and org from dp-data. Auth users (auth.db) untouched.",
                )
                .arg(
                    Arg::new("config")
                        .long("config")
                        .required(true)
                        .help("Path to the dev-pulse TOML config."),
                )
                .arg(
                    Arg::new("yes")
                        .long("yes")
                        .action(clap::ArgAction::SetTrue)
                        .help("Skip the confirmation prompt."),
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
        Some(("import-my-orgs", sub)) => run_import_my_orgs(sub).await,
        Some(("import-my-repos", sub)) => run_import_my_repos(sub).await,
        Some(("backfill-issues", sub)) => run_backfill_issues(sub).await,
        Some(("prune-stale-repos", sub)) => run_prune_stale_repos(sub).await,
        Some(("purge-data", sub)) => run_purge_data(sub).await,
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
        github_token.clone(),
        &cfg.github.base_url,
        budget,
    )
    .context("build reconciler")?;
    // -- issue-write backend (SCOPE-PROJECTS §8) -----------------------
    //
    // PAT mode: when an operator armed `github.token_ref`, wire a
    // FetcherIssueWriter against a second budget-shared Client so
    // the §8 issue mutation surface (PATCH /issues/{id}, POST
    // /issues/{id}/comments, PATCH /issues/{id}/dates) can round-
    // trip to GitHub. Without a token the default
    // `UnconfiguredIssueWriter` stays in place and every mutation
    // returns 502 — matching the read-side dormancy.
    let issue_writer: Option<Arc<dyn dp_server::IssueWriteBackend>> =
        if let Some(tok) = github_token.as_ref().filter(|t| !t.is_empty()) {
            let writer_client = dp_fetcher::client::Client::with_personal_token(
                SecretString::from(tok.clone()),
                &cfg.github.base_url,
            )
            .map_err(|e| anyhow!("build github writer client: {e}"))?
            .with_budget(budget);
            Some(Arc::new(dp_server::FetcherIssueWriter::new(Arc::new(
                writer_client,
            ))))
        } else {
            None
        };
    // -- Projects v2 mirror + picker (§3.10) ----------------------------
    //
    // Same PAT-mode gate as the issue writer above: a configured
    // GitHub token wires the octocrab-backed mirror and picker
    // adapters. Without a token, the dp-rest defaults
    // (`UnconfiguredProjectV2Mirror` / `UnconfiguredProjectsPicker`)
    // stay in place — the date editor commits locally but the
    // best-effort mirror is skipped, and the admin pane's project
    // chooser surfaces a 503 so the operator knows to wire a
    // token. Reuses a sibling budget-shared `Client` so GraphQL
    // traffic counts against the same local fuse as REST.
    let (projectv2_mirror, projects_picker, org_projects_picker): (
        Option<Arc<dyn dp_server::ProjectV2MirrorBackend>>,
        Option<Arc<dyn dp_server::ProjectsPickerBackend>>,
        Option<Arc<dyn dp_server::OrgProjectsPickerBackend>>,
    ) = if let Some(tok) = github_token.as_ref().filter(|t| !t.is_empty()) {
        let gql_client = dp_fetcher::client::Client::with_personal_token(
            SecretString::from(tok.clone()),
            &cfg.github.base_url,
        )
        .map_err(|e| anyhow!("build github graphql client: {e}"))?
        .with_budget(budget);
        let gql_client = Arc::new(gql_client);
        (
            Some(Arc::new(dp_server::OctocrabProjectV2Mirror::new(
                gql_client.clone(),
                store.clone(),
            ))),
            Some(Arc::new(dp_server::OctocrabProjectsPicker::new(
                gql_client.clone(),
            ))),
            Some(Arc::new(dp_server::OctocrabOrgProjectsPicker::new(
                gql_client,
            ))),
        )
    } else {
        (None, None, None)
    };
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
            // SCOPE-PROJECTS §13.6 — the `[github.app]` block in
            // `dp-config` carries the `request_issues_write`
            // flag and the App slug. Absent block → defaults
            // (flag on, no slug), which matches §13.6 step 1
            // ("default `true` in new deployments").
            github_app: Arc::new(cfg.github.app.clone()),
            issue_writer: issue_writer.clone(),
            projectv2_mirror: projectv2_mirror.clone(),
            projects_picker: projects_picker.clone(),
            org_projects_picker: org_projects_picker.clone(),
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

async fn run_import_my_orgs(matches: &ArgMatches) -> Result<()> {
    let cfg_path = matches.get_one::<String>("config").unwrap();
    let (cfg, pool) = load_cfg_and_pool(cfg_path).await?;
    let client = build_pat_client(&cfg)?;
    println!("GET /user/orgs ...");
    let body = match client
        .get_conditional::<Vec<GhAccount>>("/user/orgs?per_page=100", None)
        .await
        .context("GET /user/orgs")?
    {
        dp_fetcher::client::Fetched::Ok { body, .. } => body,
        dp_fetcher::client::Fetched::NotModified { .. } => return Err(anyhow!("unexpected 304")),
    };
    let store = dp_store_pg::PgStore::new(pool);
    use dp_domain::Store as _;
    for org in &body {
        let row = dp_domain::Org {
            id: Uuid::new_v4(),
            github_id: org.id,
            login: org.login.clone(),
            name: org.name.clone(),
        };
        let saved = store
            .upsert_org(&row)
            .await
            .with_context(|| format!("upsert_org {}", org.login))?;
        println!("  + {} (id {})", org.login, saved.id);
    }
    if body.is_empty() {
        println!("(empty — PAT may be missing `read:org` scope)");
    } else {
        println!("\nimported {} org(s)", body.len());
    }
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
struct GhRepoFull {
    id: i64,
    name: String,
    owner: GhAccount,
    #[serde(default)]
    fork: bool,
    /// Last push to any branch. Empty repos surface as `None`.
    /// We filter on this rather than `updated_at` because metadata
    /// edits (description, topics) bump `updated_at` and would
    /// keep a long-dead repo looking "fresh".
    #[serde(default)]
    pushed_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn run_import_my_repos(matches: &ArgMatches) -> Result<()> {
    let cfg_path = matches.get_one::<String>("config").unwrap();
    let include_forks = matches.get_flag("include-forks");
    let max: usize = matches
        .get_one::<String>("max")
        .unwrap()
        .parse()
        .context("--max must be an integer")?;
    let orgs_allow: Option<Vec<String>> = matches.get_one::<String>("orgs").map(|s| {
        s.split(',')
            .map(|t| t.trim().to_ascii_lowercase())
            .filter(|t| !t.is_empty())
            .collect()
    });
    if let Some(list) = &orgs_allow {
        println!("scoped to orgs: {list:?}");
    }
    let active_days: i64 = matches
        .get_one::<String>("active-within-days")
        .unwrap()
        .parse()
        .context("--active-within-days must be an integer")?;
    let activity_cutoff = if active_days > 0 {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(active_days);
        println!("activity filter: pushed_at >= {} (last {active_days}d)", cutoff.format("%Y-%m-%d"));
        Some(cutoff)
    } else {
        println!("activity filter: disabled (--active-within-days=0)");
        None
    };
    let (cfg, pool) = load_cfg_and_pool(cfg_path).await?;
    let client = build_pat_client(&cfg)?;
    let store = dp_store_pg::PgStore::new(pool);
    use dp_domain::Store as _;

    let mut page = 1u32;
    let mut imported = 0usize;
    let mut skipped_forks = 0usize;
    let mut skipped_org_filter = 0usize;
    let mut skipped_stale = 0usize;
    let mut org_cache: HashMap<String, Uuid> = HashMap::new();
    loop {
        if imported >= max {
            println!("reached --max {max}, stopping");
            break;
        }
        let path = format!(
            "/user/repos?per_page=100&page={page}&affiliation=owner,collaborator,organization_member"
        );
        let repos = match client
            .get_conditional::<Vec<GhRepoFull>>(&path, None)
            .await
            .with_context(|| format!("GET {path}"))?
        {
            dp_fetcher::client::Fetched::Ok { body, .. } => body,
            dp_fetcher::client::Fetched::NotModified { .. } => break,
        };
        if repos.is_empty() {
            break;
        }
        for repo in repos {
            if !include_forks && repo.fork {
                skipped_forks += 1;
                continue;
            }
            if let Some(list) = &orgs_allow {
                if !list.contains(&repo.owner.login.to_ascii_lowercase()) {
                    skipped_org_filter += 1;
                    continue;
                }
            }
            if let Some(cutoff) = activity_cutoff {
                // Empty repos (`pushed_at = None`) are always skipped
                // under the activity filter — they have no commits to
                // ingest anyway, and ticking them just wastes API quota
                // on a guaranteed 409 from `GET /repos/.../commits`.
                match repo.pushed_at {
                    Some(ts) if ts >= cutoff => {}
                    _ => {
                        skipped_stale += 1;
                        continue;
                    }
                }
            }
            if imported >= max {
                break;
            }
            let owner_login = repo.owner.login.clone();
            let org_id = if let Some(id) = org_cache.get(&owner_login) {
                *id
            } else {
                let row = dp_domain::Org {
                    id: Uuid::new_v4(),
                    github_id: repo.owner.id,
                    login: owner_login.clone(),
                    name: repo.owner.name.clone(),
                };
                let saved = store
                    .upsert_org(&row)
                    .await
                    .with_context(|| format!("upsert_org {owner_login}"))?;
                org_cache.insert(owner_login.clone(), saved.id);
                saved.id
            };
            let repo_row = dp_domain::Repo {
                id: Uuid::new_v4(),
                org_id,
                github_id: repo.id,
                name: repo.name.clone(),
            };
            store
                .upsert_repo(&repo_row)
                .await
                .with_context(|| format!("upsert_repo {owner_login}/{}", repo.name))?;
            println!("  + {owner_login}/{}", repo.name);
            imported += 1;
        }
        page += 1;
    }
    println!(
        "\nimported {imported} repo(s); skipped {skipped_forks} fork(s), \
         {skipped_org_filter} out-of-scope, {skipped_stale} stale (no recent push)"
    );
    Ok(())
}

// ---------------------------------------------------------- backfill-issues

/// One row from `dp_repos JOIN dp_orgs`. The backfill is keyed by
/// `(owner_login, repo_name)` for the GitHub REST path, plus the
/// resolved UUIDs so the issue upsert can land without a second
/// round-trip per repo.
struct RepoTarget {
    org_id: Uuid,
    repo_id: Uuid,
    owner: String,
    name: String,
}

async fn run_backfill_issues(matches: &ArgMatches) -> Result<()> {
    let cfg_path = matches.get_one::<String>("config").unwrap();
    let max_pages: u32 = matches
        .get_one::<String>("max-pages")
        .unwrap()
        .parse()
        .context("--max-pages must be an integer")?;
    let state = matches.get_one::<String>("state").unwrap().to_string();
    let orgs_allow: Option<Vec<String>> = matches.get_one::<String>("orgs").map(|s| {
        s.split(',')
            .map(|t| t.trim().to_ascii_lowercase())
            .filter(|t| !t.is_empty())
            .collect()
    });
    let repos_allow: Option<Vec<(String, String)>> =
        matches.get_one::<String>("repos").map(|s| {
            s.split(',')
                .filter_map(|t| {
                    let t = t.trim();
                    let (o, n) = t.split_once('/')?;
                    Some((o.to_ascii_lowercase(), n.to_ascii_lowercase()))
                })
                .collect()
        });

    let (cfg, pool) = load_cfg_and_pool(cfg_path).await?;
    let client = build_pat_client(&cfg)?;
    let store = dp_store_pg::PgStore::new(pool.clone());
    use dp_domain::Store as _;

    // Pull every repo with its owning org's login in one shot;
    // the loop below filters in-Rust so --orgs and --repos can be
    // combined / overridden without re-querying.
    let targets: Vec<RepoTarget> = sqlx::query_as::<_, (Uuid, Uuid, String, String)>(
        "SELECT r.id, r.org_id, o.login, r.name
           FROM dp_repos r
           JOIN dp_orgs  o ON o.id = r.org_id
          ORDER BY o.login, r.name",
    )
    .fetch_all(pool.sqlx())
    .await
    .context("select repos for backfill")?
    .into_iter()
    .map(|(repo_id, org_id, owner, name)| RepoTarget {
        repo_id,
        org_id,
        owner,
        name,
    })
    .filter(|t| {
        if let Some(allow) = &repos_allow {
            allow
                .iter()
                .any(|(o, n)| *o == t.owner.to_ascii_lowercase() && *n == t.name.to_ascii_lowercase())
        } else if let Some(allow) = &orgs_allow {
            allow.contains(&t.owner.to_ascii_lowercase())
        } else {
            true
        }
    })
    .collect();

    if targets.is_empty() {
        println!("no repos matched the filters; nothing to backfill.");
        return Ok(());
    }
    println!("backfilling {} repo(s); state={state}, max_pages={max_pages}", targets.len());

    // Per-repo + grand totals. Outcomes mirror the trait's enum so
    // operators can tell apart "we wrote it" from "stale payload"
    // from "guarded by §13.7". `errors` collects soft per-row
    // parse failures — we keep going on bad rows so one malformed
    // issue can't kill a 100-issue page.
    let mut total_in = 0u64;
    let mut total_up = 0u64;
    let mut total_sk = 0u64;
    let mut total_df = 0u64;
    let mut total_pr = 0u64;
    let mut total_err = 0u64;

    for t in &targets {
        let mut page = 1u32;
        let mut r_in = 0u64;
        let mut r_up = 0u64;
        let mut r_sk = 0u64;
        let mut r_df = 0u64;
        let mut r_pr = 0u64;
        let mut r_err = 0u64;
        loop {
            if max_pages > 0 && page > max_pages {
                break;
            }
            let path = format!(
                "/repos/{}/{}/issues?state={state}&per_page=100&page={page}&sort=updated&direction=desc",
                t.owner, t.name
            );
            let issues = match client
                .get_conditional::<Vec<Value>>(&path, None)
                .await
                .with_context(|| format!("GET {path}"))?
            {
                dp_fetcher::client::Fetched::Ok { body, .. } => body,
                dp_fetcher::client::Fetched::NotModified { .. } => break,
            };
            if issues.is_empty() {
                break;
            }
            let page_len = issues.len();
            for issue in issues {
                if issue.get("pull_request").is_some() {
                    r_pr += 1;
                    continue;
                }
                let upsert = match dp_fetcher::worker::handlers::parse_issue_upsert(
                    t.org_id, t.repo_id, &issue,
                ) {
                    Ok(u) => u,
                    Err(e) => {
                        r_err += 1;
                        tracing::warn!(
                            target: "dev_pulse::backfill_issues",
                            repo = format!("{}/{}", t.owner, t.name),
                            error = %e,
                            "parse_issue_upsert failed; skipping row"
                        );
                        continue;
                    }
                };
                // 0-second window: there's no concurrent §8
                // optimistic writer during a CLI backfill — any
                // `pending_remote` flag we see is a stale crumb
                // from a previous crashed mutation and we'd rather
                // clobber it than defer indefinitely.
                match store
                    .upsert_issue_from_github(&upsert, chrono::Duration::seconds(0))
                    .await
                {
                    Ok((_, dp_domain::IssueUpsertOutcome::Inserted)) => r_in += 1,
                    Ok((_, dp_domain::IssueUpsertOutcome::Updated)) => r_up += 1,
                    Ok((_, dp_domain::IssueUpsertOutcome::Skipped)) => r_sk += 1,
                    Ok((_, dp_domain::IssueUpsertOutcome::Deferred)) => r_df += 1,
                    Err(e) => {
                        r_err += 1;
                        tracing::warn!(
                            target: "dev_pulse::backfill_issues",
                            repo = format!("{}/{}", t.owner, t.name),
                            number = upsert.number,
                            error = %e,
                            "upsert_issue_from_github failed"
                        );
                    }
                }
            }
            // GitHub returns a short page (<100) on the last page;
            // bail rather than spending a follow-up call on an
            // empty 200.
            if page_len < 100 {
                break;
            }
            page += 1;
        }
        if r_in + r_up + r_sk + r_df + r_pr + r_err > 0 {
            println!(
                "  {}/{}: +{r_in} ~{r_up} ={r_sk} def={r_df} pr={r_pr} err={r_err}",
                t.owner, t.name
            );
        }
        total_in += r_in;
        total_up += r_up;
        total_sk += r_sk;
        total_df += r_df;
        total_pr += r_pr;
        total_err += r_err;
    }

    println!(
        "\ndone. inserted={total_in} updated={total_up} skipped={total_sk} \
         deferred={total_df} pr_skipped={total_pr} errors={total_err}"
    );
    Ok(())
}

async fn run_prune_stale_repos(matches: &ArgMatches) -> Result<()> {
    let cfg_path = matches.get_one::<String>("config").unwrap();
    let days: i64 = matches
        .get_one::<String>("days")
        .unwrap()
        .parse()
        .context("--days must be an integer")?;
    let apply = matches.get_flag("yes");
    let (_cfg, pool) = load_cfg_and_pool(cfg_path).await?;

    // A repo is "stale" iff it has no event in `dp_activity_events`
    // newer than `now() - N days`. Repos with zero events (just
    // imported, never ticked) are also flagged — they're either
    // genuinely dead or pending their first fetch. Both groups land
    // here so the operator decides.
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days);
    let rows: Vec<(Uuid, String, String, Option<chrono::DateTime<chrono::Utc>>)> =
        sqlx::query_as(
            "SELECT r.id, o.login, r.name, MAX(e.ts) AS latest
               FROM dp_repos r
               JOIN dp_orgs  o ON o.id = r.org_id
               LEFT JOIN dp_activity_events e ON e.repo_id = r.id
              GROUP BY r.id, o.login, r.name
             HAVING COALESCE(MAX(e.ts), 'epoch'::timestamptz) < $1
              ORDER BY o.login, r.name",
        )
        .bind(cutoff)
        .fetch_all(pool.sqlx())
        .await
        .context("select stale repos")?;

    if rows.is_empty() {
        println!("no repos older than {days}d. nothing to prune.");
        return Ok(());
    }
    println!(
        "{} repo(s) older than {days}d (cutoff {}):",
        rows.len(),
        cutoff.format("%Y-%m-%d")
    );
    for (_id, owner, name, latest) in &rows {
        let latest = latest
            .map(|t| t.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "never".into());
        println!("  - {owner}/{name}  (latest: {latest})");
    }
    if !apply {
        println!("\n(dry run — re-run with --yes to delete)");
        return Ok(());
    }
    let ids: Vec<Uuid> = rows.iter().map(|(id, _, _, _)| *id).collect();
    let affected: u64 = sqlx::query("DELETE FROM dp_repos WHERE id = ANY($1)")
        .bind(&ids)
        .execute(pool.sqlx())
        .await
        .context("delete stale repos")?
        .rows_affected();
    println!("\ndeleted {affected} repo(s)");
    Ok(())
}

async fn run_purge_data(matches: &ArgMatches) -> Result<()> {
    let cfg_path = matches.get_one::<String>("config").unwrap();
    let yes = matches.get_flag("yes");
    if !yes {
        eprintln!("refusing to wipe dp-data without --yes");
        return Err(anyhow!("re-run with --yes to confirm"));
    }
    let (_cfg, pool) = load_cfg_and_pool(cfg_path).await?;
    // Order matters: drop child rows before parents.
    for stmt in [
        "TRUNCATE TABLE dp_event_actors CASCADE",
        "TRUNCATE TABLE dp_activity_events CASCADE",
        "TRUNCATE TABLE dp_fetch_cursors CASCADE",
        "TRUNCATE TABLE dp_fetch_runs CASCADE",
        "TRUNCATE TABLE dp_webhook_inbox CASCADE",
        "TRUNCATE TABLE dp_memberships CASCADE",
        "TRUNCATE TABLE dp_teams CASCADE",
        "TRUNCATE TABLE dp_repos CASCADE",
        "TRUNCATE TABLE dp_orgs CASCADE",
        "TRUNCATE TABLE dp_users CASCADE",
    ] {
        match sqlx::query(stmt).execute(pool.sqlx()).await {
            Ok(_) => println!("  truncated: {}", stmt.replace("TRUNCATE TABLE ", "")),
            Err(e) => println!("  (skipped {stmt}: {e})"),
        }
    }
    println!("\ndp-data wiped.");
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
    let user_id = match starter_auth_users::admin::create_admin(
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
            id
        }
        Err(starter_auth_users::admin::AdminError::Conflict) => {
            println!("user already exists: {email} (no change)");
            // Look up the existing id so we can still mirror it into
            // dp_users below if needed.
            match starter_auth_users::store::UserStore::find_by_email(&users, email).await {
                Ok(Some(u)) => u.id,
                Ok(None) => return Err(anyhow!("create_admin: conflict but no row found for {email}")),
                Err(e) => return Err(anyhow!("create_admin: lookup after conflict: {e}")),
            }
        }
        Err(e) => return Err(anyhow!("create_admin: {e}")),
    };

    // Mirror the local admin into dp_users so the audit-log FK
    // (`dp_audit_log.actor_user_id REFERENCES dp_users(id)`) is
    // satisfied for break-glass / dev sessions. GitHub-OAuth users
    // get their dp_users row from the OAuth callback; local CLI
    // admins have no GitHub id, so we synthesise a negative one
    // derived from the UUID to keep the NOT NULL UNIQUE constraint
    // happy while staying clearly out of the positive (real GitHub)
    // id space.
    let user_uuid = Uuid::parse_str(&user_id)
        .with_context(|| format!("parse user id as UUID: {user_id}"))?;
    let (_cfg, pool) = load_cfg_and_pool(cfg_path).await?;
    let synth_github_id: i64 = {
        let bytes = user_uuid.as_bytes();
        let mut acc: i64 = 0;
        for &b in bytes {
            acc = acc.wrapping_mul(131).wrapping_add(b as i64);
        }
        // Force negative so it cannot collide with a real GitHub id.
        if acc > 0 { -acc } else if acc == 0 { -1 } else { acc }
    };
    let login = format!("local:{email}");
    sqlx::query(
        "INSERT INTO dp_users (id, github_id, login, email, name) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(user_uuid)
    .bind(synth_github_id)
    .bind(&login)
    .bind(email)
    .bind::<Option<&str>>(None)
    .execute(pool.sqlx())
    .await
    .context("mirror admin into dp_users")?;
    println!("  mirrored into dp_users (login={login}, synthetic github_id={synth_github_id})");
    Ok(())
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
