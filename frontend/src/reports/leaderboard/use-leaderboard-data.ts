/**
 * `useLeaderboardData` — fans out one `/reports/org/:org_id` call per
 * (org × kind) plus one per org for the daily trend, and folds the
 * results into the `LeaderboardData` shape every chart and table on
 * the page consumes.
 *
 * Query layout:
 *
 *   - `composition[org_i × kind_j]`  — `group_by=user`, single kind.
 *     Drives the stacked bar chart + the per-kind table columns.
 *   - `trend[org_i]`                  — `group_by=day`, every kind.
 *     Drives the area trend chart.
 *
 * Total fan-out: `N_orgs × (N_kinds + 1)`. With UI-side caps
 * (default ≤ 5 orgs, ≤ 12 kinds) this stays well under react-query's
 * comfort zone. Each composition slice is cached independently so
 * toggling a kind doesn't refetch the others.
 */

import { useMemo } from "react";
import { useQueries } from "@tanstack/react-query";

import { api } from "../../api/client.js";
import type {
  CountRow,
  DataAsOf,
  ReportParams,
  ReportResponse,
} from "../../api/client.js";

import { ACTIVITY_KINDS } from "../activity-types.js";
import type {
  DirectoryMaps,
  LeaderUserRow,
  LeaderboardData,
  LeaderboardSelection,
  MixSlice,
  TrendBucket,
} from "./types.js";

export interface UseLeaderboardDataArgs {
  selection: LeaderboardSelection;
  /** Window / TZ / anchor — already serialised by `windowStateToParams`. */
  windowParams: Pick<
    ReportParams,
    "window_label" | "tz" | "anchor" | "custom_start" | "custom_end"
  >;
  directory: DirectoryMaps;
}

export interface UseLeaderboardDataResult {
  data: LeaderboardData;
  loading: boolean;
  error: Error | null;
  dataAsOf: DataAsOf | null;
}

const ACTIVITY_LABEL = new Map<string, string>(
  ACTIVITY_KINDS.map((k) => [k.key, k.label]),
);

export function useLeaderboardData({
  selection,
  windowParams,
  directory,
}: UseLeaderboardDataArgs): UseLeaderboardDataResult {
  const activeKinds = useMemo(
    () =>
      selection.kinds.length > 0
        ? selection.kinds
        : ACTIVITY_KINDS.map((k) => k.key),
    [selection.kinds],
  );

  // -- composition fan-out (org × kind, group_by=user) ----------------------
  const compositionQueries = useQueries({
    queries: selection.orgIds.flatMap((orgId) =>
      activeKinds.map((kind) => ({
        queryKey: [
          "leaderboard",
          "composition",
          orgId,
          kind,
          windowParams,
        ] as const,
        queryFn: () =>
          api.getReportOrg(orgId, {
            ...windowParams,
            scope_mode: "single_org" as const,
            group_by: "user" as const,
            activity_types: [kind],
          }),
        staleTime: 60_000,
      })),
    ),
  });

  // -- trend fan-out (one per org, group_by=day) ----------------------------
  const trendQueries = useQueries({
    queries: selection.orgIds.map((orgId) => ({
      queryKey: [
        "leaderboard",
        "trend",
        orgId,
        activeKinds,
        windowParams,
      ] as const,
      queryFn: () =>
        api.getReportOrg(orgId, {
          ...windowParams,
          scope_mode: "single_org" as const,
          group_by: "day" as const,
          activity_types: [...activeKinds],
        }),
      staleTime: 60_000,
    })),
  });

  const data = useMemo<LeaderboardData>(() => {
    if (selection.orgIds.length === 0) {
      return {
        rows: [],
        trend: [],
        mix: [],
        grandTotal: 0,
        activeContributors: 0,
      };
    }

    const userFilter =
      selection.userIds.length > 0 ? new Set(selection.userIds) : null;

    // Build per-user, per-kind, per-org accumulators.
    const byUser = new Map<string, LeaderUserRow>();
    const mixTotals = new Map<string, number>();

    compositionQueries.forEach((q, idx) => {
      const orgIdx = Math.floor(idx / activeKinds.length);
      const kindIdx = idx % activeKinds.length;
      const orgId = selection.orgIds[orgIdx];
      const kind = activeKinds[kindIdx];
      if (!orgId || !kind || !q.data) return;
      const rows = (q.data as ReportResponse<CountRow[]>).rows ?? [];
      for (const row of rows) {
        if (userFilter && !userFilter.has(row.key)) continue;
        if (row.count <= 0) continue;
        let bucket = byUser.get(row.key);
        if (!bucket) {
          const u = directory.usersById.get(row.key);
          bucket = {
            userId: row.key,
            label: u?.name ?? u?.login ?? row.key.slice(0, 8),
            login: u?.login,
            perKind: {},
            perOrg: {},
            total: 0,
          };
          byUser.set(row.key, bucket);
        }
        bucket.perKind[kind] = (bucket.perKind[kind] ?? 0) + row.count;
        bucket.perOrg[orgId] = (bucket.perOrg[orgId] ?? 0) + row.count;
        bucket.total += row.count;
        mixTotals.set(kind, (mixTotals.get(kind) ?? 0) + row.count);
      }
    });

    const rows = [...byUser.values()].sort((a, b) => b.total - a.total);

    // Trend: sum across orgs per ISO-date bucket.
    const trendTotals = new Map<string, number>();
    trendQueries.forEach((q) => {
      if (!q.data) return;
      const series = (q.data as ReportResponse<CountRow[]>).rows ?? [];
      for (const r of series) {
        trendTotals.set(r.key, (trendTotals.get(r.key) ?? 0) + r.count);
      }
    });
    const trend: TrendBucket[] = [...trendTotals.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([date, events]) => ({ date, events }));

    const mix: MixSlice[] = [...mixTotals.entries()]
      .map(([kind, count]) => ({
        kind,
        label: ACTIVITY_LABEL.get(kind) ?? kind,
        count,
      }))
      .sort((a, b) => b.count - a.count);

    const grandTotal = rows.reduce((acc, r) => acc + r.total, 0);
    return {
      rows,
      trend,
      mix,
      grandTotal,
      activeContributors: rows.length,
    };
  }, [
    compositionQueries,
    trendQueries,
    selection.orgIds,
    selection.userIds,
    activeKinds,
    directory.usersById,
  ]);

  const firstSettled =
    compositionQueries.find((q) => q.data) ?? trendQueries.find((q) => q.data);
  const dataAsOf: DataAsOf | null = firstSettled?.data?.data_as_of ?? null;
  const loading =
    compositionQueries.some((q) => q.isPending) ||
    trendQueries.some((q) => q.isPending);
  const firstError =
    compositionQueries.find((q) => q.error)?.error ??
    trendQueries.find((q) => q.error)?.error ??
    null;
  const error = firstError instanceof Error ? firstError : null;

  return { data, loading, error, dataAsOf };
}
