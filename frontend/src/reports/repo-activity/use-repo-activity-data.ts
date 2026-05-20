/**
 * `useRepoActivityData` — fans out `/reports/org/:org_id?repos=…`
 * calls and folds the results into the `RepoActivityData` shape
 * every chart and table on the repo-activity page consumes.
 *
 * Query layout (per selected org):
 *
 *   - `composition[org × kind]` — `group_by=user`, single kind, with
 *     the `repos=` filter applied. Drives the contributor chart, the
 *     per-user table, and the activity-mix donut.
 *   - `repoBreakdown[org]`       — `group_by=repo`, every selected
 *     kind. Drives the per-repo breakdown table.
 *   - `trend[org]`               — `group_by=day`, every selected
 *     kind. Drives the daily trend area chart.
 *
 * The composition fan-out matches the leaderboard's so the existing
 * chart components can be reused unchanged.
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
import type { LeaderUserRow } from "../leaderboard/types.js";
import type {
  RepoActivityData,
  RepoActivityDirectory,
  RepoActivityRow,
  RepoActivitySelection,
} from "./types.js";
import type { MixSlice, TrendBucket } from "../leaderboard/types.js";

export interface UseRepoActivityArgs {
  selection: RepoActivitySelection;
  windowParams: Pick<
    ReportParams,
    "window_label" | "tz" | "anchor" | "custom_start" | "custom_end"
  >;
  directory: RepoActivityDirectory;
}

export interface UseRepoActivityResult {
  data: RepoActivityData;
  loading: boolean;
  error: Error | null;
  dataAsOf: DataAsOf | null;
}

const ACTIVITY_LABEL = new Map<string, string>(
  ACTIVITY_KINDS.map((k) => [k.key, k.label]),
);

const EMPTY_DATA: RepoActivityData = {
  userRows: [],
  repoRows: [],
  trend: [],
  mix: [],
  grandTotal: 0,
  activeContributors: 0,
  activeRepos: 0,
};

export function useRepoActivityData({
  selection,
  windowParams,
  directory,
}: UseRepoActivityArgs): UseRepoActivityResult {
  const activeKinds = useMemo(
    () =>
      selection.kinds.length > 0
        ? selection.kinds
        : ACTIVITY_KINDS.map((k) => k.key),
    [selection.kinds],
  );

  // Stable wire-side serialisation of the repo filter. Empty array
  // ⇒ omit the param entirely so the server falls back to "all
  // repos inside the selected orgs".
  const reposParam = useMemo(
    () => (selection.repoIds.length > 0 ? [...selection.repoIds] : undefined),
    [selection.repoIds],
  );

  // -- composition fan-out (org × kind, group_by=user) ----------------------
  const compositionQueries = useQueries({
    queries: selection.orgIds.flatMap((orgId) =>
      activeKinds.map((kind) => ({
        queryKey: [
          "repo-activity",
          "composition",
          orgId,
          kind,
          reposParam ?? null,
          windowParams,
        ] as const,
        queryFn: () =>
          api.getReportOrg(orgId, {
            ...windowParams,
            scope_mode: "single_org" as const,
            group_by: "user" as const,
            activity_types: [kind],
            ...(reposParam ? { repos: reposParam } : {}),
          }),
        staleTime: 60_000,
      })),
    ),
  });

  // -- repo-breakdown fan-out (one per org, group_by=repo, all kinds) -------
  const repoQueries = useQueries({
    queries: selection.orgIds.flatMap((orgId) =>
      activeKinds.map((kind) => ({
        queryKey: [
          "repo-activity",
          "by-repo",
          orgId,
          kind,
          reposParam ?? null,
          windowParams,
        ] as const,
        queryFn: () =>
          api.getReportOrg(orgId, {
            ...windowParams,
            scope_mode: "single_org" as const,
            group_by: "repo" as const,
            activity_types: [kind],
            ...(reposParam ? { repos: reposParam } : {}),
          }),
        staleTime: 60_000,
      })),
    ),
  });

  // -- trend fan-out (one per org, group_by=day) ----------------------------
  const trendQueries = useQueries({
    queries: selection.orgIds.map((orgId) => ({
      queryKey: [
        "repo-activity",
        "trend",
        orgId,
        activeKinds,
        reposParam ?? null,
        windowParams,
      ] as const,
      queryFn: () =>
        api.getReportOrg(orgId, {
          ...windowParams,
          scope_mode: "single_org" as const,
          group_by: "day" as const,
          activity_types: [...activeKinds],
          ...(reposParam ? { repos: reposParam } : {}),
        }),
      staleTime: 60_000,
    })),
  });

  const data = useMemo<RepoActivityData>(() => {
    if (selection.orgIds.length === 0) {
      return EMPTY_DATA;
    }

    // -- user rows ---------------------------------------------------------
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

    const userRows = [...byUser.values()].sort((a, b) => b.total - a.total);

    // -- repo rows ---------------------------------------------------------
    const byRepo = new Map<string, RepoActivityRow>();

    repoQueries.forEach((q, idx) => {
      const orgIdx = Math.floor(idx / activeKinds.length);
      const kindIdx = idx % activeKinds.length;
      const orgId = selection.orgIds[orgIdx];
      const kind = activeKinds[kindIdx];
      if (!orgId || !kind || !q.data) return;
      const rows = (q.data as ReportResponse<CountRow[]>).rows ?? [];
      for (const row of rows) {
        if (row.count <= 0) continue;
        let bucket = byRepo.get(row.key);
        if (!bucket) {
          const r = directory.reposById.get(row.key);
          const orgLogin = r ? directory.orgsById.get(r.org_id)?.login ?? r.org_login : undefined;
          bucket = {
            repoId: row.key,
            label: r?.slug ?? row.key.slice(0, 8),
            orgId: r?.org_id,
            orgLogin,
            perKind: {},
            total: 0,
            contributors: 0,
          };
          byRepo.set(row.key, bucket);
        }
        bucket.perKind[kind] = (bucket.perKind[kind] ?? 0) + row.count;
        bucket.total += row.count;
      }
    });

    const repoRows = [...byRepo.values()].sort((a, b) => b.total - a.total);

    // -- trend -------------------------------------------------------------
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

    const grandTotal = userRows.reduce((acc, r) => acc + r.total, 0);

    return {
      userRows,
      repoRows,
      trend,
      mix,
      grandTotal,
      activeContributors: userRows.length,
      activeRepos: repoRows.length,
    };
  }, [
    compositionQueries,
    repoQueries,
    trendQueries,
    selection.orgIds,
    activeKinds,
    directory.usersById,
    directory.reposById,
    directory.orgsById,
  ]);

  const firstSettled =
    compositionQueries.find((q) => q.data) ??
    repoQueries.find((q) => q.data) ??
    trendQueries.find((q) => q.data);
  const dataAsOf: DataAsOf | null = firstSettled?.data?.data_as_of ?? null;
  const loading =
    compositionQueries.some((q) => q.isPending) ||
    repoQueries.some((q) => q.isPending) ||
    trendQueries.some((q) => q.isPending);
  const firstError =
    compositionQueries.find((q) => q.error)?.error ??
    repoQueries.find((q) => q.error)?.error ??
    trendQueries.find((q) => q.error)?.error ??
    null;
  const error = firstError instanceof Error ? firstError : null;

  return { data, loading, error, dataAsOf };
}
