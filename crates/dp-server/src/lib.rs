//! `dp-server` — composition root for dev-pulse. Glues `dp-store-pg`,
//! `dp-rest`, `dp-mcp`, `dp-fetcher`, and `dp-reports` into a single
//! `ServerBuilder` (axum + utoipa + metrics + principal + MCP).
//!
//! Stage 1 scaffold: empty crate.
