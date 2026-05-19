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
use dp_fetcher::backfill::{Backfill, BackfillStats};
use dp_fetcher::reconciler::{Scheduler, Scope, TickStats};
use uuid::Uuid;

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

/// Run a one-shot backfill for `org_id` against the configured
/// historical window (TODO §Phase 2, default 90 days).
///
/// Stage 9's shared seam: the bin's `backfill` subcommand and the
/// `dp-server` install-time hook both call this. The
/// [`Backfill`] driver itself does no CLI parsing — the bin
/// resolves the `org_id` from `--org` (or iterates installations
/// in install-time mode) and constructs the [`Backfill`] with a
/// **dedicated** octocrab client wrapper so live webhook traffic
/// keeps its share of the rate-limit budget.
///
/// Resumability lives entirely in the cursor: a crashed
/// backfill picks up at the high-water timestamp the previous
/// run reached, so re-invoking this function is the recovery
/// path — there is no separate "resume" CLI verb.
pub async fn backfill_org(backfill: Arc<Backfill>, org_id: Uuid) -> Result<BackfillStats> {
    let stats = backfill.run_for_org(org_id, None).await?;
    tracing::info!(
        target: "dp_cli::backfill",
        %org_id,
        chunks  = stats.chunks,
        items   = stats.items,
        errors  = stats.errors,
        skipped = stats.skipped,
        "backfill complete"
    );
    Ok(stats)
}

// Coverage note: the end-to-end semantics of `fetch_now` and
// `backfill_org` (coalescing, run-log writes, cursor
// advancement, resumability) are exercised in
// `dp_fetcher::reconciler::tests` and
// `dp_fetcher::backfill::tests`. This crate is a thin adapter;
// extra tests here would duplicate that surface.
