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
    <Card>
      <CardHeader>
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "flex-start",
            gap: "1rem",
            flexWrap: "wrap",
          }}
        >
          <div>
            <CardTitle>Home-org split (cross-company)</CardTitle>
            <CardDescription>
              <code>GET /reports/home-org-split</code> · contribution proportions per company.
              No leaderboard — totals + share only (§4 design constraint).
            </CardDescription>
          </div>
          <DataAsOfBanner data={dataAsOf} loading={anyLoading && !dataAsOf} />
        </div>
      </CardHeader>
      <CardContent style={{ display: "grid", gap: "1rem" }}>
        <WindowPicker value={windowState} onChange={setWindowState} />

        <LensTabs value={lens} onChange={setLens}>
          <p
            data-testid="headline"
            style={{
              fontSize: "1rem",
              margin: "0 0 1rem",
              color: "var(--foreground)",
            }}
          >
            {anyLoading && !dataAsOf ? "Loading cross-company split…" : headline}
          </p>
          <div
            style={{
              border: "1px solid var(--border)",
              borderRadius: "var(--radius-md, 0.5rem)",
              background: "var(--card)",
              overflow: "hidden",
            }}
          >
            <table
              style={{ width: "100%", borderCollapse: "collapse" }}
              data-testid="home-org-split-table"
            >
              <thead style={{ background: "var(--muted)" }}>
                <tr>
                  <th style={headerStyle}>Home org</th>
                  <th style={{ ...headerStyle, textAlign: "right" }}>Total</th>
                  <th style={{ ...headerStyle, textAlign: "right" }}>Share</th>
                  <th style={{ ...headerStyle, textAlign: "right" }}>Trend</th>
                </tr>
              </thead>
              <tbody>
                {rolled.length === 0 ? (
                  <tr>
                    <td colSpan={4} style={{ ...cellStyle, color: "var(--muted-foreground)" }}>
                      {anyLoading ? "Loading…" : "No data in window."}
                    </td>
                  </tr>
                ) : (
                  rolled.map((row) => {
                    const pct = grandTotal > 0 ? (row.total / grandTotal) * 100 : 0;
                    return (
                      <tr key={row.orgId}>
                        <td style={cellStyle}>{row.orgLabel}</td>
                        <td style={numStyle}>{row.total}</td>
                        <td style={numStyle}>
                          <div
                            style={{
                              display: "inline-flex",
                              alignItems: "center",
                              gap: "0.5rem",
                              justifyContent: "flex-end",
                            }}
                          >
                            <span
                              aria-hidden
                              style={{
                                width: "4rem",
                                height: "0.5rem",
                                background: "var(--muted)",
                                borderRadius: "999px",
                                overflow: "hidden",
                                display: "inline-block",
                              }}
                            >
                              <span
                                style={{
                                  display: "block",
                                  height: "100%",
                                  width: `${pct.toFixed(1)}%`,
                                  background: "var(--primary)",
                                }}
                              />
                            </span>
                            <span>{pct.toFixed(1)}%</span>
                          </div>
                        </td>
                        <td style={{ ...numStyle, width: "10rem" }}>
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
