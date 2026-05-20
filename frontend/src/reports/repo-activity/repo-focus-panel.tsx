/**
 * `RepoFocusPanel` — drilldown for a single repo. Answers the
 * question "which developers have been doing all the work on this
 * repo?" by fanning out one `/reports/org/:org_id?repos=<repoId>`
 * call per activity kind (with `group_by=user`) and folding the
 * results into the same `LeaderUserRow` shape the page-level
 * contributor chart uses.
 *
 * Sits below the per-repo breakdown table on the repo-activity
 * page and is only mounted when the user has clicked a row in
 * that table.
 */

import { useMemo } from "react";
import { useQueries } from "@tanstack/react-query";
import { X } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

import { api } from "../../api/client.js";
import type {
  CountRow,
  ReportParams,
  ReportResponse,
  RepoSummaryDto,
} from "../../api/client.js";

import { ACTIVITY_KINDS } from "../activity-types.js";
import {
  ContributorBarChart,
  LeaderboardTable,
  type DirectoryMaps,
  type LeaderUserRow,
} from "../leaderboard/index.js";

export interface RepoFocusPanelProps {
  /** Repo to drill into. */
  repo: RepoSummaryDto;
  /** Activity kinds the page is filtered to. Empty == every kind. */
  kinds: ReadonlyArray<string>;
  /** Shared window params (label / tz / anchor / custom range). */
  windowParams: Pick<
    ReportParams,
    "window_label" | "tz" | "anchor" | "custom_start" | "custom_end"
  >;
  /** User + org directory — the chart and table need it to resolve
   *  display labels for each bucket key. */
  directory: DirectoryMaps;
  /** Called when the user dismisses the panel. */
  onClose: () => void;
}

export function RepoFocusPanel({
  repo,
  kinds,
  windowParams,
  directory,
  onClose,
}: RepoFocusPanelProps): JSX.Element {
  const activeKinds = useMemo(
    () => (kinds.length > 0 ? [...kinds] : ACTIVITY_KINDS.map((k) => k.key)),
    [kinds],
  );

  // One composition call per kind, restricted to a single repo.
  const queries = useQueries({
    queries: activeKinds.map((kind) => ({
      queryKey: [
        "repo-focus",
        repo.org_id,
        repo.id,
        kind,
        windowParams,
      ] as const,
      queryFn: () =>
        api.getReportOrg(repo.org_id, {
          ...windowParams,
          scope_mode: "single_org" as const,
          group_by: "user" as const,
          repos: [repo.id],
          activity_types: [kind],
        }),
      staleTime: 60_000,
    })),
  });

  const { rows, grandTotal } = useMemo(() => {
    const byUser = new Map<string, LeaderUserRow>();
    queries.forEach((q, idx) => {
      const kind = activeKinds[idx];
      if (!kind || !q.data) return;
      const series = (q.data as ReportResponse<CountRow[]>).rows ?? [];
      for (const row of series) {
        if (row.count <= 0) continue;
        let bucket = byUser.get(row.key);
        if (!bucket) {
          const u = directory.usersById.get(row.key);
          bucket = {
            userId: row.key,
            label: u?.name ?? u?.login ?? row.key.slice(0, 8),
            login: u?.login,
            perKind: {},
            perOrg: { [repo.org_id]: 0 },
            total: 0,
          };
          byUser.set(row.key, bucket);
        }
        bucket.perKind[kind] = (bucket.perKind[kind] ?? 0) + row.count;
        bucket.perOrg[repo.org_id] = (bucket.perOrg[repo.org_id] ?? 0) + row.count;
        bucket.total += row.count;
      }
    });
    const sorted = [...byUser.values()].sort((a, b) => b.total - a.total);
    return {
      rows: sorted,
      grandTotal: sorted.reduce((acc, r) => acc + r.total, 0),
    };
  }, [queries, activeKinds, directory.usersById, repo.org_id]);

  const loading = queries.some((q) => q.isPending);
  const errored = queries.find((q) => q.error)?.error;

  return (
    <Card
      data-testid="repo-focus-panel"
      className="border-primary/40 ring-1 ring-primary/20"
    >
      <CardHeader className="flex flex-row items-start justify-between gap-2 space-y-0">
        <div>
          <CardTitle className="font-mono text-base">{repo.slug}</CardTitle>
          <CardDescription>
            Who&apos;s been working on this repo · {rows.length} contributor
            {rows.length === 1 ? "" : "s"} · {grandTotal.toLocaleString()} event
            {grandTotal === 1 ? "" : "s"} in window.
          </CardDescription>
        </div>
        <Button
          variant="ghost"
          size="sm"
          onClick={onClose}
          aria-label="Close repo drilldown"
          data-testid="repo-focus-close"
        >
          <X className="size-4" />
        </Button>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {errored ? (
          <p className="text-sm text-destructive">
            Failed to load: {(errored as Error).message}
          </p>
        ) : loading && rows.length === 0 ? (
          <p className="text-sm text-muted-foreground">Loading…</p>
        ) : rows.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No contributors recorded activity on this repo in the selected
            window.
          </p>
        ) : (
          <>
            <ContributorBarChart rows={rows} />
            <LeaderboardTable
              rows={rows}
              grandTotal={grandTotal}
              directory={directory}
            />
          </>
        )}
      </CardContent>
    </Card>
  );
}
