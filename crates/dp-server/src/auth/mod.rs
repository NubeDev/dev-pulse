//! Phase 4 stage 9 — GitHub OAuth + authz composition wiring.
//!
//! This module is `dp_server`'s public seam for everything the
//! `starter-auth-oauth` + `starter-authz` integration needs from
//! dev-pulse:
//!
//! * [`config::GitHubAuthConfig`] — the `[auth.github]` block from
//!   `dp-config` (client id + secret reference + allow-list +
//!   refresh interval). The bin layer parses TOML / env into this
//!   struct and hands it down.
//! * [`github_orgs::GithubOrgsStamper`] — the
//!   `starter_auth_users::PrincipalExtrasLookup` impl that augments
//!   the standard `oauth.*` block (provided by
//!   `starter_auth_oauth::OAuthPrincipalExtras`) with two extra
//!   fields the authz policy keys on:
//!     - `oauth.github_orgs`: `Vec<String>` — the GitHub org login
//!       list from `GET /user/orgs`. Per `starter-authz` R8 the
//!       attribute bus stamps every authenticated request with this.
//!     - `oauth.in_allowed_org`: `bool` — `true` iff
//!       `github_orgs ∩ auth.github.allow_orgs ≠ ∅`. The policy
//!       file (`crates/dp-server/policy/dev-pulse.toml`) keys its
//!       single allow rule on this boolean because the `starter-
//!       authz::condition` grammar has no `intersects` operator;
//!       stamping the derived boolean once at session-mint is the
//!       in-bounds equivalent.
//! * [`github_orgs::GithubOrgsSource`] — a tiny trait the stamper
//!   calls to fetch the org list for a given operator. Two impls
//!   ship here:
//!     - [`github_orgs::StaticGithubOrgsSource`] — tests + the
//!       initial bin-layer placeholder. Pre-seeded `(user_id ->
//!       org-list)` map.
//!     - The real `OctocrabGithubOrgsSource` belongs in the bin
//!       layer because constructing it requires resolving the
//!       per-user access token from a secrets store; doing it here
//!       would drag `starter-secrets-file` into the composition
//!       crate. The trait keeps the seam open.
//! * [`github_orgs::CachedGithubOrgsSource`] — TTL-cache wrapper.
//!   One source call per session-mint; cached entries refreshed
//!   lazily per `auth.github.org_refresh_interval` (default 1h).
//!   Never on the request hot path — the stamper hits the cache
//!   first and only calls the underlying source on miss / expiry.
//! * [`policy::load_static_engine`] — load
//!   `starter-authz::StaticRbacEngine` from
//!   `crates/dp-server/policy/dev-pulse.toml`. Returns
//!   `Arc<dyn PolicyEngine>` so dp-server's [`crate::AppState`]
//!   stays impl-agnostic. The wrapper also translates the engine's
//!   generic `no_matching_rule` deny reason into the SCOPE D4.2
//!   `awaiting_access` reason so the on-the-wire 403 error code is
//!   stable for clients.
//! * [`policy::register_dev_pulse_resources`] — registers every
//!   resource kind dp-rest uses (`reports`, `users`, `orgs`,
//!   `teams`, `home_org`, `admin`) on the supplied
//!   `StaticRegistry`. Unknown kinds short-circuit to
//!   `Decision::Deny { reason: "unknown_resource" }` per
//!   `starter-authz` R3, so missing one of these would silently
//!   break a route even with a valid session.

pub mod config;
pub mod github_orgs;
pub mod policy;

pub use config::{GitHubAuthConfig, GitHubAuthConfigError};
pub use github_orgs::{
    CachedGithubOrgsSource, GithubOrgsError, GithubOrgsSource, GithubOrgsStamper,
    StaticGithubOrgsSource,
};
pub use policy::{
    load_static_engine, register_dev_pulse_resources, AwaitingAccessEngine, PolicyLoadError,
    AWAITING_ACCESS_REASON,
};
