/**
 * Per-org and per-repo breakdown tables for a single user — pivots
 * the leaderboard's "user ranked by activity across orgs" view to
 * "orgs and repos ranked by this user's activity in each".
 *
 * Shares the visual vocabulary with the leaderboard table (Card +
 * Progress share bar + per-kind columns) so the user report has the
 * same look-and-feel as the dashboard the brief calls out.
 */

import { useMemo } from "react";

import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

import { ACTIVITY_KINDS } from "../activity-types.js";

import type { UserBreakdownRow } from "./use-user-breakdown.js";

interface BreakdownTableProps {
  title: string;
  description: string;
  emptyMessage: string;
  loading: boolean;
  rows: ReadonlyArray<UserBreakdownRow>;
  subjectColumn: string;
  testId: string;
  limit?: number;
}

function BreakdownTable({
  title,
  description,
  emptyMessage,
  loading,
  rows,
  subjectColumn,
  testId,
  limit = 25,
}: BreakdownTableProps): JSX.Element {
  const visible = useMemo(() => rows.slice(0, limit), [rows, limit]);
  const grandTotal = useMemo(
    () => rows.reduce((acc, r) => acc + r.total, 0),
    [rows],
  );
  const activeKinds = useMemo(() => {
    const set = new Set<string>();
    for (const r of visible) {
      for (const [k, v] of Object.entries(r.perKind)) {
        if (v > 0) set.add(k);
      }
    }
    return ACTIVITY_KINDS.filter((k) => set.has(k.key));
  }, [visible]);

  return (
    <Card data-testid={testId}>
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        <CardDescription>
          {loading
            ? "Loading…"
            : visible.length === rows.length
              ? `${visible.length} ${visible.length === 1 ? "row" : "rows"} · ${description}`
              : `Top ${visible.length} of ${rows.length} · ${description}`}
        </CardDescription>
      </CardHeader>
      <CardContent className="px-2 sm:px-6">
        {loading && rows.length === 0 ? (
          <div className="space-y-2 py-2">
            <Skeleton className="h-8 w-full" />
            <Skeleton className="h-8 w-full" />
            <Skeleton className="h-8 w-full" />
          </div>
        ) : visible.length === 0 ? (
          <p className="py-6 text-sm text-muted-foreground">{emptyMessage}</p>
        ) : (
          <div className="overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-10">#</TableHead>
                  <TableHead>{subjectColumn}</TableHead>
                  <TableHead className="w-[180px]">Share</TableHead>
                  {activeKinds.map((k) => (
                    <TableHead
                      key={k.key}
                      className="text-right whitespace-nowrap"
                    >
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
                  return (
                    <TableRow
                      key={r.id}
                      data-testid={`${testId}-row-${i}`}
                    >
                      <TableCell className="text-muted-foreground tabular-nums">
                        {i + 1}
                      </TableCell>
                      <TableCell>
                        <div className="flex flex-col">
                          <span className="font-medium">{r.label}</span>
                          {r.sublabel ? (
                            <span className="text-xs text-muted-foreground">
                              {r.sublabel}
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

export interface UserBreakdownTablesProps {
  orgRows: ReadonlyArray<UserBreakdownRow>;
  repoRows: ReadonlyArray<UserBreakdownRow>;
  loading: boolean;
  subjectLabel: string | null;
}

export function UserBreakdownTables({
  orgRows,
  repoRows,
  loading,
  subjectLabel,
}: UserBreakdownTablesProps): JSX.Element {
  const who = subjectLabel ?? "this user";
  return (
    <div className="flex flex-col gap-4 px-4 lg:gap-6 lg:px-6">
      <BreakdownTable
        testId="user-breakdown-orgs"
        title="By organisation"
        description={`Where ${who} contributes across orgs.`}
        emptyMessage={`No org-level activity recorded for ${who} in this window.`}
        loading={loading}
        rows={orgRows}
        subjectColumn="Org"
      />
      <BreakdownTable
        testId="user-breakdown-repos"
        title="By repository"
        description={`Per-repo breakdown for ${who}.`}
        emptyMessage={`No repo-level activity recorded for ${who} in this window.`}
        loading={loading}
        rows={repoRows}
        subjectColumn="Repo"
      />
    </div>
  );
}
