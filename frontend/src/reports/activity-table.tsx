/**
 * Activity table — one row per activity type, with sortable Total
 * column and a sparkline trend computed from the bucketed counts.
 *
 * The page upstream fires one `getReportUser` query per activity
 * type with `group_by=day`; this component receives the resulting
 * `CountRow[]` per type and renders the rolled-up table.
 */

import { useState } from "react";
import { Button } from "@nube/starter-ui-kit/components/button";
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

  const headerStyle: React.CSSProperties = {
    textAlign: "left",
    fontWeight: 600,
    fontSize: "0.75rem",
    color: "var(--muted-foreground)",
    textTransform: "uppercase",
    letterSpacing: "0.04em",
    padding: "0.5rem 0.75rem",
    borderBottom: "1px solid var(--border)",
  };
  const cellStyle: React.CSSProperties = {
    padding: "0.5rem 0.75rem",
    borderBottom: "1px solid var(--border)",
    fontSize: "0.875rem",
    verticalAlign: "middle",
  };
  const numStyle: React.CSSProperties = {
    ...cellStyle,
    textAlign: "right",
    fontVariantNumeric: "tabular-nums",
  };

  return (
    <div
      style={{
        border: "1px solid var(--border)",
        borderRadius: "var(--radius-md, 0.5rem)",
        background: "var(--card)",
        overflow: "hidden",
      }}
    >
      <table
        style={{
          width: "100%",
          borderCollapse: "collapse",
        }}
        data-testid="activity-table"
      >
        <thead style={{ background: "var(--muted)" }}>
          <tr>
            <th style={headerStyle}>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => toggleSort("label")}
                style={{ padding: 0, height: "auto", color: "inherit", fontSize: "inherit", textTransform: "inherit", letterSpacing: "inherit", fontWeight: "inherit" }}
              >
                Activity {sortIndicator("label")}
              </Button>
            </th>
            <th style={{ ...headerStyle, textAlign: "right" }}>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => toggleSort("total")}
                style={{ padding: 0, height: "auto", color: "inherit", fontSize: "inherit", textTransform: "inherit", letterSpacing: "inherit", fontWeight: "inherit" }}
              >
                Total {sortIndicator("total")}
              </Button>
            </th>
            <th style={{ ...headerStyle, textAlign: "right" }}>Trend</th>
          </tr>
        </thead>
        <tbody>
          {sorted.map((row) => (
            <tr key={row.kind}>
              <td style={cellStyle}>{row.label}</td>
              <td style={numStyle}>
                {row.loading ? (
                  <Skeleton
                    data-testid="activity-skel-total"
                    style={{
                      height: "0.875rem",
                      width: "2.5rem",
                      borderRadius: "0.25rem",
                      marginLeft: "auto",
                    }}
                  />
                ) : (
                  row.total
                )}
              </td>
              <td style={{ ...numStyle, width: "10rem" }}>
                {row.loading ? (
                  <Skeleton
                    data-testid="activity-skel-trend"
                    style={{
                      height: "1.25rem",
                      width: "100%",
                      borderRadius: "0.25rem",
                    }}
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
                style={{
                  ...cellStyle,
                  textAlign: "center",
                  color: "var(--muted-foreground)",
                  padding: "1.5rem 0.75rem",
                  borderBottom: "none",
                }}
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
