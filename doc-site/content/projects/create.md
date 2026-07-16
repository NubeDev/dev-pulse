---
title: Create a project
description: How to create a new dev-pulse project — name, org, dates, lead.
---

# Create a project

Open the **+ New project** dialog from either:

- the **project portfolio** header (`#/reports/projects`), or
- the **Projects** section in the sidebar.

The dialog collects a few fields and then drops you onto the new project's detail page so
you can link repos and add issues immediately.

## Fields

| Field          | Required | Notes                                                                                  |
| -------------- | -------- | -------------------------------------------------------------------------------------- |
| **Name**       | Yes      | Up to 200 characters. Make it specific — it's the headline everywhere the project shows. |
| **Org**        | Yes      | The GitHub org this project belongs to. First visible org is pre-selected.             |
| **Description**| No       | Short summary of what the project delivers. Shown under the name on the detail page.   |
| **Start**      | No       | Date the project starts. Used by the Gantt and the Timeline KPI.                       |
| **Due**        | No       | Date the project is due. Drives the overdue/soon/ok pill.                              |
| **Lead**       | No       | A GitHub user accountable for the project. **Scoped to members of the chosen org.**    |

A few things to know up front:

- **Status defaults to Active.** There's no status field on create; set it to Backlog or
  Done later from the detail page if you need to.
- **The lead is org-scoped.** Switching the org clears the lead pick — a user who is a
  member of one org is, by definition, not a member of another. You can also leave the
  lead blank and assign it later from the [detail page's Lead tile](/projects/detail#lead).
- **Dates are whole days.** Pick a date from the calendar; it's stored as midnight UTC.
  Leave either field blank to leave it unset.
- **A board mirror is optional and happens later.** The description mentions you *can*
  mirror dates to a GitHub Projects v2 board — that's the [link-a-board](/projects/link-boards-repos)
  step, done after the project exists.

## After you click Create project

1. The dialog closes and the project is saved.
2. You're navigated to the new project's detail page (`#/projects/{id}`).
3. The very first thing you'll usually do is **link a repo** — until you do, the page
   shows a **"No linked repos"** warning and milestones + most surfaces stay empty. Hit
   **Link a repo…** in that warning, or open **Settings &rsaquo; Repos &rsaquo; Manage repos…**.
   See [Link boards &amp; repos](/projects/link-boards-repos).

## Editing later

Nothing here is permanent. From the detail page you can change the name and description
(**Edit details**), the start/due dates (the **Timeline** tile's pencil), the lead (the
**Lead** tile), and the status (archive/restore via the Settings menu). All of these go
through the same safe edit path that won't overwrite someone else's concurrent change.

## Next

- [Project detail page](/projects/detail)
- [Link boards &amp; repos](/projects/link-boards-repos)
