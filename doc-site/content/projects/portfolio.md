---
title: Project portfolio
description: The cross-org project report at #/reports/projects — who is on track, slipping, or done.
---

# Project portfolio

**Route:** `#/reports/projects`

The **project portfolio** is the front door to managing projects. It shows every project
you can see across every org, with KPI tiles up top, a filter bar, and a **Table** or
**Gantt** view of the rows. Use it to answer "which of my projects are overdue right
now?" in one click — without opening individual projects.

Open it from the **Projects &rsaquo; Portfolio** item in the sidebar, or go straight to
`#/reports/projects`.

## The KPI strip

Four tiles summarise the whole filtered set:

| Tile       | What it counts                                                          |
| ---------- | ------------------------------------------------------------------------ |
| **Total**  | Projects matching the current filters (and the total across all pages). |
| **On track** | Projects not overdue, plus the average progress %.                    |
| **Overdue**| Projects past their due date. Turns red when the count is non-zero.      |
| **Completed** | Projects in **Done** status, and how many open issues remain.       |

## The filter bar

The portfolio is driven entirely by the **URL**, so any view you build is shareable and
bookmarkable. The filter bar writes these URL parameters for you:

- **Status** chips — `active`, `backlog`, `done`, `archived`. Pick one to switch to a
  grouped view (one sub-table per status); pick **All** to clear it and get a single
  flat table. The sidebar's Active / Backlog / Done / Archived items are just shortcuts
  here with a single status pre-selected.
- **Hide overdue** toggle — drops rows past their due date (`hide_overdue=1`).
- **Tags** — filter by org-scoped tags. Tags from different orgs are unioned; same-named
  tags are disambiguated by org login. **Match** any vs. all controls whether rows must
  carry one of the chosen tags or all of them (`tag_match=any|all`).

### URL parameters at a glance

| Param           | Example                       | Effect                                  |
| --------------- | ----------------------------- | --------------------------------------- |
| `status`        | `?status=active,backlog`      | CSV of statuses to include.             |
| `hide_overdue`  | `?hide_overdue=1`             | Hide rows past their due date.          |
| `tags`          | `?tags=<id1>,<id2>`           | CSV of tag ids to filter on.            |
| `tag_match`     | `?tag_match=all`              | `any` (default) or `all`.               |
| `sort`          | `?sort=due_asc_nulls_last`    | Column sort (default: due date, soonest-first, nulls last). |
| `page`          | `?page=2`                     | Page number for large portfolios.       |

> Saved state: **none** for now. The URL *is* the saved state — bookmark it or copy it
> into a chat to share exactly what you're looking at.

## The table

Each row is one project. The columns:

- **Project** — the row's click target; opens the project detail page (`#/projects/{id}`).
- **Org** — the GitHub org the project belongs to.
- **Status** — Active / Backlog / Done / Archived.
- **Due** — the due date, rendered as the same coloured pill used in the KPI tiles:
  `due in 6d`, `due today`, `5d overdue`.
- **Slip** — how far the due date has moved (uses the *current* `due_at`).
- **Progress** — a bar showing closed issues as a percentage of total.
- **Open / Overdue** — open issue count, with how many of those are overdue.
- **Lead** — the GitHub user accountable for the project (pseudonymised if the user has
  been soft-deleted).

### Grouped vs. flat

- **One status selected** → a single flat table (e.g. only Active projects).
- **No status or several statuses** → grouped sub-tables, one per status, each in the
  same column shape.

Click any column header that offers a sort affordance to change the ordering; the choice
is written to `?sort=`.

## The Gantt tab

Switch to **Gantt** to see the same rows plotted on a timeline of start → due. Projects
with no timeline data are omitted from the chart (but stay in the table). Use the Gantt
to eyeball scheduling collisions and which projects are stacked up in the same week.

## Pagination

If the matching set is larger than one page, a footer shows the page number, page size,
and total — and lets you step through pages (`?page=2`, `?page=3`, …). The design budget
is `total < 1000` projects; for most orgs the whole portfolio fits on a page.

## Create a project from here

The **+ New project** button in the page header opens the create dialog. On success you
drop straight onto the new project's detail page so you can link repos and add issues
right away. See [Create a project](/projects/create).

## Next

- [Create a project](/projects/create)
- [Project detail page](/projects/detail)
