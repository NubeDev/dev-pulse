/**
 * `RepoActivityHeatmapPanel` — when does this repo light up?
 *
 * Source: `GET /repos/{id}/activity-heatmap`. Bucketed in the
 * viewer's local IANA timezone via `Intl` so "9am" on the grid
 * matches whoever is reading.
 *
 * SCOPE §4 fit: describes the **repo's** activity cadence (push
 * times, PR-merge windows, review storms) — never an individual
 * contributor's. Intentionally not mounted on the user-report or
 * leaderboard pages.
 *
 * Visual: a 7×24 grid with one cell per `(dow, hour)`. Cells use
 * a single hue with opacity scaled to `count / max(count)` so
 * the eye reads relative intensity at a glance. Empty windows
 * render the grid greyed out with a "no data" hint.
 */

import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";

import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

import { api } from "../../api/client.js";
import type {
  HeatmapBucketDto,
  RepoActivityHeatmapDto,
  RepoSummaryDto,
} from "../../api/client.js";

const WINDOW_DAYS = 90;

const DOW_LABELS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] as const;

export interface RepoActivityHeatmapPanelProps {
  /** The focused repo (already loaded into the directory). */
  repo: RepoSummaryDto;
}

export function RepoActivityHeatmapPanel({
  repo,
}: RepoActivityHeatmapPanelProps): JSX.Element {
  // Detect the viewer's local IANA zone once per render. `Intl`
  // is universally available in the browsers we target; fall
  // back to UTC if the runtime is exotic.
  const tz = useMemo(() => {
    try {
      return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
    } catch {
      return "UTC";
    }
  }, []);

  const q = useQuery({
    queryKey: ["repo-activity-heatmap", repo.id, tz],
    queryFn: () =>
      api.getRepoActivityHeatmap(repo.id, { timezone: tz }),
    staleTime: 60_000,
  });

  return (
    <Card>
      <CardHeader className="pb-3">
        <CardTitle className="text-base">Activity heatmap</CardTitle>
        <CardDescription>
          Last {WINDOW_DAYS} days · events by day-of-week × hour-of-day
          in <span className="font-mono">{tz}</span>.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {q.isPending ? (
          <p className="text-sm text-muted-foreground">Computing…</p>
        ) : q.isError ? (
          <p className="text-sm text-destructive">
            Failed to load heatmap:{" "}
            {q.error instanceof Error ? q.error.message : "unknown error"}
          </p>
        ) : (
          <Body data={q.data} />
        )}
      </CardContent>
    </Card>
  );
}

function Body({ data }: { data: RepoActivityHeatmapDto }): JSX.Element {
  if (data.total === 0) {
    return (
      <p className="text-sm text-muted-foreground">
        No activity events in the last {WINDOW_DAYS} days.
      </p>
    );
  }
  // Build a `(dow, hour) → count` lookup once. The DTO is
  // already sorted and dense (168 cells) but indexed lookup is
  // cheaper than a linear find per cell.
  const grid = new Map<string, number>();
  let max = 0;
  for (const b of data.buckets) {
    grid.set(`${b.dow}:${b.hour}`, b.count);
    if (b.count > max) max = b.count;
  }
  return (
    <div className="flex flex-col gap-3">
      <p className="text-xs text-muted-foreground">
        {data.total.toLocaleString()} events · darker cell = more
        activity. Peak hour saw {max.toLocaleString()}{" "}
        {max === 1 ? "event" : "events"}.
      </p>
      <div className="overflow-x-auto">
        <table className="text-[10px] tabular-nums">
          <thead>
            <tr>
              <th className="w-8" />
              {Array.from({ length: 24 }, (_, h) => (
                <th
                  key={h}
                  className="px-0.5 text-center font-normal text-muted-foreground"
                >
                  {/* Show only every 3rd label to keep the header readable. */}
                  {h % 3 === 0 ? h : ""}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {DOW_LABELS.map((label, dow) => (
              <tr key={label}>
                <td className="pr-2 text-right text-muted-foreground">
                  {label}
                </td>
                {Array.from({ length: 24 }, (_, hour) => {
                  const count = grid.get(`${dow}:${hour}`) ?? 0;
                  return (
                    <Cell key={hour} count={count} max={max} hour={hour} dow={label} />
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function Cell({
  count,
  max,
  hour,
  dow,
}: {
  count: number;
  max: number;
  hour: number;
  dow: string;
}): JSX.Element {
  // Map `count/max` ∈ [0, 1] onto opacity ∈ [0.05, 1]. The 0.05
  // floor keeps empty cells visible as a faint grid rather than
  // disappearing into the card background.
  const intensity = max === 0 ? 0 : count / max;
  const opacity = count === 0 ? 0.06 : 0.15 + intensity * 0.85;
  const hourLabel =
    hour === 0
      ? "12am"
      : hour < 12
        ? `${hour}am`
        : hour === 12
          ? "12pm"
          : `${hour - 12}pm`;
  return (
    <td
      className="h-4 w-3.5 border border-background"
      style={{
        backgroundColor: `rgb(59 130 246 / ${opacity})`,
      }}
      title={`${dow} ${hourLabel} · ${count.toLocaleString()} ${
        count === 1 ? "event" : "events"
      }`}
    />
  );
}
