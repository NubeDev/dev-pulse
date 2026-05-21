# dev-pulse — Scope (Project & Issue Date Reports) — *design proposal*

> Status: **proposal**. Companion to [SCOPE.md](SCOPE.md) §8 (reporting
> dimensions) and [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md) (entities). This
> file is the source of truth for the **cross-project portfolio report**
> until it is promoted into [SCOPE.md](SCOPE.md). When that happens the
> normative copy lives there and this doc becomes "design rationale" the
> way SCOPE-PROJECTS.md already is.

---

## 1. Vision

A single report that answers, in one screen:

> *"Across every project I can see — which ones are on track, which are
> slipping, and which issues are doing the slipping?"*

The base product (SCOPE.md) tells a manager *what happened* across orgs
in a window. The project workbench (PROJECT-VIEW.md) tells them *what
is happening inside one project*. This report is the missing third
view: **the portfolio**. Projects and their issues both carry
`start_at` / `due_at`, and we already store the truth needed to compute
slip, burn-down, and timeline conflicts — we just don't yet **render**
it across more than one project at a time.

---

## 2. Why a separate document

- Reporting on date-bearing entities (projects + issues) is a different
  shape from the SCOPE.md §8 *activity-events* reports: the unit of
  measurement is **a planned window** (`start_at` → `due_at`), not a
  stream of `activity_events` rows. Bolting it onto the §15.6
  envelope would force a second meaning onto `Window` and confuse the
  §15.7 metric → role mapping.
- The data sources (`dp_projects`, `dp_issue_dates`, `dp_issues`,
  `dp_project_issues`) are already first-class — we don't need a new
  store layer, just a new query module + endpoint.
- Keeping the scope split lets us promote into SCOPE.md as a new §22
  ("Project portfolio report") once the design has shaken out, same as
  the SCOPE-PROJECTS.md → SCOPE.md §16-§19 promotion.

---

## 3. Goals

1. **One portfolio query.** Given the caller's visibility (orgs +
   memberships per SCOPE.md §15), return one row per visible project,
   plus a `kpis` block per row, in a single round trip.
2. **Date-aware KPIs.** Every row carries the same shape the §6.3
   detail page already shows in the [KPI grid](frontend/src/projects/project-detail-page.tsx):
   `progress`, `issue mix`, `timeline`, plus two new portfolio-only
   metrics — `slip_days` and `issue_overdue_count`.
3. **Filterable by status and window.** Re-uses the existing
   `dp_projects.status` enum + the §0.4 `Window` contract (only the
   resolved `(start, end)` interval — labels are out of scope here).
4. **Drill-through preserves URL state.** Each row links to the
   existing `#/projects/{id}` detail page; the report itself owns no
   per-project URL state.
5. **One surface, three callers.** REST handler + frontend page + MCP
   tool all consume the same envelope / response pair, the same way
   SCOPE.md §15.6 already locks the activity-report envelope.

---

## 4. Non-goals (for now)

- **No Gantt / timeline visualisation.** v1 is a table. Charting lives
  with PROJECT-VIEW.md follow-ups, not here.
- **No per-assignee report.** "Which issues is *Alice* slipping on" is
  a §15.7 activity-report problem, not a portfolio one.
- **No saved views.** PROJECT-VIEW.md §5.4 saved views are
  **per-project**. The portfolio report is a single page with simple
  URL params; it doesn't get its own `dp_project_views`-style table.
- **No predicting future slip.** We report what the dates currently
  say. Forecasting (velocity, burn-up) is explicitly deferred.
- **No editing from the report.** Date edits stay on the §6.3 detail
  page (existing "Edit timeline" dialog). The report is read-only.
- **No GitHub Projects v2 mirror.** Mirroring lives on
  `dp_project_board_links`; the report reads our authoritative
  `start_at` / `due_at` only.

---

## 5. Key entities (no new tables)

The report is a **pure query** over existing tables. New rows /
schemas: **zero**.

| Source                       | Provides                                            |
|------------------------------|-----------------------------------------------------|
| `dp_projects`                | `id, name, status, start_at, due_at, primary_milestone_id, version, lead_user_id, org_id` |
| `dp_projects` (denorm)       | `issue_count, closed_issue_count, board_link_count` |
| `dp_project_issues`          | Issue membership per project                        |
| `dp_issues`                  | `state` (open / closed), `repo_id, number, title`   |
| `dp_issue_dates`             | Per-issue `start_at, due_at` (SCOPE-PROJECTS §3.10) |
| `dp_orgs`                    | `login` for the org chip                            |
| `dp_users` (via `lead_user_id`) | Lead login + avatar for the "Lead" column         |

All "now" comparisons use the request's resolved UTC `now`, not
`NOW()` inline in SQL — same rule as §0.4 windows.

---

## 6. Request envelope

```rust
pub struct ProjectPortfolioRequest {
    /// Empty = every org the caller can see.
    pub orgs: Vec<OrgId>,

    /// Restrict to the listed statuses. Empty = `[Active, Backlog]`
    /// (the §6.1 sidebar default).
    pub statuses: Vec<ProjectStatus>,

    /// Optional window. When `Some(_)`, a project is included iff
    /// its `[start_at, due_at]` overlaps the window. `None` ⇒ no
    /// timeline filter. Uses the §0.4 `Window` contract: labelled
    /// resolved server-side, the response echoes the UTC pair.
    pub window: Option<WindowSpec>,

    /// `false` (default) ⇒ include projects whose `due_at` is
    /// in the past but still `status = active` (slipping).
    /// `true` ⇒ hide them. Mirrors the §6.1 sidebar "Hide done".
    pub hide_overdue: bool,

    /// Sort key. Maps to a small whitelist so the SQL is fixed.
    /// Default: `due_asc_nulls_last`.
    pub sort: PortfolioSort,

    /// `1`-based page + size — same envelope shape as
    /// `GET /projects` (SCOPE-PROJECTS §6.1).
    pub limit: u32,
    pub offset: u32,
}

pub enum PortfolioSort {
    DueAscNullsLast,
    DueDescNullsLast,
    SlipDaysDesc,
    ProgressAsc,
    NameAsc,
    UpdatedDesc,
}
```

The envelope **does not** include `users` / `teams` /
`activity_types` / `actor_roles` — those belong to the §15.6 activity
envelope. Cross-references between the two reports happen at the row
level (click-through), not by envelope union.

---

## 7. Response envelope

```rust
pub struct ProjectPortfolioResponse {
    pub rows: Vec<ProjectPortfolioRow>,

    /// Resolved `(start, end)` echoed back, per §0.4. `None` when
    /// the request omitted `window`.
    pub resolved_window: Option<(DateTime<Utc>, DateTime<Utc>)>,

    /// Resolved server-side; used by every `*_days` field below.
    pub now: DateTime<Utc>,

    /// Total matching rows (across pages). Same envelope shape as
    /// `GET /projects` to keep the pager honest.
    pub total: u32,
    pub limit: u32,
    pub offset: u32,

    /// Portfolio-level rollups computed across `rows` (not `total`)
    /// so the figures are honest about the page. UI shows
    /// "across N visible projects".
    pub kpis: PortfolioKpis,
}

pub struct ProjectPortfolioRow {
    pub id: ProjectId,
    pub org_id: OrgId,
    pub org_login: String,
    pub name: String,
    pub status: ProjectStatus,

    pub start_at: Option<DateTime<Utc>>,
    pub due_at:   Option<DateTime<Utc>>,

    pub issue_count:        i32,
    pub closed_issue_count: i32,
    /// `closed / total`, integer percent. `0` when total = 0.
    pub progress_pct: i32,

    /// Days between `now` and `due_at`.
    ///   - positive  ⇒ days remaining,
    ///   - negative  ⇒ days overdue (`status` may still be active),
    ///   - `None`    ⇒ no `due_at`.
    pub slip_days: Option<i32>,

    /// Issues attached to the project whose own `due_at < now`
    /// AND `state = open`. Includes issues with no `due_at` only
    /// when the project's `due_at` is also past (then they
    /// inherit the project's deadline). Documented at §9.
    pub issue_overdue_count: i32,

    /// Lead chip — `None` when unassigned.
    pub lead: Option<UserChip>,

    /// `true` iff there is at least one `dp_project_board_links` row.
    pub mirrored_to_github: bool,

    /// CAS token, echoed straight through so the row can deep-link
    /// into the §6.3 page without a fresh fetch.
    pub version: i32,
}

pub struct PortfolioKpis {
    /// Sum across visible rows.
    pub total_projects: i32,
    pub on_track: i32,        // due_at IS NULL OR due_at >= now
    pub overdue: i32,         // due_at < now AND status = active
    pub completed: i32,       // status = done
    /// Average integer percent across rows with `issue_count > 0`.
    pub avg_progress_pct: i32,
    pub total_issues_open: i32,
    pub total_issues_overdue: i32,
}
```

---

## 8. Filtering / visibility rules

- **Visibility:** identical to `GET /projects` (SCOPE-PROJECTS §6.1).
  The handler resolves "orgs the caller can see" once via the
  existing helper and joins against it — no new ACL surface.
- **Soft-delete:** archived projects are returned **only** when
  `statuses` includes `Archived`. This matches the sidebar's
  "Archived" toggle, not a separate flag.
- **Empty-state symmetry:** an empty `rows` with `total = 0` is the
  normal "you have no projects" response; the UI never shows a spinner
  for an empty portfolio (same rule the §6.2 list already follows).

---

## 9. Derived metrics — exact definitions

> Locking these here so REST, MCP, and the frontend never compute
> their own. Same rule as SCOPE.md §15.7.

**`progress_pct`** — `round(closed_issue_count * 100 / issue_count)`
when `issue_count > 0`; otherwise `0`. Matches the §6.3 KPI tile.

**`slip_days`** — `floor((due_at - now) / 1 day)` in UTC. `None`
when `due_at IS NULL`. Note this can be **positive on an `Active`
project that has already missed its date** if the date has been
edited forward; the sign + status combination is what marks
"slipping".

**`issue_overdue_count`** — `COUNT(*)` over the project's issues
where:

- `dp_issues.state = 'open'`, AND
- either `dp_issue_dates.due_at < now`, OR
  (`dp_issue_dates.due_at IS NULL` AND `dp_projects.due_at < now`).

The fall-through to the project's date is intentional: an issue
with no per-issue deadline is overdue iff the **project** is overdue.
This stops a project of 50 dateless issues from reporting "0
overdue" the day after the project's own `due_at` passes.

**`on_track / overdue / completed` in `PortfolioKpis`** — mutually
exclusive buckets:

| Bucket    | Predicate                                                 |
|-----------|-----------------------------------------------------------|
| Completed | `status = done`                                           |
| Overdue   | `status IN (active, backlog) AND due_at < now`            |
| On track  | everything else (incl. `due_at IS NULL` and `status = archived` when archived was filtered in) |

---

## 10. SQL / store layer

- New module `dp-reports::project_portfolio` — pure SQL string
  builder + envelope types, **no `sqlx`**. Same boundary as
  `leaderboard.rs` (§15.6).
- One query, one round trip. The `issue_overdue_count` lateral
  joins `dp_project_issues` → `dp_issues` → `dp_issue_dates` so we
  don't fan out to one query per row.
- `dp-store-pg::project_portfolio_query` is the only consumer; it
  binds parameters and decodes into `ProjectPortfolioRow`.
- Indexes already in place are enough for v1: `dp_projects
  (org_id, status, due_at)` from migration `0022_projects.sql`
  covers the dominant scan path. No new migration.

---

## 11. REST surface

- `POST /reports/project-portfolio`
  - Body: `ProjectPortfolioRequest`.
  - Response: `ProjectPortfolioResponse`.
  - Auth: same as `GET /projects` — caller's org-visibility set.
  - Pagination: re-uses the existing `limit`/`offset` envelope
    (no cursor surface; portfolios are small — `total < 1000` is
    the design budget).
- Uses POST (not GET) because the envelope carries a structured
  `window` object, matching SCOPE.md §15.6's choice for the same
  reason.
- Added to `openapi.rs` alongside the existing report routes; no
  new tag — it joins the `reports` tag.

## 12. MCP surface (Phase 5)

- Tool name: `project_portfolio`.
- Input schema: `ProjectPortfolioRequest`.
- Output schema: `ProjectPortfolioResponse`.
- Same shape locking as SCOPE.md §15.6 — adding a field for the
  REST side adds it to the MCP tool simultaneously.

## 13. Frontend surface

- New route: `#/reports/projects` — sits beside
  [`#/reports/freshness`](frontend/src/reports) so it picks up the
  existing nav slot for "cross-cutting" reports.
- One table, columns:

  | Project | Org | Status | Due | Slip | Progress | Open / Overdue | Lead |
  |---------|-----|--------|-----|------|----------|----------------|------|

  - **Project** is the row click-target → `#/projects/{id}`.
  - **Slip** renders as the same coloured pill the §6.3 KPI tile
    uses (`due in 6d` / `due today` / `5d overdue`) — re-uses
    [`relativeDue`](frontend/src/projects/project-detail-page.tsx)
    rather than re-implementing.
  - **Progress** is the same `<Progress>` bar from the KPI tile.
- The portfolio KPI strip sits above the table, four tiles
  (Total / On track / Overdue / Completed), reusing the existing
  `KpiTile` component so the styles never diverge.
- URL params:
  - `?status=active,backlog`
  - `?window=this-quarter` (label only; window resolution is
    server-side per §0.4)
  - `?sort=due_asc_nulls_last`
  - `?hide_overdue=1`
  - `?page=2`
- Saved-state: **none** for v1. The URL is the saved state.

---

## 14. Auth implications

None new. The handler reuses the existing org-visibility helper that
`GET /projects` already calls. Soft-deleted users surfaced as a row's
`lead` are pseudonymised the same way every other surface
pseudonymises them (SCOPE.md §15.x users).

---

## 15. Success criteria

1. From the new report page, a manager can answer "which of my
   projects are overdue right now?" **in one click**, with no need to
   open individual projects.
2. The page loads in `< 200 ms` server-time for `≤ 200` projects
   (the design budget — measured on the existing dev-pulse
   Postgres instance with no extra indexes).
3. The page renders identically to the §6.3 KPI tiles for the
   per-row metrics — no second source of truth.
4. The same `ProjectPortfolioRequest` envelope, byte-for-byte,
   drives the REST handler, the MCP tool, and the frontend
   `useReportProjectPortfolio` hook.

---

## 16. Open questions

1. **Per-issue rollup view.** Should the portfolio page have a "flat
   issues across all projects" mode (one row per issue, with the
   project as a column)? Useful for a date-driven inbox; out of v1
   because it duplicates the §6.5 cross-project issue list. **Decision
   needed before promotion.**
2. **Slip baselines.** v1 only knows the **current** `due_at`. Do we
   want to keep a history table of `due_at` edits so "slip" can be
   defined against the *original* deadline rather than the latest?
   Probably yes, but it needs its own migration + audit verb and is
   deferred to a follow-up.
3. **CSV export.** Listed as a non-goal but the table shape is
   trivially CSV-able. Cheap to add; revisit once the table itself
   ships.
4. **Cross-org Gantt.** Out of scope for v1 (§4); revisit once two
   independent users have asked for it.

---

## 17. Cross-references

- [SCOPE.md](SCOPE.md) §0.4 — `Window` contract.
- [SCOPE.md](SCOPE.md) §8 — reporting dimensions.
- [SCOPE.md](SCOPE.md) §15.6 — activity-report envelope (sibling
  shape; deliberately disjoint).
- [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md) §3.10 — per-issue
  `start_at` / `due_at`.
- [PROJECT-VIEW.md](PROJECT-VIEW.md) §6.3 — single-project KPI grid
  whose visual language this report mirrors.

---

## 18. Decisions (open → resolved log)

| Date       | Decision                                                                                                                                       |
|------------|------------------------------------------------------------------------------------------------------------------------------------------------|
| *proposal* | Report lives in its own envelope; **not** an extension of SCOPE.md §15.6. Different unit of measurement (planned window vs activity stream).   |
| *proposal* | No new tables. Pure query over `dp_projects` + `dp_project_issues` + `dp_issues` + `dp_issue_dates`.                                            |
| *proposal* | Issue is "overdue" iff its own `due_at < now`, **or** it has no `due_at` and the parent project's `due_at < now`. Locked at §9.                |
| *proposal* | `now` is resolved server-side once per request and echoed in the response; no `NOW()` interpolation in SQL.                                    |
| *proposal* | v1 is a table only. Gantt / charting is explicitly out of scope (§4).                                                                          |
