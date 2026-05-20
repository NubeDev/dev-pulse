//! Issue write-path scaffolding — the SCOPE-PROJECTS §8.2 / §8.5 /
//! §13.7 building blocks the per-verb dp-rest issue handlers (and
//! the §8.5 timeout sweeper) compose against.
//!
//! Stage 9 of the projects-issues job lands the **CAS-on-version
//! primitive** + the **timeout sweeper** here. Wiring the actual
//! per-verb `POST /issues` / `PATCH /issues/{n}` / `POST
//! /issues/{n}/comments` handlers — and the octocrab call inside
//! them — happens in a later stage. What *is* in scope:
//!
//! 1. [`AcquiredSlot`] — the audit-trail-bearing receipt returned
//!    by [`acquire_issue_mutation_slot`]. Holds the CAS-bumped
//!    `version` and the persisted [`IssueMutation`] row so the
//!    handler can ship them to the UI on success / failure.
//! 2. [`acquire_issue_mutation_slot`] — does §8.2 step 5 (atomic
//!    CAS on `dp_issues.version` + raise `pending_remote`) and
//!    immediately writes the `pending` audit row. Returns
//!    [`AcquireOutcome::Acquired`] on success and
//!    [`AcquireOutcome::Stale`] (with the current local version)
//!    when the CAS missed; the caller translates the latter to
//!    `409 stale_local_version` per §8.3.
//! 3. [`commit_issue_mutation`] — §8.2 step 7: clear
//!    `pending_remote`, mark the audit row `committed`, write the
//!    `dp_audit_log` verb row.
//! 4. [`rollback_issue_mutation`] — §8.2 step 8: bump `version`
//!    again to invalidate any cached `expected_version`, clear
//!    `pending_remote`, mark the audit row `failed` with the
//!    verbatim error, write the verb row. The pre-mutation field
//!    re-application lives in the caller (it knows the diff); the
//!    primitive here owns only the CAS-shaped columns.
//! 5. [`sweep_pending_remote_timeouts`] — the §8.5 sweeper. Walks
//!    `dp_issues.pending_remote = true` rows older than the
//!    configured timeout, rolls each one back (`release` with
//!    `bump_version_again = true`), updates the audit row (if
//!    present) to `pending_remote_timeout`, and writes the
//!    [`audit::ISSUE_PENDING_REMOTE_TIMEOUT`] verb row. Designed
//!    to be invoked from the same scheduler tick that drives the
//!    reconciler — single-shot, idempotent.
//!
//! None of these call GitHub. The octocrab round-trip is between
//! [`acquire_issue_mutation_slot`] and [`commit_issue_mutation`] /
//! [`rollback_issue_mutation`], in the per-verb handler — see
//! SCOPE-PROJECTS §8.2 step 6.

use chrono::{Duration, Utc};
use dp_domain::issue_mutation::{IssueMutation, IssueMutationOp, IssueMutationResult};
use dp_domain::store::{Store, StoreError};
use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::audit;

/// Outcome of [`acquire_issue_mutation_slot`]. Two terminal shapes:
/// the slot was acquired (CAS hit + audit row written) or the
/// caller's `expected_version` was stale.
#[derive(Debug)]
pub enum AcquireOutcome {
    /// CAS landed; the GitHub round-trip may proceed.
    Acquired(AcquiredSlot),
    /// CAS missed. The body carries the *current* local
    /// `dp_issues.version` so the UI can rehydrate. Per
    /// SCOPE-PROJECTS §8.3 the dp-rest handler maps this to
    /// `409 stale_local_version`.
    Stale {
        /// Current `dp_issues.version` as observed after the CAS
        /// miss — what the UI should reload.
        current_version: i64,
    },
}

/// Receipt for a successful §8.2 step 5 CAS. Carries the bumped
/// version and the in-flight `IssueMutation` audit row.
#[derive(Debug, Clone)]
pub struct AcquiredSlot {
    /// `dp_issues.version` after the CAS (= `expected_version + 1`).
    pub new_version: i64,
    /// The `dp_issue_mutations` row, in [`IssueMutationResult::Pending`]
    /// state.
    pub mutation: IssueMutation,
}

/// §8.2 steps 5–6 entrypoint: CAS the issue row, raise
/// `pending_remote`, and write the pending audit row.
///
/// On a CAS miss returns [`AcquireOutcome::Stale`] without writing
/// any audit row (a stale write that never started has no audit
/// trail by design — there is no GitHub I/O to record).
pub async fn acquire_issue_mutation_slot(
    store: &dyn Store,
    actor_user_id: Uuid,
    issue_id: Uuid,
    repo_id: Uuid,
    expected_version: i64,
    op: IssueMutationOp,
    diff: JsonValue,
) -> Result<AcquireOutcome, StoreError> {
    let new_version = match store
        .try_acquire_issue_pending_remote(issue_id, expected_version, actor_user_id)
        .await?
    {
        Some(v) => v,
        None => {
            // CAS missed — either the version is stale or a
            // concurrent writer already holds the slot. Either
            // way, surface the current version so the UI reloads.
            let current = store.get_issue_version(issue_id).await?;
            return Ok(AcquireOutcome::Stale {
                current_version: current,
            });
        }
    };
    let mutation = IssueMutation {
        id: Uuid::new_v4(),
        actor_user_id,
        issue_id,
        repo_id,
        op,
        version_before: expected_version,
        version_after: new_version,
        diff,
        result: IssueMutationResult::Pending,
        github_delivery_id: None,
        error: None,
        created_at: Utc::now(),
        finished_at: None,
    };
    let stored = store.record_issue_mutation(&mutation).await?;
    Ok(AcquireOutcome::Acquired(AcquiredSlot {
        new_version,
        mutation: stored,
    }))
}

/// §8.2 step 7: GitHub call returned success. Clear the pending
/// flag (no second `version` bump), transition the audit row to
/// `committed`, write the per-verb `dp_audit_log` row.
pub async fn commit_issue_mutation(
    store: &dyn Store,
    slot: &AcquiredSlot,
    github_delivery_id: Option<&str>,
) -> Result<(), StoreError> {
    store
        .release_issue_pending_remote(slot.mutation.issue_id, false)
        .await?;
    store
        .update_issue_mutation_result(
            slot.mutation.id,
            IssueMutationResult::Committed,
            github_delivery_id,
            None,
        )
        .await?;
    audit::record(
        store,
        slot.mutation.actor_user_id,
        audit::issue_audit_verb(slot.mutation.op),
        slot.mutation.issue_id.to_string(),
    )
    .await?;
    Ok(())
}

/// §8.2 step 8: GitHub call failed. Bump `version` *again* so any
/// concurrent reader sees the rollback as a change, clear the
/// pending flag, transition the audit row to `failed`, write the
/// per-verb `dp_audit_log` row.
///
/// Re-applying the pre-mutation field values to `dp_issues` is the
/// caller's responsibility — this primitive owns only the CAS /
/// audit columns. The caller should re-apply the fields **after**
/// this returns so its update lands on the post-bump row.
pub async fn rollback_issue_mutation(
    store: &dyn Store,
    slot: &AcquiredSlot,
    error: &str,
) -> Result<i64, StoreError> {
    let new_version = store
        .release_issue_pending_remote(slot.mutation.issue_id, true)
        .await?;
    store
        .update_issue_mutation_result(
            slot.mutation.id,
            IssueMutationResult::Failed,
            None,
            Some(error),
        )
        .await?;
    audit::record(
        store,
        slot.mutation.actor_user_id,
        audit::issue_audit_verb(slot.mutation.op),
        slot.mutation.issue_id.to_string(),
    )
    .await?;
    Ok(new_version)
}

// ---------------------------------------------------------------------------
// §8.5 timeout sweeper
// ---------------------------------------------------------------------------

/// Per-tick report from [`sweep_pending_remote_timeouts`]. Read by
/// callers (the scheduler / a future `/admin/sweep` endpoint) to
/// surface metrics + tests.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SweepReport {
    /// `dp_issues` rows that were rolled back (`pending_remote`
    /// flag cleared + `version` bumped again).
    pub issues_rolled_back: usize,
    /// `dp_issue_mutations` rows that were transitioned from
    /// `pending` to `pending_remote_timeout`. May be less than
    /// `issues_rolled_back` if the synchronous handler crashed
    /// *before* writing the audit row — those issues still get
    /// the `dp_audit_log` row, just without a matching audit
    /// table entry to update.
    pub mutations_marked_timed_out: usize,
}

/// §8.5 sweeper. Walks `dp_issues` rows where `pending_remote =
/// true` and `pending_remote_at < now() - timeout`, rolls each one
/// back, and writes the audit trail.
///
/// **Ordering** — rollback first, then audit. If the audit write
/// fails the row is already rolled back and the next tick will
/// observe a clean row (no double-rollback). The audit row is
/// idempotent w.r.t. the `dp_issue_mutations.result = 'pending'`
/// guard.
///
/// **Idempotence** — safe to invoke from multiple ticks racing each
/// other; the `release_issue_pending_remote` UPDATE is a no-op on
/// a row whose flag has already cleared, and the
/// `update_issue_mutation_result` WHERE clause refuses to overwrite
/// a non-`pending` row.
pub async fn sweep_pending_remote_timeouts(
    store: &dyn Store,
    timeout: Duration,
) -> Result<SweepReport, StoreError> {
    let cutoff = Utc::now() - timeout;
    let pending = store
        .list_issues_with_pending_remote_older_than(cutoff)
        .await?;
    let stuck_mutations = store
        .list_pending_issue_mutations_older_than(cutoff)
        .await?;
    let mut report = SweepReport::default();
    for row in pending {
        // 1. Roll the issue row back: clear pending_remote, bump
        //    version (§8.2 step 8 path applied retroactively).
        store
            .release_issue_pending_remote(row.issue_id, true)
            .await?;
        report.issues_rolled_back += 1;
        // 2. Match against the audit table by issue_id — there's
        //    at most one pending row per issue at any given time
        //    because acquire_issue_mutation_slot is gated on
        //    `pending_remote = false`.
        if let Some(m) = stuck_mutations.iter().find(|m| m.issue_id == row.issue_id) {
            // Some() — the audit row exists; transition it.
            // None() — the synchronous handler crashed before
            // recording it; we still write the dp_audit_log row
            // below so the §11 transparency export still answers.
            store
                .update_issue_mutation_result(
                    m.id,
                    IssueMutationResult::PendingRemoteTimeout,
                    None,
                    Some("pending_remote_timeout swept by reconciler"),
                )
                .await?;
            report.mutations_marked_timed_out += 1;
        }
        // 3. §8.5 audit row. Target carries the issue id so the
        //    §11 "mutations against this issue" query picks it up
        //    even when no `dp_issue_mutations` row exists.
        audit::record(
            store,
            row.actor_user_id,
            audit::ISSUE_PENDING_REMOTE_TIMEOUT,
            row.issue_id.to_string(),
        )
        .await?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::DateTime;
    use dp_domain::audit::AuditEntry;
    use dp_domain::event::{ActivityEvent, ActorRole, EventActor};
    use dp_domain::fetch::{FetchCursor, FetchRun, FetchRunKind, ResourceKind};
    use dp_domain::membership::Membership;
    use dp_domain::org::Org;
    use dp_domain::repo::Repo;
    use dp_domain::store::{EventActorRow, PendingRemoteIssue};
    use dp_domain::team::Team;
    use dp_domain::user::User;
    use dp_domain::webhook::WebhookDelivery;
    use dp_domain::window::Window;
    use std::sync::Mutex;

    /// Tiny in-memory fake satisfying the §8.2 / §8.5 trait
    /// surface this module touches. Other Store methods stay on
    /// the trait's default impls (errors / empty), which is fine
    /// — none of them are reached by the code under test.
    #[derive(Default)]
    struct FakeStore {
        inner: Mutex<FakeInner>,
    }
    #[derive(Default)]
    struct FakeInner {
        // issue_id -> (version, pending, pending_at, actor)
        issues: std::collections::HashMap<
            Uuid,
            (i64, bool, Option<DateTime<Utc>>, Option<Uuid>, Uuid),
        >,
        mutations: Vec<IssueMutation>,
        audit: Vec<AuditEntry>,
    }

    impl FakeStore {
        fn seed_issue(&self, id: Uuid, repo_id: Uuid, version: i64) {
            self.inner
                .lock()
                .unwrap()
                .issues
                .insert(id, (version, false, None, None, repo_id));
        }
    }

    #[async_trait]
    impl Store for FakeStore {
        async fn try_acquire_issue_pending_remote(
            &self,
            issue_id: Uuid,
            expected_version: i64,
            actor_user_id: Uuid,
        ) -> Result<Option<i64>, StoreError> {
            let mut g = self.inner.lock().unwrap();
            let row = g.issues.get_mut(&issue_id).ok_or_else(|| {
                StoreError::NotFound {
                    entity: "issue",
                    id: issue_id.to_string(),
                }
            })?;
            if row.0 != expected_version || row.1 {
                return Ok(None);
            }
            row.0 += 1;
            row.1 = true;
            row.2 = Some(Utc::now());
            row.3 = Some(actor_user_id);
            Ok(Some(row.0))
        }
        async fn release_issue_pending_remote(
            &self,
            issue_id: Uuid,
            bump: bool,
        ) -> Result<i64, StoreError> {
            let mut g = self.inner.lock().unwrap();
            let row = g.issues.get_mut(&issue_id).ok_or_else(|| {
                StoreError::NotFound {
                    entity: "issue",
                    id: issue_id.to_string(),
                }
            })?;
            row.1 = false;
            row.2 = None;
            row.3 = None;
            if bump {
                row.0 += 1;
            }
            Ok(row.0)
        }
        async fn get_issue_version(&self, issue_id: Uuid) -> Result<i64, StoreError> {
            self.inner
                .lock()
                .unwrap()
                .issues
                .get(&issue_id)
                .map(|r| r.0)
                .ok_or_else(|| StoreError::NotFound {
                    entity: "issue",
                    id: issue_id.to_string(),
                })
        }
        async fn list_issues_with_pending_remote_older_than(
            &self,
            cutoff: DateTime<Utc>,
        ) -> Result<Vec<PendingRemoteIssue>, StoreError> {
            let g = self.inner.lock().unwrap();
            Ok(g.issues
                .iter()
                .filter(|(_, r)| r.1 && r.2.map(|t| t < cutoff).unwrap_or(false))
                .map(|(id, r)| PendingRemoteIssue {
                    issue_id: *id,
                    repo_id: r.4,
                    version: r.0,
                    actor_user_id: r.3.unwrap(),
                    pending_remote_at: r.2.unwrap(),
                })
                .collect())
        }
        async fn record_issue_mutation(
            &self,
            m: &IssueMutation,
        ) -> Result<IssueMutation, StoreError> {
            self.inner.lock().unwrap().mutations.push(m.clone());
            Ok(m.clone())
        }
        async fn update_issue_mutation_result(
            &self,
            id: Uuid,
            result: IssueMutationResult,
            delivery: Option<&str>,
            err: Option<&str>,
        ) -> Result<(), StoreError> {
            let mut g = self.inner.lock().unwrap();
            let m = g
                .mutations
                .iter_mut()
                .find(|m| m.id == id && matches!(m.result, IssueMutationResult::Pending))
                .ok_or_else(|| StoreError::NotFound {
                    entity: "dp_issue_mutations(pending)",
                    id: id.to_string(),
                })?;
            m.result = result;
            m.github_delivery_id = delivery.map(str::to_owned).or(m.github_delivery_id.clone());
            m.error = err.map(str::to_owned).or(m.error.clone());
            m.finished_at = Some(Utc::now());
            Ok(())
        }
        async fn list_pending_issue_mutations_older_than(
            &self,
            cutoff: DateTime<Utc>,
        ) -> Result<Vec<IssueMutation>, StoreError> {
            Ok(self
                .inner
                .lock()
                .unwrap()
                .mutations
                .iter()
                .filter(|m| {
                    matches!(m.result, IssueMutationResult::Pending) && m.created_at < cutoff
                })
                .cloned()
                .collect())
        }
        async fn record_audit_log(&self, e: &AuditEntry) -> Result<(), StoreError> {
            self.inner.lock().unwrap().audit.push(e.clone());
            Ok(())
        }

        // --- everything else is a minimal stub --------------------
        async fn upsert_user(&self, u: &User) -> Result<User, StoreError> {
            Ok(u.clone())
        }
        async fn get_user(&self, _: Uuid) -> Result<User, StoreError> {
            unimplemented!()
        }
        async fn get_user_by_github_id(&self, _: i64) -> Result<User, StoreError> {
            unimplemented!()
        }
        async fn list_users(&self) -> Result<Vec<User>, StoreError> {
            Ok(vec![])
        }
        async fn pseudonymise_user(&self, _: Uuid) -> Result<(), StoreError> {
            Ok(())
        }
        async fn upsert_org(&self, o: &Org) -> Result<Org, StoreError> {
            Ok(o.clone())
        }
        async fn upsert_team(&self, t: &Team) -> Result<Team, StoreError> {
            Ok(t.clone())
        }
        async fn upsert_repo(&self, r: &Repo) -> Result<Repo, StoreError> {
            Ok(r.clone())
        }
        async fn upsert_membership(&self, m: &Membership) -> Result<Membership, StoreError> {
            Ok(m.clone())
        }
        async fn list_memberships_for_user(
            &self,
            _: Uuid,
        ) -> Result<Vec<Membership>, StoreError> {
            Ok(vec![])
        }
        async fn set_home_org(
            &self,
            _: Uuid,
            _: Uuid,
            _: Option<Uuid>,
        ) -> Result<(), StoreError> {
            Ok(())
        }
        async fn record_event(
            &self,
            e: &ActivityEvent,
        ) -> Result<ActivityEvent, StoreError> {
            Ok(e.clone())
        }
        async fn add_event_actors(&self, _: &[EventActor]) -> Result<(), StoreError> {
            Ok(())
        }
        async fn list_event_actor_rows_in_window(
            &self,
            _: &Window,
            _: &[Uuid],
            _: &[Uuid],
            _: &[Uuid],
            _: &[ActorRole],
        ) -> Result<Vec<EventActorRow>, StoreError> {
            Ok(vec![])
        }
        async fn get_cursor(
            &self,
            _: Uuid,
            _: Option<Uuid>,
            _: ResourceKind,
        ) -> Result<FetchCursor, StoreError> {
            Err(StoreError::NotFound {
                entity: "fetch_cursor",
                id: String::new(),
            })
        }
        async fn put_cursor(&self, _: &FetchCursor) -> Result<(), StoreError> {
            Ok(())
        }
        async fn start_fetch_run(&self, _: FetchRunKind) -> Result<Uuid, StoreError> {
            Ok(Uuid::new_v4())
        }
        async fn finish_fetch_run(
            &self,
            _: Uuid,
            _: i64,
            _: i64,
            _: bool,
        ) -> Result<(), StoreError> {
            Ok(())
        }
        async fn list_recent_fetch_runs(
            &self,
            _: i64,
        ) -> Result<Vec<FetchRun>, StoreError> {
            Ok(vec![])
        }
        async fn data_as_of(&self) -> Result<dp_domain::freshness::DataAsOf, StoreError> {
            Ok(dp_domain::freshness::DataAsOf::default())
        }
        async fn enqueue_webhook(&self, _: &WebhookDelivery) -> Result<(), StoreError> {
            Ok(())
        }
        async fn claim_webhooks(&self, _: i64) -> Result<Vec<WebhookDelivery>, StoreError> {
            Ok(vec![])
        }
        async fn mark_webhook_processed(&self, _: Uuid) -> Result<(), StoreError> {
            Ok(())
        }
        async fn mark_webhook_failed(&self, _: Uuid, _: &str) -> Result<(), StoreError> {
            Ok(())
        }
    }

    fn store() -> FakeStore {
        FakeStore::default()
    }

    #[tokio::test]
    async fn acquire_then_commit_runs_clean() {
        let s = store();
        let issue = Uuid::new_v4();
        let repo = Uuid::new_v4();
        let actor = Uuid::new_v4();
        s.seed_issue(issue, repo, 7);
        let out = acquire_issue_mutation_slot(
            &s,
            actor,
            issue,
            repo,
            7,
            IssueMutationOp::Update,
            serde_json::json!({"after": {"title": "x"}}),
        )
        .await
        .unwrap();
        let slot = match out {
            AcquireOutcome::Acquired(s) => s,
            AcquireOutcome::Stale { .. } => panic!("expected acquire"),
        };
        assert_eq!(slot.new_version, 8);
        commit_issue_mutation(&s, &slot, Some("delivery-42"))
            .await
            .unwrap();
        // Version stays at 8 on commit (no second bump).
        assert_eq!(s.get_issue_version(issue).await.unwrap(), 8);
        let g = s.inner.lock().unwrap();
        assert_eq!(g.mutations.len(), 1);
        assert!(matches!(
            g.mutations[0].result,
            IssueMutationResult::Committed
        ));
        assert_eq!(
            g.mutations[0].github_delivery_id.as_deref(),
            Some("delivery-42")
        );
        // dp_audit_log row written with the right verb.
        assert_eq!(g.audit.len(), 1);
        assert_eq!(g.audit[0].action, audit::ISSUE_UPDATE);
    }

    #[tokio::test]
    async fn acquire_with_stale_version_returns_current() {
        let s = store();
        let issue = Uuid::new_v4();
        s.seed_issue(issue, Uuid::new_v4(), 9);
        let out = acquire_issue_mutation_slot(
            &s,
            Uuid::new_v4(),
            issue,
            Uuid::new_v4(),
            7, // stale
            IssueMutationOp::Close,
            serde_json::json!({}),
        )
        .await
        .unwrap();
        match out {
            AcquireOutcome::Stale { current_version } => assert_eq!(current_version, 9),
            _ => panic!("expected stale"),
        }
    }

    #[tokio::test]
    async fn rollback_bumps_version_twice_total() {
        let s = store();
        let issue = Uuid::new_v4();
        s.seed_issue(issue, Uuid::new_v4(), 3);
        let slot = match acquire_issue_mutation_slot(
            &s,
            Uuid::new_v4(),
            issue,
            Uuid::new_v4(),
            3,
            IssueMutationOp::Close,
            serde_json::json!({}),
        )
        .await
        .unwrap()
        {
            AcquireOutcome::Acquired(s) => s,
            _ => panic!(),
        };
        // After acquire, version = 4.
        assert_eq!(slot.new_version, 4);
        let v = rollback_issue_mutation(&s, &slot, "github 5xx").await.unwrap();
        // After rollback, version = 5 (= initial + 2).
        assert_eq!(v, 5);
        assert_eq!(s.get_issue_version(issue).await.unwrap(), 5);
        let g = s.inner.lock().unwrap();
        assert!(matches!(g.mutations[0].result, IssueMutationResult::Failed));
        assert_eq!(g.mutations[0].error.as_deref(), Some("github 5xx"));
        assert_eq!(g.audit[0].action, audit::ISSUE_CLOSE);
    }

    #[tokio::test]
    async fn sweeper_rolls_back_stale_pending_and_emits_audit() {
        let s = store();
        let issue = Uuid::new_v4();
        s.seed_issue(issue, Uuid::new_v4(), 2);
        let slot = match acquire_issue_mutation_slot(
            &s,
            Uuid::new_v4(),
            issue,
            Uuid::new_v4(),
            2,
            IssueMutationOp::Comment,
            serde_json::json!({"after": {"body": "ping"}}),
        )
        .await
        .unwrap()
        {
            AcquireOutcome::Acquired(s) => s,
            _ => panic!(),
        };
        // Backdate so the sweeper sees the row.
        {
            let mut g = s.inner.lock().unwrap();
            let row = g.issues.get_mut(&issue).unwrap();
            row.2 = Some(Utc::now() - Duration::seconds(120));
            // Backdate the audit row too so the sweeper's pending
            // enumeration finds it.
            g.mutations[0].created_at = Utc::now() - Duration::seconds(120);
        }
        let report = sweep_pending_remote_timeouts(&s, Duration::seconds(60))
            .await
            .unwrap();
        assert_eq!(report.issues_rolled_back, 1);
        assert_eq!(report.mutations_marked_timed_out, 1);
        // Version: 2 (initial) -> 3 (acquire) -> 4 (sweeper rollback).
        assert_eq!(s.get_issue_version(issue).await.unwrap(), 4);
        let g = s.inner.lock().unwrap();
        assert!(matches!(
            g.mutations[0].result,
            IssueMutationResult::PendingRemoteTimeout
        ));
        assert_eq!(g.audit[0].action, audit::ISSUE_PENDING_REMOTE_TIMEOUT);
        let _ = slot; // suppress unused
    }

    #[tokio::test]
    async fn sweeper_handles_missing_audit_row() {
        // Simulates the "handler crashed before record_issue_mutation"
        // edge case: pending flag set on the issue row, no audit row.
        let s = store();
        let issue = Uuid::new_v4();
        let actor = Uuid::new_v4();
        s.seed_issue(issue, Uuid::new_v4(), 1);
        // Acquire the slot but yank the audit row to mimic the crash.
        s.try_acquire_issue_pending_remote(issue, 1, actor)
            .await
            .unwrap();
        {
            let mut g = s.inner.lock().unwrap();
            let row = g.issues.get_mut(&issue).unwrap();
            row.2 = Some(Utc::now() - Duration::seconds(120));
        }
        let report = sweep_pending_remote_timeouts(&s, Duration::seconds(60))
            .await
            .unwrap();
        assert_eq!(report.issues_rolled_back, 1);
        // No audit table row existed; the count reflects that.
        assert_eq!(report.mutations_marked_timed_out, 0);
        // The dp_audit_log row is still written.
        let g = s.inner.lock().unwrap();
        assert_eq!(g.audit.len(), 1);
        assert_eq!(g.audit[0].action, audit::ISSUE_PENDING_REMOTE_TIMEOUT);
    }
}
