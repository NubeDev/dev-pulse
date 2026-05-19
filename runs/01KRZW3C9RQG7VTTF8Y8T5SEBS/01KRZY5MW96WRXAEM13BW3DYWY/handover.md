## Done

- Added `crates/dp-fetcher/src/reconciler/{mod,synth,targets,tests}.rs` — Reconciler, Scope (All/Org/Repo), Scheduler with coalescing guard, do_tick shared seam, conditional-GET cursor handling, list-response→webhook-payload synthesis
- Synthesized deliveries flow through `crate::worker::apply_delivery` (zero handler duplication); SCOPE §6 edge cases (co-author/squash/bot/unattributed) inherited automatically
- Per-(org, repo, resource_kind) cursors read from `fetch_cursors`: `since` becomes the query parameter, `etag` becomes `If-None-Match`; 304 only bumps `updated_at`, 200 advances `since` to max-observed timestamp and persists new etag
- Scheduler uses `Mutex<Option<JoinHandle<()>>>` + a oneshot for result observation — concurrent triggers coalesce to a no-op without the originator holding the lock across the await
- Writes one `fetch_runs` row of kind `Reconciler` per tick with items/errors/partial totals
- `dp-cli::fetch_now(scheduler, scope)` and `dp-rest::admin::admin_router` (`POST /admin/refresh?org_id=&repo_id=`) route through the same scheduler
- Extended `worker::test_store::FakeStore` with working cursor storage + `get_cursor_sync` accessor for reconciler tests
- Tests: wiremock-driven (PR list synthesises and applies, 304 preserves since/etag, Scope::Repo narrows, scheduler coalesces overlapping triggers) plus 2 dp-rest admin route tests; full workspace `cargo test` green; `scripts/check-boundaries.sh` OK
- Commit `9a58245` on branch `codeless/phase-2-ingestion`

## Next

- Stage 9 (per TODO.md) — fresh session per job spec; not started here

## What you need to know

- Reconciler only ticks `PullRequests`, `Issues`, `Commits` by default — Reviews / ReviewComments / WorkflowRuns / Deployments / Releases stay webhook-only because their list endpoints lack a `since=` parameter (documented in `Reconciler::new`). Override via `Reconciler::with_kinds(&[…])` in tests/bin
- Synthesis is intentionally lossy on fields the list endpoint doesn't return (e.g. `pull_request.merged_by` lives on the PR detail endpoint, not `/pulls`). Worst case is a missing actor row; webhook redelivery or future detail-endpoint backfill closes the gap
- `RepoTarget` carries both internal UUIDs and GitHub numeric IDs because handlers `upsert_*` key on `github_id`. The bin layer is responsible for materialising targets via `TargetProvider` (`StaticTargets` exists for tests / static config)
- `Scheduler::try_trigger_now` is the *single* operator entrypoint — fetch-now (dp-cli) and POST /admin/refresh (dp-rest) both go through it so the mutex is the only synchronisation point. The originating caller's result comes back via an internal oneshot; the JoinHandle in the slot is `JoinHandle<()>`
- `Scheduler::run` uses `MissedTickBehavior::Skip` paired with `tokio::time::interval(tick_interval)`, fires immediately on start (no 4h cold-start tax), and exits cleanly on the `watch::Receiver<bool>` shutdown signal
- dp-cli's `lib.rs` now depends on dp-fetcher (was domain-only). dp-rest similarly. Both still respect the §0.6 boundary rule (only forbidden crates are dp-domain / dp-fetcher / dp-reports)

## Open questions

- Bin-layer wiring (`crates/dev-pulse/src/main.rs`) doesn't actually spawn a Scheduler yet — that ties into composition-root work the later stages own. Stage 8 ships the building blocks, not the boot sequence
- `TargetProvider` implementation backed by the real Postgres store (enumerating repos with their github_ids) is left for the bin layer / dp-store-pg stage; current production wiring would need `StaticTargets` or a custom impl
