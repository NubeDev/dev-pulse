//! Reconciler — Stage 8 of the dev-pulse ingestion layer
//! (TODO §0.1, §0.3, SCOPE §10).
//!
//! Webhooks are primary (Stages 4–5); this module is the safety
//! net. GitHub's webhook contract is at-least-once but it is not
//! "always-once" — deliveries can be dropped during outages, App
//! permissions changes, or installation reinstalls. The reconciler
//! ticks every 4 hours (configurable), reads the per-`(org, repo,
//! resource_kind)` cursors written to `fetch_cursors` (TODO §0.3),
//! issues conditional GETs against the GitHub REST API via
//! [`crate::client::Client`], and feeds anything new through the
//! **same** [`crate::worker::apply_delivery`] path the webhook
//! worker uses — Stage 8's "zero code duplication" invariant.
//!
//! ## Tick anatomy
//!
//! `do_tick(scope)`:
//!
//! 1. Open a `fetch_runs` row of kind [`FetchRunKind::Reconciler`]
//!    (the per-tick run log per TODO §0.3).
//! 2. Enumerate [`RepoTarget`]s from the injected
//!    [`TargetProvider`], filter by the supplied [`Scope`].
//! 3. For each `(target × resource_kind)`:
//!    - Read the cursor (`since`, `etag`) from the store.
//!    - Call the appropriate `client.list_*` with the etag.
//!    - On 304: no work; update `cursor.updated_at`.
//!    - On 200: synthesise webhook deliveries via
//!      [`synth`] and dispatch through `apply_delivery`. Advance
//!      `since` to the max timestamp observed, persist the new
//!      etag.
//!    - On rate-limit errors: stop *this* `(target, kind)` and
//!      mark the run partial; other targets keep going.
//! 4. Close the run with `(items, errors, partial)` totals.
//!
//! ## Coalescing
//!
//! The [`Scheduler`] holds a `Mutex<Option<JoinHandle<…>>>` for the
//! currently-running tick. If the interval fires while a tick is
//! still draining, the new fire turns into a no-op (we don't queue
//! ticks). The same handle field is what `fetch-now` / `POST
//! /admin/refresh` consult — an operator-triggered tick coalesces
//! the same way against an in-flight scheduled tick.
//!
//! ## Test surface
//!
//! Reconciler tests exercise the full path via wiremock standing
//! in for `api.github.com` plus the [`crate::worker::test_store`]
//! `FakeStore`. The Stage 3 client wrapper is the only thing that
//! talks to octocrab; reconciler tests don't need to mock it
//! deeper.

pub(crate) mod synth;
mod targets;
#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use dp_domain::{
    FetchCursor, FetchRunKind, ResourceKind, Store, StoreError,
};
use tokio::sync::{oneshot, watch, Mutex};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::client::{Client, ClientError, Fetched};
use crate::worker::{apply_delivery, HandlerError};

pub use targets::{RepoTarget, StaticTargets, TargetProvider};

/// Scopes a single [`Reconciler::do_tick`] call.
///
/// `Scope::All` is the scheduled-tick default. `Scope::Org` /
/// `Scope::Repo` exist for operator-triggered refreshes — the CLI
/// `fetch-now --org X` flow + `POST /admin/refresh?repo_id=…` map
/// onto these variants so they can stay narrow and not blow rate
/// limit budget when an operator just wants to re-check one repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Every target the [`TargetProvider`] returns.
    All,
    /// Only targets whose `org_id` matches.
    Org(Uuid),
    /// One repo. `org_id` is included so the cursor lookup stays
    /// keyed on the same composite the store uses.
    Repo {
        /// Org the repo belongs to.
        org_id: Uuid,
        /// Repo to reconcile.
        repo_id: Uuid,
    },
}

impl Scope {
    fn matches(&self, t: &RepoTarget) -> bool {
        match self {
            Scope::All => true,
            Scope::Org(o) => t.org_id == *o,
            Scope::Repo { org_id, repo_id } => t.org_id == *org_id && t.repo_id == *repo_id,
        }
    }
}

/// What [`Reconciler::do_tick`] reports back. Mirrors what's
/// written to the `fetch_runs` row.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TickStats {
    /// Total webhook deliveries synthesised + applied in this tick
    /// (one per PR / issue, one per `push` aggregation for commits).
    pub items: i64,
    /// Targets × kinds that errored — including rate-limit pauses.
    pub errors: i64,
    /// `true` if any `(target, kind)` failed but at least one
    /// succeeded.
    pub partial: bool,
}

/// Errors the reconciler bubbles out the top of [`Reconciler::do_tick`].
///
/// Per-`(target, kind)` failures are absorbed into `TickStats.errors`
/// + `partial=true`. The only escapes are store-level failures the
/// caller couldn't have done anything about.
#[derive(Debug, thiserror::Error)]
pub enum ReconcilerError {
    /// Store failed during run-log bookkeeping or target enumeration.
    #[error("store: {0}")]
    Store(#[from] StoreError),
}

/// The reconciler. Cheap to clone — every field is an `Arc`.
#[derive(Clone)]
pub struct Reconciler {
    store: Arc<dyn Store>,
    client: Arc<Client>,
    targets: Arc<dyn TargetProvider>,
    /// Resource kinds to reconcile every tick. Per-kind list of
    /// what `do_tick` should iterate; we keep it on the reconciler
    /// (not Scope) so a future config knob can disable e.g. commits
    /// without recompiling.
    kinds: Arc<[ResourceKind]>,
}

impl Reconciler {
    /// Build a reconciler with the default resource-kind set:
    /// pull requests, issues, commits. (Reviews / review-comments
    /// / workflow_runs / deployments / releases ride the webhook
    /// path exclusively for now — they have no `since=` parameter
    /// on the list endpoint that would let us page cheaply, so the
    /// cost/benefit favours webhook-only.)
    pub fn new(
        store: Arc<dyn Store>,
        client: Arc<Client>,
        targets: Arc<dyn TargetProvider>,
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
        }
    }

    /// Override the set of resource kinds reconciled per tick.
    /// Mostly for tests that want to exercise one kind in isolation.
    pub fn with_kinds(mut self, kinds: &[ResourceKind]) -> Self {
        self.kinds = Arc::from(kinds);
        self
    }

    /// Run one reconciler tick. Same entrypoint the scheduler, the
    /// `fetch-now` CLI, and `POST /admin/refresh` call — Stage 8's
    /// shared seam.
    pub async fn do_tick(&self, scope: Scope) -> Result<TickStats, ReconcilerError> {
        let run_id = self.store.start_fetch_run(FetchRunKind::Reconciler).await?;
        let all_targets = self.targets.list_targets().await?;
        let targets: Vec<RepoTarget> = all_targets
            .into_iter()
            .filter(|t| scope.matches(t))
            .collect();

        let mut stats = TickStats::default();
        let mut successes: i64 = 0;

        for target in &targets {
            for &kind in self.kinds.iter() {
                let span = tracing::info_span!(
                    target: "dp_fetcher::reconciler",
                    "reconciler.target",
                    org   = %target.org_id,
                    repo  = %target.repo_id,
                    owner = %target.owner_login,
                    name  = %target.repo_name,
                    kind  = ?kind,
                );
                let _enter = span.enter();
                match self.reconcile_one(target, kind).await {
                    Ok(applied) => {
                        stats.items += applied;
                        successes += 1;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "reconcile failed");
                        stats.errors += 1;
                    }
                }
            }
        }

        stats.partial = stats.errors > 0 && successes > 0;
        self.store
            .finish_fetch_run(run_id, stats.items, stats.errors, stats.partial)
            .await?;

        Ok(stats)
    }

    /// Reconcile one `(target, resource_kind)`. Returns the number
    /// of synthesised deliveries successfully applied.
    async fn reconcile_one(
        &self,
        target: &RepoTarget,
        kind: ResourceKind,
    ) -> Result<i64, OneError> {
        // ---- 1. Load the cursor. -------------------------------
        let cursor = match self
            .store
            .get_cursor(target.org_id, Some(target.repo_id), kind)
            .await
        {
            Ok(c) => Some(c),
            Err(StoreError::NotFound { .. }) => None,
            Err(e) => return Err(OneError::Store(e)),
        };
        let since = cursor.as_ref().and_then(|c| c.since);
        let etag = cursor.as_ref().and_then(|c| c.etag.clone());

        // ---- 2. Fetch from GitHub via the wrapped client. ------
        let fetched = match kind {
            ResourceKind::PullRequests => {
                self.client
                    .list_pull_requests(&target.owner_login, &target.repo_name, etag.as_deref())
                    .await
            }
            ResourceKind::Issues => {
                self.client
                    .list_issues(&target.owner_login, &target.repo_name, since, etag.as_deref())
                    .await
            }
            ResourceKind::Commits => {
                self.client
                    .list_commits(&target.owner_login, &target.repo_name, since, etag.as_deref())
                    .await
            }
            // Other resource kinds ride the webhook path
            // exclusively for now — see [`Reconciler::new`].
            _ => return Ok(0),
        }
        .map_err(OneError::Client)?;

        // ---- 3. Decide what to do based on Fetched. ------------
        let (body, new_etag, has_change) = match fetched {
            Fetched::NotModified { .. } => {
                // No body to dispatch. Still bump cursor.updated_at
                // so operators can see the reconciler is alive even
                // on quiet repos.
                let updated = FetchCursor {
                    org_id: target.org_id,
                    repo_id: Some(target.repo_id),
                    resource_kind: kind,
                    since,
                    etag,
                    last_event_id: cursor.as_ref().and_then(|c| c.last_event_id.clone()),
                    updated_at: Utc::now(),
                };
                self.store.put_cursor(&updated).await.map_err(OneError::Store)?;
                return Ok(0);
            }
            Fetched::Ok { body, etag, .. } => (body, etag, true),
        };
        let _ = has_change;

        // ---- 4. Synthesise webhook deliveries. -----------------
        let items: &[serde_json::Value] = body
            .as_array()
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
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
                Err(HandlerError::Ignored { .. }) => {
                    // Benign — the synthesised payload was a kind
                    // the handler does not care about (e.g. an
                    // already-closed PR that's also already
                    // ingested). Count as success — there is
                    // nothing more to do.
                    applied += 1;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "synth dispatch failed");
                }
            }
        }

        // ---- 6. Advance cursor. --------------------------------
        let new_since = match kind {
            ResourceKind::PullRequests => synth::max_timestamp(
                items,
                &["updated_at", "closed_at", "merged_at", "created_at"],
            ),
            ResourceKind::Issues => synth::max_timestamp(
                items,
                &["updated_at", "closed_at", "created_at"],
            ),
            ResourceKind::Commits => synth::max_timestamp(
                items,
                &["commit.committer.date", "commit.author.date"],
            ),
            _ => None,
        };
        let advanced_since = match (since, new_since) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        let updated = FetchCursor {
            org_id: target.org_id,
            repo_id: Some(target.repo_id),
            resource_kind: kind,
            since: advanced_since,
            etag: new_etag.or(etag),
            last_event_id: cursor.and_then(|c| c.last_event_id),
            updated_at: Utc::now(),
        };
        self.store.put_cursor(&updated).await.map_err(OneError::Store)?;

        Ok(applied)
    }
}

/// Used inside [`Reconciler::reconcile_one`]. Top-level
/// [`Reconciler::do_tick`] swallows these into `stats.errors`.
#[derive(Debug, thiserror::Error)]
enum OneError {
    #[error("client: {0}")]
    Client(ClientError),
    #[error("store: {0}")]
    Store(StoreError),
}

/// Scheduled-tick driver around a [`Reconciler`].
///
/// Spawns a `tokio::time::interval` and calls
/// [`Reconciler::do_tick`] on each tick. **Coalescing** is the
/// invariant Stage 8 calls out: an interval fire that lands while
/// a previous tick is still in flight turns into a no-op — we do
/// not queue ticks, and we do not run ticks in parallel. The same
/// `Mutex<Option<JoinHandle<…>>>` is exposed via
/// [`Scheduler::try_trigger_now`] so an operator-triggered tick
/// (CLI or `POST /admin/refresh`) coalesces against the in-flight
/// scheduled tick using the same guard — there is exactly one
/// reconciler tick running at any moment.
pub struct Scheduler {
    reconciler: Arc<Reconciler>,
    tick_interval: Duration,
    /// The in-flight tick. We use `Mutex<Option<JoinHandle<()>>>`
    /// rather than an atomic flag so that `is_finished()` lets a
    /// later trigger distinguish "tick still running" (coalesce)
    /// from "tick completed and the slot just hasn't been cleared
    /// yet" (start a new one).
    ///
    /// The handle is `JoinHandle<()>` rather than
    /// `JoinHandle<Result<TickStats, _>>` because the result is
    /// surfaced to the originating caller via a oneshot channel —
    /// holding the result on the handle would force the caller to
    /// keep the lock across the await, which would defeat
    /// coalescing for other concurrent callers.
    current: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl Scheduler {
    /// Build a scheduler. `tick_interval` defaults to 4h in the
    /// production wiring (TODO §Phase 2 + `reconciler.tick_interval`
    /// in starter-config). Tests pass much shorter intervals.
    pub fn new(reconciler: Arc<Reconciler>, tick_interval: Duration) -> Self {
        Self {
            reconciler,
            tick_interval,
            current: Arc::new(Mutex::new(None)),
        }
    }

    /// Try to trigger a `do_tick(scope)` run now, joining the
    /// in-flight handle if there is one.
    ///
    /// Returns `Ok(None)` when a previous tick was still running
    /// (the no-op coalesce path). Returns `Ok(Some(stats))` when
    /// this call ran a tick to completion. Errors only escape on a
    /// store-level reconciler failure.
    pub async fn try_trigger_now(
        &self,
        scope: Scope,
    ) -> Result<Option<TickStats>, ReconcilerError> {
        // Phase 1: under the lock, decide whether to coalesce or
        // spawn. We deliberately do *not* hold the lock across the
        // await that follows — concurrent callers must be able to
        // grab the lock, observe the in-flight handle, and decide
        // to coalesce while this caller is still awaiting its
        // oneshot.
        let rx = {
            let mut guard = self.current.lock().await;
            if let Some(handle) = guard.as_ref() {
                if !handle.is_finished() {
                    tracing::debug!(
                        target: "dp_fetcher::reconciler",
                        "tick already in flight; coalescing"
                    );
                    return Ok(None);
                }
                // Slot has a finished handle from a previous run —
                // drop it so we start fresh.
                let _ = guard.take();
            }
            let (tx, rx) = oneshot::channel();
            let rec = self.reconciler.clone();
            let handle = tokio::spawn(async move {
                let r = rec.do_tick(scope).await;
                let _ = tx.send(r);
            });
            *guard = Some(handle);
            rx
        };

        // Phase 2: await the result via the oneshot. The mutex is
        // released; other callers landing here right now will
        // observe the in-flight handle and coalesce.
        match rx.await {
            Ok(Ok(stats)) => Ok(Some(stats)),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                // Sender dropped without sending — the task
                // panicked. Surface as "no result" rather than
                // poisoning the scheduler.
                tracing::error!(
                    target: "dp_fetcher::reconciler",
                    "tick task ended without producing a result"
                );
                Ok(None)
            }
        }
    }

    /// Run the periodic loop until `shutdown` fires. The first
    /// tick fires immediately so we don't sit through a 4h cold
    /// start before the first reconcile pass.
    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.tick_interval);
        // `MissedTickBehavior::Skip` is the right pairing with our
        // coalescing rule: if we somehow stalled past several
        // intervals, just run the next one rather than burst-firing.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if *shutdown.borrow() {
                        return;
                    }
                    if let Err(e) = self.try_trigger_now(Scope::All).await {
                        tracing::error!(
                            target: "dp_fetcher::reconciler",
                            error = %e,
                            "scheduled tick failed"
                        );
                    }
                }
                res = shutdown.changed() => {
                    if res.is_err() || *shutdown.borrow() {
                        return;
                    }
                }
            }
        }
    }
}
