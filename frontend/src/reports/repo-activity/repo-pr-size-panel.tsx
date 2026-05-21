/**
 * `RepoPrSizePanel` — repo-level pull-request size distribution.
 * Renders p50 / p90 / p95 for additions, deletions, total lines,
 * changed files, and commits over a rolling 90-day window.
 *
 * Source: `GET /repos/{id}/pr-size-stats`. Backed by the JSONB
 * payload GitHub already ships on every PR webhook — no schema
 * change, no extra API call, no cost beyond a single SQL query.
 *
 * SCOPE §4 fit: percentiles describe the **repo's** PR-size
 * profile (cadence of small vs. large changes), never an
 * individual contributor's. This panel is intentionally never
 * mounted on the user-report or leaderboard surfaces.
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
  RepoPrSizeStatsDto,
  RepoSummaryDto,
} from "../../api/client.js";

const WINDOW_DAYS = 90;

export interface RepoPrSizePanelProps {
  /** The focused repo (already loaded into the directory). */
  repo: RepoSummaryDto;
}

export function RepoPrSizePanel({
  repo,
}: RepoPrSizePanelProps): JSX.Element {
  const q = useQuery({
    queryKey: ["repo-pr-size-stats", repo.id],
    queryFn: () => api.getRepoPrSizeStats(repo.id),
    // Percentiles only move when a new PR merges — cache for a
    // minute so back-and-forth between repos doesn't re-hit the
    // aggregator.
    staleTime: 60_000,
  });

  return (
    <Card>
      <CardHeader className="pb-3">
        <CardTitle className="text-base">
          PR size distribution
        </CardTitle>
        <CardDescription>
          Last {WINDOW_DAYS} days · percentiles across merged PRs in{" "}
          <span className="font-mono">{repo.slug}</span>.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {q.isPending ? (
          <p className="text-sm text-muted-foreground">Computing…</p>
        ) : q.isError ? (
          <p className="text-sm text-destructive">
            Failed to load distribution:{" "}
            {q.error instanceof Error ? q.error.message : "unknown error"}
          </p>
        ) : (
          <Body data={q.data} />
        )}
      </CardContent>
    </Card>
  );
}

function Body({ data }: { data: RepoPrSizeStatsDto }): JSX.Element {
  if (data.sample_n === 0) {
    return (
      <p className="text-sm text-muted-foreground">
        No merged PRs in the last {WINDOW_DAYS} days carried diff-size
        data yet.
      </p>
    );
  }
  if (data.sample_n < 5) {
    return (
      <p className="text-sm text-muted-foreground">
        Sample too small ({data.sample_n} merged{" "}
        {data.sample_n === 1 ? "PR" : "PRs"} in the last {WINDOW_DAYS}{" "}
        days). Percentiles need n ≥ 5 to be meaningful.
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
            <Row label="Lines added" t={data.additions} />
            <Row label="Lines removed" t={data.deletions} />
            <Row label="Total lines (add+del)" t={data.total_lines} />
            <Row label="Files touched" t={data.changed_files} />
            <Row label="Commits / PR" t={data.commits} />
          </tbody>
        </table>
      </div>
    </div>
  );
}

function Row({
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
        {fmt(t.p50)}
      </td>
      <td className="py-1.5 px-4 text-right font-mono tabular-nums">
        {fmt(t.p90)}
      </td>
      <td className="py-1.5 pl-4 text-right font-mono tabular-nums">
        {fmt(t.p95)}
      </td>
    </tr>
  );
}

function fmt(n: number | null | undefined): string {
  if (n === null || n === undefined) return "—";
  // PR-size numbers are integers in practice (lines, files,
  // commits) but `percentile_cont` returns a float — round for
  // readability and avoid 12.000000000001 surprises.
  if (n < 10) return n.toFixed(1).replace(/\.0$/, "");
  return Math.round(n).toLocaleString();
}
