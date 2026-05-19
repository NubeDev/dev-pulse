## Done

- Added `crates/dp-reports/src/lenses.rs` with three pure functions: `single_org(rows, org_id)`, `all_orgs_combined(rows)` (dedup on `(user_id, event_id)` pairs, first-seen wins, deterministic), and `per_org_split(rows)` returning `BTreeMap<(user_id, org_id), Vec<EventActorRow>>`.
- Wired `pub mod lenses` into `crates/dp-reports/src/lib.rs` and re-exported `dp_domain::store::EventActorRow` from the crate root.
- Added 11 unit tests including the explicit SCOPE §11.4 trust regression `all_orgs_combined_cross_org_co_author_counts_once_per_user`, plus order-preservation, empty-input, multi-role-collapse, and distinct-users-on-one-event cases.
- `cargo test -p dp-reports` → 21 passed. `scripts/check-boundaries.sh` → OK.
- Committed as `a4cf0a5` on branch `codeless/phase-3-reports` with the stage-title commit message.

## Next

- Stage 5 (per project workflow). Not started — a fresh session picks it up.

## What you need to know

- Lenses operate on `&[EventActorRow]` and return owned `Vec<EventActorRow>` / `BTreeMap`. They clone rows; that's intentional so downstream report code can rebind to grouping/aggregation passes without ownership gymnastics. If row volumes ever justify zero-copy, the obvious move is to return `Vec<&EventActorRow>` and let downstream collect — defer that until benchmarks demand it.
- `all_orgs_combined` is first-seen-wins by `(user_id, event_id)`. Input order from `list_event_actor_rows_in_window` is the implicit tiebreaker — if the Postgres impl doesn't `ORDER BY ts, event_id, role` then snapshot tests downstream will be flaky.
- `single_org` re-filters by `org_id` even though the store usually narrows in SQL. Belt-and-suspenders, single source of truth — don't remove it.
- `per_org_split` deliberately does NOT dedup; that's documented in `per_org_split_does_not_dedup_multi_role_rows`. Downstream metrics that want unique-event-per-(user,org) counts must apply `(user_id, event_id)` dedup themselves.
- Zero `starter_*` imports; only `dp_domain` + std + `uuid` + `chrono` (test-only). Boundary check passes.

## Open questions

- The TODO §0.2 phrasing "co-authored commit spanning two orgs" is slightly ambiguous given that `activity_events` uses unique `external_id` (so a single physical commit dedupes to one `event_id` regardless of org reach). The cross-org test models the case where the same `event_id` is reachable under two `org_id`s in the rowset (e.g. fork reconciliation surfacing the row under both). If the Phase 1 store guarantees that can never happen, the test still passes and the rule is still right — it just covers a strictly stronger property than the schema produces in practice. Worth confirming with whoever owns SCOPE §11.4 fixtures.
