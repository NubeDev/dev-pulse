/**
 * Donut chart breaking the selected activity into its constituent
 * `EventKind`s. Sits next to the contributor bar chart in the
 * dashboard's chart row.
 */

import { useMemo } from "react";
import { Cell, Pie, PieChart } from "recharts";

import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "@/components/ui/chart";

import type { MixSlice } from "./types.js";

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

export interface ActivityMixChartProps {
  mix: ReadonlyArray<MixSlice>;
  /** Headline number rendered in the centre of the donut. */
  grandTotal: number;
}

export function ActivityMixChart({
  mix,
  grandTotal,
}: ActivityMixChartProps): JSX.Element {
  const config = useMemo<ChartConfig>(() => {
    const c: ChartConfig = {};
    mix.forEach((slice, i) => {
      c[slice.kind] = {
        label: slice.label,
        color: PALETTE[i % PALETTE.length] ?? "var(--chart-1)",
      };
    });
    return c;
  }, [mix]);

  const data = useMemo(
    () =>
      mix
        .filter((s) => s.count > 0)
        .map((s, i) => ({
          name: s.label,
          value: s.count,
          fill: PALETTE[i % PALETTE.length] ?? "var(--chart-1)",
        })),
    [mix],
  );

  const top = data[0];
  const topShare = top && grandTotal > 0
    ? ((top.value / grandTotal) * 100).toFixed(1)
    : null;

  return (
    <Card data-testid="leaderboard-mix-chart" className="@container/card">
      <CardHeader>
        <CardTitle>Activity mix</CardTitle>
        <CardDescription>
          Share of total events by activity type.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col items-center gap-4 px-2 pt-2 sm:px-6">
        {data.length === 0 ? (
          <p className="px-2 py-6 text-sm text-muted-foreground">
            No events to break down yet.
          </p>
        ) : (
          <>
            <ChartContainer
              config={config}
              className="mx-auto aspect-square h-[260px] w-full max-w-[260px]"
            >
              <PieChart>
                <ChartTooltip
                  cursor={false}
                  content={<ChartTooltipContent hideLabel />}
                />
                <Pie
                  data={data}
                  dataKey="value"
                  nameKey="name"
                  innerRadius={70}
                  outerRadius={110}
                  paddingAngle={2}
                  strokeWidth={1}
                >
                  {data.map((entry, idx) => (
                    <Cell key={`slice-${idx}`} fill={entry.fill} />
                  ))}
                </Pie>
              </PieChart>
            </ChartContainer>
            <div className="text-center">
              <div className="text-2xl font-semibold tabular-nums">
                {grandTotal.toLocaleString()}
              </div>
              <div className="text-xs text-muted-foreground">
                total events
                {top && topShare
                  ? ` · ${top.name} leads with ${topShare}%`
                  : ""}
              </div>
            </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}
