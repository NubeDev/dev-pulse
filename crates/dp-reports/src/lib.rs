//! `dp-reports` — report query layer (TODO §Phase 3, SCOPE §8).
//!
//! Implements the three org-scope lenses (SCOPE §8.1) with
//! `event_actors`-aware de-dup (TODO §0.2). Every report accepts the
//! single [`ReportRequest`] envelope and the server resolves
//! `(label, tz, anchor)` into a concrete UTC `[start, end)` via
//! [`resolve_window`] (TODO §0.4) — never the frontend.
//!
//! Boundary rule (TODO §0.6): zero `starter_*` imports. Verified by
//! `scripts/check-boundaries.sh` in CI.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod envelope;

pub use envelope::{
    resolve_window, resolve_window_at, GroupBy, ReportRequest, ResolveError, ScopeMode,
    WindowLabel, WindowSpec,
};

// Re-export the resolved Window type from dp-domain so callers only
// need to depend on dp-reports for the request/response shapes.
pub use dp_domain::window::{Window, WindowAnchor};
