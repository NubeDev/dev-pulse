/**
 * `RepoCiPanel` — repo-level CI workflow-run health.
 *
 * Source: `GET /repos/{id}/ci-stats`. Backed by the JSONB
 * payload GitHub already ships on every `workflow_run.completed`
 * webhook — no schema change beyond the existing event store.
 *
 * SCOPE §4 fit: success rate, conclusion mix, and duration
 * percentiles describe the **repo's** CI pipeline, not any
 * individual contributor. This panel is intentionally never
 * mounted on the user-report or leaderboard surfaces.
 *
 * Sample-size guard (SCOPE §15.9): duration percentiles are
 * `null` when fewer than 5 runs in the window had a recorded
 * positive duration; the panel shows the actual `n` plus a
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
  RepoCiStatsDto,
  RepoSummaryDto,
} from "../../api/client.js";

const WINDOW_DAYS = 90;

export interface RepoCiPanelProps {
  /** The focused repo (already loaded into the directory). */
  repo: RepoSummaryDto;
}

export function RepoCiPanel({ repo }: RepoCiPanelProps): JSX.Element {
  const q = useQuery({
    queryKey: ["repo-ci-stats", repo.id],
    queryFn: () => api.getRepoCiStats(repo.id),
    // Workflow runs only land on the completed webhook — cache a
    // minute so switching between repos doesn't re-hit the
    // aggregator.
    staleTime: 60_000,
  });

  return (
    <Card>
      <CardHeader className="pb-3">
        <CardTitle className="text-base">CI workflow health</CardTitle>
        <CardDescription>
          Last {WINDOW_DAYS} days · workflow-run conclusions and
          duration percentiles for{" "}
          <span className="font-mono">{repo.slug}</span>.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {q.isPending ? (
          <p className="text-sm text-muted-foreground">Computing…</p>
        ) : q.isError ? (
          <p className="text-sm text-destructive">
            Failed to load CI stats:{" "}
            {q.error instanceof Error ? q.error.message : "unknown error"}
          </p>
        ) : (
          <Body data={q.data} />
        )}
      </CardContent>
    </Card>
  );
}

function Body({ data }: { data: RepoCiStatsDto }): JSX.Element {
  if (data.total_runs === 0) {
    return (
      <p className="text-sm text-muted-foreground">
        No workflow runs in the last {WINDOW_DAYS} days.
      </p>
    );
  }
  return (
    <div className="flex flex-col gap-4">
      <Headline data={data} />
      <Conclusions data={data} />
      <Durations data={data} />
    </div>
  );
}

function Headline({ data }: { data: RepoCiStatsDto }): JSX.Element {
  // success_rate is null only when both success and failure are
  // zero (every run cancelled / skipped / etc.) — keep the headline
  // honest rather than rendering "0%".
  const rateText =
    data.success_rate === null || data.success_rate === undefined
      ? "—"
      : `${(data.success_rate * 100).toFixed(1)}%`;
  const denom = data.success + data.failure;
  return (
    <div className="flex items-baseline gap-3">
      <span className="text-3xl font-semibold tabular-nums">
        {rateText}
      </span>
      <span className="text-xs text-muted-foreground">
        success rate
        {denom > 0
          ? ` · ${data.success.toLocaleString()} of ${denom.toLocaleString()} terminal runs`
          : " · no success/failure runs to score"}
      </span>
    </div>
  );
}

function Conclusions({ data }: { data: RepoCiStatsDto }): JSX.Element {
  return (
    <div className="grid grid-cols-2 gap-x-4 gap-y-1.5 text-sm sm:grid-cols-5">
      <Stat label="Total" value={data.total_runs} />
      <Stat label="Success" value={data.success} />
      <Stat label="Failure" value={data.failure} />
      <Stat label="Cancelled" value={data.cancelled} />
      <Stat
        label="Other"
        value={data.other}
        hint="skipped / neutral / timed out / action required / stale"
      />
    </div>
  );
}

function Stat({
  label,
  value,
  hint,
}: {
  label: string;
  value: number;
  hint?: string;
}): JSX.Element {
  return (
    <div className="flex flex-col">
      <span
        className="text-xs text-muted-foreground"
        title={hint}
      >
        {label}
      </span>
      <span className="font-mono tabular-nums">
        {value.toLocaleString()}
      </span>
    </div>
  );
}

function Durations({ data }: { data: RepoCiStatsDto }): JSX.Element {
  if (data.duration_sample_n === 0) {
    return (
      <p className="text-xs text-muted-foreground">
        No completed runs in the window carried timing data.
      </p>
    );
  }
  if (data.duration_sample_n < 5) {
    return (
      <p className="text-xs text-muted-foreground">
        Duration sample too small ({data.duration_sample_n}{" "}
        {data.duration_sample_n === 1 ? "run" : "runs"} with timing
        data). Percentiles need n ≥ 5 to be meaningful.
      </p>
    );
  }
  return (
    <div className="flex flex-col gap-1.5">
      <p className="text-xs text-muted-foreground">
        Duration percentiles over {data.duration_sample_n} timed{" "}
        {data.duration_sample_n === 1 ? "run" : "runs"}.
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
            <DurationRow label="Run duration" t={data.duration_seconds} />
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

/** Format a duration in seconds as `Hh Mm Ss` (drops leading zero
 *  units). Falls back to "—" for the n<5 null case. */
function fmtDuration(secs: number | null | undefined): string {
  if (secs === null || secs === undefined) return "—";
  if (secs < 1) return "<1s";
  const total = Math.round(secs);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const parts: string[] = [];
  if (h > 0) parts.push(`${h}h`);
  if (m > 0) parts.push(`${m}m`);
  if (s > 0 || parts.length === 0) parts.push(`${s}s`);
  return parts.join(" ");
}
