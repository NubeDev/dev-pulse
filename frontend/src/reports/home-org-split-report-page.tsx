/**
 * `GET /reports/home-org-split` — cross-company executive view.
 *
 * Same skeleton as user/team/org: page heading → filter Card (no
 * entity picker, this report is the whole org population) →
 * Data-as-of Alert → results Card with TabsList + shadcn Table.
 *
 * Two queries fire in parallel:
 *
 *   - The home-org-split call itself (no `group_by`) drives the
 *     totals/share table.
 *   - A second call with `group_by=day` drives the per-org sparkline.
 *
 * No leaderboard — totals + share only (§4 design constraint). The
 * Share cell renders a shadcn Progress bar alongside the percent so
 * the relative shape reads at a glance without becoming a ranking.
 */

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@nube/starter-ui-kit/components/card";
import { Progress } from "@nube/starter-ui-kit/components/progress";

import { api } from "../api/client.js";
import type {
  CountRow,
  DataAsOf,
  HomeOrgSplitRow,
  OrgDto,
  ReportResponse,
  ScopeMode,
} from "../api/client.js";

import { DataAsOfBanner } from "./data-as-of.jsx";
import { LENSES, LensTabs } from "./lens-tabs.jsx";
import { Sparkline } from "./trend-sparkline.jsx";
import {
  WindowPicker,
  FILTER_GRID_CLASS,
  defaultWindowState,
  windowStateToParams,
  type WindowState,
} from "./window-picker.jsx";
import { PageHeading } from "../components/page-heading.jsx";
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
import { Skeleton } from "../components/skeleton.jsx";

const USE_MOCK = import.meta.env.VITE_USE_MOCK_REPORTS === "1";

function mockOrgs(): OrgDto[] {
  return [
    { id: "00000000-0000-0000-0000-0000000000a1", github_id: 101, login: "acme", name: "Acme" },
    { id: "00000000-0000-0000-0000-0000000000b2", github_id: 102, login: "globex", name: "Globex" },
    { id: "00000000-0000-0000-0000-0000000000c3", github_id: 103, login: "initech", name: "Initech" },
  ];
}

function mockSplit(): ReportResponse<HomeOrgSplitRow[]> {
  const day = 86_400_000;
  const today = Date.UTC(2026, 4, 20);
  const rows: HomeOrgSplitRow[] = [
    { user_id: "11111111-1111-1111-1111-111111111111", org_id: "00000000-0000-0000-0000-0000000000a1", count: 142 },
    { user_id: "22222222-2222-2222-2222-222222222222", org_id: "00000000-0000-0000-0000-0000000000a1", count: 87 },
    { user_id: "33333333-3333-3333-3333-333333333333", org_id: "00000000-0000-0000-0000-0000000000b2", count: 53 },
    { user_id: "44444444-4444-4444-4444-444444444444", org_id: "00000000-0000-0000-0000-0000000000b2", count: 41 },
    { user_id: "55555555-5555-5555-5555-555555555555", org_id: "00000000-0000-0000-0000-0000000000c3", count: 19 },
  ];
  return {
    rows,
    resolved_window: {
      start: new Date(today - 7 * day).toISOString(),
      end: new Date(today).toISOString(),
      label: "last_7_days",
      tz: "UTC",
    },
    data_as_of: {
      headline: new Date(today - 7 * 60_000).toISOString(),
      per_org: {},
      reconciler_latest: new Date(today - 7 * 60_000).toISOString(),
      webhook_latest: new Date(today - 60_000).toISOString(),
    },
  };
}

function mockTrend(): ReportResponse<HomeOrgSplitRow[]> {
  const day = 86_400_000;
  const today = Date.UTC(2026, 4, 20);
  const rows: HomeOrgSplitRow[] = [];
  const orgs = mockOrgs();
  for (const o of orgs) {
    for (let i = 6; i >= 0; i--) {
      const ts = new Date(today - i * day).toISOString();
      const seed = o.github_id + i * 11;
      rows.push({
        user_id: ts,
        org_id: o.id,
        count: Math.max(0, ((seed % 17) + (i % 3) * 4)),
      });
    }
  }
  return { ...mockSplit(), rows };
}

interface RolledRow {
  orgId: string;
  orgLabel: string;
  total: number;
  trend: ReadonlyArray<CountRow>;
}

function rollup(
  rows: ReadonlyArray<HomeOrgSplitRow>,
  trendRows: ReadonlyArray<HomeOrgSplitRow>,
  orgs: ReadonlyArray<OrgDto>,
): RolledRow[] {
  const totals = new Map<string, number>();
  for (const r of rows) {
    totals.set(r.org_id, (totals.get(r.org_id) ?? 0) + r.count);
  }

  const trendByOrg = new Map<string, Map<string, number>>();
  for (const r of trendRows) {
    const inner = trendByOrg.get(r.org_id) ?? new Map<string, number>();
    const bucketKey = /^\d{4}-\d{2}-\d{2}T/.test(r.user_id) ? r.user_id : "_";
    inner.set(bucketKey, (inner.get(bucketKey) ?? 0) + r.count);
    trendByOrg.set(r.org_id, inner);
  }

  const orgById = new Map(orgs.map((o) => [o.id, o]));
  return [...totals.entries()]
    .map(([orgId, total]) => {
      const o = orgById.get(orgId);
      const label = o?.name ?? o?.login ?? orgId.slice(0, 8);
      const buckets = trendByOrg.get(orgId) ?? new Map<string, number>();
      const trend: CountRow[] = [...buckets.entries()]
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([key, count]) => ({ key, count }));
      return { orgId, orgLabel: label, total, trend };
    })
    .sort((a, b) => b.total - a.total);
}

export function HomeOrgSplitReportPage(): JSX.Element {
  const [windowState, setWindowState] = useState<WindowState>(defaultWindowState());
  const [lens, setLens] = useState<ScopeMode>("per_org_split");

  const params = useMemo(
    () => ({
      ...windowStateToParams(windowState),
      scope_mode: lens,
    }),
    [windowState, lens],
  );

  const orgsQuery = useQuery({
    queryKey: ["orgs"],
    queryFn: () => (USE_MOCK ? Promise.resolve(mockOrgs()) : api.listOrgs()),
  });
  const orgs: ReadonlyArray<OrgDto> = orgsQuery.data ?? [];

  const totalsQuery = useQuery({
    queryKey: ["report-home-org-split", "totals", params],
    queryFn: () => {
      if (USE_MOCK) return Promise.resolve(mockSplit());
      return api.getReportHomeOrgSplit(params);
    },
  });

  const trendQuery = useQuery({
    queryKey: ["report-home-org-split", "trend", params],
    queryFn: () => {
      if (USE_MOCK) return Promise.resolve(mockTrend());
      return api.getReportHomeOrgSplit({ ...params, group_by: "day" });
    },
  });

  const rolled = useMemo(
    () => rollup(totalsQuery.data?.rows ?? [], trendQuery.data?.rows ?? [], orgs),
    [totalsQuery.data, trendQuery.data, orgs],
  );

  const grandTotal = rolled.reduce((acc, r) => acc + r.total, 0);

  const dataAsOf: DataAsOf | null =
    totalsQuery.data?.data_as_of ?? trendQuery.data?.data_as_of ?? null;
  const anyLoading = totalsQuery.isPending || trendQuery.isPending;

  const headline = useMemo(() => {
    if (rolled.length === 0) {
      return "No cross-company activity in the selected window.";
    }
    const lensLabel = LENSES.find((l) => l.value === lens)?.label ?? "";
    const top = rolled.slice(0, 3);
    const parts = top.map((r) => {
      const pct = grandTotal > 0 ? Math.round((r.total / grandTotal) * 100) : 0;
      return `${r.orgLabel} ${pct}%`;
    });
    const joined = parts.length === 1
      ? parts[0]
      : `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
    return `Across ${rolled.length} home-orgs, contribution split: ${joined} (${lensLabel}).`;
  }, [rolled, grandTotal, lens]);

  return (
    <div className="grid gap-6">
      <PageHeading
        title="Home-org split"
        description={
          <>
            <code className="font-mono text-xs">GET /reports/home-org-split</code> ·
            cross-company contribution proportions. No leaderboard — totals + share only.
          </>
        }
      />

      <Card>
        <CardHeader>
          <CardTitle className="text-lg font-medium">Filters</CardTitle>
        </CardHeader>
        <CardContent>
          <div className={FILTER_GRID_CLASS}>
            <WindowPicker value={windowState} onChange={setWindowState} />
          </div>
        </CardContent>
      </Card>

      <DataAsOfBanner data={dataAsOf} loading={anyLoading && !dataAsOf} />

      <Card>
        <CardHeader>
          <CardTitle className="text-lg font-medium">Contribution split</CardTitle>
        </CardHeader>
        <CardContent>
          <LensTabs value={lens} onChange={setLens}>
            <p
              data-testid="headline"
              className="text-sm text-foreground"
            >
              {anyLoading && !dataAsOf ? "Loading cross-company split…" : headline}
            </p>

            {anyLoading && rolled.length === 0 ? (
              <div className="grid gap-2">
                <Skeleton className="h-9 w-full" />
                <Skeleton className="h-9 w-full" />
                <Skeleton className="h-9 w-full" />
              </div>
            ) : rolled.length === 0 ? (
              <Empty>
                <EmptyHeader>
                  <EmptyTitle>No cross-company activity</EmptyTitle>
                  <EmptyDescription>
                    No home-org contributions were recorded in the selected window.
                  </EmptyDescription>
                </EmptyHeader>
              </Empty>
            ) : (
              <div className="overflow-hidden rounded-xl border bg-card">
                <Table data-testid="home-org-split-table">
                  <TableHeader className="bg-muted/40">
                    <TableRow>
                      <TableHead>Home org</TableHead>
                      <TableHead className="text-right">Total</TableHead>
                      <TableHead className="text-right">Share</TableHead>
                      <TableHead className="text-right">Trend</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {rolled.map((row) => {
                      const pct = grandTotal > 0 ? (row.total / grandTotal) * 100 : 0;
                      return (
                        <TableRow key={row.orgId}>
                          <TableCell className="font-medium">{row.orgLabel}</TableCell>
                          <TableCell className="text-right tabular-nums">{row.total}</TableCell>
                          <TableCell className="text-right tabular-nums">
                            <div className="inline-flex items-center justify-end gap-2">
                              <Progress
                                value={pct}
                                aria-hidden
                                className="h-1.5 w-20"
                              />
                              <span className="w-12 text-right">{pct.toFixed(1)}%</span>
                            </div>
                          </TableCell>
                          <TableCell className="text-right">
                            <span className="ml-auto inline-flex h-8 w-24 items-center justify-end align-middle">
                              <Sparkline
                                points={row.trend.map((r) => ({ key: r.key, value: r.count }))}
                                width={96}
                                height={32}
                                ariaLabel={`${row.orgLabel} trend, ${row.trend.length} buckets, total ${row.total}`}
                              />
                            </span>
                          </TableCell>
                        </TableRow>
                      );
                    })}
                  </TableBody>
                </Table>
              </div>
            )}
          </LensTabs>
        </CardContent>
      </Card>
    </div>
  );
}
