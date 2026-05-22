import type {
  PortfolioSort,
  ProjectPortfolioRow,
  ProjectStatusDto,
} from "../../api/client.js";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { IconArrowUp, IconArrowDown } from "@tabler/icons-react";
import { ArchiveIcon, CheckCircle2Icon, RotateCcwIcon } from "lucide-react";
import { Spinner } from "@/components/ui/spinner";
import { api } from "../../api/client.js";
import { navigate, projectDetailRoute } from "../../routes.js";
import { ProjectAvatar } from "../../projects/project-avatar.js";
import {
  useArchiveProject,
  usePatchProject,
} from "../../projects/use-projects-data.js";
import { DUE_TONE_CLASSES, SORT_PAIRS, STATUS_TONE } from "./portfolio-constants.js";
import { buildRoute, currentParams } from "./portfolio-url.js";
import { relativeDue } from "./portfolio-kpis.js";

export function SortableHead({
  label,
  axis,
  current,
  route,
  align,
}: {
  label: string;
  axis: "name" | "due" | "progress";
  current: PortfolioSort;
  route: string;
  align?: "right";
}): JSX.Element {
  const [asc, desc] = SORT_PAIRS[axis]!;
  const active = current === asc || current === desc;
  const isDesc = current === desc;
  const next: PortfolioSort = active && asc !== desc ? (isDesc ? asc : desc) : asc;
  const onClick = () => {
    const params = currentParams(route);
    params.set("sort", next);
    params.delete("page");
    navigate(buildRoute(params));
  };
  return (
    <TableHead className={align === "right" ? "text-right" : undefined}>
      <button
        type="button"
        onClick={onClick}
        className={cn(
          "inline-flex items-center gap-1 hover:text-foreground",
          active ? "text-foreground" : "text-muted-foreground",
        )}
        data-testid={`portfolio-sort-${axis}`}
      >
        {label}
        {active && asc !== desc ? (
          isDesc ? (
            <IconArrowDown size={14} />
          ) : (
            <IconArrowUp size={14} />
          )
        ) : null}
      </button>
    </TableHead>
  );
}

export function PortfolioTable({
  rows,
  nowMs,
  sort,
  route,
}: {
  rows: ProjectPortfolioRow[];
  nowMs: number;
  sort: PortfolioSort;
  route: string;
}): JSX.Element {
  return (
    <Table data-testid="portfolio-table">
      <TableHeader>
        <TableRow>
          <SortableHead label="Project" axis="name" current={sort} route={route} />
          <TableHead>Org</TableHead>
          <TableHead>Status</TableHead>
          <SortableHead label="Due" axis="due" current={sort} route={route} />
          <SortableHead
            label="Progress"
            axis="progress"
            current={sort}
            route={route}
            align="right"
          />
          <TableHead className="text-right">Open / Overdue</TableHead>
          <TableHead className="text-right">Tasks/Issues done</TableHead>
          <TableHead>Milestones</TableHead>
          <TableHead>Lead</TableHead>
          <TableHead className="w-[56px]" aria-label="Actions" />
        </TableRow>
      </TableHeader>
      <TableBody>
        {rows.map((row) => {
          const due = relativeDue(row.due_at, nowMs);
          const open = row.issue_count - row.closed_issue_count;
          return (
            <TableRow
              key={row.id}
              data-testid={`portfolio-row-${row.id}`}
              className="group cursor-pointer"
              onClick={() => {
                window.location.hash = projectDetailRoute(row.id);
              }}
            >
              <TableCell className="font-medium">
                <div className="flex min-w-0 items-center gap-3">
                  <ProjectAvatar id={row.id} name={row.name} />
                  <div className="min-w-0">
                    <a
                      href={`#${projectDetailRoute(row.id)}`}
                      className="block truncate hover:underline"
                      onClick={(e) => e.stopPropagation()}
                    >
                      {row.name}
                    </a>
                    {row.mirrored_to_github ? (
                      <span className="text-[10px] uppercase tracking-wide text-muted-foreground">
                        mirrored to GitHub
                      </span>
                    ) : null}
                  </div>
                </div>
              </TableCell>
              <TableCell className="text-muted-foreground">
                {row.org_login}
              </TableCell>
              <TableCell>
                <span
                  className={cn(
                    "inline-flex items-center rounded-md px-2 py-0.5 text-xs font-medium capitalize ring-1 ring-inset",
                    STATUS_TONE[row.status],
                  )}
                >
                  {row.status}
                </span>
              </TableCell>
              <TableCell>
                {due ? (
                  <Badge
                    variant="outline"
                    className={DUE_TONE_CLASSES[due.tone]}
                  >
                    {due.label}
                  </Badge>
                ) : (
                  <span className="text-muted-foreground">—</span>
                )}
              </TableCell>
              <TableCell className="text-right">
                <div className="flex items-center justify-end gap-2">
                  <span className="text-xs tabular-nums text-muted-foreground">
                    {row.progress_pct}%
                  </span>
                  <Progress value={row.progress_pct} className="w-20" />
                </div>
              </TableCell>
              <TableCell className="text-right tabular-nums">
                <span>{open}</span>
                {row.issue_overdue_count > 0 ? (
                  <span className="ml-2 text-red-600 dark:text-red-400">
                    ({row.issue_overdue_count} overdue)
                  </span>
                ) : null}
              </TableCell>
              <TableCell className="text-right">
                <ViewsDoneCell projectId={row.id} />
              </TableCell>
              <TableCell>
                <MilestonesCell projectId={row.id} />
              </TableCell>
              <TableCell className="text-muted-foreground">
                {row.lead ? row.lead.login : "—"}
              </TableCell>
              <TableCell
                className="text-right"
                onClick={(e) => e.stopPropagation()}
              >
                <PortfolioRowArchive row={row} />
              </TableCell>
            </TableRow>
          );
        })}
      </TableBody>
    </Table>
  );
}

function PortfolioRowArchive({
  row,
}: {
  row: ProjectPortfolioRow;
}): JSX.Element {
  const qc = useQueryClient();
  const isArchived = row.status === "archived";
  const archive = useArchiveProject(row.id);
  const patch = usePatchProject(row.id);
  const pending = archive.isPending || patch.isPending;

  const onClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    const opts = {
      onSuccess: () =>
        qc.invalidateQueries({ queryKey: ["report-project-portfolio"] }),
    };
    if (isArchived) {
      patch.mutate(
        { expected_version: row.version, status: "active" },
        opts,
      );
    } else {
      archive.mutate({ expected_version: row.version }, opts);
    }
  };

  const label = isArchived ? "Restore project" : "Archive project";
  const Icon = isArchived ? RotateCcwIcon : ArchiveIcon;

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant="ghost"
          size="icon"
          className="size-7 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100"
          disabled={pending}
          onClick={onClick}
          aria-label={label}
          data-testid={
            isArchived
              ? `portfolio-restore-${row.id}`
              : `portfolio-archive-${row.id}`
          }
        >
          {pending ? <Spinner /> : <Icon className="size-3.5" />}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

/**
 * Per-row "Views done" indicator. Counts project views where every
 * issue is closed (`open_issue_count === 0` with at least one issue
 * tracked) — i.e. the gate/view is finished. We fetch via the
 * existing `GET /projects/{id}/views` endpoint; react-query
 * de-duplicates and caches per project. Eventually this should roll
 * up server-side on `ProjectPortfolioRow` (avoiding N requests on
 * large portfolios), but for v1 demo scale (< 1000 rows, typically
 * < 20 visible) the per-row fetch is acceptable.
 */
function ViewsDoneCell({ projectId }: { projectId: string }): JSX.Element {
  const q = useQuery({
    queryKey: ["project-views-count", projectId],
    queryFn: () => api.listProjectViews(projectId),
    staleTime: 30_000,
  });
  if (q.isLoading) {
    return <span className="text-xs text-muted-foreground">…</span>;
  }
  if (q.isError || !q.data) {
    return <span className="text-xs text-muted-foreground">—</span>;
  }
  const views = q.data;
  const withIssues = views.filter(
    (v) => (v.total_issue_count ?? 0) > 0,
  );
  const total = withIssues.length;
  if (total === 0) {
    return <span className="text-xs text-muted-foreground">—</span>;
  }
  const done = withIssues.filter(
    (v) => (v.open_issue_count ?? 0) === 0,
  ).length;
  const allDone = done === total;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          className={cn(
            "inline-flex items-center gap-1 text-xs tabular-nums",
            allDone
              ? "text-emerald-600 dark:text-emerald-400"
              : "text-muted-foreground",
          )}
          data-testid={`portfolio-views-done-${projectId}`}
          data-complete={allDone ? "true" : "false"}
        >
          {allDone ? <CheckCircle2Icon className="size-3.5" /> : null}
          {done}/{total}
        </span>
      </TooltipTrigger>
      <TooltipContent>
        {allDone
          ? "All project views complete (every issue closed)"
          : `${done} of ${total} project views have every issue closed`}
      </TooltipContent>
    </Tooltip>
  );
}

/**
 * Per-row milestones summary. Shows up to three chips for the
 * project's open + recently-closed milestones, each coloured by
 * completion state (emerald when every attached issue is closed
 * or the milestone is formally closed). Same per-project fetch
 * pattern as `ViewsDoneCell` — react-query dedupes and caches.
 *
 * Order: open milestones first (so the next-due is leftmost),
 * then closed. We cap at three chips and show "+N" overflow to
 * keep the row height stable.
 */
function MilestonesCell({ projectId }: { projectId: string }): JSX.Element {
  const q = useQuery({
    queryKey: ["project-milestones-summary", projectId],
    queryFn: () => api.listProjectMilestones(projectId, true),
    staleTime: 30_000,
  });
  if (q.isLoading) {
    return <span className="text-xs text-muted-foreground">…</span>;
  }
  if (q.isError || !q.data || q.data.length === 0) {
    return <span className="text-xs text-muted-foreground">—</span>;
  }
  const all = q.data;
  // Open first, then closed; preserve API order within each group
  // (the backend orders by due date / number — good enough).
  const open = all.filter((m) => m.state === "open");
  const closed = all.filter((m) => m.state === "closed");
  const ordered = [...open, ...closed];
  const MAX = 3;
  const visible = ordered.slice(0, MAX);
  const overflow = ordered.length - visible.length;
  return (
    <div className="flex flex-wrap items-center gap-1">
      {visible.map((m) => {
        const total = m.open_issues + m.closed_issues;
        const allDone =
          m.state === "closed" ||
          (m.open_issues === 0 && m.closed_issues > 0);
        return (
          <Tooltip key={m.id}>
            <TooltipTrigger asChild>
              <span
                className={cn(
                  "inline-flex max-w-[10rem] items-center gap-1 truncate rounded-md px-1.5 py-0.5 text-xs ring-1 ring-inset",
                  allDone
                    ? "bg-emerald-50 text-emerald-700 ring-emerald-200 dark:bg-emerald-950/40 dark:text-emerald-300 dark:ring-emerald-900"
                    : "bg-muted text-muted-foreground ring-border",
                )}
                data-testid={`portfolio-milestone-${m.id}`}
                data-complete={allDone ? "true" : "false"}
              >
                {allDone ? (
                  <CheckCircle2Icon className="size-3 shrink-0" />
                ) : null}
                <span className="truncate">{m.title}</span>
                {total > 0 ? (
                  <span className="tabular-nums opacity-70">
                    {m.closed_issues}/{total}
                  </span>
                ) : null}
              </span>
            </TooltipTrigger>
            <TooltipContent>
              {m.title}
              {total > 0
                ? ` — ${m.closed_issues}/${total} issues closed`
                : ""}
              {m.due_on ? ` · due ${m.due_on}` : ""}
            </TooltipContent>
          </Tooltip>
        );
      })}
      {overflow > 0 ? (
        <span
          className="text-xs text-muted-foreground"
          data-testid={`portfolio-milestone-overflow-${projectId}`}
        >
          +{overflow}
        </span>
      ) : null}
    </div>
  );
}

export function PaginationFooter({
  page,
  pageSize,
  total,
  route,
}: {
  page: number;
  pageSize: number;
  total: number;
  route: string;
}): JSX.Element {
  const lastPage = Math.max(1, Math.ceil(total / pageSize));
  const params = currentParams(route);
  const goto = (next: number) => {
    if (next <= 1) params.delete("page");
    else params.set("page", String(next));
    navigate(buildRoute(params));
  };
  const startIdx = (page - 1) * pageSize + 1;
  const endIdx = Math.min(page * pageSize, total);
  return (
    <div
      className="flex items-center justify-between text-sm text-muted-foreground"
      data-testid="portfolio-pagination"
    >
      <span>
        Showing {startIdx}–{endIdx} of {total}
      </span>
      <div className="flex gap-2">
        <Button
          variant="outline"
          size="sm"
          disabled={page <= 1}
          onClick={() => goto(page - 1)}
        >
          Previous
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={page >= lastPage}
          onClick={() => goto(page + 1)}
        >
          Next
        </Button>
      </div>
    </div>
  );
}
