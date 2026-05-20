//! [`AppState`] — the per-process state every dp-rest handler holds.
//!
//! Phase 4 stage 3 only wires the [`Store`] handle (every report
//! handler reads through it). Later Phase 4 stages widen this struct
//! to add the reconciler scheduler (admin refresh), webhook secret
//! (HMAC validation), audit-log writer, and the OAuth-derived
//! principal cache. Wider widening is intentionally deferred so this
//! stage stays surgically focused on the report surface.
//!
//! The struct is `Clone` (cheap — every field is an `Arc`) so the
//! axum `Router::with_state(...)` extractor pattern works without
//! per-request allocation.

use std::sync::Arc;

use dp_domain::store::Store;

/// Application state shared across every dp-rest handler.
#[derive(Clone)]
pub struct AppState {
    /// Persistence handle. Reports read; admin handlers (later
    /// stages) will write.
    pub store: Arc<dyn Store>,
}

impl AppState {
    /// Convenience constructor.
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self { store }
    }
}
