## Done

- Migration `0015_issue_dates.sql`: `dp_issue_dates` (PK `issue_id`, both timestamps nullable, `CHECK (start_at IS NULL OR due_at IS NULL OR start_at <= due_at)`, plus `mirror_node_id` / `mirror_synced_at` / `mirror_error` / `updated_at` provenance), optional `dp_repo_project_link` (PK `repo_id`, `project_node_id`, `start_field_node_id`, `due_field_node_id`), and a stub `dp_projectv2_mirror_tasks` outbox (`kind` CHECK in `('mirror_dates','pull_back')`, partial index over unprocessed rows).
- Domain: `dp_domain::issue_dates::{IssueDates, RepoProjectLink, ProjectV2MirrorTask, ProjectV2MirrorTaskKind}` plus `Store` methods `get_issue_dates`, `upsert_issue_dates`, `record_issue_dates_mirror_result`, `get_repo_project_link`, `enqueue_projectv2_mirror_task`, `claim_projectv2_mirror_tasks` (all with default-impl no-ops so existing fakes stay green) and the new `IssueDatesMirrorOutcome<'a>` outcome enum.
- `PgStore` implements every new trait method; the upsert maps the schema CHECK violation back to `StoreError::Invalid` so the handler can return a stable 400 code.
- `dp-rest::issue_dates`: `PATCH /issues/{id}/dates` handler gated on `(issues, write)`, with the `ProjectV2MirrorBackend` trait + `UnconfiguredProjectV2Mirror` default + `MirrorDatesOk` / `MirrorError`. Synchronous local upsert; if `dp_repo_project_link` exists, enqueues a `mirror_dates` outbox row and `tokio::spawn`s the GraphQL round-trip whose outcome is written back to `dp_issue_dates.mirror_error` / `mirror_synced_at`. Pre-validates `start_at <= due_at` to surface `400 invalid_date_window`; never blocks the local save on the mirror.
- `AppState` gains `projectv2_mirror: Arc<dyn ProjectV2MirrorBackend>` with `with_projectv2_mirror(...)`; `dp_server` mounts `issue_dates_router`.
- New audit verb `audit::ISSUE_DATES_UPDATE` (`issue.dates_update`).
- OpenAPI registers `patch_issue_dates`, `PatchIssueDatesRequest`, `IssueDatesDto`; snapshot regenerated via `UPDATE_OPENAPI_SNAPSHOT=1`.
- Tests cover local-only upsert (no link → no spawn, no outbox), invalid window rejection, mirror success (writes `mirror_node_id` back, clears `mirror_error`), mirror failure (records error, local save still 200). `cargo test --workspace --exclude dp-fetcher` green; `cargo build --workspace` clean.
- Committed as `468b9a8` with message starting `stage 7:`.

## Next

- Stage 8 of the triage-slice-2 job.

## What you need to know

- The handler route is `/issues/{id}/dates` (matching the existing `/issues/{id}` convention from stages 4/6) — the job text's `by-id` notation is just shorthand; nothing else in the codebase uses a `by-id` path style.
- `UnconfiguredProjectV2Mirror::mirror_dates` returns `MirrorError::Unconfigured`, which the spawned task treats as a silent no-op (no `mirror_error` row). This keeps unconfigured deployments from accreting bogus error rows while still letting the outbox row land (so a later worker rebuild can drain it). If you want unconfigured deployments to also skip the outbox enqueue, gate the `enqueue` call on `state.projectv2_mirror` being non-default — left as-is because the outbox is the durability story.
- Issue node id resolution is stubbed as `format!("issue:{repo_id}:{number}")`; a real GraphQL backend must resolve the actual `I_...` node id (likely via `repository{ issue(number) }`). `dp_issues` does not currently persist GitHub node ids — adding `dp_issues.github_node_id` would let the handler pass the real value directly.
- `dp_projectv2_mirror_tasks` has `gen_random_uuid()` as the PK default; this relies on the `pgcrypto` extension being available. Confirmed precedent: prior migrations rely on the same default in the dp schema.
- Pre-existing `dp-fetcher` failures from earlier stages are still present; this stage did not touch that crate.

## Open questions

- Whether the slice-3 mirror worker should consume `claim_projectv2_mirror_tasks` (durable outbox) or trust the spawned in-process attempt (current behavior). I left both seams in place; the outbox is the source of truth for retries while the spawn is the fast path.
- Whether `dp_issues.github_node_id` should land in a follow-up migration so the mirror backend can issue real `addProjectV2ItemById(contentId=…)` calls without an extra GraphQL lookup.
