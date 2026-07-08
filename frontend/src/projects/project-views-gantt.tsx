/**
 * Project "Schedule" tab — a Gantt timeline of the project's saved
 * views (PROJECT-VIEW.md §5.4). Each view becomes one bar spanning
 * its `start_date → due_date`; dragging the bar (or its edges), or
 * editing the inline From/To date cells, writes the new dates back
 * through `PATCH /projects/{id}/views/{id}`.
 *
 * Views with no dates yet render in a *pending* state — a greyed,
 * draggable placeholder bar anchored on the project's own timeline.
 * Dropping it onto the chart (or typing a date into its row) commits
 * the dates and graduates the view to "scheduled".
 *
 * SINGLE SOURCE OF TRUTH: every mutation (drag or inline cell edit)
 * funnels through `commitDates`, which optimistically patches the
 * shared react-query `views` cache *before* the network round-trip.
 * The chart bars and the inline table both read from that one cache,
 * so they can never drift apart. There is no separate date-edit
 * dialog — the inline From/To cells are the editor, and a "hide
 * dates" toggle in the toolbar collapses the panel to just the name
 * column when the user wants more room for the chart.
 *
 * Adapted from the portfolio timeline
 * (`reports/portfolio/portfolio-gantt.tsx`) — same `gantt-task-react`
 * mechanics, but writing to the per-view date columns instead of the
 * project row.
 */

import { useMemo, useRef, useState } from "react";
import { Gantt, ViewMode, type Task as GanttTask } from "gantt-task-react";
import "gantt-task-react/dist/index.css";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { DateInput } from "@/components/ui/date-input";
import { cn } from "@/lib/utils";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from "../components/empty.jsx";
import type { ProjectDto } from "../api/client.js";
import type {
  ProjectViewDto,
  ProjectViewWriteBody,
} from "../api/schemas/projects.js";
import { navigate, projectDetailRouteWithParams } from "../routes.js";
import { gateMetaForName, orderGateViews } from "./icon-for-name.js";
import {
  projectsKeys,
  useProjectViews,
  useUpdateProjectView,
} from "./use-projects-data.js";

const DAY_MS = 86_400_000;
/** Default bar length when a view has only one date (or none). */
const DEFAULT_SPAN_MS = 14 * DAY_MS;

/** Column widths for the custom task-list table. The Name column is
 *  deliberately narrow ("minified") to give the editable date cells
 *  room — the full gate label is still available on hover (`title`). */
const COL_NAME_W = 152;
const COL_DATE_W = 150;

const VIEW_MODES: { value: ViewMode; label: string }[] = [
  { value: ViewMode.Day, label: "Day" },
  { value: ViewMode.Week, label: "Week" },
  { value: ViewMode.Month, label: "Month" },
  { value: ViewMode.Year, label: "Year" },
];

/** Amber palette for unscheduled (pending) bars — the app's
 *  "needs attention" tone (same family as local-only issue rows),
 *  so an unscheduled view is unmistakable against the default
 *  slate-blue scheduled bars. */
const PENDING_STYLES: GanttTask["styles"] = {
  backgroundColor: "#fbbf24",
  backgroundSelectedColor: "#f59e0b",
  progressColor: "#f59e0b",
  progressSelectedColor: "#d97706",
};

type RowMeta = {
  view: ProjectViewDto;
  hasStart: boolean;
  hasDue: boolean;
  pending: boolean;
};

/** Signature of the shared date-commit function, passed to the
 *  inline table through a ref so the memoised table stays stable. */
type CommitFn = (
  view: ProjectViewDto,
  startDate: string | null,
  dueDate: string | null,
) => Promise<boolean>;

/** `YYYY-MM-DD` → local-midnight Date (no TZ drift). */
function parseYmd(s: string): Date {
  return new Date(`${s}T00:00:00`);
}

/** Date → `YYYY-MM-DD` using local components (matches `parseYmd`). */
function toYmd(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/** Display label for a view. Gate views (`G1`…`G8`) expand to the
 *  full gate title — e.g. `G1 — Executive Summary` — so the Gantt
 *  reads like the wizard/strip rather than bare codes. Non-gate views
 *  keep their stored name. gantt-task-react also surfaces this in its
 *  default hover tooltip, so the full label shows on hover too. */
function viewLabel(v: ProjectViewDto): string {
  const meta = gateMetaForName(v.name);
  return meta ? `${v.name} — ${meta.tooltip}` : v.name;
}

function viewProgress(v: ProjectViewDto): number {
  const total = v.total_issue_count ?? 0;
  const open = v.open_issue_count ?? 0;
  if (total <= 0) return 0;
  return Math.round(((total - open) / total) * 100);
}

function viewToTask(
  v: ProjectViewDto,
  anchorMs: number,
): { task: GanttTask; meta: RowMeta } {
  const hasStart = !!v.start_date;
  const hasDue = !!v.due_date;
  const pending = !hasStart && !hasDue;

  let startMs: number;
  let endMs: number;
  if (pending) {
    startMs = anchorMs;
    endMs = anchorMs + DEFAULT_SPAN_MS;
  } else {
    startMs = v.start_date
      ? parseYmd(v.start_date).getTime()
      : parseYmd(v.due_date!).getTime() - DEFAULT_SPAN_MS;
    endMs = v.due_date
      ? parseYmd(v.due_date).getTime()
      : parseYmd(v.start_date!).getTime() + DEFAULT_SPAN_MS;
  }

  const label = viewLabel(v);
  return {
    task: {
      id: v.id,
      type: "task",
      name: pending ? `${label} · unscheduled` : label,
      start: new Date(startMs),
      end: new Date(Math.max(endMs, startMs + DAY_MS)),
      progress: viewProgress(v),
      isDisabled: false,
      ...(pending ? { styles: PENDING_STYLES } : {}),
    },
    meta: { view: v, hasStart, hasDue, pending },
  };
}

// ---------------------------------------------------------------------------
// Custom task-list (the left-hand Name / From / To grid). gantt-task-react
// measures this panel's real DOM width to offset the chart, so non-uniform
// column widths are safe. Both the header and the body rows use explicit
// `headerHeight` / `rowHeight` box-sizing so they line up with the chart's
// calendar header and grid rows exactly.
// ---------------------------------------------------------------------------

/** Build the task-list header. Reads `minimizedRef` so it can hide the
 *  From/To columns when the panel is collapsed to names-only; the
 *  library measures the resulting DOM width, so the chart offsets
 *  itself automatically. Like the table below, it is created through
 *  a factory + `useMemo` so its identity is stable across data
 *  changes (avoids needless reconciliation churn). */
function makeScheduleTaskListHeader(
  minimizedRef: React.MutableRefObject<boolean>,
): React.FC<{
  headerHeight: number;
  rowWidth: string;
  fontFamily: string;
  fontSize: string;
}> {
  return function ScheduleTaskListHeader({
    headerHeight,
    fontFamily,
    fontSize,
  }): JSX.Element {
    const minimized = minimizedRef.current;
    return (
      <div
        className="flex items-center border-b border-r border-border/70 bg-muted/40 text-xs font-medium text-muted-foreground"
        style={{ height: headerHeight, fontFamily, fontSize, boxSizing: "border-box" }}
        data-testid="project-views-gantt-table-header"
      >
        <div
          className="px-3"
          style={{ minWidth: COL_NAME_W, maxWidth: COL_NAME_W }}
        >
          Name
        </div>
        {!minimized && (
          <>
            <div
              className="px-2"
              style={{ minWidth: COL_DATE_W, maxWidth: COL_DATE_W }}
            >
              From
            </div>
            <div
              className="px-2"
              style={{ minWidth: COL_DATE_W, maxWidth: COL_DATE_W }}
            >
              To
            </div>
          </>
        )}
      </div>
    );
  };
}

/** Build a stable-identity table component. It reads the live view
 *  metadata, commit handler, minimise flag, and navigate handler
 *  through refs, so the component never needs to be re-created on
 *  data change — the inline date inputs keep focus across the
 *  optimistic re-render that each edit triggers. The library
 *  re-invokes it with fresh `tasks` whenever the cache updates, so
 *  the *rendered* rows still track the latest dates.
 *
 *  When `minimizedRef.current` is true the From/To cells are omitted
 *  entirely (the panel collapses to names-only); the user toggles
 *  that from the toolbar. The date edits still come from — and write
 *  to — the same `commitDates` source of truth as the chart bars, so
 *  the two surfaces can never disagree regardless of mode. */
function makeScheduleTaskListTable(
  metaRef: React.MutableRefObject<Map<string, RowMeta>>,
  commitRef: React.MutableRefObject<CommitFn>,
  minimizedRef: React.MutableRefObject<boolean>,
  navigateRef: React.MutableRefObject<(viewId: string) => void>,
): React.FC<{
  rowHeight: number;
  rowWidth: string;
  fontFamily: string;
  fontSize: string;
  locale: string;
  tasks: GanttTask[];
  selectedTaskId: string;
  setSelectedTask: (taskId: string) => void;
  onExpanderClick: (task: GanttTask) => void;
}> {
  return function ScheduleTaskListTable({ rowHeight, fontFamily, fontSize, tasks }) {
    const minimized = minimizedRef.current;
    return (
      <div style={{ fontFamily, fontSize }} data-testid="project-views-gantt-table">
        {tasks.map((t) => {
          const meta = metaRef.current.get(t.id);
          if (!meta) return null;
          const { view, pending } = meta;
          const label = viewLabel(view);
          return (
            <div
              key={t.id}
              className="flex items-center border-b border-r border-border/70"
              style={{ height: rowHeight, boxSizing: "border-box" }}
              data-testid={`project-views-gantt-row-${view.id}`}
            >
              <button
                type="button"
                onClick={() => navigateRef.current(view.id)}
                title={`Open “${label}”`}
                className={cn(
                  "truncate px-3 text-left text-sm hover:underline",
                  pending && "italic text-muted-foreground",
                )}
                style={{ minWidth: COL_NAME_W, maxWidth: COL_NAME_W }}
              >
                {label}
              </button>
              {!minimized && (
                <>
                  <div
                    className="px-2"
                    style={{ minWidth: COL_DATE_W, maxWidth: COL_DATE_W }}
                  >
                    <DateInput
                      aria-label={`${view.name} start date`}
                      data-testid={`project-views-gantt-start-${view.id}`}
                      value={view.start_date ?? ""}
                      onChange={(e) =>
                        void commitRef.current(
                          view,
                          e.target.value || null,
                          view.due_date ?? null,
                        )
                      }
                    />
                  </div>
                  <div
                    className="px-2"
                    style={{ minWidth: COL_DATE_W, maxWidth: COL_DATE_W }}
                  >
                    <DateInput
                      aria-label={`${view.name} due date`}
                      data-testid={`project-views-gantt-due-${view.id}`}
                      value={view.due_date ?? ""}
                      onChange={(e) =>
                        void commitRef.current(
                          view,
                          view.start_date ?? null,
                          e.target.value || null,
                        )
                      }
                    />
                  </div>
                </>
              )}
            </div>
          );
        })}
      </div>
    );
  };
}

export function ProjectViewsGantt({
  project,
}: {
  project: ProjectDto;
}): JSX.Element {
  const qc = useQueryClient();
  const views = useProjectViews(project.id);
  const updateView = useUpdateProjectView(project.id);
  const [viewMode, setViewMode] = useState<ViewMode>(ViewMode.Month);
  // Row ordering: "view" honours the saved per-view `position` (the
  // workbench tab-strip order); "date" sorts by the bar's start date.
  // Defaults to "view" — we never silently order by date.
  const [orderBy, setOrderBy] = useState<"view" | "date">("view");
  // Whether the editable From/To date columns are shown alongside the
  // name in the task-list panel. Collapsing to names-only gives the
  // chart more horizontal room; the dates can still be adjusted by
  // dragging the bars. Inline edits and drags share the same state.
  const [datesMinimized, setDatesMinimized] = useState(false);

  // Anchor for pending bars: the project's own start, else its due
  // (back-dated a span), else today. Keeps unscheduled views near the
  // part of the timeline the user is already looking at.
  const anchorMs = useMemo(() => {
    if (project.start_at) return new Date(project.start_at).getTime();
    if (project.due_at)
      return new Date(project.due_at).getTime() - DEFAULT_SPAN_MS;
    return Date.now();
  }, [project.start_at, project.due_at]);

  // Saved view order — MUST match the workbench tab strip. The
  // workbench does NOT order by the persisted `position`: gate views
  // (G1…G8) are POSTed out of order by the create wizard, so the strip
  // runs `orderGateViews()` to re-impose canonical G1→G8 by *name*
  // (non-gate views keep their stored position). We call the exact
  // same helper here so the Gantt rows line up with the strip — using
  // `position` instead is what made the rows look "out of order".
  const orderedViews = useMemo(
    () => orderGateViews(views.data ?? []),
    [views.data],
  );

  // Build the bars, then sort by the chosen mode. CRUCIAL: gantt-task-
  // react does NOT render in array order — it re-sorts internally by
  // each task's `displayOrder` (falling back to MAX_VALUE when unset,
  // which scrambles the rows). So we assign a 1-based `displayOrder`
  // matching our final order. 1-based because the library treats
  // `displayOrder || MAX_VALUE` as falsy for 0.
  const { tasks, metaById } = useMemo(() => {
    const entries = orderedViews.map((v) => viewToTask(v, anchorMs));
    if (orderBy === "date") {
      // Array.sort is stable, so equal start dates keep position order.
      entries.sort((a, b) => a.task.start.getTime() - b.task.start.getTime());
    }
    const out: GanttTask[] = [];
    const meta = new Map<string, RowMeta>();
    entries.forEach((e, i) => {
      const task = { ...e.task, displayOrder: i + 1 };
      out.push(task);
      meta.set(task.id, e.meta);
    });
    return { tasks: out, metaById: meta };
  }, [orderedViews, anchorMs, orderBy]);

  // Views with no dates yet — surfaced in the banner so they can be
  // scheduled with a click instead of having to find + drag the bar.
  const unscheduled = useMemo(
    () => orderedViews.filter((v) => !v.start_date && !v.due_date),
    [orderedViews],
  );

  /** Write start/due (either may be null) back to a view. Shared by the
   *  drag handler, the inline table cells, and the "Schedule" buttons /
   *  dialog. Optimistically patches the shared `views` cache first so
   *  every surface (bars, table, dialog) reflects the change instantly
   *  and stays consistent; rolls the cache back if the write fails. */
  const commitDates: CommitFn = async (view, startDate, dueDate) => {
    const key = projectsKeys.views(project.id);
    const prev = qc.getQueryData<ProjectViewDto[]>(key);
    qc.setQueryData<ProjectViewDto[]>(key, (old) =>
      (old ?? []).map((v) =>
        v.id === view.id
          ? { ...v, start_date: startDate, due_date: dueDate }
          : v,
      ),
    );

    const body: ProjectViewWriteBody = {
      name: view.name,
      group_by: view.group_by,
      filter_clauses: view.filter_clauses,
      sort: view.sort,
      categories: view.categories,
      start_date: startDate,
      due_date: dueDate,
    };
    try {
      await updateView.mutateAsync({ viewId: view.id, body });
      toast.success(`Updated schedule for ${view.name}`);
      return true;
    } catch (err) {
      // Roll the optimistic patch back so the UI matches the server.
      if (prev) qc.setQueryData(key, prev);
      const msg = err instanceof Error ? err.message : "Update failed";
      toast.error(`Failed to update ${view.name}: ${msg}`);
      return false;
    }
  };

  // Refs let the memoised (stable-identity) table read the latest
  // metadata / handlers without being re-created on every render —
  // essential so an inline date input keeps focus through the
  // optimistic re-render its own edit triggers.
  const metaRef = useRef(metaById);
  metaRef.current = metaById;
  const commitRef = useRef<CommitFn>(commitDates);
  commitRef.current = commitDates;
  const minimizedRef = useRef(datesMinimized);
  minimizedRef.current = datesMinimized;
  const navigateRef = useRef((viewId: string) =>
    navigate(projectDetailRouteWithParams(project.id, { view: viewId })),
  );
  navigateRef.current = (viewId: string) =>
    navigate(projectDetailRouteWithParams(project.id, { view: viewId }));

  const TaskListTable = useMemo(
    () =>
      makeScheduleTaskListTable(metaRef, commitRef, minimizedRef, navigateRef),
    [],
  );
  // `datesMinimized` is in the deps so the header is rebuilt on toggle
  // (the body reads `minimizedRef.current`, but the rebuild is what
  // actually forces a fresh render of the panel).
  const TaskListHeader = useMemo(
    () => makeScheduleTaskListHeader(minimizedRef),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [datesMinimized],
  );

  /** Schedule an unscheduled view at its suggested default span (the
   *  same window the greyed bar previews). The cache patch repaints it
   *  as a scheduled bar; the user can then drag to fine-tune. */
  const scheduleView = (view: ProjectViewDto): void => {
    void commitDates(
      view,
      toYmd(new Date(anchorMs)),
      toYmd(new Date(anchorMs + DEFAULT_SPAN_MS)),
    );
  };

  const scheduleAll = (): void => {
    for (const v of unscheduled) scheduleView(v);
  };

  const handleDateChange = async (task: GanttTask): Promise<boolean> => {
    const meta = metaById.get(task.id);
    if (!meta) return false;

    // Pending bars commit *both* dates (scheduling them for the first
    // time). Already-dated bars only rewrite the date(s) that existed,
    // matching the project-portfolio behaviour.
    const { view } = meta;
    const newStart =
      meta.pending || meta.hasStart ? toYmd(task.start) : view.start_date ?? null;
    const newDue =
      meta.pending || meta.hasDue ? toYmd(task.end) : view.due_date ?? null;

    // commitDates does the optimistic cache patch, so the bar snaps to
    // its dropped position immediately (and rolls back on failure).
    return commitDates(view, newStart, newDue);
  };

  if (views.isPending) {
    return (
      <p
        className="px-2 py-6 text-sm text-muted-foreground"
        data-testid="project-views-gantt-loading"
      >
        Loading schedule…
      </p>
    );
  }

  if ((views.data ?? []).length === 0) {
    return (
      <Empty data-testid="project-views-gantt-empty">
        <EmptyHeader>
          <EmptyTitle>No views to schedule</EmptyTitle>
        </EmptyHeader>
        <EmptyDescription>
          Saved views on this project show up here as a timeline. Create
          a view on the Workbench tab, then drag it onto the schedule.
        </EmptyDescription>
      </Empty>
    );
  }

  return (
    <div className="flex flex-col gap-3" data-testid="project-views-gantt">
      {unscheduled.length > 0 && (
        <div
          className="flex flex-col gap-2 rounded-md border border-amber-300 bg-amber-50 px-3 py-2.5 dark:border-amber-900/60 dark:bg-amber-950/30"
          data-testid="project-views-gantt-unscheduled"
        >
          <div className="flex flex-wrap items-center justify-between gap-2">
            <span className="text-sm font-medium text-amber-800 dark:text-amber-200">
              {unscheduled.length} view{unscheduled.length === 1 ? "" : "s"}{" "}
              not scheduled
            </span>
            <Button
              size="sm"
              variant="outline"
              className="border-amber-400 text-amber-800 hover:bg-amber-100 dark:text-amber-200"
              disabled={updateView.isPending}
              onClick={scheduleAll}
              data-testid="project-views-gantt-schedule-all"
            >
              Schedule all
            </Button>
          </div>
          <p className="text-xs text-amber-700 dark:text-amber-300/80">
            Click Schedule to drop a view onto the timeline at the
            suggested dates, then drag the bar (or edit the From/To
            cells) to fine-tune — or drag the amber bar directly.
          </p>
          <div className="flex flex-wrap gap-2">
            {unscheduled.map((v) => (
              <Button
                key={v.id}
                size="sm"
                variant="outline"
                className="h-7 gap-1.5 border-amber-300 bg-white/60 text-xs text-amber-900 hover:bg-amber-100 dark:bg-transparent dark:text-amber-100"
                disabled={updateView.isPending}
                onClick={() => scheduleView(v)}
                data-testid={`project-views-gantt-schedule-${v.id}`}
                title={`Schedule "${v.name}" at the suggested dates`}
              >
                <span className="truncate max-w-[16rem]">{viewLabel(v)}</span>
                <span className="opacity-70">· Schedule</span>
              </Button>
            ))}
          </div>
        </div>
      )}
      <div className="flex flex-wrap items-center gap-2 px-2">
        <span className="text-xs text-muted-foreground">Order by:</span>
        <Button
          size="sm"
          variant={orderBy === "view" ? "default" : "outline"}
          onClick={() => setOrderBy("view")}
          data-testid="project-views-gantt-order-view"
          title="Keep the saved view order (the workbench tab-strip order)"
        >
          View
        </Button>
        <Button
          size="sm"
          variant={orderBy === "date" ? "default" : "outline"}
          onClick={() => setOrderBy("date")}
          data-testid="project-views-gantt-order-date"
          title="Sort rows by start date"
        >
          Date
        </Button>

        <span className="ml-3 text-xs text-muted-foreground">Zoom:</span>
        {VIEW_MODES.map((m) => (
          <Button
            key={m.value}
            size="sm"
            variant={viewMode === m.value ? "default" : "outline"}
            onClick={() => setViewMode(m.value)}
            data-testid={`project-views-gantt-zoom-${m.value}`}
          >
            {m.label}
          </Button>
        ))}
        <Button
          size="sm"
          variant="outline"
          className="ml-3"
          onClick={() => setDatesMinimized((v) => !v)}
          data-testid="project-views-gantt-toggle-dates"
          title={
            datesMinimized
              ? "Show the From/To date columns in the task list"
              : "Hide the From/To date columns (chart bars still show dates)"
          }
        >
          {datesMinimized ? "Show dates" : "Hide dates"}
        </Button>
        <span className="ml-auto text-xs text-muted-foreground">
          {datesMinimized
            ? "Dates hidden — drag a bar to adjust, or show dates to edit inline"
            : "Edit dates inline or drag a bar · amber bars are unscheduled"}
        </span>
      </div>
      <Gantt
        tasks={tasks}
        viewMode={viewMode}
        listCellWidth={
          datesMinimized
            ? `${COL_NAME_W}px`
            : `${COL_NAME_W + COL_DATE_W * 2}px`
        }
        TaskListHeader={TaskListHeader}
        TaskListTable={TaskListTable}
        columnWidth={
          viewMode === ViewMode.Year
            ? 240
            : viewMode === ViewMode.Month
              ? 160
              : viewMode === ViewMode.Week
                ? 120
                : 80
        }
        rowHeight={44}
        barCornerRadius={4}
        ganttHeight={Math.min(600, Math.max(180, tasks.length * 44 + 8))}
        preStepsCount={2}
        onDateChange={handleDateChange}
      />
    </div>
  );
}
