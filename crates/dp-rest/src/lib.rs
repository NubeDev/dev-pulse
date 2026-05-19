//! `dp-rest` — axum `Router` fragments dev-pulse mounts onto the
//! starter-server app.
//!
//! Stage 8 adds the [`admin`] module: a `POST /admin/refresh`
//! route that triggers a reconciler tick via the same
//! [`Scheduler::try_trigger_now`] entrypoint the CLI's `fetch-now`
//! and the scheduled-interval loop use. The route lives behind
//! `with_principal` in the bin layer (it's an operator-only
//! action), but the router fragment here doesn't enforce auth —
//! that's a composition-layer decision per the starter pattern.
//!
//! [`Scheduler::try_trigger_now`]: dp_fetcher::reconciler::Scheduler::try_trigger_now

pub mod admin;

pub use admin::{admin_router, AdminState, RefreshQuery, RefreshResponse};
