//! `dp-fetcher` — ingestion (TODO §0.1, §0.3, Phase 2).
//!
//! Subdivides into `webhook`, `reconciler`, `backfill`, and a
//! single rate-limit-paced octocrab client wrapper. Stage 3 lands
//! the [`client`] wrapper — the **only** place in dev-pulse that
//! constructs an [`octocrab::Octocrab`]. Reconciler and backfill
//! (later stages) call typed methods on [`client::Client`] rather
//! than touching octocrab directly.
//!
//! Boundary rule (TODO §0.6): zero `starter_*` imports. GitHub App
//! credentials originate in `starter-secrets-file`, but the
//! resolution happens in the unrestricted bin / `dp-server` layer
//! which then constructs a
//! [`client::InstallationCredentials`] value and hands it to this
//! crate.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod client;
