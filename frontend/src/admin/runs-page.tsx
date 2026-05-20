/**
 * Admin · Runs page — paginated `GET /admin/runs` log with auto-refresh.
 *
 * Renders one row per `fetch_runs` record (kind, started, finished,
 * items, errors, partial) with a derived status column (running /
 * partial / failed / clean) so the operator can scan the log without
 * cross-referencing the numeric columns.
 *
 * Pagination is offset-based — `dp-rest` accepts `limit` + `offset`
 * and returns at most `limit` rows; we infer "has next page" from
 * `rows.length === limit` (one extra page reveals an empty table,
 * which is acceptable for an admin tool).
 *
 * Auto-refresh: react-query's `refetchInterval` ticks every 15s so
 * a freshly started reconciler shows up without an explicit reload.
 * The fetch is paused (`refetchIntervalInBackground: false`) when
 * the tab is hidden so the dashboard doesn't poll when nobody is
 * watching.
 *
 * Markup: semantic `<table>` + Tailwind utility classes (the kit
 * doesn't ship a Table primitive — see `reports/activity-table.tsx`
 * for the same pattern). Status column is a shadcn `Badge` whose
 * border + text colour is driven by a `STATUS_CLASS` className map
 * (one of running/partial/failed/clean).
 */

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Alert, AlertDescription, AlertTitle } from "@nube/starter-ui-kit/components/alert";
import { Badge } from "@nube/starter-ui-kit/components/badge";
import { Button } from "@nube/starter-ui-kit/components/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@nube/starter-ui-kit/components/card";
import { cn } from "@nube/starter-ui-kit/lib/utils";

import { api } from "../api/client.js";
import type { FetchRunDto } from "../api/client.js";
import { USE_MOCK, paginateMockRuns } from "./mocks.js";

const PAGE_SIZE = 25;
const REFRESH_MS = 15_000;

type RunStatus = "running" | "partial" | "failed" | "clean";

function statusOf(run: FetchRunDto): RunStatus {
  if (run.finished === null || run.finished === undefined) return "running";
  if (run.partial) return "partial";
  if (run.errors > 0 && run.items === 0) return "failed";
  return "clean";
}

/** className tuples drive the Badge border + text colour for each
 *  run status. Kept off the inline `style` so the Tailwind dark-mode
 *  variants apply without us special-casing them at render time. */
const STATUS_CLASS: Record<RunStatus, { label: string; badge: string }> = {
  running: {
    label: "Running",
    badge: "border-blue-500 text-blue-600 dark:text-blue-400",
  },
  partial: {
    label: "Partial",
    badge: "border-amber-500 text-amber-600 dark:text-amber-400",
  },
  failed: {
    label: "Failed",
    badge: "border-red-500 text-red-600 dark:text-red-400",
  },
  clean: {
    label: "Clean",
    badge: "border-emerald-500 text-emerald-600 dark:text-emerald-400",
  },
};

const HEADER_CLASS =
  "border-b border-border px-3 py-2 text-left text-xs font-semibold uppercase tracking-wider text-muted-foreground";
const CELL_CLASS = "border-b border-border px-3 py-2 align-middle text-sm";

function formatDuration(startedIso: string, finishedIso: string | null | undefined): string {
  if (!finishedIso) return "—";
  const ms = new Date(finishedIso).getTime() - new Date(startedIso).getTime();
  if (!Number.isFinite(ms) || ms < 0) return "—";
  if (ms < 1000) return `${ms}ms`;
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  return `${m}m ${s % 60}s`;
}

function formatTs(iso: string | null | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function RunsPage(): JSX.Element {
  const [page, setPage] = useState(0);
  const offset = page * PAGE_SIZE;

  const runsQuery = useQuery({
    queryKey: ["admin-runs", PAGE_SIZE, offset],
    queryFn: () =>
      USE_MOCK
        ? Promise.resolve(paginateMockRuns(PAGE_SIZE, offset))
        : api.listRuns({ limit: PAGE_SIZE, offset }),
    refetchInterval: REFRESH_MS,
    refetchIntervalInBackground: false,
  });

  const runs = runsQuery.data ?? [];
  const hasNext = runs.length === PAGE_SIZE;
  const hasPrev = page > 0;

  return (
    <Card>
      <CardHeader>
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <CardTitle>Fetch runs</CardTitle>
            <CardDescription>
              <code>GET /admin/runs</code> · paginated reconciler/backfill log,
              auto-refreshing every {Math.round(REFRESH_MS / 1000)}s.
            </CardDescription>
          </div>
          <div className="flex items-center gap-2">
            <span
              data-testid="runs-refresh-status"
              className="text-[0.8125rem] text-muted-foreground"
            >
              {runsQuery.isFetching ? "Refreshing…" : "Live"}
            </span>
            <Button
              size="sm"
              variant="outline"
              onClick={() => void runsQuery.refetch()}
              disabled={runsQuery.isFetching}
              data-testid="runs-refresh"
            >
              Refresh now
            </Button>
          </div>
        </div>
      </CardHeader>
      <CardContent className="grid gap-4">
        {runsQuery.error ? (
          <Alert variant="destructive" data-testid="runs-error">
            <AlertTitle>Failed to load runs</AlertTitle>
            <AlertDescription>{runsQuery.error.message}</AlertDescription>
          </Alert>
        ) : null}

        {runsQuery.isPending && runs.length === 0 ? (
          <p className="text-muted-foreground">Loading runs…</p>
        ) : runs.length === 0 ? (
          <p data-testid="runs-empty" className="text-muted-foreground">
            {hasPrev ? "No more runs on this page." : "No fetch runs recorded yet."}
          </p>
        ) : (
          <div className="overflow-hidden rounded-md border border-border bg-card">
            <table
              data-testid="runs-table"
              className="w-full border-collapse"
            >
              <thead className="bg-muted">
                <tr>
                  <th className={HEADER_CLASS}>Kind</th>
                  <th className={HEADER_CLASS}>Started</th>
                  <th className={HEADER_CLASS}>Finished</th>
                  <th className={cn(HEADER_CLASS, "text-right")}>Duration</th>
                  <th className={cn(HEADER_CLASS, "text-right")}>Items</th>
                  <th className={cn(HEADER_CLASS, "text-right")}>Errors</th>
                  <th className={HEADER_CLASS}>Status</th>
                </tr>
              </thead>
              <tbody>
                {runs.map((r) => {
                  const status = statusOf(r);
                  const meta = STATUS_CLASS[status];
                  return (
                    <tr
                      key={r.id}
                      data-run-id={r.id}
                      data-run-status={status}
                    >
                      <td className={CELL_CLASS}>
                        <code>{r.kind}</code>
                      </td>
                      <td className={CELL_CLASS}>{formatTs(r.started)}</td>
                      <td className={CELL_CLASS}>
                        {formatTs(r.finished ?? null)}
                      </td>
                      <td className={cn(CELL_CLASS, "text-right tabular-nums")}>
                        {formatDuration(r.started, r.finished ?? null)}
                      </td>
                      <td className={cn(CELL_CLASS, "text-right tabular-nums")}>
                        {r.items.toLocaleString()}
                      </td>
                      <td
                        className={cn(
                          CELL_CLASS,
                          "text-right tabular-nums",
                          r.errors > 0 && "text-destructive",
                        )}
                      >
                        {r.errors.toLocaleString()}
                      </td>
                      <td className={CELL_CLASS}>
                        <Badge
                          variant="outline"
                          data-testid="run-status-badge"
                          className={cn("border", meta.badge)}
                        >
                          {meta.label}
                        </Badge>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}

        <div className="flex items-center justify-between pt-1">
          <span className="text-[0.8125rem] text-muted-foreground">
            Page {page + 1} · showing rows {offset + 1}–{offset + runs.length}
          </span>
          <div className="flex gap-2">
            <Button
              size="sm"
              variant="outline"
              disabled={!hasPrev}
              onClick={() => setPage((p) => Math.max(0, p - 1))}
              data-testid="runs-prev"
            >
              Previous
            </Button>
            <Button
              size="sm"
              variant="outline"
              disabled={!hasNext}
              onClick={() => setPage((p) => p + 1)}
              data-testid="runs-next"
            >
              Next
            </Button>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

export const __test__ = { statusOf, formatDuration };
