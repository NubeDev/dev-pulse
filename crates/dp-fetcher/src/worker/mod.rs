//! Webhook worker — Stage 5 of the dev-pulse ingestion layer
//! (TODO §Phase-2, §0.1, §0.2).
//!
//! The receiver (Stage 4) does the *minimum* synchronous work and
//! returns 200. This module is the everything-else: a long-running
//! task that drains `webhook_inbox` via [`Store::claim_webhooks`],
//! dispatches each delivery through [`handlers::apply_delivery`],
//! and marks the row processed (or failed-with-retry) on the way
//! out.
//!
//! ## Drain contract
//!
//! One `drain_once` call is one "batch":
//!
//! 1. [`Store::start_fetch_run`] with
//!    [`FetchRunKind::WebhookWorker`] — the run log row tracks the
//!    batch boundary (TODO §Phase-2 §10 ops requirement).
//! 2. [`Store::claim_webhooks`] takes up to `batch_size` rows
//!    under `FOR UPDATE SKIP LOCKED`. Two workers racing on the
//!    same row would be a correctness bug (double-processing,
//!    double-counted actors); skip-locked is the Postgres-canonical
//!    way to guarantee at-most-one claim per row.
//! 3. Each delivery flows through [`handlers::apply_delivery`].
//!    Success → [`Store::mark_webhook_processed`]. Failure →
//!    [`Store::mark_webhook_failed`] with the error message; the
//!    row stays claimable on the next drain (GitHub's at-least-once
//!    delivery contract is the safety net).
//!    [`HandlerError::Ignored`] counts as success — the event
//!    body was well-formed but uninteresting (e.g. a
//!    `pull_request.action = "labeled"`); we don't want it sitting
//!    in the inbox forever.
//! 4. [`Store::finish_fetch_run`] with the success / error totals
//!    closes out the run row.
//!
//! ## Shutdown
//!
//! The worker exits cooperatively when the cancellation channel
//! from `dp-server` fires. We use [`tokio::sync::watch`] because
//! the receiver side is `Clone` — multiple workers can subscribe
//! to the same shutdown signal without the server having to know
//! how many it spawned.
//!
//! The cancellation only stops the *poll* loop. A drain that's
//! already in flight finishes (or surfaces an error and marks the
//! row failed) so the next drain knows where it stood. Quick-exit
//! on a half-drained batch would leak claims under
//! `FOR UPDATE SKIP LOCKED` until the transaction times out, which
//! is worse than waiting for the batch to finish.

pub mod handlers;
pub mod trailers;
#[cfg(test)]
pub(crate) mod test_store;
#[cfg(test)]
mod fixture_tests;

use std::sync::Arc;
use std::time::Duration;

use dp_domain::{FetchRunKind, Store, StoreError};
use tokio::sync::watch;

pub use handlers::{apply_delivery, HandlerError, HandlerOutcome};

/// Counters returned by one [`Worker::drain_once`] call. The
/// values feed the run-log row and surface through tracing for
/// operator observability.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DrainStats {
    /// Deliveries claimed off the inbox in this batch.
    pub claimed: i64,
    /// Deliveries marked processed (handler succeeded or
    /// surfaced a benign `Ignored`).
    pub processed: i64,
    /// Deliveries marked failed (handler returned an error other
    /// than `Ignored`). The row stays claimable.
    pub failed: i64,
}

/// Errors the worker raises out the top — these are things the
/// outer task can't keep running through.
#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    /// The store itself failed (claim / mark-processed /
    /// mark-failed / fetch-run write). We bubble these so the
    /// supervisor (in `dp-server`) can restart the worker.
    #[error("store: {0}")]
    Store(#[from] StoreError),
}

/// Long-running drain task for `webhook_inbox`.
///
/// Cheap to construct (it owns nothing but an `Arc<dyn Store>`
/// and two scalars). Spawn it on the runtime via
/// [`Worker::run`].
#[derive(Clone)]
pub struct Worker {
    store: Arc<dyn Store>,
    /// Max rows per [`Store::claim_webhooks`] call.
    batch_size: i64,
    /// Time to wait between drains when the inbox is empty.
    /// Cancellation interrupts this immediately.
    idle_poll: Duration,
}

impl Worker {
    /// Build a worker with the production defaults: 100 rows per
    /// drain, 250ms idle poll. Both are tunable via the
    /// `with_*` builders below; the bin layer is expected to pull
    /// the values from config (TODO §Phase-2 keeps tuning out of
    /// this crate).
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self {
            store,
            batch_size: 100,
            idle_poll: Duration::from_millis(250),
        }
    }

    /// Override the per-drain batch size. Larger batches amortise
    /// the run-log overhead; smaller batches lower latency from
    /// "row in inbox" to "row applied". The default is the
    /// observed sweet spot on the deployment target.
    pub fn with_batch_size(mut self, n: i64) -> Self {
        self.batch_size = n.max(1);
        self
    }

    /// Override the idle-poll interval. Tests want zero (just
    /// loop); production wants ~250ms so we don't hammer the
    /// store when the inbox is quiet.
    pub fn with_idle_poll(mut self, d: Duration) -> Self {
        self.idle_poll = d;
        self
    }

    /// Drain one batch. The unit of work the worker reports a
    /// `fetch_runs` row for.
    ///
    /// `start_fetch_run` runs **before** `claim_webhooks` even
    /// when there is nothing to claim — the empty-batch row is
    /// useful "the worker is alive and looked" telemetry.
    pub async fn drain_once(&self) -> Result<DrainStats, WorkerError> {
        let run_id = self.store.start_fetch_run(FetchRunKind::WebhookWorker).await?;
        let claimed = self.store.claim_webhooks(self.batch_size).await?;

        let mut stats = DrainStats {
            claimed: claimed.len() as i64,
            processed: 0,
            failed: 0,
        };

        for delivery in &claimed {
            // Per-delivery span so the structured logs join on
            // `webhook.delivery_id` the receiver already set.
            let span = tracing::info_span!(
                target: "dp_fetcher::worker",
                "webhook.apply",
                webhook.delivery_id = %delivery.delivery_id,
                webhook.event = %delivery.event,
            );
            let _enter = span.enter();

            match apply_delivery(self.store.as_ref(), delivery).await {
                Ok(outcome) => {
                    tracing::debug!(
                        events = outcome.events,
                        actors = outcome.actors,
                        "applied"
                    );
                    self.store.mark_webhook_processed(delivery.id).await?;
                    stats.processed += 1;
                }
                Err(e) if e.is_benign() => {
                    // Recognised but uninteresting — still mark
                    // processed so the row leaves the inbox.
                    tracing::debug!(reason = %e, "ignored");
                    self.store.mark_webhook_processed(delivery.id).await?;
                    stats.processed += 1;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "handler failed; leaving claimable");
                    self.store
                        .mark_webhook_failed(delivery.id, &e.to_string())
                        .await?;
                    stats.failed += 1;
                }
            }
        }

        // partial = the batch finished but some rows failed; the
        // next drain will pick them back up.
        self.store
            .finish_fetch_run(run_id, stats.processed, stats.failed, stats.failed > 0)
            .await?;

        Ok(stats)
    }

    /// Run drains forever until `shutdown` fires. Reading `true`
    /// off the channel (or the sender being dropped) ends the
    /// loop after the current drain settles.
    ///
    /// Errors out of `drain_once` are logged-and-retried, not
    /// fatal — a single bad claim shouldn't take the worker
    /// down. A persistent store outage will show up as a
    /// continuous error stream and the supervisor can decide.
    pub async fn run(self, mut shutdown: watch::Receiver<bool>) -> Result<(), WorkerError> {
        loop {
            if *shutdown.borrow() {
                tracing::info!(target: "dp_fetcher::worker", "shutdown received before drain");
                return Ok(());
            }

            match self.drain_once().await {
                Ok(stats) if stats.claimed == 0 => {
                    // Empty inbox — sleep until either the poll
                    // interval elapses or shutdown fires. The
                    // select! is the cooperative-cancellation
                    // point: a `watch::changed` wakeup is
                    // instant, no 250ms tax on shutdown.
                    tokio::select! {
                        _ = tokio::time::sleep(self.idle_poll) => {}
                        res = shutdown.changed() => {
                            if res.is_err() || *shutdown.borrow() {
                                tracing::info!(target: "dp_fetcher::worker", "shutdown during idle poll");
                                return Ok(());
                            }
                        }
                    }
                }
                Ok(stats) => {
                    tracing::debug!(
                        target: "dp_fetcher::worker",
                        claimed = stats.claimed,
                        processed = stats.processed,
                        failed = stats.failed,
                        "drained batch"
                    );
                    // Re-check shutdown without sleeping — there
                    // may be more rows waiting.
                }
                Err(e) => {
                    tracing::error!(
                        target: "dp_fetcher::worker",
                        error = %e,
                        "drain failed; backing off"
                    );
                    tokio::select! {
                        _ = tokio::time::sleep(self.idle_poll) => {}
                        res = shutdown.changed() => {
                            if res.is_err() || *shutdown.borrow() {
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_store::FakeStore;
    use super::*;
    use dp_domain::{ActorRole, EventKind, WebhookDelivery};
    use serde_json::json;
    use std::time::Duration;
    use uuid::Uuid;

    fn delivery(event: &str, payload: serde_json::Value) -> WebhookDelivery {
        WebhookDelivery {
            id: Uuid::new_v4(),
            delivery_id: format!("d-{}", Uuid::new_v4()),
            event: event.into(),
            payload,
            received_at: chrono::Utc::now(),
            processed_at: None,
            error: None,
        }
    }

    fn pr_opened() -> WebhookDelivery {
        delivery(
            "pull_request",
            json!({
                "action": "opened",
                "repository": {
                    "id": 1, "name": "r",
                    "owner": { "id": 1, "login": "o" }
                },
                "pull_request": {
                    "node_id": format!("PR_{}", Uuid::new_v4()),
                    "created_at": "2024-01-01T00:00:00Z",
                    "user": { "id": 7, "login": "alice" }
                }
            }),
        )
    }

    #[tokio::test]
    async fn drain_processes_and_marks_each_delivery() {
        let store = Arc::new(FakeStore::new());
        store.enqueue_webhook_for_test(pr_opened());
        store.enqueue_webhook_for_test(pr_opened());

        let w = Worker::new(store.clone() as Arc<dyn Store>);
        let stats = w.drain_once().await.unwrap();
        assert_eq!(stats.claimed, 2);
        assert_eq!(stats.processed, 2);
        assert_eq!(stats.failed, 0);
        // Both rows are now processed — second drain claims nothing.
        let stats2 = w.drain_once().await.unwrap();
        assert_eq!(stats2.claimed, 0);

        // A fetch_runs row exists per drain, of kind WebhookWorker.
        let runs = store.fetch_runs();
        assert_eq!(runs.len(), 2);
        for r in runs {
            assert!(matches!(
                r.kind,
                dp_domain::FetchRunKind::WebhookWorker
            ));
            assert!(r.finished.is_some());
        }
    }

    #[tokio::test]
    async fn handler_failure_marks_failed_and_leaves_claimable() {
        // A pull_request payload missing the required `repository`
        // block triggers a HandlerError::MissingField — the worker
        // must mark it failed (not processed) and the row must
        // stay claimable for the next drain.
        let store = Arc::new(FakeStore::new());
        let bad = delivery(
            "pull_request",
            json!({
                "action": "opened",
                "pull_request": {
                    "node_id": "PR_x", "created_at": "2024-01-01T00:00:00Z",
                    "user": { "id": 1, "login": "alice" }
                }
            }),
        );
        store.enqueue_webhook_for_test(bad);

        let w = Worker::new(store.clone() as Arc<dyn Store>);
        let s = w.drain_once().await.unwrap();
        assert_eq!(s.claimed, 1);
        assert_eq!(s.processed, 0);
        assert_eq!(s.failed, 1);

        // The row stays claimable.
        let pending = store.pending_count();
        assert_eq!(pending, 1);
        let err = store.last_error_for_pending().expect("error set");
        assert!(err.contains("missing"), "{err}");
    }

    #[tokio::test]
    async fn benign_ignored_marks_processed_not_failed() {
        // A `pull_request.action = "labeled"` is well-formed but
        // uninteresting. Handler returns Ignored → worker marks
        // processed so it leaves the inbox.
        let store = Arc::new(FakeStore::new());
        let d = delivery(
            "pull_request",
            json!({
                "action": "labeled",
                "repository": {
                    "id": 1, "name": "r",
                    "owner": { "id": 1, "login": "o" }
                },
                "pull_request": {
                    "node_id": "PR_l", "created_at": "2024-01-01T00:00:00Z",
                    "user": { "id": 1, "login": "alice" }
                }
            }),
        );
        store.enqueue_webhook_for_test(d);

        let stats = Worker::new(store.clone() as Arc<dyn Store>)
            .drain_once()
            .await
            .unwrap();
        assert_eq!(stats.processed, 1);
        assert_eq!(stats.failed, 0);
    }

    #[tokio::test]
    async fn run_exits_when_shutdown_channel_signals() {
        // Enqueue something so the first iteration does real work,
        // then flip shutdown after a short delay. The run loop
        // must observe the signal and return.
        let store = Arc::new(FakeStore::new());
        for _ in 0..3 {
            store.enqueue_webhook_for_test(pr_opened());
        }
        let (tx, rx) = watch::channel(false);
        let worker = Worker::new(store.clone() as Arc<dyn Store>)
            .with_idle_poll(Duration::from_millis(10));
        let handle = tokio::spawn(worker.run(rx));

        // Give the loop a moment to drain the queued rows, then
        // request shutdown. The test passes if the spawned task
        // returns Ok(()) inside the timeout — i.e. the loop did
        // observe the signal.
        tokio::time::sleep(Duration::from_millis(80)).await;
        tx.send(true).unwrap();
        let res =
            tokio::time::timeout(Duration::from_secs(2), handle).await.expect("worker exited");
        res.unwrap().unwrap();

        // The events should have been recorded.
        assert!(store.events_count() >= 3);
    }

    #[tokio::test]
    async fn run_exits_when_shutdown_sender_dropped() {
        // Dropping the sender side counts as "shutdown" too —
        // watch::Receiver::changed returns Err on a closed channel
        // and the loop must treat that the same as a `true` value.
        let store = Arc::new(FakeStore::new());
        let (tx, rx) = watch::channel(false);
        let worker = Worker::new(store.clone() as Arc<dyn Store>)
            .with_idle_poll(Duration::from_millis(10));
        let handle = tokio::spawn(worker.run(rx));
        // Nothing in the inbox → worker is in the idle-poll
        // select. Dropping tx closes the channel.
        tokio::time::sleep(Duration::from_millis(30)).await;
        drop(tx);
        let res =
            tokio::time::timeout(Duration::from_secs(2), handle).await.expect("worker exited");
        res.unwrap().unwrap();
    }

    #[tokio::test]
    async fn end_to_end_one_pr_opens_event_with_author_actor() {
        let store = Arc::new(FakeStore::new());
        store.enqueue_webhook_for_test(pr_opened());
        Worker::new(store.clone() as Arc<dyn Store>)
            .drain_once()
            .await
            .unwrap();
        let ev = store.only_event();
        assert_eq!(ev.kind, EventKind::PullRequestOpened);
        assert!(store
            .roles_for_login(ev.id, "alice")
            .contains(&ActorRole::Author));
    }
}
