//! `dp-rest` — axum `Router` fragments dev-pulse mounts onto the
//! starter-server app.
//!
//! Modules:
//!
//! * [`admin`] — `POST /admin/refresh` (stage 8 of Phase 2, the
//!   operator-triggered reconciler tick). Auth is added by the
//!   composition layer; the router fragment here doesn't enforce it.
//! * [`reports`] — Phase 4 stage 3 report surface: `GET /reports/user/:id`,
//!   `/team/:id`, `/org/:id`, `/home-org-split`, `/freshness`. Every
//!   handler echoes the resolved [`Window`][dp_reports::Window] back
//!   per TODO §0.4 and carries [`DataAsOfDto`] per §11.7.
//! * [`state`] — shared [`AppState`] (currently just a [`Store`]
//!   handle; later Phase 4 stages widen it).
//! * [`error`] — one [`ApiError`] type every handler returns.
//!
//! Boundary note (§0.6): `dp-rest` is an edge crate, so starter-*
//! imports are allowed here. Stage 3 doesn't need any; the
//! `with_principal` / `require_permission` wrappers land in later
//! stages.
//!
//! [`Scheduler::try_trigger_now`]: dp_fetcher::reconciler::Scheduler::try_trigger_now
//! [`Store`]: dp_domain::store::Store

pub mod admin;
pub mod error;
pub mod reports;
pub mod state;

pub use admin::{admin_router, AdminState, RefreshQuery, RefreshResponse};
pub use error::ApiError;
pub use reports::{
    freshness_report, home_org_split_report, org_report, reports_router, team_report,
    user_report, CountRow, DataAsOfDto, HomeOrgSplitRow, ReportQuery, ReportResponse,
};
pub use state::AppState;
