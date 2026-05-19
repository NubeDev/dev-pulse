//! GitHub App credentials — the value type the rest of dev-pulse
//! hands to [`crate::client::Client`] when it needs to mint an
//! installation-scoped octocrab.
//!
//! ## Why this lives in `dp-fetcher` and not in a starter adapter
//!
//! TODO §0.6 forbids `starter_*` imports anywhere in `dp-fetcher`.
//! The actual *source* of these bytes is `starter-secrets-file`
//! (TODO §1, Phase 0 decisions), but the resolution happens in the
//! `dev-pulse` bin / `dp-server` composition layer, which is
//! unrestricted. Those layers read the secret, build an
//! [`AppCredentials`], and hand the value to this crate. That keeps
//! `dp-fetcher` storage- and secrets-backend-agnostic while still
//! routing real key material from the documented backend.
//!
//! ## Rotation
//!
//! Per the stage-0 decision pinned in `SCOPE.md`, the webhook
//! secret and the App private key both live in
//! `starter-secrets-file` with a documented rotation path. The
//! shape here is deliberately a *value* (cloneable, sendable) so a
//! rotation is performed by constructing a new [`AppCredentials`]
//! and swapping the [`Client`](crate::client::Client) for one
//! built from it — no global state.

use secrecy::SecretString;

/// A GitHub App's identity. The PEM private key is held in a
/// [`SecretString`] so it cannot accidentally end up in logs or
/// `Debug` output.
#[derive(Clone)]
pub struct AppCredentials {
    /// Numeric GitHub App ID (the "App ID" surfaced on the App
    /// settings page, **not** the slug).
    pub app_id: u64,
    /// PKCS#1 or PKCS#8 RSA private key, as a PEM string.
    pub private_key_pem: SecretString,
}

impl std::fmt::Debug for AppCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppCredentials")
            .field("app_id", &self.app_id)
            .field("private_key_pem", &"<redacted>")
            .finish()
    }
}

/// Identifies a single GitHub App *installation* — i.e. the
/// per-org token bucket the reconciler / backfill / webhook worker
/// will use to call the API.
///
/// Tokens are per-installation, not per-org — dev-pulse stores the
/// installation_id alongside the org row. The bin layer builds one
/// of these per active installation.
#[derive(Clone, Debug)]
pub struct InstallationCredentials {
    /// The App these credentials authenticate as.
    pub app: AppCredentials,
    /// Installation ID GitHub assigned when the App was installed
    /// in a given org.
    pub installation_id: u64,
}

/// Error category for JWT / PEM parsing performed at
/// [`Client::for_installation`](crate::client::Client::for_installation)
/// construction time. Kept narrow so [`crate::client::ClientError`]
/// can wrap it without leaking jsonwebtoken types into the public
/// surface.
#[derive(Debug, thiserror::Error)]
pub enum JwtError {
    /// PEM did not parse, or jsonwebtoken rejected it.
    #[error("github app private key did not parse: {0}")]
    Encode(#[from] jsonwebtoken::errors::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway 2048-bit RSA key generated specifically for unit
    /// tests. Has no relationship to any real GitHub App.
    const TEST_PRIVATE_KEY: &str = include_str!("test_keys/test_rsa.pem");

    fn creds() -> AppCredentials {
        AppCredentials {
            app_id: 12345,
            private_key_pem: SecretString::from(TEST_PRIVATE_KEY.to_string()),
        }
    }

    #[test]
    fn app_credentials_debug_redacts_key() {
        let dbg = format!("{:?}", creds());
        assert!(dbg.contains("<redacted>"), "key leaked in Debug: {dbg}");
        assert!(!dbg.contains("BEGIN RSA"), "PEM leaked in Debug: {dbg}");
        assert!(!dbg.contains("BEGIN PRIVATE"), "PEM leaked in Debug: {dbg}");
    }

    #[test]
    fn installation_credentials_clone_keeps_app_id() {
        let ic = InstallationCredentials {
            app: creds(),
            installation_id: 99,
        };
        assert_eq!(ic.clone().installation_id, 99);
        assert_eq!(ic.app.app_id, 12345);
    }

    #[test]
    fn pem_is_parseable_rsa() {
        // The wrapper depends on the bundled fixture being a valid
        // RSA PEM. If the file is regenerated and accidentally
        // committed as DER or an EC key, this test catches it
        // before the slower wiremock suite.
        let key = jsonwebtoken::EncodingKey::from_rsa_pem(TEST_PRIVATE_KEY.as_bytes());
        assert!(key.is_ok(), "test fixture not a valid RSA PEM: {:?}", key.err());
    }
}
