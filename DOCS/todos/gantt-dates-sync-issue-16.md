# TODO — Gantt chart date dialog sync (issue #16)

GitHub: https://github.com/NubeDev/dev-pulse/issues/16

## Resolved

- [x] **Removed the per-view "edit dates" popout dialog
  (`EditViewDatesDialog`).** The dialog held its own local
  `startDate` / `dueDate` state which could drift from the chart bars
  after a drag (the original bug). The inline From/To cells in the
  task-list panel are now the *only* date editor, and both they and
  the drag handler write through the single `commitDates` source of
  truth (optimistic react-query cache patch), so the chart and the
  editor can never disagree.

  Changed in
  [frontend/src/projects/project-views-gantt.tsx](../../frontend/src/projects/project-views-gantt.tsx):
  deleted `EditViewDatesDialog` + its instantiation; removed the
  bar `onClick` that opened it; the name cell now navigates straight
  to the view (replacing the dialog's "Open view" action).

- [x] **Added a "Hide dates" / "Show dates" toggle** in the toolbar
  (`project-views-gantt-toggle-dates`). Collapsing hides the editable
  From/To columns in the task-list panel (names-only) and gives the
  chart more horizontal room. The chart bars still show/adjust dates
  via dragging while hidden. The toggle reuses the same `commitDates`
  state as the gantt, so showing dates again reflects whatever the
  bars were dragged to.

  Implementation: a `datesMinimized` state feeds a `minimizedRef`
  that the header and table factories read; `listCellWidth` switches
  between `COL_NAME_W` (minimised) and `COL_NAME_W + 2*COL_DATE_W`
  (expanded) so the library re-measures the panel width.

## Notes

- The project-level "Edit timeline" dialog in
  [frontend/src/projects/project-detail-page.tsx](../../frontend/src/projects/project-detail-page.tsx)
  (`EditDatesDialog`, edits the *project's* `start_at`/`due_at`) was
  intentionally left in place — it edits a different entity (the
  project, not a view) and was not the subject of the sync bug.
