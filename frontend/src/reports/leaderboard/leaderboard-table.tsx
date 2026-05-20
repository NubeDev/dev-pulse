/**
 * Ranked contributors table — one row per user with a per-kind
 * breakdown, share-of-total bar, and per-org chip strip.
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
import type { DirectoryMaps, LeaderUserRow } from "./types.js";

export interface LeaderboardTableProps {
  rows: ReadonlyArray<LeaderUserRow>;
  grandTotal: number;
  directory: DirectoryMaps;
  /** Which kinds to render as columns. Defaults to the kinds that
   *  have at least one event in the visible rows. */
  kinds?: ReadonlyArray<string>;
  /** Max rows to display (default 25). */
  limit?: number;
}

export function LeaderboardTable({
  rows,
  grandTotal,
  directory,
  kinds,
  limit = 25,
}: LeaderboardTableProps): JSX.Element {
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
    <Card data-testid="leaderboard-table-card">
      <CardHeader>
        <CardTitle>Ranked contributors</CardTitle>
        <CardDescription>
          {visible.length === rows.length
            ? `${visible.length} contributor${visible.length === 1 ? "" : "s"}`
            : `Top ${visible.length} of ${rows.length} contributors`}
          {" · share of total + per-activity breakdown."}
        </CardDescription>
      </CardHeader>
      <CardContent className="px-2 sm:px-6">
        {visible.length === 0 ? (
          <p className="py-6 text-sm text-muted-foreground">
            No contributors match the current filters.
          </p>
        ) : (
          <div className="overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-10">#</TableHead>
                  <TableHead>User</TableHead>
                  <TableHead className="w-[180px]">Share</TableHead>
                  <TableHead>Orgs</TableHead>
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
                  const orgIds = Object.keys(r.perOrg);
                  return (
                    <TableRow key={r.userId} data-testid={`leader-row-${i}`}>
                      <TableCell className="text-muted-foreground tabular-nums">
                        {i + 1}
                      </TableCell>
                      <TableCell>
                        <div className="flex flex-col">
                          <span className="font-medium">{r.label}</span>
                          {r.login && r.login !== r.label ? (
                            <span className="text-xs text-muted-foreground">
                              {r.login}
                            </span>
                          ) : null}
                        </div>
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
                        <div className="flex flex-wrap gap-1">
                          {orgIds.map((orgId) => {
                            const o = directory.orgsById.get(orgId);
                            return (
                              <Badge
                                key={orgId}
                                variant="secondary"
                                className="text-[0.6875rem] font-normal"
                              >
                                {o?.login ?? orgId.slice(0, 6)}
                                <span className="ml-1 tabular-nums text-muted-foreground">
                                  {r.perOrg[orgId]}
                                </span>
                              </Badge>
                            );
                          })}
                        </div>
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
