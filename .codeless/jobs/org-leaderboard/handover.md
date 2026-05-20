# Handover — after stage 3 (scaffold leaderboard envelope + thin SQL builder)

Stage 3 is done. Next agent picks up **stage 4**: extend to all four
`SubjectKind` values and all three `OrgScope` modes; lock the §6.1
tie-break order and §6.8 `home_org_label` aggregation (incl. the
`__unlabeled__` bucket).

## What landed in stage 3

`crates/dp-reports/src/leaderboard.rs` (new module, re-exported from
`crates/dp-reports/src/lib.rs`):

- **Types** (ORG-REPORTS §3 / §4):
  - `SubjectKind` — `user | team | org | home_org_label`, snake_case
    wire form.
  - `MetricId` — internally tagged `{family, id}` so adding
    `Duration(...)` later is non-breaking (duration store fetch is a
    Phase-3 follow-up per `STAGE-1-COMPOSABILITY.md` §3).
  - `LeaderboardEnvelope` — window, scope_mode, orgs/repos/teams,
    actor_roles override, subject, rank_by, include_bots (default
    false per §6.4). **No** `also_compute` / `subject_ids` / `page`
    fields yet — those land in stages 7/8/6 and are additive.
  - `ResolvedLeaderboardEnvelope` — carries `resolved_at` +
    `resolved_window` so identical input + identical resolved_at
    produce identical output (§4 / §6.5 cursor pinning).
  - `LeaderboardResponse` — `envelope + headline + rows + footer`,
    matching the §4 jsonc.
  - `LeaderboardRow` — `rank, subject_id, subject_kind, subject_label,
    subject_org (Option, only serialised in `per_org_split`),
    primary, context, sparkline, active_orgs`.
  - `LeaderboardContext` — `active_days, repos_touched, extras` (the
    extras map is the §6.3 `also_compute` payload slot; serialised
    omitted when empty).
  - `LeaderboardFooter` — five-field reconciliation footer locked
    in shape (zeroed in stage 3; §6.2/§6.4/§6.6 wire stages fill it).
- **Errors** — `LeaderboardError` enum is `#[non_exhaustive]` and
  shared across surfaces. Stage 3 only emits
  `SubjectNotYetWired` / `ScopeModeNotYetWired` /
  `SingleOrgRequiresOneOrg` / `Resolve(ResolveError)`. Stage 6 will
  add `CursorWindowMismatch`; stage 8 will add `SubjectIdsTooLarge`.
- **`resolve_leaderboard_envelope(env, now)`** — guards stage 3
  scope (subject=user + single_org + exactly one org id) and stamps
  `resolved_at = now`. Tests pin a deterministic clock.
- **`build_user_single_org_sql()`** — `&'static str` SQL emitter.
  Selects `subject_id / primary_value / active_days / repos_touched /
  active_orgs`, `GROUP BY ea.user_id`, and applies the §6.1
  tie-break `primary_value DESC → active_days DESC → subject_id ASC`
  in `ORDER BY`. No `LIMIT` / `OFFSET` (pagination is stage 6).
- **`USER_SINGLE_ORG_BIND_ORDER`** — documents the six bind params
  so the store adapter and any integration test can't drift (§11.4).
  Tested for length 6 and that `$1..$6` all appear.
- **14 unit tests** in the `tests` module cover: JSON round-trip,
  metric-id wire form, snake_case subject_kind, `include_bots`
  default, stage-3 rejection of unwired subject/scope/zero-or-many
  orgs, `resolved_at` + window echo, tie-break order in the SQL,
  expected projection columns, no `LIMIT`/`OFFSET`, bind-order
  length, `subject_org` `skip_serializing_if = "Option::is_none"`,
  and footer fields always serialised.

## Verification

- `cargo build --workspace` — clean.
- `cargo test -p dp-reports leaderboard` — 14/14 green.
- `bash scripts/check-boundaries.sh` — OK (zero `starter_*` imports).

## What you need to know for stage 4

- The scaffold deliberately keeps the SQL builder a pure
  `&'static str`. The store adapter (`crates/dp-store-pg/src/store.rs`)
  isn't wired to call it yet — that integration is intentionally
  deferred so stage 4 can extend the builder to all four subjects /
  three scope modes *before* anyone depends on a one-subject path.
- The `active_orgs` column is hard-coded `1::bigint` in single-org
  SQL on purpose: stage 4's `all_orgs_combined` / `per_org_split`
  variants compute it properly and share the row-mapper.
- `LeaderboardContext::extras` is the §6.3 `also_compute` slot —
  reserve the field but stage 7 owns the actual fan-out logic.
- Per `STAGE-1-COMPOSABILITY.md`: when stage 4 adds the
  `union_of_default_roles(rank_by, also_compute)` widening helper,
  it lives in the leaderboard engine, **not** in `METRIC_ROLE_MAP`.
- Stage 4 still won't need to touch
  `crates/dp-reports/src/aggregate.rs` — the count reducers
  (`count_by_user / _team / _org`) are already the building blocks
  per the stage-1 note.
- `MetricId::Duration(...)` is still parked behind the missing
  `list_duration_samples_in_window` store method. Leaderboards
  against count metrics ship first; duration metrics flip on once
  that fetch lands. Not a stage 4 blocker.

## Open questions

- (none) — stage 3 introduced no new SCOPE questions. SCOPE Q3 + Q4
  remain owned by stages 9 (permission gate) and the frontend
  wiring stage respectively.
