---
title: Link boards & repos
description: Connect a dev-pulse project to GitHub Projects boards and repos.
---

# Link boards &amp; repos

A project is just a container until you connect it to GitHub. **Linked repos** are where
the project's issues and milestones actually live; **linked GitHub Projects boards** are
optional mirrors for the project's status. You manage both from the [detail page's](/projects/detail)
Settings menu.

> **Prerequisite:** the project must belong to the same GitHub org as the repos and boards
> you link. dev-pulse only shows repos/boards in that org.

## Why link repos

Until at least one repo is linked, the project page shows a red **"No linked repos"**
banner and:

- the [milestones strip](/projects/milestones) is empty (milestones come from linked repos),
- the [issue workbench](/projects/workbench) has no issues to show,
- most KPIs stay at zero.

Linking a repo is almost always the first thing you do on a new project.

## Linking a GitHub Projects board

From **Settings &rsaquo; + Link a board…** (in the **Boards** group), the **Link-a-board**
dialog lists the org's GitHub Projects v2 boards. Pick one to link. The link is what lets
dev-pulse mirror the project's status between its own model and the GitHub board.

Once linked:

- the board shows as a **blue badge** in the project header (clickable through to GitHub),
- it appears in the **Boards** section of the Settings menu with an **Unlink** action,
- the [Linked surfaces KPI](/projects/detail#linked-surfaces) count goes up.

To unlink, open **Settings** and click **Unlink** next to the board. Unlinking doesn't
delete anything on GitHub — it just stops dev-pulse tracking the board for this project.

## Managing repos

From **Settings &rsaquo; Repos &rsaquo; Manage repos…**, the repo picker lets you choose
which repos in the org this project draws from. Toggle repos on/off to control which ones
contribute issues and milestones.

Once linked:

- each repo shows as a **green badge** in the project header (clickable through to GitHub),
- the repo's issues become available to add to the [workbench](/projects/workbench),
- the repo's milestones appear on the [milestones strip](/projects/milestones),
- the **"No linked repos"** banner disappears.

## The mirror status

When dev-pulse syncs a linked board, each link shows a mirror status row:

- on success: `Last sync: 14:23:07 ✓`
- on failure: `Last sync: failed — <message>`

If a link is failing, use **Re-link** to unlink and re-open the link dialog and try again.

## What writes through to GitHub

Some actions on a linked project write **through to GitHub** (via the dev-pulse GitHub App,
or a personal access token if that's how your install is configured). Specifically:

- [milestone](/projects/milestones) create / edit / close / reopen / delete,
- dates mirrored to a linked GitHub Projects board.

Everything else (saved views, the lead, status, tags, the project's own name) lives only
in dev-pulse.

## Next

- [Create a project](/projects/create) — the step that usually comes right before this one.
- [Milestones](/projects/milestones) — what lights up once a repo is linked.
- [Issue workbench](/projects/workbench) — what to do with the issues once they're in.
