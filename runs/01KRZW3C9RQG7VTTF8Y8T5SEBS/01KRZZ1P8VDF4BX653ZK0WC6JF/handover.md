## Done

- created crates/dp-fetcher/src/backfill/{mod.rs,tests.rs}; registered `pub mod backfill` in lib.rs
- `Backfill { store, client, targets, kinds, config }` runs one-shot per-org with `run_for_org(org_id, shutdown)`, opening a fetch_runs row of kind `Backfill` per `(target × kind)` chunk
- `BackfillConfig` defaults to 90 days + 1000-request rate-limit headroom; voluntary yield via `honour_headroom` sleeps until `x-ratelimit-reset` (capped at 1h) when remaining drops under the threshold — keeps live webhook budget intact
- resumable: reads `fetch_cursors` on entry, uses `max(cursor.since, window_start)` as effective lower bound, advances cursor.since to max-of(prior, effective, response high-water) after each chunk
- synthesises webhook-shaped deliveries via `reconciler::synth` (now `pub(crate)`) and dispatches through `worker::apply_delivery` — same path as worker + reconciler, zero duplication
- `dp_cli::backfill_org(Arc<Backfill>, Uuid)` added as the shared CLI / install-time seam
- 6 backfill unit tests pass (apply-path, resume short-circuit, cursor advance, org filtering, shutdown cancel, default values); full workspace tests + scripts/check-boundaries.sh pass
- committed as 221f465 with the stage-9 message

## Next

- (none) — fresh session picks up stage 10

## What you need to know

- `Backfill` takes an `Arc<Client>`. The stage spec required a "separate octocrab client wrapper instance with a lower headroom threshold, or shared with priority" — the construction site (bin/dp-server) is responsible for handing in a dedicated `Client` built from the same `InstallationCredentials`. Documented in the module docs.
- Backfill kinds default to `[PullRequests, Issues, Commits]` (same as reconciler); other kinds (reviews/workflow_runs/etc.) short-circuit to `Skipped` since the GitHub list endpoints don't expose `since=` cheaply — they ride the webhook path. `with_kinds(&[…])` overrides for tests.
- `reconciler::synth` visibility changed from `mod synth` to `pub(crate) mod synth` so backfill can call it. No external API surface change.
- `BackfillConfig.window` is `std::time::Duration`; bin layer maps `starter-config`'s `backfill.window_days` into it.
- The `Cancelled` error is the only mid-run escape (besides store failures); chunk errors are absorbed into `stats.errors`.

## Open questions

- (none)
