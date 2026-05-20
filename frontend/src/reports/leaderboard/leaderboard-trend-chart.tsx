/**
 * Daily-trend area chart for the whole leaderboard selection.
 * Reuses the shared `ChartAreaInteractive` from `components/`, which
 * is the same component the user / team / org pages render.
 */

import { useMemo } from "react";

import { ChartAreaInteractive } from "@/components/chart-area-interactive";
import type { ChartConfig } from "@/components/ui/chart";

import type { TrendBucket } from "./types.js";

const CONFIG: ChartConfig = {
  events: { label: "Events", color: "var(--accent-info)" },
};

export interface LeaderboardTrendChartProps {
  trend: ReadonlyArray<TrendBucket>;
}

export function LeaderboardTrendChart({
  trend,
}: LeaderboardTrendChartProps): JSX.Element {
  const data = useMemo(
    () => trend.map((t) => ({ date: t.date, events: t.events })),
    [trend],
  );
  return (
    <ChartAreaInteractive
      title="Activity trend"
      description="Daily events across the selected orgs, users and activity types."
      data={data}
      config={CONFIG}
      testId="leaderboard-trend-chart"
    />
  );
}
