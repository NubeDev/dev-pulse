//! [`WebhookDelivery`] — one row in `webhook_inbox`.
//!
//! Per TODO §0.1, the webhook receiver does the minimum work
//! synchronously (HMAC verify + enqueue), and a worker drains the
//! inbox idempotently. GitHub redelivers on failure, so `delivery_id`
//! must be unique to dedupe replays.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

/// A queued webhook awaiting (or post-) processing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookDelivery {
    /// Internal primary key.
    pub id: Uuid,
    /// `X-GitHub-Delivery` — globally unique per delivery attempt.
    /// Unique index on this column gives replay-safety.
    pub delivery_id: String,
    /// `X-GitHub-Event` (e.g. `pull_request`, `issues`,
    /// `workflow_run`).
    pub event: String,
    /// Raw delivery body. Kept verbatim so the worker can re-parse
    /// after a schema change without a re-fetch from GitHub.
    pub payload: JsonValue,
    /// When the receiver enqueued it. UTC.
    pub received_at: DateTime<Utc>,
    /// When the worker finished applying it. `None` while pending.
    pub processed_at: Option<DateTime<Utc>>,
    /// Last error message, if processing has failed at least once.
    /// `None` on a fresh row or a clean success.
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trips_through_json() {
        let d = WebhookDelivery {
            id: Uuid::nil(),
            delivery_id: "72d3162e-cc78-11e3-81ab-4c9367dc0958".into(),
            event: "pull_request".into(),
            payload: json!({ "action": "opened" }),
            received_at: Utc::now(),
            processed_at: None,
            error: None,
        };
        let back: WebhookDelivery =
            serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(d, back);
    }
}
