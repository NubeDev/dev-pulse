## Done

- Added `POST /me/inbox/bulk` (4 ops: mark_all_seen / snooze_all / done_all / inbox_all) — `crates/dp-rest/src/inbox.rs` with `BulkInboxOp`, `BulkInboxRequest`, `BulkInboxResponse`, wired into `inbox_router` and re-exported from `lib.rs`.
- New `Store::set_inbox_state_bulk` trait method (default error) + PG implementation upserting `dp_user_issue_state` rows in one statement.
- Extended pinned audit vocabulary in `crates/dp-rest/src/audit.rs`: `IDENTITY_ADD/REMOVE/VERIFY/MERGE`, `DATE_SET`, `REPO_SYNC_REQUESTED`, `BULK_INBOX_SEEN/SNOOZE/DONE/INBOX`.
- `request_repo_sync` now records `REPO_SYNC_REQUESTED` (best-effort; never blocks the 202).
- Registered `bulk_inbox` + DTOs in `DevPulseApi`; regenerated `openapi.snapshot.json`.
- Migration `0017_user_issue_state_touch_trigger.sql` ships the BEFORE UPDATE trigger that auto-stamps `updated_at = now()`.
- Committed as `stage 8: OpenAPI + audit + bulk inbox …`. `cargo build` clean; `cargo test -p dp-rest` green (80 + 1 passed).

## Next

- Stage 9 (per the project plan) — slice-2 frontend / identity-manager work; a fresh session will pick it up.

## What you need to know

- `BULK_INBOX_SEEN` audit verb was added even though the stage description lists `BULK_INBOX_*` generically — needed for the `mark_all_seen` op for parity.
- Bulk audit `target` is `count=<n>` (operator intent, not per-row); per-row writes remain un-audited per the existing inbox-module rationale.
- `DATE_SET` is an additional vocabulary entry — the existing dates handler still emits `ISSUE_DATES_UPDATE`; `DATE_SET` is reserved for future bulk / Projects-v2 pull-back paths.
- Pre-existing dp-fetcher reconciler test failures (`not_modified_keeps_since_and_etag…`, `pr_list_synthesises_deliveries…`, `phase2_smoke::missed_webhook_detected_by_reconciler`) were present before this stage — confirmed via `git stash` reproduction.

## Open questions

- Stage description references "§6 BEFORE UPDATE trigger" but `linear-projects-idea.md` §6 has no explicit trigger DDL; implemented as a defensive `updated_at = now()` backstop on `dp_user_issue_state`. Confirm this matches intent.
- `set_inbox_state_bulk` returns `rows_affected` (insert + update). For `mark_all_seen` we instead report `issue_ids.len()` because the existing `mark_issues_seen` doesn't surface a count — flag if a precise touched count is required there.
