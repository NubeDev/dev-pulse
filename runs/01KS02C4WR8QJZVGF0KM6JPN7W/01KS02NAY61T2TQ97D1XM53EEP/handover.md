## Done

- Added `chrono-tz` (0.10) to workspace deps; wired into `dp-reports/Cargo.toml` along with `serde`, `serde_json`, `thiserror`, `uuid` and `dp-domain`.
- Created `crates/dp-reports/src/envelope.rs` with `ReportRequest`, `WindowSpec`, `WindowLabel`, `ScopeMode`, `GroupBy`, `ResolveError`, and `resolve_window` / `resolve_window_at(now)`.
- Re-exported `Window` and `WindowAnchor` from `dp-domain::window` through `dp-reports::lib` so callers depend on one crate.
- Local-midnight → UTC conversion handles DST corner cases: ambiguous local times take the earliest instant; skipped local times walk forward 1h up to 6 times.
- 10 unit tests pass (`cargo test -p dp-reports`); anchor matrix covers viewer/org/utc, NY spring-forward + fall-back, UTC year-end, Sydney year-end last_month + last_7_days, Feb 2024 leap month + leap day, plus custom-window validation and TZ rejection.
- `scripts/check-boundaries.sh` still green — no `starter_*` imports in dp-reports.
- Committed as `f7e88a4` on `codeless/phase-3-reports`.

## Next

- Stage 4: report query layer proper (the three SCOPE §8.1 lenses, counts filtered by `actor_roles`, percentile aggregates via `percentile_cont`, `data_as_of` envelope, spot-check fixture harness). Will consume `ReportRequest` + `resolve_window` from this stage.

## What you need to know

- `WindowLabel::Last7Days/Last30Days/Last90Days` need explicit `#[serde(rename = ...)]` because serde's `snake_case` produces `last7_days` not `last_7_days` — the wire form `last_7_days` is what SCOPE §15.8 / external consumers expect, and a regression test pins it.
- The worktree had no `../starter` directory; I added a symlink (`ln -s /home/user/code/rust/starter ../starter`) so the workspace builds. This is not committed (it's outside the repo); fresh worktrees may need the same.
- `resolve_window_at(spec, now)` is the test-friendly entry point — production callers use `resolve_window(spec)` which delegates with `Utc::now()`.
- `Window` (the resolved value type) lives in `dp-domain::window` and was already present; this stage only added `WindowSpec` (the request shape) in `dp-reports`.
- `ReportRequest` fields `orgs`, `users`, `teams`, `activity_types`, `actor_roles` all default to empty `Vec`; `group_by` defaults to `None`. Consumers can omit them in JSON.

## Open questions

- (none)
