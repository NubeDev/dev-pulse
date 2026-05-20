/**
 * Per-repo breakdown table for the repo-activity report. One row
 * per repo with a per-kind breakdown, share-of-total bar, and an
 * org chip — visually consistent with the contributor leaderboard.
 */

import { useMemo } from "react";

import { Badge } from "@/components/ui/badge";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

import { ACTIVITY_KINDS } from "../activity-types.js";
import type { RepoActivityRow } from "./types.js";

export interface RepoBreakdownTableProps {
  rows: ReadonlyArray<RepoActivityRow>;
  grandTotal: number;
  /** Which kinds to render as columns. Defaults to kinds with at
   *  least one event in the visible rows. */
  kinds?: ReadonlyArray<string>;
  /** Max rows to display (default 25). */
  limit?: number;
  /** Called when the user clicks a row — opens the per-repo
   *  contributor drilldown. */
  onSelectRepo?: (repoId: string) => void;
  /** Currently focused repo, highlighted in the table. */
  selectedRepoId?: string | null;
}

export function RepoBreakdownTable({
  rows,
  grandTotal,
  kinds,
  limit = 25,
  onSelectRepo,
  selectedRepoId,
}: RepoBreakdownTableProps): JSX.Element {
  const visible = useMemo(() => rows.slice(0, limit), [rows, limit]);

  const activeKinds = useMemo(() => {
    if (kinds && kinds.length > 0) {
      return ACTIVITY_KINDS.filter((k) => kinds.includes(k.key));
    }
    const set = new Set<string>();
    for (const r of visible) {
      for (const k of Object.keys(r.perKind)) {
        if ((r.perKind[k] ?? 0) > 0) set.add(k);
      }
    }
    return ACTIVITY_KINDS.filter((k) => set.has(k.key));
  }, [visible, kinds]);

  return (
    <Card data-testid="repo-breakdown-table-card">
      <CardHeader>
        <CardTitle>Activity by repo</CardTitle>
        <CardDescription>
          {visible.length === rows.length
            ? `${visible.length} repo${visible.length === 1 ? "" : "s"}`
            : `Top ${visible.length} of ${rows.length} repos`}
          {onSelectRepo
            ? " · click a row to see who's been working on that repo."
            : " · share of total + per-activity breakdown."}
        </CardDescription>
      </CardHeader>
      <CardContent className="px-2 sm:px-6">
        {visible.length === 0 ? (
          <p className="py-6 text-sm text-muted-foreground">
            No repos match the current filters.
          </p>
        ) : (
          <div className="overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-10">#</TableHead>
                  <TableHead>Repo</TableHead>
                  <TableHead className="w-[180px]">Share</TableHead>
                  <TableHead>Org</TableHead>
                  {activeKinds.map((k) => (
                    <TableHead key={k.key} className="text-right whitespace-nowrap">
                      {k.label}
                    </TableHead>
                  ))}
                  <TableHead className="text-right">Total</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {visible.map((r, i) => {
                  const share =
                    grandTotal > 0 ? (r.total / grandTotal) * 100 : 0;
                  const focused = selectedRepoId === r.repoId;
                  return (
                    <TableRow
                      key={r.repoId}
                      data-testid={`repo-row-${i}`}
                      data-state={focused ? "selected" : undefined}
                      onClick={
                        onSelectRepo ? () => onSelectRepo(r.repoId) : undefined
                      }
                      className={
                        onSelectRepo
                          ? "cursor-pointer hover:bg-muted/50"
                          : undefined
                      }
                    >
                      <TableCell className="text-muted-foreground tabular-nums">
                        {i + 1}
                      </TableCell>
                      <TableCell>
                        <span className="font-mono text-xs">{r.label}</span>
                      </TableCell>
                      <TableCell>
                        <div className="flex items-center gap-2">
                          <Progress value={share} className="h-1.5 w-24" />
                          <span className="text-xs tabular-nums text-muted-foreground">
                            {share.toFixed(1)}%
                          </span>
                        </div>
                      </TableCell>
                      <TableCell>
                        {r.orgLogin ? (
                          <Badge
                            variant="secondary"
                            className="text-[0.6875rem] font-normal"
                          >
                            {r.orgLogin}
                          </Badge>
                        ) : (
                          <span className="text-muted-foreground">—</span>
                        )}
                      </TableCell>
                      {activeKinds.map((k) => (
                        <TableCell
                          key={k.key}
                          className="text-right tabular-nums"
                        >
                          {r.perKind[k.key] ? (
                            r.perKind[k.key]
                          ) : (
                            <span className="text-muted-foreground">·</span>
                          )}
                        </TableCell>
                      ))}
                      <TableCell className="text-right font-mono font-medium">
                        {r.total.toLocaleString()}
                      </TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
