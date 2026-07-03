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
 * SINGLE SOURCE OF TRUTH: every mutation (drag, inline cell edit,
 * dialog Save) funnels through `commitDates`, which optimistically
 * patches the shared react-query `views` cache *before* the network
 * round-trip. The chart bars, the inline table, and the edit dialog
 * all read from that one cache, so they can never drift apart — the
 * earlier bug (drag updated a private `overrides` map + the dialog
 * held a stale snapshot, so the dialog showed pre-drag dates until a
 * full page refresh) is structurally impossible now.
 *
 * Adapted from the portfolio timeline
 * (`reports/portfolio/portfolio-gantt.tsx`) — same `gantt-task-react`
 * mechanics, but writing to the per-view date columns instead of the
 * project row.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { Gantt, ViewMode, type Task as GanttTask } from "gantt-task-react";
import "gantt-task-react/dist/index.css";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { DateInput } from "@/components/ui/date-input";
import { Label } from "@/components/ui/label";
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

/** Header row — mirrors the chart's `headerHeight`. */
function ScheduleTaskListHeader({
  headerHeight,
  fontFamily,
  fontSize,
}: {
  headerHeight: number;
  rowWidth: string;
  fontFamily: string;
  fontSize: string;
}): JSX.Element {
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
    </div>
  );
}

/** Build a stable-identity table component. It reads the live view
 *  metadata and commit handler through refs, so the component never
 *  needs to be re-created on data change — the inline date inputs keep
 *  focus across the optimistic re-render that each edit triggers. The
 *  library re-invokes it with fresh `tasks` whenever the cache updates,
 *  so the *rendered* rows still track the latest dates. */
function makeScheduleTaskListTable(
  metaRef: React.MutableRefObject<Map<string, RowMeta>>,
  commitRef: React.MutableRefObject<CommitFn>,
  openRef: React.MutableRefObject<(viewId: string) => void>,
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
                onClick={() => openRef.current(view.id)}
                title={label}
                className={cn(
                  "truncate px-3 text-left text-sm hover:underline",
                  pending && "italic text-muted-foreground",
                )}
                style={{ minWidth: COL_NAME_W, maxWidth: COL_NAME_W }}
              >
                {label}
              </button>
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
  // The view whose dates are being edited in the click-to-edit dialog.
  // Stored by *id* (not a snapshot object) so the dialog always renders
  // from the freshest cached view — this is what keeps it in sync with a
  // just-dragged / just-typed date instead of showing a stale snapshot.
  const [editViewId, setEditViewId] = useState<string | null>(null);

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

  // The view currently open in the dialog, resolved live from the cache
  // (never a captured snapshot). After any edit the cache updates and
  // this re-resolves to the fresh dates automatically.
  const editView = editViewId
    ? metaById.get(editViewId)?.view ?? null
    : null;

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
  const openRef = useRef((viewId: string) => setEditViewId(viewId));
  openRef.current = (viewId: string) => setEditViewId(viewId);

  const TaskListTable = useMemo(
    () => makeScheduleTaskListTable(metaRef, commitRef, openRef),
    [],
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
        <span className="ml-auto text-xs text-muted-foreground">
          Edit dates inline or drag a bar · click a name to open · amber
          bars are unscheduled
        </span>
      </div>
      <Gantt
        tasks={tasks}
        viewMode={viewMode}
        listCellWidth={`${COL_NAME_W}px`}
        TaskListHeader={ScheduleTaskListHeader}
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
        onClick={(t) => {
          const m = metaById.get(t.id);
          if (m) setEditViewId(m.view.id);
        }}
      />

      <EditViewDatesDialog
        view={editView}
        saving={updateView.isPending}
        suggestedStart={toYmd(new Date(anchorMs))}
        suggestedDue={toYmd(new Date(anchorMs + DEFAULT_SPAN_MS))}
        onOpenChange={(open) => {
          if (!open) setEditViewId(null);
        }}
        onSave={(start, due) =>
          editView
            ? commitDates(editView, start, due)
            : Promise.resolve(false)
        }
        onOpenView={() => {
          if (editView) {
            navigate(
              projectDetailRouteWithParams(project.id, { view: editView.id }),
            );
          }
        }}
      />
    </div>
  );
}

/**
 * Click-to-edit date picker for a single view's schedule. Mirrors the
 * project-level "Edit timeline" dialog (Start / Due `DateInput`s,
 * blank clears the field). Clearing both dates un-schedules the view —
 * it returns to the amber "pending" state on the chart.
 *
 * `view` is resolved live from the query cache by the parent, so the
 * inputs are seeded from the freshest dates each time the dialog opens.
 * We seed once per opened view (`seededIdRef`) so a background refetch
 * of the same view can't clobber whatever the user is mid-typing.
 */
function EditViewDatesDialog({
  view,
  saving,
  suggestedStart,
  suggestedDue,
  onOpenChange,
  onSave,
  onOpenView,
}: {
  view: ProjectViewDto | null;
  saving: boolean;
  suggestedStart: string;
  suggestedDue: string;
  onOpenChange: (open: boolean) => void;
  onSave: (start: string | null, due: string | null) => Promise<boolean>;
  onOpenView: () => void;
}): JSX.Element {
  const [startDate, setStartDate] = useState("");
  const [dueDate, setDueDate] = useState("");
  const seededIdRef = useRef<string | null>(null);

  // Seed the inputs when a view is opened (id transition), not on every
  // object-identity change — so an optimistic patch / refetch of the
  // same view doesn't wipe an in-progress edit. An unscheduled view
  // pre-fills the suggested span so the user can just hit Save; a
  // scheduled view shows its real dates.
  useEffect(() => {
    if (!view) {
      seededIdRef.current = null;
      return;
    }
    if (seededIdRef.current === view.id) return;
    seededIdRef.current = view.id;
    setStartDate(view.start_date ?? suggestedStart);
    setDueDate(view.due_date ?? suggestedDue);
  }, [view, suggestedStart, suggestedDue]);

  const rangeError =
    startDate && dueDate && parseYmd(startDate) > parseYmd(dueDate)
      ? "Start must be on or before Due."
      : null;

  const onSubmit = async (e: React.FormEvent): Promise<void> => {
    e.preventDefault();
    if (rangeError) return;
    const ok = await onSave(startDate || null, dueDate || null);
    if (ok) onOpenChange(false);
  };

  return (
    <Dialog open={view !== null} onOpenChange={onOpenChange}>
      <DialogContent
        className="sm:max-w-md"
        data-testid="edit-view-dates-dialog"
      >
        <DialogHeader>
          <DialogTitle>
            Schedule “{view ? viewLabel(view) : ""}”
          </DialogTitle>
          <DialogDescription>
            Set this view's start and due dates. Leave both blank to
            un-schedule it (it returns to the amber pending state).
          </DialogDescription>
        </DialogHeader>

        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => void onSubmit(e)}
        >
          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-2">
              <Label htmlFor="edit-view-start">Start</Label>
              <DateInput
                id="edit-view-start"
                data-testid="edit-view-start"
                value={startDate}
                onChange={(e) => setStartDate(e.target.value)}
              />
            </div>
            <div className="flex flex-col gap-2">
              <Label htmlFor="edit-view-due">Due</Label>
              <DateInput
                id="edit-view-due"
                data-testid="edit-view-due"
                value={dueDate}
                onChange={(e) => setDueDate(e.target.value)}
              />
            </div>
          </div>

          {rangeError && (
            <p className="text-xs text-destructive">{rangeError}</p>
          )}

          <DialogFooter className="items-center sm:justify-between">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="text-muted-foreground"
              onClick={onOpenView}
              data-testid="edit-view-open"
            >
              Open view →
            </Button>
            <div className="flex gap-2">
              <Button
                type="button"
                variant="ghost"
                onClick={() => onOpenChange(false)}
                disabled={saving}
              >
                Cancel
              </Button>
              <Button
                type="submit"
                data-testid="edit-view-save"
                disabled={saving || rangeError !== null}
              >
                {saving ? "Saving…" : "Save"}
              </Button>
            </div>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
