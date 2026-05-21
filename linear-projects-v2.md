# Projects: first-class planning surface for dev-pulse

> A re-pitch of the §3.10 Projects v2 mirror. Replaces the
> "admin sets up a per-repo board link, then dates silently
> sync" model with a first-class **Project** object the team
> plans against — with optional, transparent GitHub Projects v2
> mirroring as one of several sync targets.
>
> Companion to [linear-projects-idea.md](linear-projects-idea.md)
> (the triage workbench) and [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md)
> (pins / tags / issue CRUD). Where those documents stop at
> "issues need a home in a kanban-style structure," this one
> picks up: that home is a **Project**, owned by us, optionally
> mirrored to GitHub.

---

## 0. Progress log

### 2026-05-21 — v1 design pitch (this document)

Authored after the §3.10 mirror landed and the resulting UX was
unanimously judged unusable from the workflow page. The mirror
plumbing — `dp_repo_project_link`, `gh_projectv2_*`,
`OctocrabProjectV2Mirror`, the admin pane — is retained as the
*transport layer* but stops being the user-facing surface. The
admin pane becomes the "advanced / paste node ids" escape
hatch (§9.4); the primary surface is the `#/projects` section
described here.

### 2026-05-21 — peer-review pass

Applied fixes from peer review:

- Added `dp_project_board_items` to track the projected item
  node id per (link, issue); retired the repurposing of
  `dp_issue_dates.mirror_*` (§5, §1 of mirror semantics).
- Resolved the multi-project ambiguity (§4, §6.5, §14.4):
  v1 ships **one project per issue** as a hard storage
  constraint; the join table gets `UNIQUE (issue_id)`.
- Tightened permissions: `archive`, `lead_user_id` change,
  and `board.unlink` now gated on `created_by ∨ lead_user_id
  ∨ admin` (§9.2).
- Normalized the picker endpoint into a DTO instead of
  leaking the GraphQL envelope (§7.3).
- Migration 0024 now renames `dp_repo_project_link` to
  `_deprecated_dp_repo_project_link`; physical drop deferred
  one release (§8, §11).
- Defined the partial-failure contract for the mirror fan-out:
  `207 Multi-Status` with per-link outcome array (§7.4).
- Added a `SyncStatus` aggregate UI affordance to replace the
  single-line "Synced HH:mm:ss" footnote (§6.5).
- Added partial-unique constraint on project name (excludes
  archived); denormalized issue/closed counts onto
  `dp_projects` to avoid 3× COUNT(*) per list row (§5, §7.1).
- Added `expected_version` to bulk add (§7.2).
- Removed the dead "pull dates back — disabled" checkbox
  from the §6.4 dialog.
- Specified sidebar treatment of `archived` (§6.1).
- Added a board-metadata refresh policy (§6.4).
- Capped bulk-add batch size at 100 (§7.2, §9.3).

---

## 1. TL;DR

A **Project** is a dev-pulse-owned container of issues across
repos, with a goal, optional start / due dates, a lead, and a
derived status (% of issues closed). Projects live in the main
sidebar — not in admin. Users create projects, add issues from
the triage page, watch progress, and **optionally** wire a
GitHub Projects v2 board as a sync target so issue dates round-
trip to GitHub.

Naming: the surface is **`Projects`** (top-level sidebar item).
Internally the table is `dp_projects`, the section URL is
`#/projects`. The GitHub-side concept is always written as
**"GitHub board"** or **"Projects v2 board"** in the UI to
avoid the naming collision.

---

## 2. Why a new document

[SCOPE-PROJECTS.md](SCOPE-PROJECTS.md) §1 explicitly named this
work as "out of scope for now" — the v1 GitHub-write surface
was issue CRUD + tag links, not a planning object. The §3.10
mirror in [linear-projects-idea.md](linear-projects-idea.md)
landed as a *transport* (write dates to a board) without
landing the *thing* the user actually wants to plan against.

The result: a date editor that mirrors to a board the user has
never heard of, configured on a page they have to be told
exists, with a node-id paste field if the picker fails.

This document specifies the missing piece — the **Project**
itself — and demotes the §3.10 mirror to one of its
implementation details.

---

## 3. Goals

### 3.1 Primary goals

- **First-class Project object** with name, description, lead,
  start / due, status (active / backlog / done / archived),
  issue list. Owned by dev-pulse; not derived from anything on
  GitHub.
- **Cross-repo issue membership**: one project contains issues
  drawn from any repos in the same org. Adding `repo-a#42` and
  `repo-b#17` to "Rubix v2 launch" is a single click each.
- **In-context project ops from triage**: in the workflow
  detail pane (§14 in linear-projects-idea.md), an issue
  surfaces its project membership with `[+ Add to project]` /
  `[Change…]` / `[Remove]` controls. No admin-page detour.
- **Optional GitHub board mirror**: a project can be linked to
  one or more GitHub Projects v2 boards. When linked, issue
  start / due edits flow to the board's Start / Due date
  fields. The link uses GraphQL pickers; **no node-id paste
  field is ever shown** on the primary path (see §9.4 for the
  advanced escape hatch).
- **Derived progress + due-date health**: list view shows
  `% closed`, `Due in N days`, on-track / at-risk / overdue
  badges. Nothing the user has to maintain by hand.

### 3.2 Secondary goals

- **Per-project ownership**: a `lead_user_id` and a free-text
  `description`. The lead drives default visibility (the lead's
  org) and is the default `Mentioned` filter target in §3.2.
- **Sortable, filterable list page**: status / lead / due-date
  / org / search-by-name. Same React Query plumbing as the
  triage list.
- **Counts in the sidebar**: `Projects ▸ Active (3)` like the
  smart-view rail in §3.2.
- **Audit parity** with §8.5 issue writes — every project
  CRUD, every issue add/remove, every board link/unlink lands
  an `audit_log` row with the same shape as the rest of the §8
  write surface.

---

## 4. Non-goals (for v1)

- **No two-way mirror.** Editing dates on the GitHub board
  does not (yet) pull back into dev-pulse. Stub task type
  exists per §15.3 of linear-projects-idea.md; populating it
  needs the §6 projection table and a webhook subscription.
- **No project templates.**
- **No automation rules** ("when label = `rubix-v2` is applied,
  auto-add to project X"). Users add manually for v1.
- **No milestones.** GitHub milestones can be a future sync
  source; not in v1.
- **No timeline / Gantt view.** v1 ships the issue list view
  only, grouped by status (open / closed) or assignee.
- **No project-level dependencies / blocking relationships.**
- **No cross-org projects.** A project belongs to exactly one
  `org_id`. Multi-org rollups are §10.
- **Mirror remains write-only.** Same scope as §3.10 today.
- **The §3.10 `dp_repo_project_link` table is renamed to
  `_deprecated_dp_repo_project_link`** in slice B (see §11);
  physical `DROP TABLE` is deferred one release as a safety
  rail against any unannounced operator usage.
- **One project per issue (v1).** The join table carries a
  `UNIQUE (issue_id)` constraint. Multi-project membership is
  a v2 concern — splitting an issue across two roadmaps has
  no defined mirror semantics today (which board owns the
  date?) and the §6.5 detail-pane chip is singular. Storage
  is shaped to allow the constraint to be relaxed later
  without a destructive migration.

---

## 5. Key entities

```
dp_projects                                       (NEW)
├── id                  uuid pk
├── org_id              uuid fk → dp_orgs.id      (one project, one org)
├── name                text  (required, unique within org — see partial index below)
├── description         text  (optional, markdown)
├── lead_user_id        uuid fk → dp_users.id     (nullable)
├── status              enum  ('active' | 'backlog' | 'done' | 'archived')
├── start_at            timestamptz NULL
├── due_at              timestamptz NULL
├── issue_count         int  NOT NULL DEFAULT 0   (denormalized, maintained by add/remove)
├── closed_issue_count  int  NOT NULL DEFAULT 0   (denormalized, recomputed on issue close webhook)
├── created_by          uuid fk → dp_users.id
├── created_at          timestamptz
├── updated_at          timestamptz
└── version             bigint  (CAS, §8.2 contract)

dp_project_issues                                 (NEW — join table)
├── project_id          uuid fk → dp_projects.id
├── issue_id            uuid fk → dp_issues.id
├── added_by            uuid fk → dp_users.id
├── added_at            timestamptz
├── PRIMARY KEY (project_id, issue_id)
└── UNIQUE (issue_id)                            (v1: one project per issue — see §4)

dp_project_board_links                            (NEW — replaces dp_repo_project_link)
├── id                  uuid pk
├── project_id          uuid fk → dp_projects.id
├── github_board_node_id    text   (PVT_…)
├── github_board_title       text   (cached display name; refreshed per §6.4)
├── github_board_url         text   (cached link to github.com; refreshed per §6.4)
├── github_board_cached_at   timestamptz                  (last picker-call refresh)
├── start_field_node_id      text NULL
├── due_field_node_id        text NULL
├── status_field_node_id     text NULL  (reserved; not used by mirror v1)
├── last_mirror_at           timestamptz NULL             (aggregate across this link's items)
├── last_mirror_error        text NULL
├── created_by          uuid fk → dp_users.id
├── created_at          timestamptz
├── updated_at          timestamptz
└── UNIQUE (project_id, github_board_node_id)

dp_project_board_items                            (NEW — per-(link, issue) projection state)
├── link_id             uuid fk → dp_project_board_links.id  ON DELETE CASCADE
├── issue_id            uuid fk → dp_issues.id               ON DELETE CASCADE
├── item_node_id        text NOT NULL                        (PVTI_… — the board item)
├── last_synced_at      timestamptz NULL
├── last_error          text NULL
└── PRIMARY KEY (link_id, issue_id)
```

Notes:

- `dp_project_board_links` is plural — one project can mirror
  to many boards (you may have an Eng Sprint board AND a
  cross-team Roadmap board). v1 ships writes against all of
  them; failures land per-link on `last_mirror_error` (see
  §6.4) and per-(link, issue) on `dp_project_board_items.last_error`.
- `dp_project_board_items` exists because each issue is
  projected to a **distinct** GH item node id per board. A
  single column on `dp_issue_dates` cannot represent N items;
  subsequent PATCHes need the per-(link, issue) `item_node_id`
  to know which item to update.
- The previously-repurposed `dp_issue_dates.mirror_node_id` /
  `mirror_synced_at` / `mirror_error` columns are **retired**
  by migration 0023 (dropped). All mirror state now lives on
  `dp_project_board_links` (aggregate) and
  `dp_project_board_items` (authoritative per item).
- `dp_issues.github_node_id` (migration 0021, already shipped)
  stays — it is the input to the initial `addProjectV2ItemById`
  call that populates `item_node_id`.

---

## 6. Surfaces

### 6.1 Sidebar

A new top-level section between **Workflow** and **Directory**:

```
dev-pulse
├─ Reports
├─ Workflow
│  ├─ Triage
│  ├─ Repos
│  └─ Issues
├─ Projects          ← NEW
│  ├─ Active    (3)
│  ├─ Backlog   (12)
│  ├─ Done
│  └─ Archived  (collapsed by default; no count)
├─ Directory
└─ Admin              (no longer hosts a "Projects" tab)
```

Counts are live from `GET /projects?status=active&count_only=1`
(same `useQuery` cadence as the §3.2 smart-view rail).
`Archived` is hidden behind a one-click expand and intentionally
uncounted to avoid drawing the eye to it.

### 6.2 `#/projects` — list page

Columns: name, status pill, progress bar (% closed), due date,
issue count, lead avatar, org. Default sort: `status` then
`due_at ASC NULLS LAST`. Search by name with the same `Input`
component used in §3.5.

`[+ New project]` opens a 3-field modal (name, description,
lead). Status defaults to `active`. Issue list is empty until
the user adds issues from triage.

### 6.3 `#/projects/{id}` — single project page

Header: name + status pill + `[Edit]` `[Archive]`. Body:

1. **Description** (markdown).
2. **Meta block**: Start · Due · Lead · `% closed`.
3. **Linked GitHub boards** (§6.4 affordance).
4. **Issue list** — same React Query hook + row component as
   the triage list, filtered to `project_id = {id}`. Grouped
   by `status` (open / closed) by default; group-by /
   sort-by dropdowns identical to the triage page (§3.5).

### 6.4 Link-a-board dialog (replaces the admin pane)

Triggered from §6.3 `[+ Link a GitHub board…]`. Contents:

```
┌─ Link a GitHub board ─────────────────────────────┐
│ Board on GitHub                                   │
│ [▾ NubeIO / Rubix Roadmap                      ]  │
│                                                   │
│ Map fields:                                       │
│   dev-pulse Start → [▾ Begin date              ]  │
│   dev-pulse Due   → [▾ Target date             ]  │
│                                                   │
│                       [Cancel]   [Link board]     │
└───────────────────────────────────────────────────┘
```

- Board dropdown source: `GET /orgs/{org_id}/projects-v2`
  (new endpoint, replaces today's `GET /repos/{id}/projects`).
  Returns the org's boards as a normalized DTO (§7.3) — the
  raw GraphQL envelope does not leak into the REST contract.
- Field dropdowns populate from the selected board's
  `date_fields` in the same DTO.
- **No node-id paste field on this dialog**. If the GraphQL
  call fails (token has no `project` scope, GitHub 5xx, no
  boards exist), the dialog shows a helpful error with a
  `[Open GitHub project settings]` link — never a paste box.
- Mirror status is per-link: once linked, the row in §6.3
  shows `Last sync: 14:23:07 ✓` or `Last sync: failed —
  <message>` with a `[Re-link]` button.
- **Two-way sync** (pull date changes from GitHub back into
  dev-pulse) is not in v1 and **not surfaced as a disabled
  control**. It enters the UI in slice C / v2 when the
  webhook subscription + projection table land (§4).
- **Board metadata refresh**: `github_board_title` /
  `github_board_url` are refreshed opportunistically every
  time the §7.3 picker endpoint runs (it touches every linked
  board for that org). A nightly background job re-runs the
  picker per org as a safety net so renamed/deleted boards
  surface within 24h instead of waiting for someone to open
  the dialog.

### 6.5 Workflow detail-pane integration

In the §14 detail pane, after the dates editor:

```
┌─ #384 config file for env ────────────────────────┐
│ … existing fields …                               │
│                                                   │
│ Project:  ● Rubix v2 launch    [Change…] [Remove] │
│                                                   │
│ Start    [ 2026-05-21 ]   Due  [ 2026-06-15 ]     │
│ Sync:  2 of 2 boards ✓   14:23:07         [▾]     │
│        └─ NubeIO / Rubix Roadmap   ✓ 14:23:07     │
│        └─ NubeIO / Eng Sprint 24   ✓ 14:23:08     │
└───────────────────────────────────────────────────┘
```

If no project membership: `[+ Add to project]` opens a
quick-pick of the issue's org's active projects + a `[+ New
project]` row.

The `Sync:` line is a `SyncStatus` aggregate component (one
per issue) that collapses N linked boards into a single status
plus a disclosure triangle:

- `N of N boards ✓ HH:mm:ss` — all succeeded.
- `M of N boards · 1 failed [Retry]` — partial failure;
  expanded rows show per-board error text.
- `Syncing 2 boards…` — in-flight.

Backed by the §7.4 `207 Multi-Status` response, so the UI can
render per-board outcomes without a second round-trip.

For the common single-board case the line collapses to the
familiar `✓ Synced 14:23:07 to "NubeIO / Rubix Roadmap"`.

`[Change…]` and `[Remove]` are CAS-gated against the project's
own `version` (multi-user safety per §8.2). `[Change…]`
replaces (not adds) the project membership — the §4 `UNIQUE
(issue_id)` constraint enforces this server-side.

### 6.6 Bulk add from triage

The triage list (§3.5) gains one bulk action on the existing
selection-checkbox toolbar:

```
12 issues selected   [Add to project ▾] [Mark seen] [Snooze ▾]
```

Picker is the same quick-pick as §6.5. Bulk add is one atomic
endpoint call (§7.4) so partial failures are reported per-row.

---

## 7. API

All routes are gated on `(projects, read)` / `(projects, write)`
in the §15 policy engine. Both pairs default to `allowed` for
in-org users mirroring `(issues, read|write)`.

### 7.1 Project CRUD

| Method | Path                          | Returns          |
|--------|-------------------------------|------------------|
| GET    | `/projects?org_id=&status=&q=` | `ProjectListResponse` |
| GET    | `/projects/{id}`              | `ProjectDto`     |
| POST   | `/projects`                   | `ProjectDto`     |
| PATCH  | `/projects/{id}`              | `ProjectDto`     (CAS via `expected_version`) |
| POST   | `/projects/{id}/archive`      | `ProjectDto`     |

`ProjectDto` carries `id, org_id, name, description, lead,
status, start_at, due_at, version, created_at, updated_at,
issue_count, closed_issue_count, board_link_count`. The
first two counts are **denormalized columns** on `dp_projects`
(maintained transactionally on issue add/remove and on the
issue-close webhook); `board_link_count` is a single
`COUNT(*)` per row, capped by the small fan-out (≤ ~10 links
per project in practice). The list page meets a < 200 ms
p95 SLO with a single round-trip and no per-row aggregate
subqueries.

### 7.2 Project ↔ issue membership

| Method | Path                                       | Returns |
|--------|--------------------------------------------|---------|
| GET    | `/projects/{id}/issues?…`                  | `IssueListResponse` (same shape as `/issues`) |
| POST   | `/projects/{id}/issues`                    | `BulkAddResult`  (body: `{ expected_version, issue_ids: [uuid] }`) |
| DELETE | `/projects/{id}/issues/{issue_id}?expected_version=` | 204     |
| GET    | `/issues/{id}/project`                     | `ProjectDto?` (null if none) |

`BulkAddResult` mirrors `LinkBatchResponse` from §7 of
SCOPE-PROJECTS.md — `{ added: [uuid], skipped: [{id, reason}] }`
— so the UI can render per-row outcomes.

- `issue_ids` is capped at **100 per request**. Larger
  selections from the §6.6 bulk affordance are chunked
  client-side and the toolbar reports aggregate progress.
- Bulk add is CAS-gated on the **project's** `version`,
  matching `PATCH /projects/{id}`. Single-issue removal
  takes the same `expected_version` as a query param.
- The v1 one-project-per-issue rule (§4) means an attempt to
  add an issue already in another project lands in `skipped`
  with `reason: "already_in_project"` and the existing
  project's id; the UI offers a one-click `Move here?`
  follow-up.

### 7.3 Board picker + link CRUD

| Method | Path                                              | Returns |
|--------|---------------------------------------------------|---------|
| GET    | `/orgs/{org_id}/projects-v2`                      | `OrgProjectPickerDto` |
| GET    | `/projects/{id}/board-links`                      | `[BoardLinkDto]` |
| POST   | `/projects/{id}/board-links`                      | `BoardLinkDto` |
| DELETE | `/projects/{id}/board-links/{link_id}`            | 204 (caller must be `created_by` of the project, `lead_user_id`, or admin — §9.2) |

`OrgProjectPickerDto`:

```
{
  boards: [
    {
      node_id: "PVT_…",
      title: "NubeIO / Rubix Roadmap",
      url: "https://github.com/orgs/NubeIO/projects/12",
      number: 12,
      date_fields: [
        { node_id: "PVTF_…", name: "Begin date" },
        { node_id: "PVTF_…", name: "Target date" }
      ]
    }
  ],
  fetched_at: "2026-05-21T14:23:00Z"
}
```

`BoardLinkDto`: `{ id, project_id, github_board_node_id,
github_board_title, github_board_url, start_field_node_id,
due_field_node_id, last_mirror_at, last_mirror_error }`.

The picker endpoint is org-scoped (was repo-scoped in §3.10).
It returns a **normalized DTO**, not the raw GraphQL envelope —
the REST contract must not be coupled to GitHub's GraphQL
schema. The existing `decodeProjects` decoder is re-targeted
at `OrgProjectPickerDto` (a one-time port; mechanical).

### 7.4 Inheritance from §3.10

The following remain unchanged:

- `dp_fetcher::client::gh_projectv2_add_item`,
  `gh_projectv2_update_date_field`,
  `gh_resolve_issue_node_id`, `gh_list_repo_projectv2`. The
  picker now uses an analogous `gh_list_org_projectv2`.
- `OctocrabProjectV2Mirror::mirror_dates` keeps its outer
  shape but is rewired:
  1. Resolve the issue's project (one, per §4 `UNIQUE
     (issue_id)`).
  2. For each linked board, look up the
     `dp_project_board_items` row for `(link_id, issue_id)`.
     If absent, call `gh_projectv2_add_item` to create the
     item and persist its `item_node_id`.
  3. Call `gh_projectv2_update_date_field` against that
     `item_node_id` (NOT against a single repurposed
     `mirror_node_id`).
  4. Update `last_synced_at` / `last_error` on the
     `dp_project_board_items` row; aggregate up to
     `dp_project_board_links.last_mirror_*`.
- `PATCH /issues/{id}/dates` still triggers the mirror; the
  response is now `207 Multi-Status` (§7.4) when ≥ 1 board is
  linked.

### 7.4 Mirror fan-out response contract

`PATCH /issues/{id}/dates` (and any other write that fires the
mirror) returns:

- `200 OK` with the updated `IssueDatesDto` when 0 boards are
  linked (no mirror work).
- `200 OK` with `IssueDatesDto + { mirror: { ok: true,
  per_board: [...] } }` when **all** linked boards succeed.
- `207 Multi-Status` with `IssueDatesDto + { mirror: { ok:
  false, per_board: [{ link_id, board_title, status:
  "ok"|"failed", item_node_id?, synced_at?, error? }] } }`
  when ≥ 1 board fails. The local dev-pulse write **still
  commits** — mirror failures never roll back dev-pulse state.
- `502 Bad Gateway` only when **every** linked board fails
  with a transport-level error (network down, token revoked
  org-wide). Per-board errors specific to a single board
  always surface as `207`.

The `SyncStatus` component in §6.5 renders directly off the
`per_board` array.

---

## 8. Storage migrations

Four new migrations:

- `0022_projects.sql` — `dp_projects` + `dp_project_issues`
  (with the `UNIQUE (issue_id)` v1 constraint).
- `0023_project_board_links.sql` — `dp_project_board_links` +
  `dp_project_board_items`. **Also drops** the now-unused
  `dp_issue_dates.mirror_node_id` / `mirror_synced_at` /
  `mirror_error` columns (their state moves to
  `dp_project_board_items`).
- `0024_rename_repo_project_link.sql` — `ALTER TABLE
  dp_repo_project_link RENAME TO _deprecated_dp_repo_project_link;`
  (safety rail per §11). Physical `DROP TABLE` is queued for a
  later release once we've confirmed no operator depends on
  the old shape.

Indexes:

- `dp_projects(org_id, status, due_at)` for the list page sort.
- `dp_projects(org_id, lower(name)) UNIQUE WHERE status <>
  'archived'` — partial index so users can reuse the name of
  an archived project.
- `dp_project_issues(issue_id)` — covered by the `UNIQUE
  (issue_id)` constraint above; called out for clarity.
- `dp_project_board_links(project_id)` for the mirror fan-out.
- `dp_project_board_items(issue_id)` for "which boards is
  this issue projected to?".

---

## 9. Identity, permissions, audit

### 9.1 Permission pairs

Two new pairs in the §15 policy engine:

- `(projects, read)` — view list / detail / membership.
- `(projects, write)` — create / patch / archive / add or
  remove issues / link or unlink boards.

Defaults mirror `(issues, ·)`: in-org users get both; out-of-
org users get neither and see the §15.11 access banner.

### 9.2 Lead vs. author — and elevated operations

`created_by` is immutable. Most mutations require only
`(projects, write)`:

- `name`, `description`, `start_at`, `due_at`, `status`
  (active ↔ backlog ↔ done).
- Issue add / remove (including bulk).
- Board link **create** (linking a board is a planning act).

Three operations are **elevated** — the caller must be one of
`created_by`, `lead_user_id`, or carry the `admin` role:

- `POST /projects/{id}/archive`.
- `PATCH /projects/{id}` when the patch mutates
  `lead_user_id`.
- `DELETE /projects/{id}/board-links/{link_id}`.

Rationale: these have a large blast radius — archiving hides
the project from every default view, lead change re-targets
default Mentioned filters, and unlinking a board breaks an
active sync that other teammates may be relying on.

Multi-user safety on the non-elevated path is the §8.2 CAS
pattern (`expected_version`).

### 9.3 Audit

Every project mutation lands a row in `audit_log` with verbs
mirroring §8.5:

- `project.create`, `project.update`, `project.archive`
- `project.issue.add`, `project.issue.remove`
- `project.board.link`, `project.board.unlink`

The bulk add (§7.2) lands one row per `issue_id` for
transparency (same as §8.5 issue write batching), capped at
the §7.2 per-request limit of 100 — so worst-case audit
volume per call is 100 rows.

### 9.4 Advanced escape hatch

The admin page at `#/admin/projects` is repurposed: it becomes
a diagnostics + raw-paste fallback surface used only when:

- The GraphQL picker is unavailable (token has no `project`
  scope, or the deployment has no GraphQL transport).
- An operator needs to attach a board whose org is different
  from the project's org (rare; supported via the paste box).

The page is renamed in the sidebar to `Admin ▸ Project sync`
to remove the naming collision with the new top-level
**Projects** section.

---

## 10. Reports integration

A new dimension in `GET /reports/issues`: `project_id`. Lets
the §3.2 reports page slice by project (e.g. "open bugs in
Rubix v2"). No new endpoint — extend the existing query
parameter set and add a `project_id` column to the issue
report row.

`/projects/{id}` page reuses the §3.2 chart components scoped
to the project's issue set so a project's "closed per week"
sparkline is free.

---

## 11. Migration from §3.10

The §3.10 admin page is moved to `#/admin/project-sync` (per
§9.4) and the old `dp_repo_project_link` table is dropped.
Anyone who linked a repo→board in §3.10 will need to re-link
at the project level — acceptable because the admin page was
never put in front of real users (per the §0 progress log of
this document).

Code retained from §3.10:

- The mirror plumbing in `dp_fetcher::client` (every `gh_*`
  GraphQL method).
- `OctocrabProjectV2Mirror::mirror_dates` (rewired to fan out
  per board link).
- The `IssueDatesEditor` "Syncing… / Synced HH:mm:ss / Sync
  failed" footnote (works against the new fan-out).
- `dp_issues.github_node_id` (migration 0021) + the lazy-
  resolve fallback in `IssueNodeIdRef`.

Code removed:

- `crates/dp-rest/src/repo_project_link.rs` (replaced by the
  project-scoped equivalent).
- `frontend/src/admin/projects-page.tsx` becomes
  `frontend/src/admin/project-sync-page.tsx` — same
  component, less prominent placement, no node-id paste
  field on the primary path.

---

## 12. Phasing

### Slice A — Projects without GitHub mirror (ship first)

- Migrations 0022 (projects + project_issues).
- `/projects` CRUD + membership endpoints.
- `#/projects` list page + `#/projects/{id}` detail page.
- Workflow detail-pane integration (§6.5).
- Bulk add from triage (§6.6).
- Sidebar entry + counts.
- Audit verbs.

Acceptance: a user can create a project, add 10 issues from
triage, see them grouped by status on the detail page, edit
dates locally. **No GitHub board involvement yet.**

### Slice B — Board mirror, project-scoped

- Migration 0023 (`dp_project_board_links`) + 0024 (drop old
  link table).
- `/orgs/{org_id}/projects-v2` + `/projects/{id}/board-links`
  endpoints.
- Link-a-board dialog (§6.4) replacing the admin pane.
- Mirror fan-out in `OctocrabProjectV2Mirror`.
- Per-link `last_mirror_at` / `last_mirror_error` surfacing.
- Move old admin page to `#/admin/project-sync` (§9.4).

Acceptance: a user can link a Project to a GitHub board from
the project detail page, edit a date on one of its issues
from the workflow detail pane, see the date appear on the
GitHub board within ~2s, and see `Synced 14:23:07` in the
editor.

### Slice C — Polish (deferred, may overlap with slice 3
of linear-projects-idea.md)

- Reports `project_id` dimension (§10).
- Quick-pick autocomplete in `[+ Add to project]`.
- Search-by-name in `#/projects` list.
- Counts in sidebar via dedicated `count_only=1` query
  param (avoid pulling full rows).

---

## 13. Success criteria

- A team member who has never seen the admin page can create
  a project, add 5 issues from triage, and see progress —
  **without any documentation**. (Discoverability test.)
- Linking a GitHub board never requires copying a `PVT_…`
  node id from the GitHub UI in the primary path. (Picker
  test.)
- Issue date edits in the workflow detail pane mirror to
  every linked board within 5 seconds in 95% of cases. (Same
  SLO as the §3.10 mirror.)
- A project with 100 issues renders the detail page in
  < 500 ms after data lands. (Same SLO as the triage list.)
- Zero references to `repo_project_link` remain in the
  codebase after slice B ships. (Cleanup test.)

---

## 14. Open questions

1. **Cross-org projects**: out of scope per §4, but does
   NubeIO ever want a single "Q3 stability" project spanning
   `NubeIO` + `NubeDev` + `PJNube`? If yes, `org_id` becomes
   nullable + a new `dp_project_orgs` join. Defer until asked.
2. **Project tags**: should `dp_projects` participate in the
   §7 polymorphic tag system (so a project can carry tags like
   `release: 2026.06`)? Easy add (one row in `dp_tag_targets`
   per tagged project) — defer to slice C unless requested.
3. **Default visibility**: a project's `lead`'s org drives
   default visibility today. Do we want explicit
   `dp_project_acl` rows for cross-org viewers? Out of scope
   for v1; the §15.11 access banner already covers out-of-org
   users.
4. **Issue belongs to multiple projects**: **resolved** —
   v1 ships `UNIQUE (issue_id)` on `dp_project_issues` (§4,
   §5). Multi-project membership has no defined mirror
   semantics today (which board owns the date?) and the
   detail-pane chip is singular. The constraint is a single
   `ALTER TABLE … DROP CONSTRAINT` away when v2 wants to
   relax it; no destructive migration required.

---

## 15. Companion docs

- [linear-projects-idea.md](linear-projects-idea.md) — the
  triage workbench. §3.10 (the mirror that triggered this
  rewrite), §14 (the workflow detail pane this document
  extends).
- [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md) — pins, home-grown
  project tags, issue CRUD write surface, §8.2 CAS contract,
  §11 success criteria.
- [SCOPE.md](SCOPE.md) — the original product scope. This
  document continues the §3 "core experience" thread that
  was paused after the original §3.10 mirror landed.
