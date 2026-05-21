//! Backfill — Stage 9 of the dev-pulse ingestion layer
//! (TODO §0.1, §0.3, SCOPE §10).
//!
//! Webhooks (Stages 4–5) are real-time. The reconciler (Stage 8)
//! is the safety net that runs every 4h. **Backfill is the
//! cold-start primer**: when a GitHub App is freshly installed
//! for an org, the local store has zero history; without this
//! module the org would have to wait 90 days of organic webhook
//! traffic to populate enough data for the reports in SCOPE §8
//! to be useful.
//!
//! Per TODO §Phase 2 (and the working assumption pinned in
//! `SCOPE.md`'s Decisions §0): the historical window is **90 days
//! by default**, configurable through [`BackfillConfig::window`].
//! The bin layer reads `starter-config`'s `backfill.window_days`
//! and constructs a [`BackfillConfig`]; this module does not
//! reach into config itself (the §0.6 boundary rule).
//!
//! ## Pacing — separate from the reconciler
//!
//! The stage requirement is explicit: backfill **must not starve
//! real-time webhook processing of rate-limit budget**. Two
//! mechanisms (used together):
//!
//! 1. **Separate octocrab client wrapper instance.** The bin
//!    layer constructs a dedicated [`Client`] for backfill, with
//!    the same credentials but a separate request stream. This
//!    is the "or shared with priority" clause from the stage
//!    spec: in practice we recommend separate clients so the
//!    octocrab transient state (pending retries, in-flight
//!    requests) is partitioned.
//! 2. **Lower headroom threshold.** Backfill voluntarily yields
//!    when GitHub reports `x-ratelimit-remaining` under
//!    [`BackfillConfig::rate_limit_headroom`] — default 1000.
//!    The reconciler's pacing is implicit in the
//!    [`Client`] wrapper (it errors only on actual RL exhaustion,
//!    leaving the budget for webhooks); backfill is the
//!    *aggressive* consumer, so the voluntary yield is what
//!    keeps the live path responsive. We sleep until the window
//!    resets, not until we'd be under headroom by some margin —
//!    GitHub's reset boundary is the only honest signal.
//!
//! ## Resumable via `fetch_cursors`
//!
//! A crashed backfill picks up where it left off. The
//! per-`(org, repo, resource_kind)` cursor (TODO §0.3) stores
//! the high-water `since` timestamp the backfill last reached;
//! [`Backfill::run_for_org`] reads it on entry and uses
//! `max(cursor.since, window_start)` as the effective lower
//! bound for the next pass. After each `(target, kind)` chunk
//! the cursor is updated, so a crash mid-org loses at most one
//! chunk's worth of progress.
//!
//! ## fetch_runs rows per chunk
//!
//! Every `(target × resource_kind)` chunk opens a `fetch_runs`
//! row of kind [`FetchRunKind::Backfill`] (TODO §0.3 run-log
//! requirement). `dp-rest /admin/runs` reads those rows to show
//! operators backfill progress without polling the cursor table
//! directly.
//!
//! ## Shared upsert path
//!
//! Same invariant as the reconciler (Stage 8): backfill does
//! **not** re-implement the per-event upsert. It synthesises
//! webhook-shaped deliveries from the list-endpoint payloads via
//! [`crate::reconciler::synth`] and dispatches through
//! [`crate::worker::apply_delivery`]. The co-author /
//! squash-merge / bot / unattributed handling Stage 6 pinned in
//! fixture tests therefore applies to backfilled events without
//! a second test pass.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use dp_domain::{fetch::FetchRunErrorSample, FetchCursor, FetchRunKind, ResourceKind, Store, StoreError};
use tokio::sync::watch;
use uuid::Uuid;

use crate::client::{Client, ClientError, Fetched, RateLimitSignal};
use crate::reconciler::synth;
use crate::reconciler::{RepoTarget, TargetProvider};
use crate::worker::{apply_delivery, HandlerError};

/// Knobs that shape one [`Backfill`] run.
///
/// Constructed by the bin layer from `starter-config`
/// (`backfill.window_days`, `backfill.rate_limit_headroom`) and
/// handed in by value — this crate does not touch starter-config
/// itself (TODO §0.6 boundary rule).
#[derive(Debug, Clone, Copy)]
pub struct BackfillConfig {
    /// How far back the backfill walks. Default = **90 days**,
    /// matching the working assumption in `SCOPE.md` §Decisions.
    /// Revisited per the trigger pinned in stage-0 (first target
    /// deployment).
    pub window: Duration,
    /// Voluntary yield threshold against
    /// `x-ratelimit-remaining`. When the last response showed a
    /// remaining count under this, backfill sleeps until reset.
    /// Default = **1000**, deliberately well above the
    /// reconciler's implicit "pause when actually exhausted"
    /// threshold so live webhook processing keeps budget.
    pub rate_limit_headroom: u64,
}

impl Default for BackfillConfig {
    fn default() -> Self {
        Self {
            window: Duration::from_secs(90 * 24 * 60 * 60),
            rate_limit_headroom: 1000,
        }
    }
}

/// Counters from one [`Backfill::run_for_org`] call. The values
/// are aggregated across every `(target × kind)` chunk and
/// surface through tracing + the closing log line.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BackfillStats {
    /// Chunks (= `(target, kind)` tuples) that ran end-to-end.
    pub chunks: i64,
    /// Total deliveries synthesised + applied across all chunks.
    pub items: i64,
    /// Chunks that errored (client or store). Per-chunk
    /// failures are absorbed; the org-level call still returns
    /// `Ok` so a crashed chunk does not roll back the rest.
    pub errors: i64,
    /// Chunks skipped because the cursor was already at or past
    /// the window head (resume path — nothing left to do).
    pub skipped: i64,
}

/// Errors the backfill bubbles out the top of
/// [`Backfill::run_for_org`]. Per-chunk failures are absorbed
/// into `BackfillStats.errors` + a `tracing::warn!` — the only
/// escapes are store-level failures the caller couldn't have
/// done anything about, plus a cooperative cancellation from
/// the shutdown channel.
#[derive(Debug, thiserror::Error)]
pub enum BackfillError {
    /// Store failed during target enumeration or run-log writes.
    #[error("store: {0}")]
    Store(#[from] StoreError),
    /// Cooperative cancellation observed mid-run.
    #[error("backfill cancelled")]
    Cancelled,
}

/// One-shot bounded-window historical importer.
///
/// Cheap to construct; clone-free by design — backfill is run
/// once per org, then dropped. Multiple concurrent
/// `run_for_org` calls against the same `Backfill` are
/// supported but discouraged: each one will fight the others
/// for the shared [`Client`]'s rate-limit budget, exactly the
/// situation the separate-client pacing rule is designed to
/// avoid.
pub struct Backfill {
    store: Arc<dyn Store>,
    /// **Dedicated** client wrapper. The bin layer constructs
    /// this separately from the reconciler's client so backfill
    /// cannot drain the budget the live path is using. Same
    /// credentials, separate octocrab handle.
    client: Arc<Client>,
    targets: Arc<dyn TargetProvider>,
    /// Resource kinds the backfill walks for each repo. Mirrors
    /// the reconciler's default set (PRs, issues, commits) —
    /// resource kinds that don't expose a `since=` parameter
    /// (reviews, review_comments, workflow_runs, …) ride the
    /// webhook path exclusively and so backfill cannot help.
    kinds: Arc<[ResourceKind]>,
    config: BackfillConfig,
}

impl Backfill {
    /// Build a backfill driver. `client` should be the
    /// **dedicated** wrapper (see [`Backfill`] docs). Default
    /// resource kinds are PRs + issues + commits — same as the
    /// reconciler's default for the same reason.
    pub fn new(
        store: Arc<dyn Store>,
        client: Arc<Client>,
        targets: Arc<dyn TargetProvider>,
        config: BackfillConfig,
    ) -> Self {
        Self {
            store,
            client,
            targets,
            kinds: Arc::from(
                [
                    ResourceKind::PullRequests,
                    ResourceKind::Issues,
                    ResourceKind::Commits,
                ]
                .as_slice(),
            ),
            config,
        }
    }

    /// Override the kinds backfilled. Mostly for tests that want
    /// to exercise one resource kind in isolation; production
    /// runs use the default set.
    pub fn with_kinds(mut self, kinds: &[ResourceKind]) -> Self {
        self.kinds = Arc::from(kinds);
        self
    }

    /// Backfill every repo belonging to `org_id`, for the
    /// configured window, in cursor-resumable chunks.
    ///
    /// `shutdown` lets the bin layer cooperatively cancel a
    /// long-running install-time backfill (e.g. the server is
    /// shutting down). The current chunk finishes — same
    /// reasoning as the worker: aborting mid-chunk would orphan
    /// the open `fetch_runs` row and the partial cursor advance.
    pub async fn run_for_org(
        &self,
        org_id: Uuid,
        shutdown: Option<watch::Receiver<bool>>,
    ) -> Result<BackfillStats, BackfillError> {
        let now = Utc::now();
        let window_start = now
            - chrono::Duration::from_std(self.config.window)
                .unwrap_or_else(|_| chrono::Duration::days(90));

        let all_targets = self.targets.list_targets().await?;
        let targets: Vec<RepoTarget> = all_targets
            .into_iter()
            .filter(|t| t.org_id == org_id)
            .collect();

        tracing::info!(
            target: "dp_fetcher::backfill",
            %org_id,
            repos = targets.len(),
            window_days = self.config.window.as_secs() / 86_400,
            "backfill starting"
        );

        let mut stats = BackfillStats::default();

        for target in &targets {
            for &kind in self.kinds.iter() {
                if shutdown_observed(&shutdown) {
                    return Err(BackfillError::Cancelled);
                }

                let span = tracing::info_span!(
                    target: "dp_fetcher::backfill",
                    "backfill.chunk",
                    org   = %target.org_id,
                    repo  = %target.repo_id,
                    owner = %target.owner_login,
                    name  = %target.repo_name,
                    kind  = ?kind,
                );
                let _enter = span.enter();

                match self.run_chunk(target, kind, window_start).await {
                    Ok(ChunkOutcome::Applied { items, signal }) => {
                        stats.chunks += 1;
                        stats.items += items;
                        self.honour_headroom(signal).await;
                    }
                    Ok(ChunkOutcome::Skipped) => {
                        stats.skipped += 1;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "backfill chunk failed");
                        stats.errors += 1;
                    }
                }
            }
        }

        tracing::info!(
            target: "dp_fetcher::backfill",
            %org_id,
            chunks  = stats.chunks,
            items   = stats.items,
            errors  = stats.errors,
            skipped = stats.skipped,
            "backfill complete"
        );

        Ok(stats)
    }

    /// Run one `(target, kind)` chunk. Opens a `fetch_runs` row
    /// of kind [`FetchRunKind::Backfill`], reads + advances the
    /// cursor, and synthesises deliveries through the shared
    /// `apply_delivery` path. Per-chunk run-log granularity is
    /// what lets `/admin/runs` show progress.
    async fn run_chunk(
        &self,
        target: &RepoTarget,
        kind: ResourceKind,
        window_start: chrono::DateTime<Utc>,
    ) -> Result<ChunkOutcome, ChunkError> {
        let run_id = self
            .store
            .start_fetch_run(FetchRunKind::Backfill)
            .await
            .map_err(ChunkError::Store)?;

        let result = self.run_chunk_inner(target, kind, window_start).await;

        let (items, errors, partial, outcome) = match &result {
            Ok(ChunkOutcome::Applied { items, .. }) => (*items, 0, false, None),
            Ok(ChunkOutcome::Skipped) => (0, 0, false, Some(())),
            Err(_) => (0, 1, false, None),
        };
        let _ = outcome;

        // Persist the failing error string (truncated) so
        // `/admin/runs` can show *why* a backfill chunk failed
        // without grepping the log. Skipped here for clean /
        // skipped chunks — only Err(_) writes a sample.
        if let Err(e) = &result {
            let mut msg = e.to_string();
            const ERROR_MSG_CAP: usize = 500;
            if msg.len() > ERROR_MSG_CAP {
                msg.truncate(ERROR_MSG_CAP);
                msg.push('\u{2026}');
            }
            let sample = FetchRunErrorSample {
                org: Some(target.owner_login.clone()),
                repo: Some(format!("{}/{}", target.owner_login, target.repo_name)),
                kind: Some(format!("{kind:?}")),
                error: msg,
            };
            if let Err(re) = self
                .store
                .record_fetch_run_errors(run_id, std::slice::from_ref(&sample))
                .await
            {
                tracing::warn!(error = %re, %run_id, "failed to record backfill error sample");
            }
        }

        // Best-effort close — a store failure on the close path
        // does not invalidate the upserts the chunk already
        // performed; surface it but prefer the chunk's own
        // outcome as the primary result.
        if let Err(e) = self
            .store
            .finish_fetch_run(run_id, items, errors, partial)
            .await
        {
            tracing::warn!(error = %e, %run_id, "failed to close backfill fetch_run");
        }

        result
    }

    async fn run_chunk_inner(
        &self,
        target: &RepoTarget,
        kind: ResourceKind,
        window_start: chrono::DateTime<Utc>,
    ) -> Result<ChunkOutcome, ChunkError> {
        // ---- 1. Load cursor for resume. ------------------------
        let cursor = match self
            .store
            .get_cursor(target.org_id, Some(target.repo_id), kind)
            .await
        {
            Ok(c) => Some(c),
            Err(StoreError::NotFound { .. }) => None,
            Err(e) => return Err(ChunkError::Store(e)),
        };

        // Effective lower bound: pick whichever is later between
        // (a) the window start, and (b) the cursor's high-water
        // mark from a previous backfill / reconciler pass. That
        // is what makes the backfill resumable across crashes —
        // a re-run does not refetch what's already in the store.
        let effective_since = match cursor.as_ref().and_then(|c| c.since) {
            Some(s) if s >= window_start => s,
            _ => window_start,
        };

        // Resume short-circuit: if the cursor's high-water is
        // already past "now" (clock skew, or the reconciler
        // raced ahead of us), nothing to do. We still write the
        // fetch_runs close in the caller so /admin/runs shows
        // the no-op chunk for visibility.
        if effective_since >= Utc::now() {
            return Ok(ChunkOutcome::Skipped);
        }

        // ---- 2. Conditional GET via the dedicated client. -----
        let etag = cursor.as_ref().and_then(|c| c.etag.clone());
        let fetched = match kind {
            ResourceKind::PullRequests => {
                self.client
                    .list_pull_requests(&target.owner_login, &target.repo_name, etag.as_deref())
                    .await
            }
            ResourceKind::Issues => {
                self.client
                    .list_issues(
                        &target.owner_login,
                        &target.repo_name,
                        Some(effective_since),
                        etag.as_deref(),
                    )
                    .await
            }
            ResourceKind::Commits => {
                self.client
                    .list_commits(
                        &target.owner_login,
                        &target.repo_name,
                        Some(effective_since),
                        etag.as_deref(),
                    )
                    .await
            }
            // Other kinds (reviews, workflow_runs, deployments…)
            // ride the webhook path exclusively — see module
            // docs. Treat as no-op chunk so the run-log still
            // accounts for the attempt.
            _ => return Ok(ChunkOutcome::Skipped),
        }
        .map_err(ChunkError::Client)?;

        // ---- 3. Branch on Fetched. -----------------------------
        let (body, new_etag, signal) = match fetched {
            Fetched::NotModified { signal } => {
                // No body to dispatch but bump cursor.updated_at
                // so operators can see the chunk did run.
                let updated = FetchCursor {
                    org_id: target.org_id,
                    repo_id: Some(target.repo_id),
                    resource_kind: kind,
                    since: cursor.as_ref().and_then(|c| c.since).or(Some(effective_since)),
                    etag,
                    last_event_id: cursor.as_ref().and_then(|c| c.last_event_id.clone()),
                    updated_at: Utc::now(),
                };
                self.store
                    .put_cursor(&updated)
                    .await
                    .map_err(ChunkError::Store)?;
                return Ok(ChunkOutcome::Applied { items: 0, signal });
            }
            Fetched::Ok { body, etag, signal } => (body, etag, signal),
        };

        // ---- 4. Synthesise webhook deliveries. -----------------
        let items: &[serde_json::Value] = body.as_array().map(|v| v.as_slice()).unwrap_or(&[]);
        let deliveries = match kind {
            ResourceKind::PullRequests => synth::pulls_response_to_deliveries(target, items),
            ResourceKind::Issues => synth::issues_response_to_deliveries(target, items),
            ResourceKind::Commits => synth::commits_response_to_delivery(target, items)
                .into_iter()
                .collect(),
            _ => Vec::new(),
        };

        // ---- 5. Dispatch via the shared handler path. ----------
        let mut applied: i64 = 0;
        for d in &deliveries {
            match apply_delivery(self.store.as_ref(), d).await {
                Ok(_) => applied += 1,
                Err(HandlerError::Ignored { .. }) => applied += 1,
                Err(e) => tracing::warn!(error = %e, "backfill dispatch failed"),
            }
        }

        // ---- 6. Advance cursor. --------------------------------
        let new_since = match kind {
            ResourceKind::PullRequests => synth::max_timestamp(
                items,
                &["updated_at", "closed_at", "merged_at", "created_at"],
            ),
            ResourceKind::Issues => {
                synth::max_timestamp(items, &["updated_at", "closed_at", "created_at"])
            }
            ResourceKind::Commits => {
                synth::max_timestamp(items, &["commit.committer.date", "commit.author.date"])
            }
            _ => None,
        };
        // The cursor advances to whichever is newest of:
        //   - the previous cursor.since
        //   - the effective_since we used for this chunk
        //   - the newest timestamp the response actually held
        // That guarantees a crash + restart cannot regress the
        // resume point even if the response was empty.
        let advanced_since = [cursor.as_ref().and_then(|c| c.since), Some(effective_since), new_since]
            .into_iter()
            .flatten()
            .max();
        let updated = FetchCursor {
            org_id: target.org_id,
            repo_id: Some(target.repo_id),
            resource_kind: kind,
            since: advanced_since,
            etag: new_etag.or(etag),
            last_event_id: cursor.and_then(|c| c.last_event_id),
            updated_at: Utc::now(),
        };
        self.store
            .put_cursor(&updated)
            .await
            .map_err(ChunkError::Store)?;

        Ok(ChunkOutcome::Applied { items: applied, signal })
    }

    /// Voluntary yield: if the most recent response showed the
    /// primary-budget remaining count under our headroom
    /// threshold, sleep until GitHub's `x-ratelimit-reset`.
    /// Same handling for primary-exhausted / secondary
    /// signals — those should only ever be observed if the
    /// headroom check did not catch the spend in time.
    async fn honour_headroom(&self, signal: Option<RateLimitSignal>) {
        let Some(sig) = signal else { return };
        let now = Utc::now();
        let sleep_until = match sig {
            RateLimitSignal::Ok { remaining, reset_at } => {
                if remaining < self.config.rate_limit_headroom {
                    tracing::info!(
                        target: "dp_fetcher::backfill",
                        remaining,
                        headroom = self.config.rate_limit_headroom,
                        %reset_at,
                        "backfill yielding rate-limit budget to live path"
                    );
                    Some(reset_at)
                } else {
                    None
                }
            }
            RateLimitSignal::PrimaryExhausted { reset_at } => {
                tracing::warn!(
                    target: "dp_fetcher::backfill",
                    %reset_at,
                    "backfill hit primary RL (headroom should have caught this earlier)"
                );
                Some(reset_at)
            }
            RateLimitSignal::SecondaryRateLimit { retry_at } => {
                tracing::warn!(
                    target: "dp_fetcher::backfill",
                    %retry_at,
                    "backfill hit secondary RL"
                );
                Some(retry_at)
            }
        };
        if let Some(until) = sleep_until {
            let dur = (until - now).to_std().unwrap_or(Duration::from_secs(0));
            // Cap the sleep at 1h so an absurd reset timestamp
            // cannot lock the backfill for a calendar day. The
            // next chunk will re-observe the signal anyway.
            let dur = dur.min(Duration::from_secs(3600));
            if dur > Duration::ZERO {
                tokio::time::sleep(dur).await;
            }
        }
    }
}

/// Outcome of one chunk; mapped onto the closing `fetch_runs`
/// row in [`Backfill::run_chunk`].
#[derive(Debug)]
enum ChunkOutcome {
    Applied {
        items: i64,
        signal: Option<RateLimitSignal>,
    },
    Skipped,
}

#[derive(Debug, thiserror::Error)]
enum ChunkError {
    #[error("client: {0}")]
    Client(ClientError),
    #[error("store: {0}")]
    Store(StoreError),
}

fn shutdown_observed(shutdown: &Option<watch::Receiver<bool>>) -> bool {
    shutdown.as_ref().map(|r| *r.borrow()).unwrap_or(false)
}

#[cfg(test)]
mod tests;
