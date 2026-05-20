## Done

- Migration `0013_triage_timeline_and_sync.sql`: the §6 guarded partial index `dp_activity_events_issue_idx` and the §5.4 covering index `dp_issues_updated_at_idx (updated_at DESC, id DESC)`.
- `GET /issues/{id}/timeline` mounted on `issues_read_router`; backed by `Store::list_events_for_issue` / `count_events_for_issue` in `dp-store-pg`, using the guarded predicate so the cast cannot raise.
- `GET /repos/{id}/sync-status` (auth `(repos, read)`) and `POST /repos/{id}/sync` → 202 (new auth pair `(repos, sync)`); the POST hands off to `Scheduler::try_trigger_now(Scope::Repo)`. Sync-status is synthesised from `MAX(dp_fetch_cursors.updated_at)` since `dp_repos` carries no `last_synced_at`/error column today; `last_error` is always `null` and `queued` is `false` (scheduler has no public per-repo introspection — comments in `repos.rs` and `RepoSyncStatus` flag this for a follow-up).
- `GET /reports/issues` mounted on `reports_router` under `(issues, read)`; `Store::issue_metrics` emits the §5.10 SQL — `CROSS JOIN LATERAL jsonb_array_elements_text` for `wip`, `jsonb_array_length(...) = 0` for `untriaged`, `EXTRACT(EPOCH FROM (closed_at - created_at))` for `lead_time` (median via `percentile_cont(0.5)`).
- `/me/queue` got a keyset `?after=<rfc3339>,<uuid>` cursor; `IssueListFilter.keyset_after` is plumbed into `list_inbox_issues`, which now orders by `(updated_at DESC, id DESC)` and filters with `(updated_at, id) < ($ts, $id)`. Covering index added in the same migration.
- `AppState` gained `scheduler: Option<Arc<Scheduler>>` with `with_scheduler(...)`; `dp_server::build` now wires it. Without a scheduler the POST returns `400 reconciler_unavailable`.
- OpenAPI: registered every previously-missing handler (`list_issues`, `me_queue`, `get_issue_by_id/_by_number`, `get_issue_timeline`, `list_repos`, `get_repo_sync_status`, `request_repo_sync`, `issues_report`, `mark_seen`, `set_inbox_state`) plus their DTOs; snapshot regenerated via `UPDATE_OPENAPI_SNAPSHOT=1`.
- All non-fetcher crates green; `cargo build` clean; `cargo test --workspace --exclude dp-fetcher` passes.
- Committed as `f20a9c6` with message starting `stage 6:`.

## Next

- Stage 7 of the triage-slice-2 job.

## What you need to know

- Pre-existing dp-fetcher failures (`pr_list_synthesises_deliveries_that_flow_through_apply_path`, `not_modified_keeps_since_and_etag_and_writes_no_events`, `phase2_smoke::missed_webhook_detected_by_reconciler`) remain — present on the parent branch per stage-5 handover and unaffected by this stage.
- `RepoSyncStatusDto.queued` is hard-coded `false` because `Scheduler` has no public per-repo in-flight introspection; if the frontend needs a live signal a `Scheduler::is_running(scope)` API would have to land first.
- The `/me/queue` 4-arm UNION described in §5.4 is **not** implemented in this stage — the existing single-SQL filter from slice 1 is unchanged in semantics; per-arm `LIMIT $cap` push-down therefore degrades to a single LIMIT on the outer query. The new index and the keyset cursor are in place so the eventual UNION rewrite is just a SQL swap.
- `Store::get_repo_sync_status` lives in the trait with a default `Ok(None)`; only `dp-store-pg` implements it. The §5.6 timeline / §5.10 metrics methods follow the same default-impl pattern so in-memory fakes used by other crates' tests still satisfy `dyn Store`.
- The new `(repos, sync)` auth pair is wildcard-allowed by the existing `org-gate-allow-in-org-everything` rule in `dev-pulse.toml`; no policy edit was needed.

## Open questions

- Whether `RepoSyncStatus` should grow `last_attempt_at` / `last_error` columns on `dp_repos` (or a sidecar table) so the badge can show real error text — left for a follow-up since slice 2's frontend can render the "synced Nm ago" pill from `last_synced_at` alone.
- Whether the §5.4 four-arm UNION (`assignees`/pinned repos/pinned-tag repos/inbox unread) should land in a follow-up stage or be backfilled here — current implementation keeps the slice-1 semantics and only adds the keyset / index plumbing.
