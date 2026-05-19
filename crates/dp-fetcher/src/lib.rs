//! `dp-fetcher` — ingestion (TODO §0.1, §0.3, Phase 2).
//!
//! Subdivides into `webhook`, `reconciler`, `backfill`, and a
//! single rate-limit-paced octocrab client wrapper. Stage 3 lands
//! the [`client`] wrapper — the **only** place in dev-pulse that
//! constructs an [`octocrab::Octocrab`]. Reconciler and backfill
//! (later stages) call typed methods on [`client::Client`] rather
//! than touching octocrab directly. Stage 4 adds [`webhook`] —
//! an HMAC-validating axum route fragment that enqueues to
//! `webhook_inbox` and returns 200 in under 100ms. Stage 5 adds
//! [`worker`] — the cooperative-shutdown drain task that empties
//! `webhook_inbox`, applies idempotent upserts via
//! `activity_events.external_id`, and fans out multi-actor events
//! into `event_actors` per TODO §0.2. Stage 9 adds [`backfill`]
//! — the one-shot bounded-window historical importer (default
//! 90 days from `dp-config`) that primes a freshly-installed
//! org. Backfill runs against a dedicated [`client::Client`]
//! instance with a lower rate-limit headroom so it cannot
//! starve live webhook processing of budget, and is resumable
//! via `fetch_cursors` so a crash picks up where it left off.
//!
//! Boundary rule (TODO §0.6): zero `starter_*` imports. GitHub App
//! credentials and the webhook HMAC secret originate in
//! `starter-secrets-file`, but the resolution happens in the
//! unrestricted bin / `dp-server` layer which then constructs a
//! [`client::InstallationCredentials`] value (or implements
//! [`webhook::WebhookSecretSource`]) and hands it to this crate.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod backfill;
pub mod client;
pub mod reconciler;
pub mod webhook;
pub mod worker;
