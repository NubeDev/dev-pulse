//! `Principal.extra.oauth.github_orgs` stamper + lazy cache.
//!
//! See the module-level docs in [`super`] for the wider picture.
//! This file owns the wire shape that the authz policy file keys
//! on.
//!
//! ## Wire shape stamped onto `Principal.extra`
//!
//! ```jsonc
//! {
//!   "oauth": {
//!     // ...fields written by the inner OAuthPrincipalExtras
//!     // (provider, provider_sub, email, email_domain, …) ride
//!     // along unchanged.
//!
//!     // Added by this stamper:
//!     "github_orgs": ["NubeIO", "another-org", ...],
//!     "in_allowed_org": true
//!   }
//! }
//! ```
//!
//! The stamper is composed in front of the standard
//! `starter_auth_oauth::OAuthPrincipalExtras`: when the
//! `PrincipalExtrasLookup` is asked for a user's extras, we call
//! the inner lookup, then — if it returned an `oauth` block —
//! merge our two extra fields onto it. Users without any linked
//! OAuth identity (rows-less in `oauth_identities`) get the same
//! `Value::Null` the inner lookup returns; the org-gate rule
//! therefore denies them (no `oauth.in_allowed_org == true`),
//! same as a user who is in the wrong orgs.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::{json, Value};
use starter_auth_users::{PrincipalExtrasError, PrincipalExtrasLookup};
use thiserror::Error;

use crate::auth::config::GitHubAuthConfig;

/// Errors a [`GithubOrgsSource`] impl can surface.
#[derive(Debug, Error)]
pub enum GithubOrgsError {
    /// The underlying GitHub call failed (transport, 401, rate
    /// limit, …). Surfaces as a 500 from the verify path — fail
    /// closed per `starter-auth-users::PrincipalExtrasLookup`'s
    /// contract: "impls should not bake in a fail-open fallback
    /// because doing so would hide a misconfigured identity-
    /// attribute source behind a silent empty Principal.extra."
    #[error("github orgs fetch failed: {0}")]
    Fetch(String),
    /// The operator is not registered with the source. Returned
    /// when the bin layer hasn't yet seen a session-mint for this
    /// user; the stamper treats it as "no orgs known yet" and
    /// emits an empty list rather than failing the request — the
    /// policy will deny on `in_allowed_org = false` and the next
    /// session-mint will populate the source.
    #[error("github orgs not known for user {0}")]
    Unknown(String),
}

/// One-shot fetch of an operator's GitHub org logins. Pluggable
/// so the dev-pulse bin can swap in an octocrab-backed impl
/// (using the operator's stored OAuth access token) while tests +
/// the placeholder bin path use [`StaticGithubOrgsSource`].
///
/// `user_id` is the `starter-auth-users` user id (string) — the
/// same value `PrincipalExtrasLookup::extras_for` receives.
/// Impls map it to a GitHub login + access token through whatever
/// linkage the deployment maintains.
#[async_trait]
pub trait GithubOrgsSource: Send + Sync + 'static {
    /// Fetch the org login list for `user_id`. MUST NOT cache
    /// internally — the [`CachedGithubOrgsSource`] wrapper does
    /// that, and double-caching makes "refresh interval" mean
    /// different things in different layers.
    async fn fetch_orgs(&self, user_id: &str) -> Result<Vec<String>, GithubOrgsError>;
}

/// Pre-seeded `(user_id -> orgs)` map. Used by tests and as the
/// initial placeholder source until the bin layer wires the real
/// octocrab call. An unknown user returns
/// [`GithubOrgsError::Unknown`], which the stamper translates
/// into an empty org list (→ `in_allowed_org = false`).
pub struct StaticGithubOrgsSource {
    rows: RwLock<HashMap<String, Vec<String>>>,
}

impl StaticGithubOrgsSource {
    /// Build empty.
    pub fn new() -> Self {
        Self {
            rows: RwLock::new(HashMap::new()),
        }
    }

    /// Insert / overwrite a row. Useful for tests; in production
    /// the source would never be mutated post-construction.
    pub fn insert(&self, user_id: impl Into<String>, orgs: Vec<String>) {
        self.rows.write().unwrap().insert(user_id.into(), orgs);
    }
}

impl Default for StaticGithubOrgsSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GithubOrgsSource for StaticGithubOrgsSource {
    async fn fetch_orgs(&self, user_id: &str) -> Result<Vec<String>, GithubOrgsError> {
        self.rows
            .read()
            .unwrap()
            .get(user_id)
            .cloned()
            .ok_or_else(|| GithubOrgsError::Unknown(user_id.to_string()))
    }
}

// ----------------------------------------------------------- cache

struct CacheEntry {
    orgs: Vec<String>,
    fetched_at: Instant,
}

/// TTL cache around a [`GithubOrgsSource`].
///
/// One source call per session-mint (cache miss); cached entries
/// survive `ttl` then are refetched on next access. Wrapped in
/// `Arc` and shared across the whole process by
/// [`crate::AppState`]; the rwlock contention is negligible
/// because (a) verify is the only reader and (b) cache hits hold
/// the lock for a single map lookup.
pub struct CachedGithubOrgsSource {
    inner: Arc<dyn GithubOrgsSource>,
    ttl: Duration,
    entries: RwLock<HashMap<String, CacheEntry>>,
}

impl CachedGithubOrgsSource {
    /// Wrap `inner` with a TTL cache. `ttl` typically comes from
    /// [`GitHubAuthConfig::org_refresh_interval`] (default 1h).
    pub fn new(inner: Arc<dyn GithubOrgsSource>, ttl: Duration) -> Self {
        Self {
            inner,
            ttl,
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Force a refetch on next access for `user_id`. Useful from
    /// admin endpoints ("re-check this user's orgs now") and from
    /// tests.
    pub fn invalidate(&self, user_id: &str) {
        self.entries.write().unwrap().remove(user_id);
    }

    async fn get(&self, user_id: &str) -> Result<Vec<String>, GithubOrgsError> {
        // Read-lock first — the hot path is a cache hit.
        if let Some(entry) = self.entries.read().unwrap().get(user_id) {
            if entry.fetched_at.elapsed() < self.ttl {
                return Ok(entry.orgs.clone());
            }
        }
        // Miss / expired. Fetch outside the lock so a slow GitHub
        // call doesn't block other readers.
        let orgs = match self.inner.fetch_orgs(user_id).await {
            Ok(o) => o,
            Err(GithubOrgsError::Unknown(_)) => {
                // Per the trait docs: unknown user → empty list,
                // the policy will deny.
                Vec::new()
            }
            Err(e) => return Err(e),
        };
        self.entries.write().unwrap().insert(
            user_id.to_string(),
            CacheEntry {
                orgs: orgs.clone(),
                fetched_at: Instant::now(),
            },
        );
        Ok(orgs)
    }
}

// --------------------------------------------------------- stamper

/// `PrincipalExtrasLookup` impl: wraps an inner lookup
/// (typically `starter_auth_oauth::OAuthPrincipalExtras`) and
/// augments its `oauth` block with `github_orgs` +
/// `in_allowed_org`.
///
/// Construction takes:
/// * `inner` — the standard OAuth attribute stamper. We delegate
///   first so the wire shape (`oauth.provider`, `oauth.email`,
///   etc.) stays exactly as `starter-auth-oauth` defines it.
/// * `orgs` — a [`CachedGithubOrgsSource`] (or anything
///   `GithubOrgsSource` if a deployment wants to skip the cache).
/// * `cfg` — the `[auth.github]` block; we read `allow_orgs` from
///   it to derive the boolean.
pub struct GithubOrgsStamper {
    inner: Arc<dyn PrincipalExtrasLookup>,
    orgs: Arc<CachedGithubOrgsSource>,
    cfg: Arc<GitHubAuthConfig>,
}

impl GithubOrgsStamper {
    /// Compose the stamper from its three dependencies.
    pub fn new(
        inner: Arc<dyn PrincipalExtrasLookup>,
        orgs: Arc<CachedGithubOrgsSource>,
        cfg: Arc<GitHubAuthConfig>,
    ) -> Self {
        Self { inner, orgs, cfg }
    }

    /// Build the augmented `oauth.*` block for `user_id`. Public
    /// for tests; the trait impl below is the production seam.
    pub async fn build_extras_for(
        &self,
        user_id: &str,
    ) -> Result<Value, PrincipalExtrasError> {
        // Delegate to the inner lookup first so the standard
        // `oauth.*` fields land. A `Null` result means the user
        // has no linked OAuth identity — we surface that as-is
        // because there's nothing to augment.
        let inner_value = self.inner.extras_for(user_id).await?;
        if matches!(inner_value, Value::Null) {
            return Ok(Value::Null);
        }

        // Fetch (or hit cache for) the org list.
        let orgs = self
            .orgs
            .get(user_id)
            .await
            .map_err(|e| PrincipalExtrasError::Backend(e.to_string()))?;
        let in_allowed = self.cfg.any_in_allow_list(&orgs);

        // Merge our two extra fields onto the inner oauth block.
        // We mutate a clone to avoid touching the underlying
        // Value the inner lookup might still own, and to keep
        // this branch easy to reason about.
        let mut value = inner_value;
        if let Some(obj) = value.as_object_mut() {
            let oauth = obj
                .entry("oauth")
                .or_insert_with(|| json!({}));
            if let Some(oauth_obj) = oauth.as_object_mut() {
                oauth_obj.insert("github_orgs".to_string(), json!(orgs));
                oauth_obj.insert("in_allowed_org".to_string(), json!(in_allowed));
            }
        }
        Ok(value)
    }
}

#[async_trait]
impl PrincipalExtrasLookup for GithubOrgsStamper {
    async fn extras_for(&self, user_id: &str) -> Result<Value, PrincipalExtrasError> {
        self.build_extras_for(user_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use starter_auth_users::NoPrincipalExtras;

    fn cfg() -> Arc<GitHubAuthConfig> {
        Arc::new(GitHubAuthConfig {
            client_id: "id".into(),
            client_secret_ref: "secret://x".into(),
            allow_orgs: vec!["NubeIO".into()],
            org_refresh_interval_secs: 3600,
        })
    }

    /// Wraps NoPrincipalExtras (which returns Null) so the
    /// stamper short-circuits — the production wiring uses
    /// OAuthPrincipalExtras (a non-null oauth block for linked
    /// users); this test pins the "no linked identity"
    /// fall-through.
    #[tokio::test]
    async fn null_inner_returns_null() {
        let static_src = Arc::new(StaticGithubOrgsSource::new());
        let cached = Arc::new(CachedGithubOrgsSource::new(
            static_src,
            Duration::from_secs(60),
        ));
        let stamper = GithubOrgsStamper::new(
            Arc::new(NoPrincipalExtras),
            cached,
            cfg(),
        );
        let v = stamper.extras_for("u1").await.unwrap();
        assert_eq!(v, Value::Null);
    }

    /// Fake inner lookup that returns a populated oauth block —
    /// mirrors what `OAuthPrincipalExtras` does in production.
    struct FakeInner;
    #[async_trait]
    impl PrincipalExtrasLookup for FakeInner {
        async fn extras_for(&self, _user_id: &str) -> Result<Value, PrincipalExtrasError> {
            Ok(json!({
                "oauth": {
                    "provider": "github",
                    "provider_sub": "12345",
                    "email": "u@example.com",
                    "email_domain": "example.com",
                    "email_verified": true,
                    "linked_providers": ["github"]
                }
            }))
        }
    }

    #[tokio::test]
    async fn stamper_adds_github_orgs_and_in_allowed_org_true() {
        let src = Arc::new(StaticGithubOrgsSource::new());
        src.insert("u1", vec!["NubeIO".into(), "Other".into()]);
        let cached = Arc::new(CachedGithubOrgsSource::new(
            src,
            Duration::from_secs(60),
        ));
        let stamper = GithubOrgsStamper::new(Arc::new(FakeInner), cached, cfg());

        let v = stamper.extras_for("u1").await.unwrap();
        let oauth = &v["oauth"];
        assert_eq!(oauth["provider"], "github", "inner fields ride along");
        assert_eq!(
            oauth["github_orgs"],
            json!(["NubeIO", "Other"]),
            "github_orgs added"
        );
        assert_eq!(
            oauth["in_allowed_org"], true,
            "in_allowed_org true when intersection non-empty"
        );
    }

    #[tokio::test]
    async fn stamper_marks_out_of_org_user_false() {
        let src = Arc::new(StaticGithubOrgsSource::new());
        src.insert("u2", vec!["evilcorp".into()]);
        let cached = Arc::new(CachedGithubOrgsSource::new(
            src,
            Duration::from_secs(60),
        ));
        let stamper = GithubOrgsStamper::new(Arc::new(FakeInner), cached, cfg());

        let v = stamper.extras_for("u2").await.unwrap();
        assert_eq!(v["oauth"]["in_allowed_org"], false);
    }

    #[tokio::test]
    async fn unknown_user_yields_empty_orgs_and_false() {
        let src = Arc::new(StaticGithubOrgsSource::new());
        // u3 has no row in the source.
        let cached = Arc::new(CachedGithubOrgsSource::new(
            src,
            Duration::from_secs(60),
        ));
        let stamper = GithubOrgsStamper::new(Arc::new(FakeInner), cached, cfg());

        let v = stamper.extras_for("u3").await.unwrap();
        assert_eq!(v["oauth"]["github_orgs"], json!([]));
        assert_eq!(v["oauth"]["in_allowed_org"], false);
    }

    #[tokio::test]
    async fn cache_avoids_double_fetch_within_ttl() {
        // Count fetch calls — the second extras_for should not
        // hit the inner source.
        struct Counting {
            calls: std::sync::atomic::AtomicUsize,
        }
        #[async_trait]
        impl GithubOrgsSource for Counting {
            async fn fetch_orgs(
                &self,
                _user_id: &str,
            ) -> Result<Vec<String>, GithubOrgsError> {
                self.calls
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(vec!["NubeIO".into()])
            }
        }
        let counting = Arc::new(Counting {
            calls: Default::default(),
        });
        let cached = Arc::new(CachedGithubOrgsSource::new(
            counting.clone(),
            Duration::from_secs(60),
        ));
        let stamper = GithubOrgsStamper::new(Arc::new(FakeInner), cached, cfg());

        let _ = stamper.extras_for("u1").await.unwrap();
        let _ = stamper.extras_for("u1").await.unwrap();
        let _ = stamper.extras_for("u1").await.unwrap();
        assert_eq!(
            counting.calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "cache should hit on the 2nd + 3rd call"
        );
    }
}
