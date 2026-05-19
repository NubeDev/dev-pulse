//! Prometheus metric for the webhook receiver.
//!
//! One histogram — `dp_webhook_receipt_seconds` — records the
//! time from "we have the request" to "we sent the 200 back" so
//! we can alert on regressions against the TODO §Phase-2 "under
//! 100ms" budget. Buckets are tuned to that budget: dense below
//! 100ms, then a long tail so we see the SLO violators.
//!
//! Registration happens once, at server startup, against the
//! shared [`prometheus::Registry`] owned by `dp-server`. The
//! handler reads the histogram off [`super::WebhookState`].

use prometheus::{Histogram, HistogramOpts, Registry};

/// Bucket boundaries in seconds. Dense below 100ms (the SLO),
/// looser tail above so we still capture pathological cases.
const RECEIPT_BUCKETS: &[f64] = &[
    0.001, 0.002, 0.005, 0.010, 0.020, 0.050, 0.075, 0.100, 0.150, 0.250, 0.500, 1.0, 2.5,
];

/// Holder for the receiver's metrics. Cloneable so the axum
/// handler can pull it out of state per request without
/// re-registering — `Histogram` itself is an `Arc`.
#[derive(Clone)]
pub struct WebhookMetrics {
    /// Receipt-to-200 latency, in seconds. Observed once per
    /// completed request (success path; HMAC-rejected requests
    /// are not measured — they don't represent the SLO).
    pub receipt_seconds: Histogram,
}

impl WebhookMetrics {
    /// Build the histogram and register it on `registry`. Errors
    /// surface as `prometheus::Error`; the bin layer should treat
    /// any failure as fatal (it means metrics are mis-wired).
    pub fn register(registry: &Registry) -> Result<Self, prometheus::Error> {
        let receipt_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "dp_webhook_receipt_seconds",
                "Time from webhook receipt to 200 response, in seconds. \
                 SLO: under 0.100.",
            )
            .buckets(RECEIPT_BUCKETS.to_vec()),
        )?;
        registry.register(Box::new(receipt_seconds.clone()))?;
        Ok(Self { receipt_seconds })
    }

    /// Test helper: build the metrics against a throwaway
    /// registry so unit tests of the route don't need to share
    /// a [`Registry`] with the rest of the binary.
    #[cfg(test)]
    pub fn for_test() -> Self {
        Self::register(&Registry::new()).expect("register webhook metrics")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_against_a_fresh_registry() {
        let reg = Registry::new();
        let m = WebhookMetrics::register(&reg).unwrap();
        m.receipt_seconds.observe(0.050);
        // The metric family is present and the bucket fired.
        let families = reg.gather();
        let f = families
            .iter()
            .find(|f| f.name() == "dp_webhook_receipt_seconds")
            .expect("histogram registered");
        assert_eq!(f.get_metric()[0].get_histogram().get_sample_count(), 1);
    }

    #[test]
    fn double_registration_against_same_registry_errors() {
        // Catches the "we accidentally called register twice in
        // composition" footgun before it ships.
        let reg = Registry::new();
        WebhookMetrics::register(&reg).unwrap();
        assert!(WebhookMetrics::register(&reg).is_err());
    }
}
