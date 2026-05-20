## Done

- Migration `crates/dp-store-pg/migrations/dp/0007_issues_optimistic_cas.sql` — adds `version`, `pending_remote`, `pending_remote_at`, `pending_remote_actor` columns on `dp_issues` (with the CHECK that the three pending_* fields move as a set + partial index on `pending_remote_at WHERE pending_remote = TRUE`), and the `dp_issue_mutations` audit table.
- Six new `Store` trait methods in `dp-domain` for the §8.2 CAS path + §8.5 sweeper: `try_acquire_issue_pending_remote`, `release_issue_pending_remote`, `get_issue_version`, `list_issues_with_pending_remote_older_than` (new `PendingRemoteIssue` projection), and tightened `record_issue_mutation` / `update_issue_mutation_result` / `list_pending_issue_mutations_older_than` from stubs to real signatures.
- `PgStore` impls for all of the above, including a `release` that returns the post-update version and an `update_issue_mutation_result` that is gated on `result = 'pending'` to make sweeper/handler interleaving safe.
- New `dp_rest::issues` module exposing `acquire_issue_mutation_slot` → `AcquireOutcome::{Acquired, Stale{current_version}}`, `commit_issue_mutation`, `rollback_issue_mutation`, and `sweep_pending_remote_timeouts` + `SweepReport`.
- Audit verb constants in `dp_rest::audit`: `ISSUE_CREATE/UPDATE/CLOSE/REOPEN/COMMENT/PENDING_REMOTE_TIMEOUT` + `issue_audit_verb(IssueMutationOp)` helper.
- Five new unit tests in `issues::tests` cover the four spec'd code paths (acquire+commit, stale CAS, acquire+rollback, sweeper happy path, sweeper with missing audit row). Workspace `cargo test` and `scripts/check-boundaries.sh` both green.
- Committed as `70f1c27`.

## Next

- Stage 10: wire actual per-verb dp-rest handlers (`POST /repos/{owner}/{repo}/issues`, `PATCH …/{n}`, `POST …/{n}/comments`) that thread `require_issues_write` → `acquire_issue_mutation_slot` → octocrab call → `commit_issue_mutation` / `rollback_issue_mutation`. The CAS / audit / sweeper primitives are already in place; this stage only needs the I/O layer.
- Stage 10 also needs to schedule `sweep_pending_remote_timeouts` from the dp-fetcher reconciler tick (re-using the existing scheduler) and surface `issues.pending_remote_timeout_secs` in `dp-config` (default `60s`).

## What you need to know

- The CAS UPDATE statement explicitly guards on `pending_remote = false` *and* `version = expected_version` — so a second in-flight write also takes the `Stale` branch. The sweeper is what frees a slot when a synchronous handler crashed.
- `release_issue_pending_remote` is idempotent shape-wise (it just runs the UPDATE again) but returns `Err(NotFound)` if the issue row was deleted in between; callers (sweeper, rollback) treat that as a hard error today.
- `update_issue_mutation_result` requires `result = 'pending'` in its WHERE; a no-op match returns `Err(NotFound("dp_issue_mutations(pending)", id))`. Designed so a sweeper / handler race surfaces loudly rather than silently double-finalising.
- The sweeper writes the `dp_audit_log` row *unconditionally* (even when no `dp_issue_mutations` row was found) so the §11 transparency export still answers in the "handler crashed before recording the audit row" edge case. `SweepReport.mutations_marked_timed_out` reflects this — it can be less than `issues_rolled_back`.
- Migration numbering: odd-slot lock from STAGE-1-COORDINATION.md §3 honoured (`0005_*` and `0007_*` are ours; leaderboard owns even slots).
- The actual GitHub I/O is *not* wired in this stage — that's stage 10. The handler primitives are designed so the per-verb handler does `acquire → octocrab call → commit/rollback`; rolling back the field values themselves (title, body, …) is the caller's responsibility, the primitive only owns the CAS-shaped columns.

## Open questions

- (none) — the deferred items belong to stage 10 and beyond, not stage 9.
