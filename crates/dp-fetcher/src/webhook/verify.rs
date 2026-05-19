//! HMAC SHA-256 validation for GitHub webhook deliveries.
//!
//! GitHub signs every webhook body with the App-level secret and
//! sends the result as `X-Hub-Signature-256: sha256=<hex>`. Spec:
//! <https://docs.github.com/en/webhooks/using-webhooks/validating-webhook-deliveries>
//!
//! Three properties this module preserves and the unit tests pin:
//!
//! * **Constant-time compare.** We use `hmac::Mac::verify_slice`
//!   which delegates to `subtle::ConstantTimeEq`. A naive
//!   `==` against the hex string is timing-leaky.
//! * **Fail-closed.** Missing header, malformed prefix, malformed
//!   hex, or an empty rotation bundle all return `Invalid`. The
//!   receiver maps every error here to 401 — never 200.
//! * **Rotation-aware.** [`WebhookSecretSource::current_secrets`]
//!   returns *every* currently-valid secret (typically just one,
//!   sometimes a (current, previous) pair during rotation). We
//!   try each and return success on the first match. The order
//!   does not matter for correctness — both pass through the
//!   constant-time compare — but callers should put the most
//!   likely match first to keep the common path cheap.

use hmac::{Hmac, Mac};
use secrecy::{ExposeSecret, SecretString};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// The `sha256=` prefix GitHub sends. Stripped before hex-decode.
const SIG_PREFIX: &str = "sha256=";

/// Why a signature did not validate. The receiver collapses all
/// of these to a single 401 response; the variants exist so
/// telemetry / tests can distinguish them.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SignatureError {
    /// The `X-Hub-Signature-256` header was absent. GitHub always
    /// sends one when a secret is configured; absence means
    /// either an unconfigured App (fail-closed) or a probe.
    #[error("missing X-Hub-Signature-256 header")]
    Missing,
    /// Header was present but did not start with `sha256=` or
    /// did not hex-decode to 32 bytes.
    #[error("malformed signature header")]
    Malformed,
    /// HMAC computed correctly but matched none of the candidate
    /// secrets. Either the secret has drifted on GitHub's side
    /// or the body is forged.
    #[error("signature did not match any current secret")]
    Mismatch,
    /// The rotation bundle was empty — the bin layer didn't wire
    /// up a secret. Treated as "fail closed" (we won't accept any
    /// body); surfaced as a distinct variant so an operator can
    /// see *why* every delivery is 401-ing.
    #[error("no webhook secrets configured")]
    NoSecrets,
}

/// Supplies the secret(s) the receiver should accept HMACs
/// against. Implemented in the bin / `dp-server` layer over
/// `starter-secrets-file` (TODO §0.1, §0.6) so this crate stays
/// secrets-backend-agnostic.
///
/// Returning `Vec<SecretString>` lets the implementer return
/// either a single live secret or a `[current, previous]` pair
/// during a rotation window. The bin layer is free to refresh on
/// every call (cheap if the file is unchanged) or cache.
pub trait WebhookSecretSource: Send + Sync + 'static {
    /// Return every currently-valid secret, most-likely first.
    fn current_secrets(&self) -> Vec<SecretString>;
}

/// Validate `header_value` (the raw `X-Hub-Signature-256` string,
/// or `None` if absent) against `body` using every secret in
/// `secrets`. Returns `Ok(())` on the first match, `Err` on any
/// failure mode above.
pub fn verify_signature(
    header_value: Option<&str>,
    body: &[u8],
    secrets: &[SecretString],
) -> Result<(), SignatureError> {
    let header = header_value.ok_or(SignatureError::Missing)?;
    let hex_sig = header
        .strip_prefix(SIG_PREFIX)
        .ok_or(SignatureError::Malformed)?;
    let sig_bytes = hex::decode(hex_sig).map_err(|_| SignatureError::Malformed)?;
    if sig_bytes.len() != 32 {
        return Err(SignatureError::Malformed);
    }

    if secrets.is_empty() {
        return Err(SignatureError::NoSecrets);
    }

    for secret in secrets {
        // `new_from_slice` is infallible for HmacSha256 (any key
        // length is accepted), so the unwrap is a tightening, not
        // a panic-in-prod risk.
        let mut mac = HmacSha256::new_from_slice(secret.expose_secret().as_bytes())
            .expect("HmacSha256 accepts any key length");
        mac.update(body);
        if mac.verify_slice(&sig_bytes).is_ok() {
            return Ok(());
        }
    }
    Err(SignatureError::Mismatch)
}

/// A trivial in-memory [`WebhookSecretSource`] for tests and for
/// bin-layer wiring that doesn't yet read from
/// `starter-secrets-file`. Production should prefer an impl that
/// refreshes from the secrets backend rather than holding the
/// secret indefinitely in memory.
#[derive(Clone)]
pub struct StaticSecrets {
    secrets: Vec<SecretString>,
}

impl StaticSecrets {
    /// Build from a single live secret.
    pub fn single(secret: SecretString) -> Self {
        Self { secrets: vec![secret] }
    }

    /// Build from a `[current, previous]` rotation pair (or any N
    /// secrets — order is "try-first" order).
    pub fn rotating(secrets: Vec<SecretString>) -> Self {
        Self { secrets }
    }
}

impl WebhookSecretSource for StaticSecrets {
    fn current_secrets(&self) -> Vec<SecretString> {
        self.secrets.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compute the wire-format signature for a body against a
    /// secret — gives the tests a way to drive the validator with
    /// a known-good header without copy-pasting hex.
    fn sign(body: &[u8], secret: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    #[test]
    fn happy_path_accepts_known_good_signature() {
        let body = b"{\"action\":\"opened\"}";
        let sig = sign(body, "shh");
        verify_signature(
            Some(&sig),
            body,
            &[SecretString::from("shh".to_string())],
        )
        .unwrap();
    }

    #[test]
    fn missing_header_fails_closed() {
        let body = b"x";
        let err = verify_signature(None, body, &[SecretString::from("shh".to_string())])
            .unwrap_err();
        assert_eq!(err, SignatureError::Missing);
    }

    #[test]
    fn wrong_prefix_is_malformed() {
        let body = b"x";
        let err = verify_signature(
            Some("sha1=deadbeef"),
            body,
            &[SecretString::from("shh".to_string())],
        )
        .unwrap_err();
        assert_eq!(err, SignatureError::Malformed);
    }

    #[test]
    fn non_hex_payload_is_malformed() {
        let body = b"x";
        let err = verify_signature(
            Some("sha256=not-hex-XX"),
            body,
            &[SecretString::from("shh".to_string())],
        )
        .unwrap_err();
        assert_eq!(err, SignatureError::Malformed);
    }

    #[test]
    fn wrong_length_is_malformed() {
        let body = b"x";
        // valid hex but only 2 bytes — must be 32 for SHA-256.
        let err = verify_signature(
            Some("sha256=abcd"),
            body,
            &[SecretString::from("shh".to_string())],
        )
        .unwrap_err();
        assert_eq!(err, SignatureError::Malformed);
    }

    #[test]
    fn wrong_secret_mismatches() {
        let body = b"hello";
        let sig = sign(body, "other-secret");
        let err = verify_signature(
            Some(&sig),
            body,
            &[SecretString::from("shh".to_string())],
        )
        .unwrap_err();
        assert_eq!(err, SignatureError::Mismatch);
    }

    #[test]
    fn rotation_pair_accepts_previous_secret() {
        // Mid-rotation: GitHub still signs with the old secret
        // for a window after we publish the new one. Old MUST
        // still be accepted.
        let body = b"payload";
        let sig = sign(body, "old");
        verify_signature(
            Some(&sig),
            body,
            &[
                SecretString::from("new".to_string()),
                SecretString::from("old".to_string()),
            ],
        )
        .unwrap();
    }

    #[test]
    fn empty_bundle_fails_closed() {
        let body = b"x";
        let sig = sign(body, "shh");
        let err = verify_signature(Some(&sig), body, &[]).unwrap_err();
        assert_eq!(err, SignatureError::NoSecrets);
    }

    #[test]
    fn body_tamper_is_caught() {
        // Sign one body, hand the validator a different one.
        let sig = sign(b"original", "shh");
        let err = verify_signature(
            Some(&sig),
            b"tampered",
            &[SecretString::from("shh".to_string())],
        )
        .unwrap_err();
        assert_eq!(err, SignatureError::Mismatch);
    }
}
