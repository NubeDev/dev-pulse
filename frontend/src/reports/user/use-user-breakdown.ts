/**
 * `useUserBreakdown` — fans out `/reports/user/:user_id` calls with
 * `group_by=org` and `group_by=repo` (one per activity kind, per
 * grouping axis) and folds the responses into per-org and per-repo
 * rows with a per-kind breakdown — the same shape the leaderboard
 * table consumes, but pivoted around a single user.
 *
 * Fan-out: `N_kinds × 2` queries. With the default activity-kind
 * set (~6) this is ~12 cached queries, well within react-query's
 * comfort zone.
 */

import { useMemo } from "react";
import { useQueries } from "@tanstack/react-query";

import { api } from "../../api/client.js";
import type {
  CountRow,
  ReportParams,
  ReportResponse,
} from "../../api/client.js";

import { ACTIVITY_KINDS } from "../activity-types.js";

export interface UserBreakdownRow {
  /** Bucket key (org UUID or repo UUID). */
  id: string;
  /** Display label (org login / repo slug / fallback id slice). */
  label: string;
  /** Optional secondary line — org login for repo rows. */
  sublabel?: string;
  /** Per-activity-kind counts, keyed by snake_case `EventKind`. */
  perKind: Record<string, number>;
  /** Sum of `perKind` — the column the table ranks on. */
  total: number;
}

export interface UserBreakdownData {
  orgRows: ReadonlyArray<UserBreakdownRow>;
  repoRows: ReadonlyArray<UserBreakdownRow>;
  grandTotal: number;
}

export interface UseUserBreakdownArgs {
  userId: string | null;
  /** Window / lens / org-filter params — same envelope the page
   *  uses for its per-kind fan-out (`group_by` is overridden here). */
  params: ReportParams;
  /** Org UUID → display label (login). */
  orgLabels: ReadonlyMap<string, string>;
  /** Repo UUID → `{ label, orgId? }`. */
  repoLabels: ReadonlyMap<string, { label: string; orgId?: string }>;
  /** Skip fetching (used while the page is in mock mode). */
  disabled?: boolean;
}

export interface UseUserBreakdownResult {
  data: UserBreakdownData;
  loading: boolean;
}

function foldByKey(
  responses: ReadonlyArray<ReportResponse<CountRow[]> | undefined>,
): Map<string, Record<string, number>> {
  const byId = new Map<string, Record<string, number>>();
  responses.forEach((res, i) => {
    const kind = ACTIVITY_KINDS[i]?.key;
    if (!kind || !res) return;
    for (const row of res.rows ?? []) {
      if (row.count <= 0) continue;
      let bucket = byId.get(row.key);
      if (!bucket) {
        bucket = {};
        byId.set(row.key, bucket);
      }
      bucket[kind] = (bucket[kind] ?? 0) + row.count;
    }
  });
  return byId;
}

function buildRows(
  byId: Map<string, Record<string, number>>,
  resolve: (id: string) => { label: string; sublabel?: string },
): UserBreakdownRow[] {
  const rows: UserBreakdownRow[] = [];
  for (const [id, perKind] of byId) {
    const total = Object.values(perKind).reduce((acc, v) => acc + v, 0);
    if (total <= 0) continue;
    const { label, sublabel } = resolve(id);
    rows.push({ id, label, sublabel, perKind, total });
  }
  rows.sort((a, b) => b.total - a.total);
  return rows;
}

export function useUserBreakdown({
  userId,
  params,
  orgLabels,
  repoLabels,
  disabled,
}: UseUserBreakdownArgs): UseUserBreakdownResult {
  const enabled = !!userId && !disabled;

  const orgQueries = useQueries({
    queries: ACTIVITY_KINDS.map((k) => ({
      queryKey: [
        "report-user-breakdown",
        "org",
        userId,
        k.key,
        params,
      ] as const,
      enabled,
      staleTime: 60_000,
      queryFn: () => {
        if (!userId) return Promise.reject(new Error("no user"));
        return api.getReportUser(userId, {
          ...params,
          group_by: "org" as const,
          activity_types: [k.key],
        });
      },
    })),
  });

  const repoQueries = useQueries({
    queries: ACTIVITY_KINDS.map((k) => ({
      queryKey: [
        "report-user-breakdown",
        "repo",
        userId,
        k.key,
        params,
      ] as const,
      enabled,
      staleTime: 60_000,
      queryFn: () => {
        if (!userId) return Promise.reject(new Error("no user"));
        return api.getReportUser(userId, {
          ...params,
          group_by: "repo" as const,
          activity_types: [k.key],
        });
      },
    })),
  });

  const data = useMemo<UserBreakdownData>(() => {
    const orgById = foldByKey(orgQueries.map((q) => q.data));
    const repoById = foldByKey(repoQueries.map((q) => q.data));

    const orgRows = buildRows(orgById, (id) => ({
      label: orgLabels.get(id) ?? id.slice(0, 8),
    }));
    const repoRows = buildRows(repoById, (id) => {
      const info = repoLabels.get(id);
      const orgLabel = info?.orgId ? orgLabels.get(info.orgId) : undefined;
      return {
        label: info?.label ?? id.slice(0, 8),
        sublabel: orgLabel,
      };
    });
    const grandTotal = orgRows.reduce((acc, r) => acc + r.total, 0);

    return { orgRows, repoRows, grandTotal };
  }, [orgQueries, repoQueries, orgLabels, repoLabels]);

  const loading =
    enabled &&
    (orgQueries.some((q) => q.isPending) ||
      repoQueries.some((q) => q.isPending));

  return { data, loading };
}
