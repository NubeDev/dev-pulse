/**
 * Stacked horizontal bar chart of the top contributors, segmented by
 * activity kind. Built directly on recharts (already a project dep)
 * via the shared `ChartContainer` / `ChartTooltip` primitives so the
 * tooltip and colour palette match the rest of the app.
 */

import { useMemo } from "react";
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts";

import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  ChartContainer,
  ChartLegend,
  ChartLegendContent,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "@/components/ui/chart";

import { ACTIVITY_KINDS } from "../activity-types.js";
import type { LeaderUserRow } from "./types.js";

export interface ContributorBarChartProps {
  rows: ReadonlyArray<LeaderUserRow>;
  /** How many rows to plot (default 10). */
  limit?: number;
  /** Only stack these kinds (defaults to every kind present in `rows`). */
  kinds?: ReadonlyArray<string>;
}

/** Cycle through the chart palette tokens. */
const PALETTE = [
  "var(--chart-1)",
  "var(--chart-2)",
  "var(--chart-3)",
  "var(--chart-4)",
  "var(--chart-5)",
  "var(--accent-info)",
  "var(--accent-success)",
  "var(--accent-warning)",
];

export function ContributorBarChart({
  rows,
  limit = 10,
  kinds,
}: ContributorBarChartProps): JSX.Element {
  const top = useMemo(() => rows.slice(0, limit), [rows, limit]);
  const activeKinds = useMemo(() => {
    if (kinds && kinds.length > 0) return kinds;
    const set = new Set<string>();
    for (const r of top) for (const k of Object.keys(r.perKind)) set.add(k);
    return ACTIVITY_KINDS.filter((k) => set.has(k.key)).map((k) => k.key);
  }, [top, kinds]);

  const config = useMemo<ChartConfig>(() => {
    const c: ChartConfig = {};
    activeKinds.forEach((k, i) => {
      c[k] = {
        label:
          ACTIVITY_KINDS.find((a) => a.key === k)?.label ?? k,
        color: PALETTE[i % PALETTE.length] ?? "var(--chart-1)",
      };
    });
    return c;
  }, [activeKinds]);

  const data = useMemo(
    () =>
      top.map((r) => {
        const row: Record<string, number | string> = { user: r.label };
        for (const k of activeKinds) row[k] = r.perKind[k] ?? 0;
        return row;
      }),
    [top, activeKinds],
  );

  return (
    <Card data-testid="leaderboard-bar-chart" className="@container/card">
      <CardHeader>
        <CardTitle>Top contributors</CardTitle>
        <CardDescription>
          Top {Math.min(limit, top.length)} ranked by total events, stacked by
          activity type.
        </CardDescription>
      </CardHeader>
      <CardContent className="px-2 pt-2 sm:px-6">
        {top.length === 0 ? (
          <p className="px-2 py-6 text-sm text-muted-foreground">
            No contributors in this window.
          </p>
        ) : (
          <ChartContainer
            config={config}
            className="aspect-auto w-full"
            style={{ height: `${Math.max(360, top.length * 28 + 80)}px` }}
          >
            <BarChart
              data={data}
              layout="vertical"
              margin={{ left: 12, right: 24, top: 8, bottom: 8 }}
            >
              <CartesianGrid horizontal={false} />
              <XAxis type="number" tickLine={false} axisLine={false} />
              <YAxis
                dataKey="user"
                type="category"
                tickLine={false}
                axisLine={false}
                width={140}
                tick={{ fontSize: 12 }}
              />
              <ChartTooltip
                cursor={{ fillOpacity: 0.08 }}
                content={<ChartTooltipContent indicator="dot" />}
              />
              {activeKinds.map((k) => (
                <Bar
                  key={k}
                  dataKey={k}
                  stackId="kinds"
                  fill={`var(--color-${k})`}
                  radius={[2, 2, 2, 2]}
                />
              ))}
              {activeKinds.length > 1 ? (
                <ChartLegend content={<ChartLegendContent />} />
              ) : null}
            </BarChart>
          </ChartContainer>
        )}
      </CardContent>
    </Card>
  );
}
