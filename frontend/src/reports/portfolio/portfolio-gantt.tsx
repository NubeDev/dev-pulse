import { useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Gantt, ViewMode, type Task as GanttTask } from "gantt-task-react";
import "gantt-task-react/dist/index.css";
import { Button } from "@/components/ui/button";
import { toast } from "sonner";
import { api } from "../../api/client.js";
import type { ProjectPortfolioRow } from "../../api/client.js";
import { projectDetailRoute } from "../../routes.js";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from "../../components/empty.jsx";

const GANTT_DEFAULT_SPAN_MS = 14 * 86_400_000;

const VIEW_MODES: { value: ViewMode; label: string }[] = [
  { value: ViewMode.Day, label: "Day" },
  { value: ViewMode.Week, label: "Week" },
  { value: ViewMode.Month, label: "Month" },
  { value: ViewMode.Year, label: "Year" },
];

type GanttRowMeta = {
  hasStart: boolean;
  hasDue: boolean;
  version: number;
};

function rowToGanttTask(row: ProjectPortfolioRow): {
  task: GanttTask;
  meta: GanttRowMeta;
} | null {
  const hasStart = !!row.start_at;
  const hasDue = !!row.due_at;
  if (!hasStart && !hasDue) return null;

  const startMs = row.start_at
    ? new Date(row.start_at).getTime()
    : new Date(row.due_at!).getTime() - GANTT_DEFAULT_SPAN_MS;
  const endMs = row.due_at
    ? new Date(row.due_at).getTime()
    : new Date(row.start_at!).getTime() + GANTT_DEFAULT_SPAN_MS;

  return {
    task: {
      id: row.id,
      type: "task",
      name: row.name,
      start: new Date(startMs),
      end: new Date(Math.max(endMs, startMs + 86_400_000)),
      progress: row.progress_pct,
      isDisabled: false,
    },
    meta: { hasStart, hasDue, version: row.version },
  };
}

export function PortfolioGantt({
  rows,
}: {
  rows: ProjectPortfolioRow[];
}): JSX.Element {
  const qc = useQueryClient();
  const [viewMode, setViewMode] = useState<ViewMode>(ViewMode.Month);

  const { tasks, metaById } = useMemo(() => {
    const out: GanttTask[] = [];
    const meta = new Map<string, GanttRowMeta>();
    for (const row of rows) {
      const mapped = rowToGanttTask(row);
      if (!mapped) continue;
      out.push(mapped.task);
      meta.set(row.id, mapped.meta);
    }
    return { tasks: out, metaById: meta };
  }, [rows]);

  const [overrides, setOverrides] = useState<Map<string, GanttTask>>(
    () => new Map(),
  );

  const displayed = useMemo(
    () => tasks.map((t) => overrides.get(t.id) ?? t),
    [tasks, overrides],
  );

  if (tasks.length === 0) {
    return (
      <Empty data-testid="portfolio-gantt-no-dates">
        <EmptyHeader>
          <EmptyTitle>No dates to plot</EmptyTitle>
        </EmptyHeader>
        <EmptyDescription>
          None of the visible projects have a start or due date set.
        </EmptyDescription>
      </Empty>
    );
  }

  const handleDateChange = async (task: GanttTask): Promise<boolean> => {
    const meta = metaById.get(task.id);
    if (!meta) return false;
    setOverrides((prev) => {
      const next = new Map(prev);
      next.set(task.id, task);
      return next;
    });
    try {
      await api.patchProject(task.id, {
        expected_version: meta.version,
        ...(meta.hasStart ? { start_at: task.start.toISOString() } : {}),
        ...(meta.hasDue ? { due_at: task.end.toISOString() } : {}),
      });
      toast.success(`Updated timeline for ${task.name}`);
      qc.invalidateQueries({ queryKey: ["report-project-portfolio"] });
      return true;
    } catch (err) {
      setOverrides((prev) => {
        const next = new Map(prev);
        next.delete(task.id);
        return next;
      });
      const msg = err instanceof Error ? err.message : "Update failed";
      toast.error(`Failed to update ${task.name}: ${msg}`);
      return false;
    }
  };

  return (
    <div className="flex flex-col gap-3" data-testid="portfolio-gantt">
      <div className="flex items-center gap-2 px-2">
        <span className="text-xs text-muted-foreground">View:</span>
        {VIEW_MODES.map((m) => (
          <Button
            key={m.value}
            size="sm"
            variant={viewMode === m.value ? "default" : "outline"}
            onClick={() => setViewMode(m.value)}
            data-testid={`portfolio-gantt-view-${m.value}`}
          >
            {m.label}
          </Button>
        ))}
        <span className="ml-auto text-xs text-muted-foreground">
          Drag a bar to adjust dates · click the name to open the project
        </span>
      </div>
      <Gantt
        tasks={displayed}
        viewMode={viewMode}
        listCellWidth="180px"
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
        ganttHeight={Math.min(
          600,
          Math.max(180, displayed.length * 44 + 8),
        )}
        preStepsCount={2}
        onDateChange={handleDateChange}
        onDoubleClick={(t) => {
          window.location.hash = projectDetailRoute(t.id);
        }}
      />
    </div>
  );
}
