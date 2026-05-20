## Done

- Widened `crates/dp-reports/src/leaderboard.rs` to fan out across every §2-valid `(SubjectKind, ScopeMode)` pair.
- Added `validate_subject_scope_combo` + `LeaderboardError::InvalidSubjectScopeCombo`; removed stage-3 `SubjectNotYetWired` / `ScopeModeNotYetWired` variants (no longer needed).
- Added `build_leaderboard_sql(subject, scope_mode) -> Result<&'static str, LeaderboardError>` dispatching to 9 per-combo SQL strings, all sharing `LEADERBOARD_BIND_ORDER` (6 params, `$3` is now `uuid[]`).
- Locked §6.1 tie-break in `LEADERBOARD_TIE_BREAK_ORDER_BY_CLAUSE` and asserted it appears verbatim in every dispatch SQL.
- Locked §6.8 `home_org_label` aggregation: `COALESCE(m.home_org::text, '__unlabeled__')` + `LEFT JOIN dp_memberships`; exported `HOME_ORG_LABEL_UNLABELED_BUCKET` ("__unlabeled__") and `HOME_ORG_LABEL_UNLABELED_LABEL` ("(no home org)") consts.
- `per_org_split` variants project a sixth `subject_org` column; cross-org `active_orgs` uses `count(DISTINCT e.org_id)` for user/home_org_label, hard-coded 1 elsewhere (incl. subject=org).
- Kept stage-3 `build_user_single_org_sql()` + `USER_SINGLE_ORG_BIND_ORDER` for legacy compat — its tests still pass unchanged.
- 12 new stage-4 unit tests on top of stage-3's 14 (26/26 green). `cargo build --workspace` clean; `scripts/check-boundaries.sh` OK.
- Committed as `1f01409` on `codeless/org-leaderboard` (not pushed).

## Next

- Stage 5: wire the §6.2 reconciliation footer (`unattributed_events` / `unattributed_events_metric`) and the §6.4 bot-suppression split (`bots_suppressed` / `bots_suppressed_events`) into the response, with the count-metric reconciliation identity asserted in debug builds.

## What you need to know

- The team variants reference `dp_team_members (team_id, user_id, org_id)` — a table that does NOT exist in the schema yet. The SQL is scaffolded so it lights up the moment that table lands; until then `subject=team` dispatch returns runnable-shaped SQL but the store cannot execute it. Treat as a parked dependency alongside `MetricId::Duration` (STAGE-1-COMPOSABILITY §3).
- `home_org_label` uses `dp_memberships.home_org` (a UUID) rather than a text `users.home_org_label` column. ORG-REPORTS §6.8 refers to "label" generically; the schema's nearest analog is `memberships.home_org`. If a real text-label column is added later, only the `COALESCE(...)` expression changes — the bucket constant and dispatch shape stay.
- `LEADERBOARD_BIND_ORDER` diverges from stage-3's `USER_SINGLE_ORG_BIND_ORDER` (`$3` is now `uuid[]`, not single `uuid`). Both consts are exported; downstream callers should migrate to the new unified one.
- Stage 4 deliberately keeps cross-org `orgs == []` permissive at the envelope layer — the auth layer in Phase 4 narrows it before the SQL binds. The dispatcher's `cardinality($3) >= 1` contract is enforced by callers, not in this scaffold.

## Open questions

- (none) — §6.1 + §6.8 are locked; §6.2 / §6.4 reconciliation is stage 5's job per the WORKFLOW.
