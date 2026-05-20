/**
 * Activity table — one row per activity type, with sortable Total
 * column and a sparkline trend computed from the bucketed counts.
 *
 * Layout: shadcn `Table` primitives (TableHeader / TableBody /
 * TableRow / TableHead / TableCell). Numeric columns use
 * `text-right tabular-nums`; the trend column reserves a fixed
 * `h-8 w-24` so the sparklines line up vertically across rows. Empty
 * state renders shadcn `Empty`. Loading state renders `Skeleton`
 * shapes that match the cell layout (number-sized blocks for totals,
 * a full-width thin strip for trend).
 *
 * Sort headers are wrapped in a ghost `Button` for the affordance.
 */

import { useState } from "react";
import { Button } from "@nube/starter-ui-kit/components/button";
import { cn } from "@nube/starter-ui-kit/lib/utils";

import type { CountRow } from "../api/client.js";
import { ACTIVITY_KINDS } from "./activity-types.js";
import { Sparkline } from "./trend-sparkline.jsx";
import { Skeleton } from "../components/skeleton.jsx";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from "../components/empty.jsx";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "../components/table.jsx";

export interface ActivityRow {
  /** Snake-case `EventKind`. */
  kind: string;
  label: string;
  total: number;
  /** Bucketed counts (day buckets when group_by=day). Empty array
   *  means "no events in window". */
  trend: ReadonlyArray<CountRow>;
  /** Per-row loading state — the table renders a skeleton cell
   *  while the underlying query is in flight. */
  loading?: boolean;
}

export interface ActivityTableProps {
  rows: ReadonlyArray<ActivityRow>;
}

type SortKey = "label" | "total";
type SortDir = "asc" | "desc";

export function buildActivityRows(
  perKind: ReadonlyMap<string, { rows: ReadonlyArray<CountRow>; loading: boolean }>,
): ActivityRow[] {
  return ACTIVITY_KINDS.map((k) => {
    const entry = perKind.get(k.key);
    const rows = entry?.rows ?? [];
    const sorted = [...rows].sort((a, b) => a.key.localeCompare(b.key));
    const total = rows.reduce((acc, r) => acc + r.count, 0);
    return {
      kind: k.key,
      label: k.label,
      total,
      trend: sorted,
      loading: entry?.loading ?? false,
    };
  });
}

function SortHeader({
  label,
  active,
  dir,
  align,
  onClick,
}: {
  label: string;
  active: boolean;
  dir: SortDir;
  align?: "left" | "right";
  onClick: () => void;
}): JSX.Element {
  const glyph = active ? (dir === "asc" ? "▲" : "▼") : null;
  return (
    <Button
      variant="ghost"
      size="sm"
      onClick={onClick}
      className={cn(
        "h-7 -mx-2 px-2 text-xs font-medium text-muted-foreground hover:text-foreground",
        align === "right" && "ml-auto",
      )}
    >
      {label}
      {glyph ? <span className="ml-1 text-[0.625rem]">{glyph}</span> : null}
    </Button>
  );
}

export function ActivityTable({ rows }: ActivityTableProps): JSX.Element {
  const [sortKey, setSortKey] = useState<SortKey>("total");
  const [sortDir, setSortDir] = useState<SortDir>("desc");

  function toggleSort(key: SortKey): void {
    if (sortKey === key) {
      setSortDir((d) => (d === "asc" ? "desc" : "asc"));
    } else {
      setSortKey(key);
      setSortDir(key === "label" ? "asc" : "desc");
    }
  }

  const sorted = [...rows].sort((a, b) => {
    const mul = sortDir === "asc" ? 1 : -1;
    if (sortKey === "label") return a.label.localeCompare(b.label) * mul;
    return (a.total - b.total) * mul;
  });

  const allLoaded = rows.every((r) => !r.loading);
  const allZero = allLoaded && rows.every((r) => r.total === 0);

  if (allZero) {
    return (
      <Empty data-testid="activity-table-empty">
        <EmptyHeader>
          <EmptyTitle>No activity in window</EmptyTitle>
          <EmptyDescription>
            No events were recorded for the selected entity in this window.
            Try widening the range or picking another entity.
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }

  return (
    <div className="overflow-hidden rounded-xl border bg-card">
      <Table data-testid="activity-table" className="text-sm">
        <TableHeader className="bg-muted/40">
          <TableRow>
            <TableHead>
              <SortHeader
                label="Activity"
                active={sortKey === "label"}
                dir={sortDir}
                onClick={() => toggleSort("label")}
              />
            </TableHead>
            <TableHead className="text-right">
              <SortHeader
                label="Total"
                active={sortKey === "total"}
                dir={sortDir}
                align="right"
                onClick={() => toggleSort("total")}
              />
            </TableHead>
            <TableHead className="text-right">Trend</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {sorted.map((row) => (
            <TableRow key={row.kind}>
              <TableCell className="font-medium">{row.label}</TableCell>
              <TableCell className="text-right tabular-nums">
                {row.loading ? (
                  <Skeleton
                    data-testid="activity-skel-total"
                    className="ml-auto h-4 w-10"
                  />
                ) : (
                  row.total
                )}
              </TableCell>
              <TableCell className="text-right">
                {row.loading ? (
                  <Skeleton
                    data-testid="activity-skel-trend"
                    className="ml-auto h-8 w-24"
                  />
                ) : (
                  <span className="ml-auto inline-flex h-8 w-24 items-center justify-end align-middle">
                    <Sparkline
                      points={row.trend.map((r) => ({ key: r.key, value: r.count }))}
                      width={96}
                      height={32}
                      ariaLabel={`${row.label} trend, ${row.trend.length} buckets, total ${row.total}`}
                    />
                  </span>
                )}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}
