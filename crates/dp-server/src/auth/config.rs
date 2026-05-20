//! `[auth.github]` configuration shape.
//!
//! The bin layer (`dev-pulse`) reads this out of the deployment's
//! `dp-config` file (TOML) and hands the struct down to
//! [`crate::build`]; nothing in this crate parses TOML directly.
//! We expose the struct here so dp-config can `#[derive(Deserialize)]`
//! a field of this type without dragging starter-secrets-file into
//! its surface — the `client_secret_ref` field is a string handle
//! the bin layer resolves through `starter-secrets-file` before
//! constructing the `OAuthProvider`.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default refresh interval for the per-session GitHub org cache.
/// One hour matches GitHub's typical org-membership change cadence
/// and keeps the request hot path off the GitHub API entirely. The
/// stamper still fetches once on session-mint regardless.
pub const DEFAULT_ORG_REFRESH_INTERVAL: Duration = Duration::from_secs(3600);

/// `[auth.github]` block from `dp-config`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubAuthConfig {
    /// GitHub OAuth App client id. The OAuth provider needs it in
    /// the clear; not a secret per se but kept in config for
    /// per-deployment override.
    pub client_id: String,
    /// `starter-secrets-file` handle (e.g. `"secret://oauth/github_client_secret"`)
    /// the bin layer resolves to the actual client secret before
    /// constructing `starter_auth_oauth::providers::GitHubProvider`.
    /// Stored as a handle (not the secret) so the config struct can
    /// be serialised back out for diagnostics without leaking the
    /// secret.
    pub client_secret_ref: String,
    /// Allow-listed GitHub org logins. A user whose
    /// `GET /user/orgs` intersects this list has
    /// `oauth.in_allowed_org = true` stamped on the principal; the
    /// org-gate rule in `policy/dev-pulse.toml` allows them
    /// through. Everyone else gets `403 awaiting_access` on every
    /// protected route.
    ///
    /// Example: `["NubeIO", "ACME"]`. Adding an org is a config
    /// edit + restart; no code or policy-file change.
    #[serde(default)]
    pub allow_orgs: Vec<String>,
    /// How long the in-process cache holds an operator's org list
    /// before re-fetching. Defaults to 1h via
    /// [`DEFAULT_ORG_REFRESH_INTERVAL`] when the TOML field is
    /// omitted. Serialised as seconds for human-readable configs.
    #[serde(default = "default_org_refresh_interval_secs")]
    #[serde(rename = "org_refresh_interval_secs")]
    pub org_refresh_interval_secs: u64,
}

impl GitHubAuthConfig {
    /// Refresh interval as a [`Duration`] — the shape the cache
    /// wrapper consumes.
    pub fn org_refresh_interval(&self) -> Duration {
        Duration::from_secs(self.org_refresh_interval_secs)
    }

    /// Membership-test helper used by the github_orgs stamper.
    /// `true` iff at least one of `github_orgs` (case-insensitive)
    /// appears in [`allow_orgs`]. The case-insensitivity matters
    /// because GitHub serves org logins lower-cased on some
    /// endpoints and as-written on others; we normalise both
    /// sides before comparing.
    ///
    /// [`allow_orgs`]: GitHubAuthConfig::allow_orgs
    pub fn any_in_allow_list<S: AsRef<str>>(&self, github_orgs: &[S]) -> bool {
        if self.allow_orgs.is_empty() {
            // No allow-list configured → fail closed. An operator
            // who forgets `allow_orgs` discovers it via 403 on
            // every request, not via accidental wide-open access.
            return false;
        }
        let allow_lower: Vec<String> = self
            .allow_orgs
            .iter()
            .map(|s| s.to_ascii_lowercase())
            .collect();
        github_orgs
            .iter()
            .any(|g| allow_lower.iter().any(|a| a == &g.as_ref().to_ascii_lowercase()))
    }
}

fn default_org_refresh_interval_secs() -> u64 {
    DEFAULT_ORG_REFRESH_INTERVAL.as_secs()
}

/// Errors surfaced when constructing or using a
/// [`GitHubAuthConfig`].
#[derive(Debug, Error)]
pub enum GitHubAuthConfigError {
    /// `client_id` was empty / missing.
    #[error("auth.github.client_id is required")]
    MissingClientId,
    /// `client_secret_ref` was empty / missing.
    #[error("auth.github.client_secret_ref is required (e.g. \"secret://oauth/github_client_secret\")")]
    MissingClientSecretRef,
}

impl GitHubAuthConfig {
    /// Validate required fields. The bin layer calls this once
    /// after loading the TOML so a misconfigured deployment fails
    /// loudly at boot rather than silently 403-ing every request.
    pub fn validate(&self) -> Result<(), GitHubAuthConfigError> {
        if self.client_id.trim().is_empty() {
            return Err(GitHubAuthConfigError::MissingClientId);
        }
        if self.client_secret_ref.trim().is_empty() {
            return Err(GitHubAuthConfigError::MissingClientSecretRef);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_in_allow_list_is_case_insensitive() {
        let cfg = GitHubAuthConfig {
            client_id: "x".into(),
            client_secret_ref: "secret://x".into(),
            allow_orgs: vec!["NubeIO".into(), "ACME".into()],
            org_refresh_interval_secs: 3600,
        };
        assert!(cfg.any_in_allow_list(&["nubeio"]));
        assert!(cfg.any_in_allow_list(&["other", "ACME"]));
        assert!(!cfg.any_in_allow_list(&["evilcorp"]));
    }

    #[test]
    fn empty_allow_list_fails_closed() {
        let cfg = GitHubAuthConfig {
            client_id: "x".into(),
            client_secret_ref: "secret://x".into(),
            allow_orgs: vec![],
            org_refresh_interval_secs: 3600,
        };
        // Even with non-empty github_orgs, an empty allow-list
        // must not grant access.
        assert!(!cfg.any_in_allow_list(&["NubeIO"]));
    }

    #[test]
    fn validate_requires_client_id_and_secret_ref() {
        let bad1 = GitHubAuthConfig {
            client_id: "".into(),
            client_secret_ref: "secret://x".into(),
            allow_orgs: vec![],
            org_refresh_interval_secs: 3600,
        };
        assert!(matches!(
            bad1.validate(),
            Err(GitHubAuthConfigError::MissingClientId)
        ));
        let bad2 = GitHubAuthConfig {
            client_id: "abc".into(),
            client_secret_ref: "".into(),
            allow_orgs: vec![],
            org_refresh_interval_secs: 3600,
        };
        assert!(matches!(
            bad2.validate(),
            Err(GitHubAuthConfigError::MissingClientSecretRef)
        ));
    }
}
