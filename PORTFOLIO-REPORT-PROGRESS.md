# Project Portfolio Report — Implementation Progress

Spec: [SCOPE-PROJECT-REPORTS.md](SCOPE-PROJECT-REPORTS.md)

Working autonomously in `/loop` mode. Each stage produces a self-contained commit-ready change. After each stage the agent schedules a 2-minute wake-up to continue.

## Stages

- [x] **S0 — Survey & plan.** Read codebase (mirrors `leaderboard.rs`), confirm DB schema, lock plan.
- [x] **S1 — Domain envelope.** Add `ProjectPortfolioRequest`/`Response`/`Row`/`PortfolioKpis`/`PortfolioSort` types in `dp-reports`. No SQL yet. Compile only.
- [x] **S2 — SQL builder.** Pure SQL string builder in `dp-reports::project_portfolio`, no sqlx. Unit tests on the builder.
- [x] **S3 — Store binding.** `Store::list_project_portfolio` + `PgStore` impl + `row_to_portfolio_raw`. Integration test against real Postgres deferred to S7 (existing dp-store-pg integration tests use `#[ignore]` + testcontainers; portfolio test will follow that pattern).
- [x] **S4 — REST handler.** `POST /reports/project-portfolio` in `dp-rest` + openapi registration + snapshot regenerated + 3 handler tests (empty store, over-max limit, zero limit).
- [x] **S5 — MCP tool.** **Deferred to phase 5.** `dp-mcp` is a 5-line scaffold with no `Tool`/`ToolRegistry` surface yet. Implementing portfolio first would mean inventing the framework with no sibling tools to validate the design against. Left a structured TODO in `crates/dp-mcp/src/lib.rs` pointing at the REST handler as the reference implementation when phase 5 lands.
- [x] **S6 — Frontend page.** `#/reports/projects` route, KPI strip + table, URL params (`status`, `sort`, `hide_overdue`, `page`), zod schemas, `api.getReportProjectPortfolio()`, sidebar entry. `relativeDue` / `KpiTile` duplicated rather than extracted (~20 LoC each; extraction cost > duplication cost until a third caller appears).
- [x] **S7 — Polish.** Pagination footer (prev/next + "Showing X–Y of Z"), clickable sort headers (Project/Due/Progress with asc/desc toggle on Due), `dp-store-pg` integration test (`#[ignore]`, 6 assertions covering sort order / slip_days / hide_overdue / status filter / org_login / lead projection). Final workspace tests: clean (see summary below). Boundary check: clean.

## Design decisions log

- **2026-05-21** — `UserChip` lives in `dp-reports::project_portfolio`, not `dp-domain`. The spec uses it as a convenience typedef and no other surface consumes it yet; promoting to domain can wait until a second caller appears.
- **2026-05-21** — `version` is `i64` (matching `dp_projects.version BIGINT`), not the spec's `i32`. Spec is wrong here; the existing `Project` domain type is `i64` and we must match.
- **2026-05-21** — `rollup_kpis()` is the single source of truth for the `PortfolioKpis` block. REST/MCP/frontend must call this; never recompute.
- **2026-05-21** — Visibility: the codebase has no explicit "orgs the caller can see" helper. Existing report routes rely on `with_permission` middleware to gate access and trust the caller-provided `orgs` list. Portfolio will follow the same pattern; a stricter visibility helper is an authz follow-up tracked separately.
- **2026-05-21** — `POST` (not `GET`) per spec §11, because the envelope carries a structured `window` object. This is the first POST report endpoint in the codebase.
- **2026-05-21** — Status filter binds as `text[]`, not a pg enum array. `dp_projects.status` is `TEXT` with a `CHECK (status IN (...))` constraint per `0022_projects.sql`.
- **2026-05-21** — Default-status resolution (`empty ⇒ [active,backlog]`) happens in the **REST/MCP handler**, not the SQL builder. Reason: the SQL already uses `cardinality($2)=0 ⇒ all statuses` as a sentinel; putting the default in the builder would double-encode the meaning of empty. Handler is the single place that knows the spec default.
- **2026-05-21** — `slip_days = FLOOR(EXTRACT(EPOCH FROM (due - now))/86400)` in SQL, not Rust. Computing in SQL means `PortfolioSort::SlipDaysDesc` can sort on it without a second pass; computing in Rust would force a re-sort on every page.
- **2026-05-21** — Single CTE + `COUNT(*) OVER ()` rather than two queries. Trades a tiny extra column per row for one round-trip. At the v1 design budget (`total < 1000`, spec §15) this is unambiguously the right call.
- **2026-05-21** — Layering: `PortfolioSort`, `PortfolioQueryFilter`, `PortfolioRawRow` live in `dp-domain::project` (not `dp-reports`), mirroring `ProjectListFilter`. dp-reports re-exports them and adds the wire-form `From<PortfolioRawRow>` mapper. dp-store-pg now depends on dp-reports (clean direction; only `starter_*` deps are gated by `scripts/check-boundaries.sh`).
- **2026-05-21** — Store method is on the `Store` trait with a default impl returning `Vec::new()`, matching the rest of the project-related fetches. In-memory fakes that don't care about the portfolio stay quiet.
- **2026-05-21** — REST handler takes/returns `Json<serde_json::Value>` rather than `Json<ProjectPortfolioRequest>` / `Json<ProjectPortfolioResponse>`. Reason: utoipa requires `ToSchema` on the body type, and adding `utoipa` as a dep of `dp-reports` would introduce a new boundary. Manual `serde_json::from_value` at the boundary keeps dp-reports framework-free. The handler still uses the strongly-typed envelope internally — only the wire boundary is `Value`. Cost: openapi.snapshot.json describes the body via `description`, not a precise schema. Acceptable for v1; revisit if a generated TS client is added.
- **2026-05-21** — Default-status resolution lives in the handler: empty `req.statuses` → `[Active, Backlog]`. The SQL treats `cardinality($2) = 0` as "no filter" — putting the default in the handler keeps the sentinel meaning consistent.
- **2026-05-21** — `now = Utc::now()` resolved once per request in the handler, then passed into the filter and used to build `PortfolioKpis` via `rollup_kpis`. The response echoes the resolved `now` so the frontend `relativeDue` pill doesn't drift from the server's computation.
- **2026-05-21** — Frontend: `relativeDue` and `KpiTile` are duplicated from `project-detail-page.tsx` rather than extracted. Each is ~20 LoC; extracting would mean a new shared module + import churn on both pages, with a real risk of regressing the detail page's existing visual contract. Promote when a third caller appears.
- **2026-05-21** — Frontend: row click navigates via `window.location.hash = projectDetailRoute(row.id)`; an `<a>` inside the row owns the keyboard/middle-click affordance and `stopPropagation`s to prevent double-fire. Pattern chosen so the whole row is clickable (UX) without losing accessibility (link still focusable).
- **2026-05-21** — Sort UI is **omitted from S6**. URL `?sort=` is honoured but there's no clickable column header yet — keeps the diff narrow and the spec's success criteria (open page, see what's overdue in one click) are met without it. Tracked as an S7 polish item.

## Open questions for the user

None yet — design is detailed. If any arise mid-stage, listed here for batch resolution at next user touch.

## Run log

- 2026-05-21 — kick-off. S0 survey delegated to Explore agent.
- 2026-05-21 — S1 landed. New module `crates/dp-reports/src/project_portfolio.rs` (~360 LoC) + `lib.rs` re-exports. 6 unit tests passing (`cargo test -p dp-reports project_portfolio` → 6/6 ok). Crate compiles clean against `#![warn(missing_docs)]`. No new deps.
- 2026-05-21 — S2 landed. Added `build_project_portfolio_sql(sort) -> String` + `PROJECT_PORTFOLIO_BIND_ORDER` (8 slots) + 8 new SQL-shape tests (14/14 ok total). Single CTE + `COUNT(*) OVER ()` for one-round-trip total; correlated subqueries for `issue_overdue_count` (spec §9 fall-through to project due_at) + `mirrored_to_github`; whitelisted `ORDER BY` per sort variant. Status filter is `text[]` (column is TEXT+CHECK, not pg enum).
- 2026-05-21 — S3 landed. New `PortfolioSort` / `PortfolioQueryFilter` / `PortfolioRawRow` in `dp-domain::project`; `Store::list_project_portfolio` added with default impl; `PgStore` impl binds the dp-reports SQL via 8 params + `row_to_portfolio_raw`. Added `dp-reports` as a dp-store-pg dep. Workspace builds clean; dp-reports tests still 14/14 ok; dp-domain 41/41 ok.
- 2026-05-21 — S4 landed. `POST /reports/project-portfolio` in `dp-rest::reports` + `project_portfolio_report` in `openapi.rs` paths(); openapi snapshot regenerated (`UPDATE_OPENAPI_SNAPSHOT=1 cargo test -p dp-rest --test openapi_snapshot`). 3 handler tests covering empty-store happy path + limit validation (0 / over-max). dp-rest suite 155/155 ok, snapshot 1/1 ok. Wired under `with_permission("reports", "read")` like sibling reports.
- 2026-05-21 — S5 deferred. `dp-mcp` has no `Tool` framework yet (phase 5 work). Added a structured TODO comment in the crate's lib.rs pointing at the REST handler as the reference impl. Crate still builds clean.
- 2026-05-21 — S6 landed. `frontend/src/reports/project-portfolio-page.tsx` (~340 LoC) + zod schemas in `frontend/src/api/client.ts` + `getReportProjectPortfolio()` + `ReportTab="projects"` + `ReportsPane` case + sidebar entry in `layout/app-shell.tsx`. URL params: `?status=`, `?sort=`, `?hide_overdue=1`, `?page=N`. `pnpm build` → clean, `tsc --noEmit` → 0 errors.
- 2026-05-21 — S7 landed. Added clickable sort headers (Project / Due asc-desc / Progress) + pagination footer with prev/next + page indicator, all driving URL params via `navigate()`. Added `portfolio_query_round_trips_projects_with_kpis` integration test in `crates/dp-store-pg/tests/integration.rs` (`#[ignore]`d like every sibling, runs in the dedicated CI job with Docker). Workspace tests green; boundary check green.

## Final summary

**Status: shipped.** All 7 stages complete in a single autonomous /loop session on 2026-05-21.

### Files touched

Backend (Rust):
- `crates/dp-domain/src/project.rs` — added `PortfolioSort`, `PortfolioQueryFilter`, `PortfolioRawRow`.
- `crates/dp-domain/src/store.rs` — added `Store::list_project_portfolio` default impl.
- `crates/dp-reports/src/project_portfolio.rs` — **new** (~480 LoC): request/response/row/KPI types, `rollup_kpis`, `build_project_portfolio_sql`, `PROJECT_PORTFOLIO_BIND_ORDER`, `From<PortfolioRawRow>` mapper, 14 unit tests.
- `crates/dp-reports/src/lib.rs` — module + re-exports.
- `crates/dp-reports/Cargo.toml` — no change.
- `crates/dp-store-pg/Cargo.toml` — added `dp-reports` dep.
- `crates/dp-store-pg/src/store.rs` — added `list_project_portfolio` impl + `row_to_portfolio_raw` decoder.
- `crates/dp-store-pg/tests/integration.rs` — added `portfolio_query_round_trips_projects_with_kpis` (`#[ignore]`).
- `crates/dp-rest/src/reports.rs` — added `project_portfolio_report` POST handler + 3 unit tests + router entry.
- `crates/dp-rest/src/openapi.rs` — registered the new path.
- `crates/dp-rest/tests/openapi.snapshot.json` — regenerated.
- `crates/dp-mcp/src/lib.rs` — added structured TODO for phase-5 implementation.

Frontend (TypeScript):
- `frontend/src/api/client.ts` — added portfolio zod schemas + `getReportProjectPortfolio()`.
- `frontend/src/reports/project-portfolio-page.tsx` — **new** (~470 LoC): page with KPI strip, sortable table, pagination footer, URL-param sync.
- `frontend/src/routes.ts` — added `"projects"` to `ReportTab` + dispatch case.
- `frontend/src/app.tsx` — registered `ProjectPortfolioPage` in `ReportsPane`.
- `frontend/src/layout/app-shell.tsx` — added "Projects" sidebar entry under Reports.

Docs:
- `PORTFOLIO-REPORT-PROGRESS.md` — this file (planning + run log).
- `SCOPE-PROJECT-REPORTS.md` — unchanged (spec preserved).

### Test surface added

| Surface | Tests | Status |
|---|---|---|
| `dp-reports::project_portfolio` unit tests (envelope round-trip, KPI rollup edge cases, SQL builder invariants) | 14 | green |
| `dp-rest::reports` portfolio handler tests (empty store, limit ≤ 0, limit > 200) | 3 | green |
| `dp-store-pg` integration test (`portfolio_query_round_trips_projects_with_kpis`) | 1 | `#[ignore]`d; runs in the Docker CI job |
| `dp-rest/tests/openapi_snapshot` | 1 | green (snapshot regenerated) |
| **Total new** | **19** | |

Whole-workspace `cargo test --workspace --lib --bins` is green; frontend `pnpm build` is green; `scripts/check-boundaries.sh` is green.

### Known follow-ups (intentional non-goals or deferred work)

1. **MCP tool (`dp-mcp`)** — deferred to phase 5 when the Tool framework lands. REST handler is the reference implementation.
2. **Visibility helper** — handler currently trusts the caller-supplied `orgs` list under the existing `with_permission("reports", "read")` gate. A stricter "orgs the caller can see" helper is an authz follow-up; the rest of the report routes share the same posture, so this isn't a portfolio-specific regression.
3. **OpenAPI body schemas** — the POST body/response are described as `serde_json::Value` in the snapshot, because adding `utoipa::ToSchema` to dp-reports would cross a new boundary. Revisit if a generated TS client is added.
4. **Saved views / per-issue rollup mode / CSV export** — listed as spec non-goals or open questions §16. Not started.
5. **Performance budget verification** — spec §15 sets 200ms for ≤200 projects. Not measured. The `dp_projects(org_id, status, due_at)` index from `migration 0022_projects.sql` is the dominant scan path; the dedicated correlated subqueries on `issue_overdue_count` could become hot under fan-out. Profile when more than a handful of orgs each carry hundreds of projects.
6. **Promotion to SCOPE.md §22** — the proposal in `SCOPE-PROJECT-REPORTS.md` should now be promoted into the normative SCOPE.md (this file becomes "design rationale" the way SCOPE-PROJECTS.md already is, per the spec's own §2). Mechanical edit; deferred.
7. **Frontend `relativeDue` / `KpiTile`** — duplicated from `project-detail-page.tsx`. Promote to a shared module once a third caller appears (`code-simplification` skill rule of three).

### Where to start tomorrow

If picking this up cold:
- The REST handler at `crates/dp-rest/src/reports.rs::project_portfolio_report` is the integration point.
- The SQL string lives at `crates/dp-reports/src/project_portfolio.rs::build_project_portfolio_sql`.
- The frontend page is `frontend/src/reports/project-portfolio-page.tsx`.
- Hit `http://localhost:<port>/#/reports/projects` once a dev server is running to see it.
