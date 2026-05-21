/**
 * `RepoReviewVelocityPanel` — repo-level time-to-merge
 * percentile distribution.
 *
 * Source: `GET /repos/{id}/review-velocity`. Computed straight
 * from `merged_at - created_at` on the `pull_request_merged`
 * webhook payload — no extra schema, no extra API call.
 *
 * SCOPE §4 fit: percentiles describe the **repo's** merge
 * cadence (how quickly the team turns code around), never an
 * individual contributor's. Intentionally not mounted on the
 * user-report or leaderboard surfaces.
 *
 * Sample-size guard (SCOPE §15.9): when `sample_n < 5` every
 * percentile is `null` and the panel shows the actual `n` plus a
 * "not enough data" hint instead of zeros.
 */

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
  PercentileTripleDto,
  RepoReviewVelocityDto,
  RepoSummaryDto,
} from "../../api/client.js";

const WINDOW_DAYS = 90;

export interface RepoReviewVelocityPanelProps {
  /** The focused repo (already loaded into the directory). */
  repo: RepoSummaryDto;
}

export function RepoReviewVelocityPanel({
  repo,
}: RepoReviewVelocityPanelProps): JSX.Element {
  const q = useQuery({
    queryKey: ["repo-review-velocity", repo.id],
    queryFn: () => api.getRepoReviewVelocity(repo.id),
    staleTime: 60_000,
  });

  return (
    <Card>
      <CardHeader className="pb-3">
        <CardTitle className="text-base">Review velocity</CardTitle>
        <CardDescription>
          Last {WINDOW_DAYS} days · time from PR open to merge in{" "}
          <span className="font-mono">{repo.slug}</span>.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {q.isPending ? (
          <p className="text-sm text-muted-foreground">Computing…</p>
        ) : q.isError ? (
          <p className="text-sm text-destructive">
            Failed to load review velocity:{" "}
            {q.error instanceof Error ? q.error.message : "unknown error"}
          </p>
        ) : (
          <Body data={q.data} />
        )}
      </CardContent>
    </Card>
  );
}

function Body({ data }: { data: RepoReviewVelocityDto }): JSX.Element {
  if (data.sample_n === 0) {
    return (
      <p className="text-sm text-muted-foreground">
        No PRs merged in the last {WINDOW_DAYS} days carried the
        timestamps needed to compute velocity.
      </p>
    );
  }
  if (data.sample_n < 5) {
    return (
      <p className="text-sm text-muted-foreground">
        Sample too small ({data.sample_n} merged{" "}
        {data.sample_n === 1 ? "PR" : "PRs"} in the last{" "}
        {WINDOW_DAYS} days). Percentiles need n ≥ 5 to be
        meaningful.
      </p>
    );
  }
  return (
    <div className="flex flex-col gap-3">
      <p className="text-xs text-muted-foreground">
        Based on {data.sample_n} merged{" "}
        {data.sample_n === 1 ? "PR" : "PRs"}.
      </p>
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="text-left text-xs text-muted-foreground">
              <th className="py-1 pr-4 font-medium">Metric</th>
              <th className="py-1 px-4 text-right font-medium">p50</th>
              <th className="py-1 px-4 text-right font-medium">p90</th>
              <th className="py-1 pl-4 text-right font-medium">p95</th>
            </tr>
          </thead>
          <tbody>
            <DurationRow
              label="Time to merge"
              t={data.time_to_merge_seconds}
            />
          </tbody>
        </table>
      </div>
    </div>
  );
}

function DurationRow({
  label,
  t,
}: {
  label: string;
  t: PercentileTripleDto;
}): JSX.Element {
  return (
    <tr className="border-t">
      <td className="py-1.5 pr-4">{label}</td>
      <td className="py-1.5 px-4 text-right font-mono tabular-nums">
        {fmtDuration(t.p50)}
      </td>
      <td className="py-1.5 px-4 text-right font-mono tabular-nums">
        {fmtDuration(t.p90)}
      </td>
      <td className="py-1.5 pl-4 text-right font-mono tabular-nums">
        {fmtDuration(t.p95)}
      </td>
    </tr>
  );
}

/** Time-to-merge spans hours-to-weeks, so this formatter caps at
 *  "Xd Yh" — minute precision is noise once you're past a day,
 *  and "37h 24m" is harder to scan than "1d 13h". */
function fmtDuration(secs: number | null | undefined): string {
  if (secs === null || secs === undefined) return "—";
  if (secs < 60) return `${Math.round(secs)}s`;
  const total = Math.round(secs);
  const days = Math.floor(total / 86_400);
  const hours = Math.floor((total % 86_400) / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  if (days > 0) {
    return hours > 0 ? `${days}d ${hours}h` : `${days}d`;
  }
  if (hours > 0) {
    return minutes > 0 ? `${hours}h ${minutes}m` : `${hours}h`;
  }
  return `${minutes}m`;
}
