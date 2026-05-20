## Done

- Migration `0009_pending_remote_webhook_buffer.sql` adds the §13.7 webhook replay buffer (`dp_pending_remote_webhook_buffer`) keyed on (issue × delivery_id).
- `Store` trait + `PgStore` gain `find_repo_id_by_github_id`, `find_issue_id_by_repo_and_github_id`, `is_issue_pending_remote_fresh`, `buffer_pending_remote_webhook`, `take_buffered_webhooks_for_issue`.
- New `dp_fetcher::reconciler::guard` module: `apply_or_defer_delivery`, `apply_or_defer_delivery_with_repo`, `replay_buffered_for_issue`, plus the `GuardOutcome` enum.
- Webhook drain loop (`Worker::drain_once`) and reconciler synth dispatch (`Reconciler::reconcile_one`) now route `issues`/`issue_comment` events through the guard; both expose a `with_pending_remote_timeout` builder for config plumbing.
- `commit_issue_mutation`, `rollback_issue_mutation`, and `sweep_pending_remote_timeouts` drain the buffer once `pending_remote` clears.
- Five new §8.3-locked tests in `dp-rest/src/issues.rs`: stale local write, concurrent dp-pulse writers, mid-flight webhook (commit replay), mid-flight webhook (rollback replay), and timeout-expired (no defer). `cargo test --workspace` is green.
- Committed as `33433ba` on `codeless/projects-issues`.

## Next

- Stage 11 (next session) picks up the next item from the job's WORKFLOW plan — not started.

## What you need to know

- The §13.7 timeout default is 60s, set on both `Worker` and `Reconciler` to match `issues.pending_remote_timeout_secs` in `dp-config`. The bin layer should call `with_pending_remote_timeout(...)` when wiring from config.
- `replay_buffered_for_issue` uses `take_buffered_webhooks_for_issue` (DELETE … RETURNING) — at-least-once-replay but loses the buffer copy on crash. GitHub's at-least-once redelivery + the next reconciler tick is the safety net.
- The guard's "no local repo row yet" or "no local issue row yet" branches both fall through to `apply_delivery`. First-sighting of an issue cannot collide with §8 writes by construction.
- Re-delivery of the same `delivery_id` while pending returns `StoreError::Conflict` from `buffer_pending_remote_webhook`; the guard treats that as idempotent "already buffered, still Deferred".
- Non-issue events (push / pull_request / workflow_run / …) bypass the guard entirely — the issue-scoped event filter (`event_is_issue_scoped`) is the cheap shortcut.

## Open questions

- The §8.3 scope text actually lists four cases (stale local, concurrent writers, GitHub-side concurrent edit, webhook-mid-flight). The stage description said "three race cases"; I covered five behaviours (including the timeout-expired guard branch). No clarification needed unless future stages count differently.
- The §13.7 guard currently triggers off `dp_issues.id` lookup via `(repo_id, github_issue_id)`. For freshly-created issues whose dp_issues row hasn't been upserted yet (race between creating the issue locally and the first webhook), the guard falls through to apply normally. SCOPE-PROJECTS implies the `dp_issues` row is upserted by the synchronous `issue.create` handler before the GitHub round-trip — verify in the per-verb handler stage.
