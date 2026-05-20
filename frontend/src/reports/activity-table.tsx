/**
 * Activity table — one row per activity type, with sortable Total
 * column and a sparkline trend computed from the bucketed counts.
 *
 * The page upstream fires one `getReportUser` query per activity
 * type with `group_by=day`; this component receives the resulting
 * `CountRow[]` per type and renders the rolled-up table.
 *
 * The kit doesn't ship a `Table` primitive, so the markup stays
 * semantic `<table>` / `<thead>` / `<tbody>` with Tailwind utility
 * classes applied via shared per-cell constants. shadcn `Button`
 * (ghost variant) drives the column-header sort affordances.
 */

import { useState } from "react";
import { Button } from "@nube/starter-ui-kit/components/button";
import { cn } from "@nube/starter-ui-kit/lib/utils";
import { Skeleton } from "../components/skeleton.jsx";

import type { CountRow } from "../api/client.js";
import { ACTIVITY_KINDS } from "./activity-types.js";
import { Sparkline } from "./trend-sparkline.jsx";

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

/** Shared column-header / body-cell class constants — keep the table
 *  consistent without dragging in a wrapper component for every
 *  `<th>` / `<td>`. */
const HEADER_CLASS =
  "border-b border-border px-3 py-2 text-left text-xs font-semibold uppercase tracking-wider text-muted-foreground";
const CELL_CLASS = "border-b border-border px-3 py-2 align-middle text-sm";
const NUM_CLASS = cn(CELL_CLASS, "text-right tabular-nums");
const HEADER_RIGHT_CLASS = cn(HEADER_CLASS, "text-right");

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

  // Empty-state: every row is loaded and every total is zero.
  const allLoaded = rows.every((r) => !r.loading);
  const allZero = allLoaded && rows.every((r) => r.total === 0);

  const sortIndicator = (key: SortKey): string =>
    sortKey === key ? (sortDir === "asc" ? "▲" : "▼") : "";

  return (
    <div className="overflow-hidden rounded-md border border-border bg-card">
      <table className="w-full border-collapse" data-testid="activity-table">
        <thead className="bg-muted">
          <tr>
            <th className={HEADER_CLASS}>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => toggleSort("label")}
                className="h-auto p-0 text-inherit font-inherit uppercase tracking-wider"
              >
                Activity {sortIndicator("label")}
              </Button>
            </th>
            <th className={HEADER_RIGHT_CLASS}>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => toggleSort("total")}
                className="h-auto p-0 text-inherit font-inherit uppercase tracking-wider"
              >
                Total {sortIndicator("total")}
              </Button>
            </th>
            <th className={HEADER_RIGHT_CLASS}>Trend</th>
          </tr>
        </thead>
        <tbody>
          {sorted.map((row) => (
            <tr key={row.kind}>
              <td className={CELL_CLASS}>{row.label}</td>
              <td className={NUM_CLASS}>
                {row.loading ? (
                  <Skeleton
                    data-testid="activity-skel-total"
                    className="ml-auto h-3.5 w-10 rounded-sm"
                  />
                ) : (
                  row.total
                )}
              </td>
              <td className={cn(NUM_CLASS, "w-40")}>
                {row.loading ? (
                  <Skeleton
                    data-testid="activity-skel-trend"
                    className="h-5 w-full rounded-sm"
                  />
                ) : (
                  <Sparkline
                    points={row.trend.map((r) => ({ key: r.key, value: r.count }))}
                    ariaLabel={`${row.label} trend, ${row.trend.length} buckets, total ${row.total}`}
                  />
                )}
              </td>
            </tr>
          ))}
          {allZero && (
            <tr data-testid="activity-table-empty">
              <td
                colSpan={3}
                className={cn(CELL_CLASS, "border-b-0 py-6 text-center text-muted-foreground")}
              >
                No activity recorded in the selected window.
              </td>
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}
