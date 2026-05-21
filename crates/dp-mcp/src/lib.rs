//! `dp-mcp` — `impl Tool` per query (user_activity, team_activity,
//! home_org_split, freshness). Registered into a `ToolRegistry` by
//! `dp-server`.
//!
//! Stage 1 scaffold: empty crate.
//!
//! ## Pending phase-5 tools
//!
//! - `project_portfolio` — SCOPE-PROJECT-REPORTS.md §12. Input
//!   schema: `dp_reports::ProjectPortfolioRequest`. Output schema:
//!   `dp_reports::ProjectPortfolioResponse`. Same envelope-locking
//!   rule as the REST `POST /reports/project-portfolio` handler at
//!   `dp_rest::reports::project_portfolio_report` — both surfaces
//!   must move together when a field is added. The handler logic
//!   (default-status fallback, limit validation, `now` resolution,
//!   `rollup_kpis`) is the reference implementation.
