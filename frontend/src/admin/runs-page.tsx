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
 */

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Badge } from "@nube/starter-ui-kit/components/badge";
import { Button } from "@nube/starter-ui-kit/components/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@nube/starter-ui-kit/components/card";

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

const STATUS_STYLE: Record<RunStatus, { label: string; color: string }> = {
  running: { label: "Running", color: "oklch(0.6 0.15 240)" },
  partial: { label: "Partial", color: "oklch(0.62 0.16 80)" },
  failed:  { label: "Failed",  color: "oklch(0.55 0.2 25)" },
  clean:   { label: "Clean",   color: "oklch(0.5 0.16 145)" },
};

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
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "flex-start",
            gap: "1rem",
            flexWrap: "wrap",
          }}
        >
          <div>
            <CardTitle>Fetch runs</CardTitle>
            <CardDescription>
              <code>GET /admin/runs</code> · paginated reconciler/backfill log,
              auto-refreshing every {Math.round(REFRESH_MS / 1000)}s.
            </CardDescription>
          </div>
          <div style={{ display: "flex", gap: "0.5rem", alignItems: "center" }}>
            <span
              data-testid="runs-refresh-status"
              style={{ fontSize: "0.8125rem", color: "var(--muted-foreground)" }}
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
      <CardContent style={{ display: "grid", gap: "1rem" }}>
        {runsQuery.error ? (
          <p data-testid="runs-error" style={{ color: "oklch(0.5 0.2 25)" }}>
            Failed to load runs: {runsQuery.error.message}
          </p>
        ) : null}

        {runsQuery.isPending && runs.length === 0 ? (
          <p style={{ color: "var(--muted-foreground)" }}>Loading runs…</p>
        ) : runs.length === 0 ? (
          <p data-testid="runs-empty" style={{ color: "var(--muted-foreground)" }}>
            {hasPrev ? "No more runs on this page." : "No fetch runs recorded yet."}
          </p>
        ) : (
          <div
            data-testid="runs-table"
            role="table"
            style={{
              display: "grid",
              gap: "0.125rem",
              gridTemplateColumns:
                "minmax(7rem, auto) minmax(10rem, 1.2fr) minmax(10rem, 1.2fr) minmax(5rem, auto) minmax(5rem, auto) minmax(5rem, auto) minmax(6rem, auto)",
              alignItems: "center",
              fontSize: "0.875rem",
            }}
          >
            <Header>Kind</Header>
            <Header>Started</Header>
            <Header>Finished</Header>
            <Header>Duration</Header>
            <Header>Items</Header>
            <Header>Errors</Header>
            <Header>Status</Header>
            {runs.map((r) => {
              const status = statusOf(r);
              const style = STATUS_STYLE[status];
              return (
                <Row key={r.id} data-run-id={r.id} data-run-status={status}>
                  <Cell><code>{r.kind}</code></Cell>
                  <Cell>{formatTs(r.started)}</Cell>
                  <Cell>{formatTs(r.finished ?? null)}</Cell>
                  <Cell>{formatDuration(r.started, r.finished ?? null)}</Cell>
                  <Cell>{r.items.toLocaleString()}</Cell>
                  <Cell
                    style={{
                      color: r.errors > 0 ? "oklch(0.5 0.2 25)" : undefined,
                      fontVariantNumeric: "tabular-nums",
                    }}
                  >
                    {r.errors.toLocaleString()}
                  </Cell>
                  <Cell>
                    <Badge
                      variant="outline"
                      data-testid="run-status-badge"
                      style={{ color: style.color, borderColor: style.color }}
                    >
                      {style.label}
                    </Badge>
                  </Cell>
                </Row>
              );
            })}
          </div>
        )}

        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            paddingTop: "0.25rem",
          }}
        >
          <span style={{ fontSize: "0.8125rem", color: "var(--muted-foreground)" }}>
            Page {page + 1} · showing rows {offset + 1}–{offset + runs.length}
          </span>
          <div style={{ display: "flex", gap: "0.5rem" }}>
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

function Header({ children }: { children: React.ReactNode }): JSX.Element {
  return (
    <div
      role="columnheader"
      style={{
        padding: "0.5rem 0.625rem",
        fontWeight: 600,
        borderBottom: "1px solid var(--border)",
        color: "var(--muted-foreground)",
        fontSize: "0.8125rem",
        textTransform: "uppercase",
        letterSpacing: "0.02em",
      }}
    >
      {children}
    </div>
  );
}

function Row({
  children,
  ...rest
}: { children: React.ReactNode } & React.HTMLAttributes<HTMLDivElement>): JSX.Element {
  return (
    <div role="row" style={{ display: "contents" }} {...rest}>
      {children}
    </div>
  );
}

function Cell({
  children,
  style,
}: {
  children: React.ReactNode;
  style?: React.CSSProperties;
}): JSX.Element {
  return (
    <div
      role="cell"
      style={{
        padding: "0.5rem 0.625rem",
        borderBottom: "1px solid var(--border)",
        ...style,
      }}
    >
      {children}
    </div>
  );
}

export const __test__ = { statusOf, formatDuration };
