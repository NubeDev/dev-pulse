## Done

- Added `crates/dp-reports/src/leaderboard.rs` scaffolding the leaderboard report kind per ORG-REPORTS §1–§6.
- Defined `SubjectKind`, `MetricId` (tagged `{family,id}` so a future `Duration` variant is non-breaking), `LeaderboardEnvelope`, `ResolvedLeaderboardEnvelope` (carries `resolved_at` + `resolved_window`), `LeaderboardHeadline`, `LeaderboardRow` (`subject_org` `skip_serializing_if = "Option::is_none"`, populated only in `per_org_split`), `LeaderboardContext` (incl. reserved `extras` slot for §6.3 `also_compute`), `LeaderboardFooter`, `LeaderboardResponse`, and `LeaderboardError` (`#[non_exhaustive]`).
- Added `resolve_leaderboard_envelope(env, now)` — gates stage-3 scope (subject=user, single_org, exactly one org) and stamps `resolved_at`.
- Added `build_user_single_org_sql()` (`&'static str`) + `USER_SINGLE_ORG_BIND_ORDER` documenting the six bind params; SQL applies §6.1 tie-break (`primary_value DESC → active_days DESC → subject_id ASC`) and selects `subject_id / primary_value / active_days / repos_touched / active_orgs`. No LIMIT/OFFSET.
- Re-exported the public API from `crates/dp-reports/src/lib.rs`.
- 14 unit tests covering wire form, defaults, scope guards, `resolved_at` echo, tie-break order, projection columns, no-pagination guard, bind-order length, `subject_org` omit-when-None, and always-serialised footer.
- `cargo build --workspace` clean, `cargo test -p dp-reports leaderboard` 14/14 green, `scripts/check-boundaries.sh` OK.
- Updated `.codeless/jobs/org-leaderboard/handover.md` for stage 4.
- Committed (`2a978b2`) and pushed to `codeless/org-leaderboard`.

## Next

- Stage 4: extend `SubjectKind` (team / org / home_org_label) and `ScopeMode` (all_orgs_combined / per_org_split) fan-out; lock §6.1 tie-break + §6.8 `home_org_label` aggregation incl. `__unlabeled__` bucket.

## What you need to know

- The thin SQL builder is a pure `&'static str`; no store-side adapter consumes it yet — that integration is deferred so stage 4 can widen the builder to all subjects before anyone depends on a single-subject path.
- `active_orgs` is hard-coded `1::bigint` in the single-org SQL on purpose; stage 4's cross-org variants compute it properly and share the row-mapper.
- `LeaderboardContext::extras` is the §6.3 `also_compute` slot — stage 7 owns the fan-out logic.
- `MetricId::Duration(...)` is parked behind the missing `list_duration_samples_in_window` store method (STAGE-1-COMPOSABILITY §3); leaderboard ships against count metrics first.
- Stage 4 should not need to edit `crates/dp-reports/src/aggregate.rs` — count reducers stay the building blocks.

## Open questions

- (none) — stage 3 introduced no new SCOPE questions; SCOPE Q3 + Q4 remain owned by stages 9 and the frontend wiring stage.
