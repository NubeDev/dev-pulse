//! §13.7 reconciler guard — defers in-flight optimistic writes.
//!
//! SCOPE-PROJECTS §13.7: the fetcher / webhook reconciler **must
//! not** overwrite a `dp_issues` row where `pending_remote = TRUE`
//! and `pending_remote_at` is younger than
//! `issues.pending_remote_timeout_secs` (default 60s). Webhook
//! payloads for such rows are buffered and replayed once the flag
//! clears (§8.2 step 7 / step 8 / §8.5 sweeper).
//!
//! ## Where this fits
//!
//! The webhook drain loop calls [`apply_or_defer_delivery`] *in
//! place of* the raw [`crate::worker::apply_delivery`]. For issue-
//! scoped deliveries (`issues`, `issue_comment`) the guard:
//!
//! 1. Extracts `(repository.id, issue.id)` from the payload.
//! 2. Resolves them to a `dp_issues.id` via
//!    [`Store::find_issue_id_by_repo_and_github_id`]. Cache miss
//!    (no local row yet) → not deferrable, apply normally.
//! 3. Asks the store whether the row is currently in
//!    `pending_remote = TRUE` and the stamp is younger than
//!    `timeout` ([`Store::is_issue_pending_remote_fresh`]).
//! 4. Stale or no-pending → apply normally. Fresh-pending →
//!    [`Store::buffer_pending_remote_webhook`] and return
//!    [`GuardOutcome::Deferred`] without touching anything else.
//!
//! Non-issue events (push / workflow_run / …) bypass the guard
//! entirely — they cannot collide with an optimistic local write.
//!
//! ## Replay
//!
//! The other half of §13.7 lives in `dp-rest`'s `commit_issue_mutation`,
//! `rollback_issue_mutation`, and `sweep_pending_remote_timeouts`.
//! After the flag clears, those callers invoke
//! [`replay_buffered_for_issue`] to drain the buffer and dispatch
//! each delivery through the normal handler path.

use serde_json::Value;
use uuid::Uuid;

use dp_domain::{Store, StoreError, WebhookDelivery};

use crate::worker::{apply_delivery, HandlerError, HandlerOutcome};

/// Outcome of [`apply_or_defer_delivery`]. Variants mirror the two
/// shapes the drain loop needs to count separately.
#[derive(Debug)]
pub enum GuardOutcome {
    /// The delivery was dispatched through
    /// [`crate::worker::apply_delivery`]. Carries the handler's
    /// own report so the worker's [`crate::worker::DrainStats`]
    /// can keep counting events / actors.
    Applied(HandlerOutcome),
    /// The delivery concerned an issue in fresh `pending_remote`
    /// state and was stashed in
    /// `dp_pending_remote_webhook_buffer`. The drain loop should
    /// mark the inbox row processed (the buffered copy is now the
    /// authoritative replay target).
    Deferred {
        /// The `dp_issues.id` the delivery was deflected for.
        /// Surfaced so the drain loop can trace which row's
        /// pending flag is responsible.
        issue_id: Uuid,
    },
}

/// What kinds of `X-GitHub-Event` values the guard considers.
/// Outside this set the guard short-circuits to
/// `apply_delivery` — non-issue events cannot collide with an
/// optimistic issue write.
fn event_is_issue_scoped(event: &str) -> bool {
    matches!(event, "issues" | "issue_comment")
}

/// Pull `(repository.id, issue.id)` out of an `issues` or
/// `issue_comment` payload. Returns `None` on any missing field —
/// the guard's contract is "if you can't tell, apply normally" so
/// a malformed payload still flows to the handler, which will
/// surface its own `MissingField` error.
fn extract_issue_target(p: &Value) -> Option<(i64, i64)> {
    let repo_gid = p.get("repository")?.get("id")?.as_i64()?;
    let issue_gid = p.get("issue")?.get("id")?.as_i64()?;
    Some((repo_gid, issue_gid))
}

/// Guard-front [`apply_delivery`]. See module docs.
///
/// `timeout` is plumbed in from `dp-config`'s
/// `issues.pending_remote_timeout_secs` — the same value the §8.5
/// sweeper uses. Threading it through the call site (rather than
/// reading it inside this function) keeps the guard pure for
/// tests and lets a future operator override the production
/// default without recompiling.
pub async fn apply_or_defer_delivery(
    store: &dyn Store,
    delivery: &WebhookDelivery,
    timeout: chrono::Duration,
) -> Result<GuardOutcome, HandlerError> {
    if !event_is_issue_scoped(&delivery.event) {
        // Non-issue events flow straight through — no chance of a
        // §13.7 collision.
        let out = apply_delivery(store, delivery).await?;
        return Ok(GuardOutcome::Applied(out));
    }
    let Some((repo_gid, issue_gid)) = extract_issue_target(&delivery.payload) else {
        // Malformed payload — let the handler raise its own
        // MissingField so the inbox keeps a coherent error.
        let out = apply_delivery(store, delivery).await?;
        return Ok(GuardOutcome::Applied(out));
    };
    // Resolve to the local `dp_issues.id`. We need the repo_id
    // first; that's a 1-row probe on `(github_id)`.
    let repo_id = match find_repo_id_by_github_id(store, repo_gid).await? {
        Some(r) => r,
        None => {
            // No local repo row — by definition no local issue
            // row, so nothing pending. Apply normally; the handler
            // will upsert the repo on its way through.
            let out = apply_delivery(store, delivery).await?;
            return Ok(GuardOutcome::Applied(out));
        }
    };
    let issue_id = match store
        .find_issue_id_by_repo_and_github_id(repo_id, issue_gid)
        .await
        .map_err(HandlerError::Store)?
    {
        Some(id) => id,
        None => {
            // No local issue row yet (first-sighting of this
            // issue). Cannot be in pending_remote, apply normally.
            let out = apply_delivery(store, delivery).await?;
            return Ok(GuardOutcome::Applied(out));
        }
    };
    let fresh = store
        .is_issue_pending_remote_fresh(issue_id, timeout)
        .await
        .map_err(HandlerError::Store)?;
    if !fresh {
        let out = apply_delivery(store, delivery).await?;
        return Ok(GuardOutcome::Applied(out));
    }
    // §13.7: stash the delivery and return without touching the
    // dp_issues row.
    match store.buffer_pending_remote_webhook(issue_id, delivery).await {
        Ok(()) => Ok(GuardOutcome::Deferred { issue_id }),
        Err(StoreError::Conflict(_)) => {
            // Same `delivery_id` already buffered (re-delivery
            // from GitHub). Idempotent: still "Deferred", nothing
            // more to do.
            Ok(GuardOutcome::Deferred { issue_id })
        }
        Err(e) => Err(HandlerError::Store(e)),
    }
}

/// Look up `dp_repos.id` from GitHub's repo id via the store.
/// On "no local repo row yet" the guard short-circuits to
/// "apply normally" because no local issue can exist either.
async fn find_repo_id_by_github_id(
    store: &dyn Store,
    github_repo_id: i64,
) -> Result<Option<Uuid>, HandlerError> {
    store
        .find_repo_id_by_github_id(github_repo_id)
        .await
        .map_err(HandlerError::Store)
}

/// `apply_or_defer_delivery` variant for callers that already
/// know the local `repo_id` (the reconciler's per-target loop is
/// the obvious one). Skips the repo-resolution step. The contract
/// is otherwise identical.
pub async fn apply_or_defer_delivery_with_repo(
    store: &dyn Store,
    delivery: &WebhookDelivery,
    repo_id: Uuid,
    timeout: chrono::Duration,
) -> Result<GuardOutcome, HandlerError> {
    if !event_is_issue_scoped(&delivery.event) {
        let out = apply_delivery(store, delivery).await?;
        return Ok(GuardOutcome::Applied(out));
    }
    let Some((_, issue_gid)) = extract_issue_target(&delivery.payload) else {
        let out = apply_delivery(store, delivery).await?;
        return Ok(GuardOutcome::Applied(out));
    };
    let Some(issue_id) = store
        .find_issue_id_by_repo_and_github_id(repo_id, issue_gid)
        .await
        .map_err(HandlerError::Store)?
    else {
        let out = apply_delivery(store, delivery).await?;
        return Ok(GuardOutcome::Applied(out));
    };
    if !store
        .is_issue_pending_remote_fresh(issue_id, timeout)
        .await
        .map_err(HandlerError::Store)?
    {
        let out = apply_delivery(store, delivery).await?;
        return Ok(GuardOutcome::Applied(out));
    }
    match store.buffer_pending_remote_webhook(issue_id, delivery).await {
        Ok(()) => Ok(GuardOutcome::Deferred { issue_id }),
        Err(StoreError::Conflict(_)) => Ok(GuardOutcome::Deferred { issue_id }),
        Err(e) => Err(HandlerError::Store(e)),
    }
}

/// Drain the §13.7 buffer for one issue and dispatch each
/// buffered delivery through [`apply_delivery`]. The §8.2 step 7
/// / step 8 / §8.5 sweeper hooks call this after clearing the
/// `pending_remote` flag.
///
/// Errors per-delivery are logged via `tracing` and do **not**
/// abort the replay batch — the buffer was already deleted by the
/// `take_…` call, and the next reconciler tick will re-observe
/// authoritative state. Returns the count of deliveries that
/// applied cleanly (handler returned `Ok` or `Ignored`).
pub async fn replay_buffered_for_issue(
    store: &dyn Store,
    issue_id: Uuid,
) -> Result<usize, StoreError> {
    let buffered = store.take_buffered_webhooks_for_issue(issue_id).await?;
    let mut applied = 0usize;
    for d in &buffered {
        match apply_delivery(store, d).await {
            Ok(_) => applied += 1,
            Err(HandlerError::Ignored { .. }) => applied += 1,
            Err(e) => {
                tracing::warn!(
                    target: "dp_fetcher::reconciler::guard",
                    delivery_id = %d.delivery_id,
                    error = %e,
                    "replay of buffered webhook failed; reconciler will re-observe"
                );
            }
        }
    }
    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_target_round_trips() {
        let p = json!({
            "repository": { "id": 4242 },
            "issue":      { "id": 9001 }
        });
        assert_eq!(extract_issue_target(&p), Some((4242, 9001)));
    }

    #[test]
    fn extract_target_returns_none_on_missing_fields() {
        assert_eq!(extract_issue_target(&json!({})), None);
        assert_eq!(
            extract_issue_target(&json!({ "repository": { "id": 1 } })),
            None
        );
    }

    #[test]
    fn event_scope_filter() {
        assert!(event_is_issue_scoped("issues"));
        assert!(event_is_issue_scoped("issue_comment"));
        assert!(!event_is_issue_scoped("pull_request"));
        assert!(!event_is_issue_scoped("push"));
    }
}
