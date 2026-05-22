import type {
  PortfolioKpis,
  ProjectPortfolioRow,
} from "../../api/client.js";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "@/components/ui/chart";
import { Cell, Pie, PieChart } from "recharts";
import { cn } from "@/lib/utils";
import { projectDetailRoute } from "../../routes.js";
import { DUE_TONE_CLASSES } from "./portfolio-constants.js";
import { Legend, StackedStrip } from "./portfolio-kpis.js";

const STATUS_CHART_CONFIG: ChartConfig = {
  on_track: { label: "On track", color: "var(--chart-2)" },
  overdue: { label: "Overdue", color: "var(--chart-5)" },
  completed: { label: "Completed", color: "var(--chart-1)" },
};

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

export function PortfolioCharts({
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
      {/* 1. Status mix */}
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

      {/* 2. Slip distribution */}
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

      {/* 3. Top slippers */}
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
                    href={projectDetailRoute(r.id)}
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

      {/* 4. Open issues */}
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
