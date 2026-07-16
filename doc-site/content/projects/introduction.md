---
title: Projects introduction
description: What a dev-pulse project is, and where to find it in the app.
---

# Projects

A **project** in dev-pulse groups **issues across repos** that belong to the same GitHub
organisation. Use it to pull work that lives in many repositories — or in a GitHub
Projects board — into one place where you can track progress, set a lead and deadlines,
and report on delivery.

Every project belongs to exactly **one org**. A project pulls its issues from the
**repos** and **GitHub Projects boards** you link to it; those links decide what shows up
in the milestones strip and the issue workbench.

## Where to find projects

Open the **Projects** section in the left sidebar. It expands into:

- **Portfolio** — the cross-org report at `#/reports/projects`. The one place to see
  *all* your projects at a glance: which are on track, slipping, or done. Start here.
- **Active** / **Backlog** / **Done** / **Archived** — the same portfolio, pre-filtered by
  status. Active, Backlog, and Done carry a live count badge; Archived is left uncounted
  so the eye isn't drawn to the archive bin.

> The **Portfolio** sub-item opens `#/reports/projects`. That's the page these docs use as
> the front door to managing projects.

## The two main surfaces

1. **The portfolio report** (`#/reports/projects`) — every project in a table or Gantt
   view, filterable by status and tags, with KPI tiles summarising the whole set. Covered
   in [Project portfolio](/projects/portfolio).
2. **A single project's detail page** (`#/projects/{id}`) — one project's KPIs, its
   milestones strip, and the issue workbench. This is where day-to-day management happens.
   Covered in [Project detail page](/projects/detail).

## What a project is *not*

- A project is **not** a GitHub repo. It can pull from many repos in the same org.
- A project is **not** a person. It has an optional **lead** (a GitHub user accountable
  for it), but the work itself lives in issues.
- A project is **not** a team. Anyone who can see the org can see its projects; access
  is controlled by dev-pulse roles, not per-project.

## Lifecycle

A project has one of four statuses:

| Status   | Meaning                                                          |
| -------- | ---------------------------------------------------------------- |
| Active   | Currently being worked on. The default for new projects.        |
| Backlog  | Planned but not yet started.                                     |
| Done     | Completed.                                                       |
| Archived | Hidden from default views. Data, links, and issues are preserved.|

You can archive and restore a project from its detail page (see
[Project detail page &rsaquo; Settings](/projects/detail#settings-menu)). Archiving is
soft — nothing is deleted, and you can bring a project back at any time.

## Next

Start with [Project portfolio](/projects/portfolio) to read the room across all your
projects, or jump to [Create a project](/projects/create) if you're ready to add one.
