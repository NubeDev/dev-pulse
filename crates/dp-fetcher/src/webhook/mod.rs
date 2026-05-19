//! Webhook receiver — Stage 4 of the dev-pulse ingestion layer
//! (TODO §Phase-2, §0.1).
//!
//! ## Responsibility split
//!
//! The receiver does the *minimum* synchronous work demanded by
//! GitHub's "respond in 10s or we redeliver" contract, and a
//! separate worker (Stage 5) drains the inbox.
//!
//! On a POST to `/webhooks/github` we:
//!
//! 1. Capture the receipt timestamp (for the latency histogram).
//! 2. Read the raw body bytes (HMAC is computed over the wire
//!    bytes, so we cannot let `axum::Json` reparse first).
//! 3. Pull `X-GitHub-Delivery`, `X-GitHub-Event`, and
//!    `X-Hub-Signature-256` off the headers. Any missing — 400
//!    or 401 per the table in [`router`]. **Fail-closed** on a
//!    missing signature; never accept an unsigned body.
//! 4. Validate the HMAC against every secret in the rotation
//!    bundle (current first, then previous) using a
//!    constant-time compare via the `hmac` crate's
//!    `verify_slice`. If none match — 401.
//! 5. Parse the body as JSON and call
//!    [`Store::enqueue_webhook`](dp_domain::Store::enqueue_webhook).
//!    A unique-constraint violation on `delivery_id` (the row is
//!    already in the inbox — GitHub redelivered) is **success** at
//!    this boundary: we return 200 so GitHub stops retrying. The
//!    worker handles the actual work.
//! 6. Record receipt-to-200 latency on the histogram and return
//!    200.
//!
//! The route is **not** wrapped with `with_principal` (TODO §4).
//! Authentication is the HMAC, which we verify ourselves.
//!
//! ## Performance target
//!
//! TODO §Phase-2 mandates "return 200 in under 100ms". The work
//! above is one HMAC, one JSON parse, and one indexed INSERT —
//! comfortably inside that budget on the deployment target. The
//! receipt-to-200 histogram surfaces regressions before they
//! escape to production.
//!
//! ## Secrets / rotation
//!
//! The receiver does not touch `starter-secrets-file` directly —
//! TODO §0.6 forbids `starter_*` imports in this crate. The bin /
//! `dp-server` composition layer resolves the secret (or the
//! current+previous pair, mid-rotation) and supplies the value
//! through a [`WebhookSecretSource`] implementation. The receiver
//! only sees opaque byte slices.

pub mod metrics;
mod router;
mod verify;

pub use metrics::WebhookMetrics;
pub use router::{router, WebhookState};
pub use verify::{verify_signature, SignatureError, StaticSecrets, WebhookSecretSource};
