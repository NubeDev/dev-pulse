//! `dp-cli` — thin subcommand crate registered into a
//! `starter_cli::CommandRegistry` by the top-level `dev-pulse`
//! binary. Stage 8 lands [`fetch_now`] — the `fetch-now` CLI
//! flow that operators use to kick off an immediate reconciler
//! tick without waiting for the 4h interval.
//!
//! ## Shared seam
//!
//! Stage 8 mandates that `fetch-now` and `POST /admin/refresh`
//! (the dp-rest admin route) trigger the exact same code path the
//! scheduler does. That code path is
//! [`dp_fetcher::reconciler::Scheduler::try_trigger_now`] — it
//! owns the `Mutex<Option<JoinHandle>>` coalescing guard so that
//! a manual `fetch-now` while a scheduled tick is in flight
//! observes the in-flight handle and no-ops instead of running a
//! second concurrent tick.
//!
//! `fetch_now` is intentionally a function the bin layer calls,
//! not a CLI builder that owns a clap subcommand. That keeps the
//! coupling here minimal — the bin builds the `clap::Command`,
//! parses args, and hands the parsed [`Scope`] to this function.

use std::sync::Arc;

use anyhow::Result;
use dp_fetcher::reconciler::{Scheduler, Scope, TickStats};

/// Run one reconciler tick via the scheduler's coalescing guard.
///
/// This is the single shared seam Stage 8 requires: the bin's
/// `fetch-now` subcommand, the `POST /admin/refresh` HTTP handler
/// in `dp-rest`, and the scheduler itself all dispatch through
/// [`Scheduler::try_trigger_now`].
///
/// Returns `Some(stats)` on a tick that ran end-to-end, `None`
/// when the call coalesced into an in-flight tick.
pub async fn fetch_now(scheduler: Arc<Scheduler>, scope: Scope) -> Result<Option<TickStats>> {
    let stats = scheduler.try_trigger_now(scope).await?;
    match stats.as_ref() {
        Some(s) => tracing::info!(
            target: "dp_cli::fetch_now",
            items   = s.items,
            errors  = s.errors,
            partial = s.partial,
            "fetch-now ran reconciler tick"
        ),
        None => tracing::info!(
            target: "dp_cli::fetch_now",
            "fetch-now coalesced into in-flight tick"
        ),
    }
    Ok(stats)
}

// Coverage note: the end-to-end semantics of `fetch_now`
// (coalescing, run-log writes, cursor advancement) are exercised
// in `dp_fetcher::reconciler::tests`. This crate is a thin
// adapter; an extra test here would duplicate that surface.
