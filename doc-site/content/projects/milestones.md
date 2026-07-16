---
title: Milestones
description: The milestones strip on the project detail page — a GitHub write-through roadmap.
---

# Milestones

The **milestones strip** sits above the [issue workbench](/projects/workbench) on the
project detail page. It's a horizontal roadmap of the project's **GitHub milestones** —
closed milestones first, then open ones sorted by due date soonest-first.

> Milestones come from GitHub. They're the milestones on the **repos linked to this
> project**. If nothing is linked, the strip is empty — see
> [Link boards &amp; repos](/projects/link-boards-repos).

## How it reads

- **Closed milestones** come first, with a solid completed track segment behind them.
- **Open milestones** follow, soonest-due first.
- The **first open milestone** is the *in-progress* node — it carries a pulsing dot and a
  `Today` ticker below it.
- The whole strip can be **collapsed** with the **Milestones** toggle in the section
  header; the preference is remembered on this device.

## What each milestone shows

Each node displays the milestone title, its state, and its due date. The **`⋯` overflow
menu** on each node exposes:

- **Adopt as primary** / **Clear primary** — mark the milestone as the project's primary
  focus. The primary milestone is highlighted in the [portfolio](/projects/portfolio) and
  on this page.
- **Filter to milestone** — drops a `milestone:<id>` chip onto the workbench filter so the
  issue list narrows to just this milestone. Clicking a different milestone's filter
  *swaps* the chip (it doesn't stack) — one milestone filter at a time.
- **Edit** — opens the edit dialog (title, due date, description).
- **Close** / **Reopen** — flips the milestone's state on GitHub.
- **Delete** — removes the milestone on GitHub and drops the local mirror. **Issues stay**;
  only the milestone itself is removed. A confirm dialog makes this explicit. **Can't be
  undone.**

## Creating a milestone

The **+ New milestone** button sits below the track. The dialog writes the milestone
straight through to GitHub on the linked repo (via the dev-pulse GitHub App or a personal
access token). If no repo is linked, the dialog will point you at **Manage repos…**
instead.

> Milestones are repo-scoped on GitHub. When you create one, you pick which linked repo it
> lives on.

## Primary milestone

At most **one** milestone can be the project's primary at a time. Adopting a new one
clears the previous. The primary shows up wherever the project is summarised (e.g. the
portfolio row) so stakeholders can see "this project is currently working toward X".

## Toggling the strip

The **Milestones** button in the section header collapses and expands the whole strip. The
choice is saved per-browser under `dp:projects:milestones-collapsed`, so you can keep it
out of the way on detail pages where you only care about the workbench.

## Next

- [Issue workbench](/projects/workbench)
- [Link boards &amp; repos](/projects/link-boards-repos) (a prerequisite for milestones)
