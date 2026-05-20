/**
 * KPI tile strip for the leaderboard — reuses the dashboard-01
 * `SectionCards` block from the shared components so the visual
 * rhythm matches the user / team / org report pages.
 */

import { useMemo } from "react";

import { SectionCards, type SectionCard } from "@/components/section-cards";

import type { LeaderboardData } from "./types.js";

export interface LeaderboardKpisProps {
  data: LeaderboardData;
  /** Number of selected orgs — drives the "Across N orgs" footer. */
  orgCount: number;
}

export function LeaderboardKpis({
  data,
  orgCount,
}: LeaderboardKpisProps): JSX.Element {
  const cards = useMemo<SectionCard[]>(() => {
    const top = data.rows[0];
    const second = data.rows[1];
    const topShare =
      data.grandTotal > 0 && top
        ? ((top.total / data.grandTotal) * 100).toFixed(1)
        : null;
    const gap =
      top && second ? top.total - second.total : top ? top.total : 0;

    const result: SectionCard[] = [
      {
        description: "Total events",
        value: data.grandTotal.toLocaleString(),
        footerTitle: `${data.activeContributors.toLocaleString()} contributors`,
        footerDescription:
          orgCount === 0
            ? "Pick at least one org to load."
            : `Summed across ${orgCount} org${orgCount === 1 ? "" : "s"}.`,
        testId: "kpi-total",
      },
      {
        description: "Top contributor",
        value: top?.label ?? "—",
        footerTitle: top ? `${top.total.toLocaleString()} events` : "No data",
        footerDescription: topShare ? `${topShare}% of all activity` : "",
        testId: "kpi-top-contributor",
      },
      {
        description: "Lead over #2",
        value: gap.toLocaleString(),
        footerTitle: second ? `vs ${second.label}` : "Single contributor",
        footerDescription: "Distance between rank 1 and rank 2.",
        testId: "kpi-lead",
      },
      {
        description: "Active contributors",
        value: data.activeContributors.toLocaleString(),
        footerTitle:
          data.activeContributors > 0
            ? `${(data.grandTotal / Math.max(1, data.activeContributors)).toFixed(
                1,
              )} avg events / contributor`
            : "Nobody recorded activity",
        footerDescription: "Users with at least one tracked event.",
        testId: "kpi-active",
      },
    ];
    return result;
  }, [data, orgCount]);

  return <SectionCards cards={cards} />;
}
