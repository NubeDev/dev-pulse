/**
 * `GET /reports/home-org-split` — the cross-company executive view.
 *
 * Each row of the dp-rest response is `{ user_id, org_id, count }`
 * (see `HomeOrgSplitRowSchema`); `org_id` is the user's *home* org
 * for the row's window. The exec view rolls those up by `home_org`
 * to show contribution proportions per company, alongside a per-org
 * trend sparkline so the operator can see momentum, not just totals.
 *
 * Layout follows the SCOPE §11.5 contract — headline + table +
 * trend, three-lens toggle (§8.1), "Data as of" banner per §0.3.
 * No single-score, no leaderboard (§4 design constraint): the table
 * orders by raw count but the share column is rendered alongside so
 * the relative shape is visible without becoming a ranking gesture.
 *
 * Two queries fire in parallel:
 *
 *   - The home-org-split call itself (no `group_by` — one row per
 *     `(user, home_org)` over the window total) drives the
 *     totals/share table.
 *   - A second call with `group_by=day` and the same other params
 *     drives the per-org sparkline; the response is bucketed by org
 *     because that's the only dimension the row carries that we can
 *     fan out without losing the cross-company shape.
 *
 * Mock-data smoke: when `VITE_USE_MOCK_REPORTS=1`, the queries are
 * short-circuited so the page renders without dp-server.
 */

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@nube/starter-ui-kit/components/card";
import { Progress } from "@nube/starter-ui-kit/components/progress";
import { cn } from "@nube/starter-ui-kit/lib/utils";

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
  defaultWindowState,
  windowStateToParams,
  type WindowState,
} from "./window-picker.jsx";

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
  // A handful of synthetic users spread across the three mock orgs.
  // The shape is `(user_id, home_org_id, count)` — the page rolls up
  // by home_org_id.
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
  // Bucketed mock — the page slices it back into per-org series by
  // emitting one `(user, org, count)` row per (org, day). `user_id`
  // is ignored downstream; we reuse the same UUID for every bucket.
  // `count` is the daily aggregate so the sparkline shows real shape.
  const day = 86_400_000;
  const today = Date.UTC(2026, 4, 20);
  const rows: HomeOrgSplitRow[] = [];
  const orgs = mockOrgs();
  for (const o of orgs) {
    for (let i = 6; i >= 0; i--) {
      const ts = new Date(today - i * day).toISOString();
      const seed = o.github_id + i * 11;
      // Emit one row per (org, day). Reuse `user_id` to encode the
      // bucket key — we keep it stable so the trend reducer below
      // can recover the day bucket.
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
  /** Daily trend, oldest → newest. */
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

  // Trend reducer: trendRows reuses `user_id` as the bucket key (see
  // mockTrend), but a real dp-rest response will fan rows out per
  // (user × org × day). We aggregate by (org_id, user_id) treating
  // user_id-shaped-as-RFC3339 as the bucket if it parses, else group
  // every row of the org into a single bucket.
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

  const HEADER_CLASS =
    "border-b border-border px-3 py-2 text-left text-xs font-semibold uppercase tracking-wider text-muted-foreground";
  const CELL_CLASS = "border-b border-border px-3 py-2 align-middle text-sm";
  const NUM_CLASS = cn(CELL_CLASS, "text-right tabular-nums");
  const HEADER_RIGHT_CLASS = cn(HEADER_CLASS, "text-right");

  return (
    <Card>
      <CardHeader>
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="grid gap-1">
            <CardTitle className="text-2xl font-semibold tracking-tight">
              Home-org split (cross-company)
            </CardTitle>
            <CardDescription className="text-muted-foreground">
              <code>GET /reports/home-org-split</code> · contribution proportions per company.
              No leaderboard — totals + share only (§4 design constraint).
            </CardDescription>
          </div>
          <DataAsOfBanner data={dataAsOf} loading={anyLoading && !dataAsOf} />
        </div>
      </CardHeader>
      <CardContent className="grid gap-4">
        <WindowPicker value={windowState} onChange={setWindowState} />

        <LensTabs value={lens} onChange={setLens}>
          <p
            data-testid="headline"
            className="mb-2 text-base text-foreground"
          >
            {anyLoading && !dataAsOf ? "Loading cross-company split…" : headline}
          </p>
          <div className="overflow-hidden rounded-md border border-border bg-card">
            <table
              className="w-full border-collapse"
              data-testid="home-org-split-table"
            >
              <thead className="bg-muted">
                <tr>
                  <th className={HEADER_CLASS}>Home org</th>
                  <th className={HEADER_RIGHT_CLASS}>Total</th>
                  <th className={HEADER_RIGHT_CLASS}>Share</th>
                  <th className={HEADER_RIGHT_CLASS}>Trend</th>
                </tr>
              </thead>
              <tbody>
                {rolled.length === 0 ? (
                  <tr>
                    <td colSpan={4} className={cn(CELL_CLASS, "text-muted-foreground")}>
                      {anyLoading ? "Loading…" : "No data in window."}
                    </td>
                  </tr>
                ) : (
                  rolled.map((row) => {
                    const pct = grandTotal > 0 ? (row.total / grandTotal) * 100 : 0;
                    return (
                      <tr key={row.orgId}>
                        <td className={CELL_CLASS}>{row.orgLabel}</td>
                        <td className={NUM_CLASS}>{row.total}</td>
                        <td className={NUM_CLASS}>
                          <div className="inline-flex items-center justify-end gap-2">
                            <Progress
                              value={pct}
                              aria-hidden
                              className="h-2 w-16"
                            />
                            <span>{pct.toFixed(1)}%</span>
                          </div>
                        </td>
                        <td className={cn(NUM_CLASS, "w-40")}>
                          <Sparkline
                            points={row.trend.map((r) => ({ key: r.key, value: r.count }))}
                            ariaLabel={`${row.orgLabel} trend, ${row.trend.length} buckets, total ${row.total}`}
                          />
                        </td>
                      </tr>
                    );
                  })
                )}
              </tbody>
            </table>
          </div>
        </LensTabs>
      </CardContent>
    </Card>
  );
}
