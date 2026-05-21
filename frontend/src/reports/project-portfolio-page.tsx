/**
 * Project portfolio report — `POST /reports/project-portfolio`.
 *
 * SCOPE-PROJECT-REPORTS.md. One row per visible project + a
 * portfolio-level KPI strip. v1 is a table (no Gantt, no editing —
 * see spec §4 non-goals). Drill-through to `#/projects/{id}`.
 *
 * URL params (spec §13):
 *   ?status=active,backlog        — comma-separated statuses
 *   ?sort=due_asc_nulls_last      — one of PortfolioSort
 *   ?hide_overdue=1               — toggle
 *   ?page=2                       — 1-based page
 *
 * `relativeDue` and `KpiTile` are tiny visual primitives intentionally
 * duplicated from [`project-detail-page.tsx`](../projects/project-detail-page.tsx)
 * rather than extracted into a shared module — the cost of extraction
 * (shared file, re-route imports, regression risk on the detail page)
 * is higher than the cost of two ~20-line helpers staying in step. If
 * a third caller appears, promote then.
 */

import { useMemo, useState, type ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Gantt, ViewMode, type Task as GanttTask } from "gantt-task-react";
import "gantt-task-react/dist/index.css";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { toast } from "sonner";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
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
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "@/components/ui/chart";
import { Cell, Pie, PieChart } from "recharts";
import { cn } from "@/lib/utils";
import { IconArrowUp, IconArrowDown } from "@tabler/icons-react";

import { api } from "../api/client.js";
import type {
  PortfolioKpis,
  PortfolioSort,
  ProjectPortfolioRequest,
  ProjectPortfolioRow,
  ProjectStatusDto,
} from "../api/client.js";
import { PageHeading } from "../components/page-heading.jsx";
import { Skeleton } from "../components/skeleton.jsx";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from "../components/empty.jsx";
import { navigate, projectDetailRoute, useRoute } from "../routes.js";

// ---------------------------------------------------------------------------
// URL ↔ request mapping
// ---------------------------------------------------------------------------

const VALID_SORTS: ReadonlySet<PortfolioSort> = new Set<PortfolioSort>([
  "due_asc_nulls_last",
  "due_desc_nulls_last",
  "slip_days_desc",
  "progress_asc",
  "name_asc",
  "updated_desc",
]);

const VALID_STATUSES: ReadonlySet<ProjectStatusDto> = new Set<ProjectStatusDto>([
  "active",
  "backlog",
  "done",
  "archived",
]);

const PAGE_SIZE = 50;

function buildRoute(params: URLSearchParams): string {
  const qs = params.toString();
  return qs ? `/reports/projects?${qs}` : `/reports/projects`;
}

function currentParams(route: string): URLSearchParams {
  const idx = route.indexOf("?");
  return new URLSearchParams(idx >= 0 ? route.slice(idx + 1) : "");
}

function pageFromParams(params: URLSearchParams): number {
  const raw = params.get("page");
  return raw ? Math.max(1, Number.parseInt(raw, 10) || 1) : 1;
}

function parseQuery(route: string): ProjectPortfolioRequest {
  const hashIdx = route.indexOf("?");
  const search = hashIdx >= 0 ? route.slice(hashIdx + 1) : "";
  const params = new URLSearchParams(search);

  const statusCsv = params.get("status");
  const statuses: ProjectStatusDto[] = statusCsv
    ? statusCsv
        .split(",")
        .map((s) => s.trim())
        .filter((s): s is ProjectStatusDto =>
          VALID_STATUSES.has(s as ProjectStatusDto),
        )
    : [];

  const sortRaw = params.get("sort");
  const sort: PortfolioSort =
    sortRaw && VALID_SORTS.has(sortRaw as PortfolioSort)
      ? (sortRaw as PortfolioSort)
      : "due_asc_nulls_last";

  const hide_overdue = params.get("hide_overdue") === "1";

  const pageRaw = params.get("page");
  const page = pageRaw ? Math.max(1, Number.parseInt(pageRaw, 10) || 1) : 1;
  const offset = (page - 1) * PAGE_SIZE;

  return {
    orgs: [],
    statuses,
    hide_overdue,
    sort,
    limit: PAGE_SIZE,
    offset,
  };
}

// ---------------------------------------------------------------------------
// Visual primitives — see header comment
// ---------------------------------------------------------------------------

function relativeDue(
  due: string | null | undefined,
  nowMs: number,
): { label: string; tone: "ok" | "soon" | "overdue" } | null {
  if (!due) return null;
  const target = new Date(due).getTime();
  if (Number.isNaN(target)) return null;
  const oneDay = 86_400_000;
  const days = Math.round((target - nowMs) / oneDay);
  if (days < 0) return { label: `${Math.abs(days)}d overdue`, tone: "overdue" };
  if (days === 0) return { label: "due today", tone: "soon" };
  if (days <= 7) return { label: `due in ${days}d`, tone: "soon" };
  return { label: `due in ${days}d`, tone: "ok" };
}

function KpiTile({
  label,
  value,
  hint,
}: {
  label: string;
  value: string | number;
  hint?: string;
}): JSX.Element {
  return (
    <Card className="gap-2 py-4">
      <CardHeader className="px-4">
        <CardTitle className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
          {label}
        </CardTitle>
      </CardHeader>
      <CardContent className="px-4">
        <div className="text-2xl font-semibold tabular-nums">{value}</div>
        {hint ? (
          <div className="text-xs text-muted-foreground">{hint}</div>
        ) : null}
      </CardContent>
    </Card>
  );
}

const DUE_TONE_CLASSES: Record<"ok" | "soon" | "overdue", string> = {
  ok: "border-transparent bg-emerald-100 text-emerald-900 dark:bg-emerald-900/40 dark:text-emerald-100",
  soon: "border-transparent bg-amber-100 text-amber-900 dark:bg-amber-900/40 dark:text-amber-100",
  overdue:
    "border-transparent bg-red-100 text-red-900 dark:bg-red-900/40 dark:text-red-100",
};

const STATUS_TONE: Record<ProjectStatusDto, string> = {
  active: "border-transparent bg-blue-100 text-blue-900 dark:bg-blue-900/40 dark:text-blue-100",
  backlog: "border-transparent bg-slate-200 text-slate-900 dark:bg-slate-700/60 dark:text-slate-100",
  done: "border-transparent bg-emerald-100 text-emerald-900 dark:bg-emerald-900/40 dark:text-emerald-100",
  archived: "border-transparent bg-muted text-muted-foreground",
};

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

export function ProjectPortfolioPage(): JSX.Element {
  const route = useRoute();
  const request = useMemo(() => parseQuery(route), [route]);

  const query = useQuery({
    queryKey: ["report-project-portfolio", request],
    queryFn: () => api.getReportProjectPortfolio(request),
  });

  const resp = query.data;
  const loading = query.isPending;
  const error = query.error?.message ?? null;

  const nowMs = resp ? new Date(resp.now).getTime() : Date.now();

  return (
    <div className="flex flex-col gap-4 px-4 md:gap-6 lg:px-6">
      <PageHeading
        title="Project portfolio"
        description={
          <>
            <code className="font-mono text-xs">
              POST /reports/project-portfolio
            </code>{" "}
            · which projects are on track, slipping, or done across every
            org you can see.
          </>
        }
      />

      {error ? (
        <Alert variant="destructive" data-testid="portfolio-error">
          <AlertDescription>
            Failed to load portfolio: {error}
          </AlertDescription>
        </Alert>
      ) : null}

      <div
        className="grid grid-cols-2 gap-3 md:grid-cols-4"
        data-testid="portfolio-kpis"
      >
        <KpiTile
          label="Total"
          value={resp?.kpis.total_projects ?? "—"}
          hint={
            resp ? `${resp.total} matching across all pages` : undefined
          }
        />
        <KpiTile
          label="On track"
          value={resp?.kpis.on_track ?? "—"}
          hint={
            resp
              ? `${resp.kpis.avg_progress_pct}% avg progress`
              : undefined
          }
        />
        <KpiTile
          label="Overdue"
          value={resp?.kpis.overdue ?? "—"}
          hint={
            resp
              ? `${resp.kpis.total_issues_overdue} open issues overdue`
              : undefined
          }
        />
        <KpiTile
          label="Completed"
          value={resp?.kpis.completed ?? "—"}
          hint={
            resp
              ? `${resp.kpis.total_issues_open} open issues remaining`
              : undefined
          }
        />
      </div>

      {resp && resp.kpis.total_projects > 0 ? (
        <PortfolioCharts kpis={resp.kpis} rows={resp.rows} />
      ) : null}

      <Tabs defaultValue="table" className="gap-3">
        <TabsList>
          <TabsTrigger value="table" data-testid="portfolio-tab-table">
            Table
          </TabsTrigger>
          <TabsTrigger value="gantt" data-testid="portfolio-tab-gantt">
            Gantt
          </TabsTrigger>
        </TabsList>
        <TabsContent value="table">
          <Card>
            <CardContent className="p-0">
              {loading ? (
                <div className="p-4">
                  <Skeleton className="h-8 w-full" />
                  <Skeleton className="mt-2 h-8 w-full" />
                  <Skeleton className="mt-2 h-8 w-full" />
                </div>
              ) : resp && resp.rows.length === 0 ? (
                <Empty data-testid="portfolio-empty">
                  <EmptyHeader>
                    <EmptyTitle>No projects to show</EmptyTitle>
                  </EmptyHeader>
                  <EmptyDescription>
                    Either no projects match the current filters, or you don't
                    have any projects yet.
                  </EmptyDescription>
                </Empty>
              ) : resp ? (
                <PortfolioTable
                  rows={resp.rows}
                  nowMs={nowMs}
                  sort={request.sort ?? "due_asc_nulls_last"}
                  route={route}
                />
              ) : null}
            </CardContent>
          </Card>
        </TabsContent>
        <TabsContent value="gantt">
          <Card>
            <CardContent className="p-2">
              {loading ? (
                <Skeleton className="h-[400px] w-full" />
              ) : resp && resp.rows.length > 0 ? (
                <PortfolioGantt rows={resp.rows} />
              ) : (
                <Empty data-testid="portfolio-gantt-empty">
                  <EmptyHeader>
                    <EmptyTitle>Nothing to plot</EmptyTitle>
                  </EmptyHeader>
                  <EmptyDescription>
                    No projects with timeline data in the current filter.
                  </EmptyDescription>
                </Empty>
              )}
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>

      {resp && resp.total > resp.limit ? (
        <PaginationFooter
          page={pageFromParams(currentParams(route))}
          pageSize={resp.limit}
          total={resp.total}
          route={route}
        />
      ) : null}
    </div>
  );
}

function PaginationFooter({
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

// ---------------------------------------------------------------------------
// Charts
// ---------------------------------------------------------------------------

const STATUS_CHART_CONFIG: ChartConfig = {
  on_track: { label: "On track", color: "var(--chart-2)" },
  overdue: { label: "Overdue", color: "var(--chart-5)" },
  completed: { label: "Completed", color: "var(--chart-1)" },
};

// Slip buckets — ordered worst → best so the stacked bar reads
// red-on-the-left like a heat scale.
const SLIP_BUCKETS = [
  { key: "deep_overdue", label: "Overdue >7d", color: "var(--chart-5)" },
  { key: "recent_overdue", label: "Overdue ≤7d", color: "var(--chart-4)" },
  { key: "soon", label: "Due ≤7d", color: "var(--chart-3)" },
  { key: "on_track", label: "On track", color: "var(--chart-2)" },
  { key: "undated", label: "No date", color: "var(--muted-foreground)" },
] as const;

type SlipBucket = (typeof SLIP_BUCKETS)[number]["key"];

function bucketOf(row: ProjectPortfolioRow): SlipBucket {
  if (row.status === "done") return "on_track";
  if (row.slip_days == null || row.slip_days === undefined) return "undated";
  if (row.slip_days < -7) return "deep_overdue";
  if (row.slip_days < 0) return "recent_overdue";
  if (row.slip_days <= 7) return "soon";
  return "on_track";
}

/** Compact 4-up visual rollup. Status mix · slip distribution · top
 *  slippers · open-issues strip. All derived from data already on the
 *  page — no extra endpoint. Hidden when the page has no rows because
 *  empty charts read as broken rather than honest. */
function PortfolioCharts({
  kpis,
  rows,
}: {
  kpis: PortfolioKpis;
  rows: ProjectPortfolioRow[];
}): JSX.Element {
  const statusData = [
    { key: "on_track", label: "On track", value: kpis.on_track, fill: "var(--chart-2)" },
    { key: "overdue", label: "Overdue", value: kpis.overdue, fill: "var(--chart-5)" },
    { key: "completed", label: "Completed", value: kpis.completed, fill: "var(--chart-1)" },
  ].filter((d) => d.value > 0);

  const slipCounts: Record<SlipBucket, number> = {
    deep_overdue: 0,
    recent_overdue: 0,
    soon: 0,
    on_track: 0,
    undated: 0,
  };
  for (const r of rows) slipCounts[bucketOf(r)]++;

  // Top slippers: most-negative slip_days first, drop projects that
  // aren't slipping. Cap at 3 so the card stays one screen.
  const topSlippers = rows
    .filter(
      (r) =>
        r.slip_days != null &&
        r.slip_days < 0 &&
        (r.status === "active" || r.status === "backlog"),
    )
    .sort((a, b) => (a.slip_days ?? 0) - (b.slip_days ?? 0))
    .slice(0, 3);

  const openOnTrack = Math.max(
    0,
    kpis.total_issues_open - kpis.total_issues_overdue,
  );
  const issueTotal = kpis.total_issues_open;

  return (
    <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
      {/* 1. Status mix — compact donut + inline numbers */}
      <Card className="py-3">
        <CardHeader className="px-4 pb-1">
          <CardTitle className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
            Status mix
          </CardTitle>
        </CardHeader>
        <CardContent className="flex items-center gap-3 px-4">
          <ChartContainer
            config={STATUS_CHART_CONFIG}
            className="aspect-square h-[80px] w-[80px] shrink-0"
          >
            <PieChart>
              <ChartTooltip
                cursor={false}
                content={<ChartTooltipContent hideLabel nameKey="label" />}
              />
              <Pie
                data={statusData}
                dataKey="value"
                nameKey="label"
                innerRadius={22}
                outerRadius={36}
                strokeWidth={1}
              >
                {statusData.map((d) => (
                  <Cell key={d.key} fill={d.fill} />
                ))}
              </Pie>
            </PieChart>
          </ChartContainer>
          <div className="flex flex-col gap-1 text-xs">
            {statusData.map((d) => (
              <Legend key={d.key} color={d.fill} label={d.label}>
                {d.value}
              </Legend>
            ))}
          </div>
        </CardContent>
      </Card>

      {/* 2. Slip distribution — the "shape of risk" sparkbar */}
      <Card className="py-3">
        <CardHeader className="px-4 pb-1">
          <CardTitle className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
            Slip distribution
          </CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col justify-center gap-2 px-4">
          <StackedStrip
            segments={SLIP_BUCKETS.map((b) => ({
              key: b.key,
              value: slipCounts[b.key],
              color: b.color,
            }))}
          />
          <div className="flex flex-wrap gap-x-3 gap-y-1 text-[11px]">
            {SLIP_BUCKETS.map((b) =>
              slipCounts[b.key] > 0 ? (
                <Legend key={b.key} color={b.color} label={b.label}>
                  {slipCounts[b.key]}
                </Legend>
              ) : null,
            )}
          </div>
        </CardContent>
      </Card>

      {/* 3. Top slippers — where things are slipping, exactly */}
      <Card className="py-3">
        <CardHeader className="px-4 pb-1">
          <CardTitle className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
            Top slippers
          </CardTitle>
        </CardHeader>
        <CardContent className="px-4">
          {topSlippers.length === 0 ? (
            <p className="text-xs text-muted-foreground">
              Nothing slipping. 🎉
            </p>
          ) : (
            <ul className="flex flex-col gap-1.5 text-xs">
              {topSlippers.map((r) => (
                <li
                  key={r.id}
                  className="flex items-center justify-between gap-2"
                >
                  <a
                    href={`#${projectDetailRoute(r.id)}`}
                    className="truncate font-medium hover:underline"
                    title={r.name}
                  >
                    {r.name}
                  </a>
                  <Badge
                    variant="outline"
                    className={cn(
                      "shrink-0 text-[10px]",
                      DUE_TONE_CLASSES.overdue,
                    )}
                  >
                    {Math.abs(r.slip_days ?? 0)}d overdue
                  </Badge>
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>

      {/* 4. Open issues — compressed strip */}
      <Card className="py-3">
        <CardHeader className="px-4 pb-1">
          <CardTitle className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
            Open issues
          </CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col justify-center gap-2 px-4">
          {issueTotal > 0 ? (
            <>
              <StackedStrip
                segments={[
                  {
                    key: "open_overdue",
                    value: kpis.total_issues_overdue,
                    color: "var(--chart-5)",
                  },
                  {
                    key: "open",
                    value: openOnTrack,
                    color: "var(--chart-3)",
                  },
                ]}
              />
              <div className="flex flex-wrap gap-x-3 gap-y-1 text-[11px]">
                <Legend color="var(--chart-5)" label="Overdue">
                  {kpis.total_issues_overdue}
                </Legend>
                <Legend color="var(--chart-3)" label="On track">
                  {openOnTrack}
                </Legend>
                <span className="ml-auto text-muted-foreground">
                  {kpis.avg_progress_pct}% closed
                </span>
              </div>
            </>
          ) : (
            <p className="text-xs text-muted-foreground">No open issues.</p>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

function StackedStrip({
  segments,
}: {
  segments: { key: string; value: number; color: string }[];
}): JSX.Element {
  const total = segments.reduce((a, s) => a + s.value, 0);
  if (total === 0) return <div className="h-6 rounded bg-muted" />;
  return (
    <div className="flex h-6 overflow-hidden rounded bg-muted">
      {segments.map((s) =>
        s.value > 0 ? (
          <div
            key={s.key}
            style={{
              width: `${(s.value / total) * 100}%`,
              background: s.color,
            }}
            aria-label={`${s.key}: ${s.value}`}
          />
        ) : null,
      )}
    </div>
  );
}

function Legend({
  color,
  label,
  children,
}: {
  color: string;
  label: string;
  children: ReactNode;
}): JSX.Element {
  return (
    <span className="flex items-center gap-1.5">
      <span aria-hidden className="size-2 rounded-sm" style={{ background: color }} />
      <span className="text-muted-foreground">{label}</span>
      <span className="tabular-nums">{children}</span>
    </span>
  );
}

// For each user-visible column the sort axis maps to an asc/desc pair.
const SORT_PAIRS: Partial<
  Record<"name" | "due" | "progress", [PortfolioSort, PortfolioSort]>
> = {
  name: ["name_asc", "name_asc"],
  due: ["due_asc_nulls_last", "due_desc_nulls_last"],
  progress: ["progress_asc", "progress_asc"],
};

function SortableHead({
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

// ---------------------------------------------------------------------------
// Gantt
// ---------------------------------------------------------------------------

/** When a project has only one side of the timeline set we still
 *  want to render *something* — a 14-day default window gives the
 *  bar a width without lying about the missing endpoint. The default
 *  is applied only at the visual layer; saving a drag uses whichever
 *  side the user actually moved (the other stays `null`). */
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

function PortfolioGantt({
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

function PortfolioTable({
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
          <TableHead>Lead</TableHead>
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
              className="cursor-pointer"
              onClick={() => {
                window.location.hash = projectDetailRoute(row.id);
              }}
            >
              <TableCell className="font-medium">
                <a
                  href={`#${projectDetailRoute(row.id)}`}
                  className="hover:underline"
                  onClick={(e) => e.stopPropagation()}
                >
                  {row.name}
                </a>
                {row.mirrored_to_github ? (
                  <Badge
                    variant="outline"
                    className="ml-2 text-[10px] uppercase tracking-wide"
                  >
                    mirrored
                  </Badge>
                ) : null}
              </TableCell>
              <TableCell className="text-muted-foreground">
                {row.org_login}
              </TableCell>
              <TableCell>
                <Badge
                  variant="outline"
                  className={cn("capitalize", STATUS_TONE[row.status])}
                >
                  {row.status}
                </Badge>
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
              <TableCell className="text-muted-foreground">
                {row.lead ? row.lead.login : "—"}
              </TableCell>
            </TableRow>
          );
        })}
      </TableBody>
    </Table>
  );
}
