//! Stage-8 composition tests.
//!
//! See the comment above `mod tests` in `lib.rs` for the scope
//! split — this file only asserts the things the composition root
//! is responsible for.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use async_trait::async_trait;
use starter_spi::auth::{Authenticator, Principal, Role};
use starter_spi::Result as SpiResult;

use crate::BoxedAuthenticator;

/// `BoxedAuthenticator` is the wrap-newtype dp-server uses to
/// satisfy `with_principal`'s `A: Authenticator + Sized` bound from
/// an `Arc<dyn Authenticator>` carried on [`AppState`]. The wrap
/// must forward `verify` to the inner handle 1:1 — if it short-
/// circuits or swallows a call we would silently fail open at the
/// `with_principal` seam in production.
#[tokio::test]
async fn boxed_authenticator_forwards_verify_to_inner() {
    /// Counts every `verify` call and echoes back a principal
    /// constructed from the credential so we can assert the
    /// outer wrapper passed the exact string through.
    struct CountingAuth {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl Authenticator for CountingAuth {
        async fn verify(&self, credential: &str) -> SpiResult<Principal> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            // The role/scopes shape doesn't matter — we only assert
            // call count and the credential surface area. `subject`
            // carries the credential so the test can spot a wrong-
            // arg bug if one ever appears.
            Ok(Principal {
                subject: credential.to_string(),
                role: Role::Admin,
                scopes: Vec::new(),
                tenant_id: None,
                teams: Vec::new(),
                extra: serde_json::Value::Null,
            })
        }
    }

    let inner = Arc::new(CountingAuth {
        calls: AtomicUsize::new(0),
    });
    let inner_dyn: Arc<dyn Authenticator> = inner.clone();
    let boxed = BoxedAuthenticator(inner_dyn);

    let p = boxed
        .verify("sas_abc123")
        .await
        .expect("inner returns Ok");
    assert_eq!(p.subject, "sas_abc123", "credential passed through verbatim");
    assert_eq!(
        inner.calls.load(Ordering::SeqCst),
        1,
        "inner authenticator was hit exactly once"
    );

    let _ = boxed.verify("sak_token").await.unwrap();
    assert_eq!(
        inner.calls.load(Ordering::SeqCst),
        2,
        "second call also reaches the inner authenticator"
    );
}

/// `BuildError::Metrics` is the only failure mode `build()` can
/// surface before the bin layer takes over — and it surfaces when
/// the shared prometheus registry already has the webhook
/// receipt-to-200 histogram on it (i.e. a caller re-used a registry
/// that another path had already registered against). Pin the
/// `From<prometheus::Error>` conversion so a future refactor that
/// drops `#[from]` is loud.
#[test]
fn build_error_wraps_prometheus_error() {
    let reg = prometheus::Registry::new();
    // `WebhookMetrics` itself doesn't implement `Debug`, so we
    // can't use `expect_err`; pattern-match the Result instead.
    let _first = dp_fetcher::webhook::WebhookMetrics::register(&reg)
        .ok()
        .expect("first registration succeeds");
    let err = match dp_fetcher::webhook::WebhookMetrics::register(&reg) {
        Err(e) => e,
        Ok(_) => panic!("second registration on the same registry should collide"),
    };
    let build_err: crate::BuildError = err.into();
    assert!(
        matches!(build_err, crate::BuildError::Metrics(_)),
        "prometheus errors map to BuildError::Metrics",
    );
}
