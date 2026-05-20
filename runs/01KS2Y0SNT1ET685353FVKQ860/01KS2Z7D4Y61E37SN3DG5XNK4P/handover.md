## Done

- Mounted `issues_write_router` covering POST `/issues`, PATCH `/issues/{id}`, POST `/issues/{id}/comments`, gated `(issues, write)`, composing `acquire_issue_mutation_slot` → `IssueWriteBackend` → `commit_issue_mutation` / `rollback_issue_mutation`.
- Added `ApiError::StaleLocalVersion { issue_id, current_version }` (409 with stable wire body); CAS miss surfaces this without invoking the backend.
- Introduced `IssueWriteBackend` trait + `UnconfiguredIssueWriter` default + `IssueWriteError` mapping (Validation → 400 `github_validation_failed`, Upstream/Unconfigured → 400 `upstream_unavailable`).
- `AppState.issue_writer` + `with_issue_writer`; `Store::get_repo` / `Store::get_org` defaults so handlers resolve `(org_login, repo_name)`.
- Registered new paths + DTOs in `DevPulseApi`; regenerated openapi snapshot.
- Mounted the new router from `dp-server::build` next to `issues_read`.
- 6 in-module integration tests pass: PATCH happy path, CAS miss → 409, GitHub-failure rollback with §13.7 buffer drain, plus create-issue + create-comment happy paths.
- Committed as `87fd742`.

## Next

- Stage 6 of the triage-slice-2 job.

## What you need to know

- The production octocrab implementation of `IssueWriteBackend` is **not** wired yet; the deployment default is `UnconfiguredIssueWriter`, which returns `400 upstream_unavailable`. Bin layer must call `AppState::with_issue_writer(...)` before traffic flows. This is a deliberate fail-loud default per the operator policy ("durable fix").
- For PATCH, the audit verb is selected by `IssuePatch.state` — `"closed"` ⇒ `issue.close`, `"open"` ⇒ `issue.reopen`, anything else (including labels/title only) ⇒ `issue.update`.
- POST `/issues` does **not** go through the CAS (no local row yet); it calls the backend, writes an `issue.create` audit row with target `"{repo_id}#{number}"`, and lets the next fetcher / webhook tick materialise the `dp_issues` row.
- Three pre-existing test failures in `dp-fetcher` (`pr_list_synthesises_deliveries_that_flow_through_apply_path`, `not_modified_keeps_since_and_etag_and_writes_no_events`, `phase2_smoke::missed_webhook_detected_by_reconciler`) are present on the parent branch — verified by `git stash` — and out of scope for this stage.

## Open questions

- (none)
