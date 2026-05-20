/**
 * `GET /reports/org/:org_id` page — same skeleton as user/team/
 * home-org-split. Single Org selector populated from `GET /orgs`.
 */

import { useMemo, useState, useId } from "react";
import { useQueries, useQuery } from "@tanstack/react-query";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@nube/starter-ui-kit/components/card";
import { Label } from "@nube/starter-ui-kit/components/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@nube/starter-ui-kit/components/select";

import { api } from "../api/client.js";
import type {
  CountRow,
  DataAsOf,
  OrgDto,
  ReportResponse,
  ScopeMode,
} from "../api/client.js";
import { navigate, useRoute } from "../routes.js";

import { ACTIVITY_KINDS } from "./activity-types.js";
import { ActivityTable, buildActivityRows } from "./activity-table.jsx";
import { DataAsOfBanner } from "./data-as-of.jsx";
import { LENSES, LensTabs } from "./lens-tabs.jsx";
import {
  WindowPicker,
  FILTER_GRID_CLASS,
  defaultWindowState,
  windowStateToParams,
  type WindowState,
} from "./window-picker.jsx";
import { PageHeading } from "../components/page-heading.jsx";

const USE_MOCK = import.meta.env.VITE_USE_MOCK_REPORTS === "1";

function mockResponse(kind: string): ReportResponse<CountRow[]> {
  const day = 86_400_000;
  const today = Date.UTC(2026, 4, 20);
  const rows: CountRow[] = [];
  const base = kind.length * 8;
  for (let i = 6; i >= 0; i--) {
    const value = Math.max(0, base + ((i * 7 + kind.charCodeAt(0)) % 13) - 3);
    rows.push({ key: new Date(today - i * day).toISOString(), count: value });
  }
  return {
    rows,
    resolved_window: {
      start: new Date(today - 7 * day).toISOString(),
      end: new Date(today).toISOString(),
      label: "last_7_days",
      tz: "UTC",
    },
    data_as_of: {
      headline: new Date(today - 6 * 60_000).toISOString(),
      per_org: {},
      reconciler_latest: new Date(today - 6 * 60_000).toISOString(),
      webhook_latest: new Date(today - 45_000).toISOString(),
    },
  };
}

function mockOrgs(): OrgDto[] {
  return [
    { id: "00000000-0000-0000-0000-0000000000a1", github_id: 101, login: "acme", name: "Acme" },
    { id: "00000000-0000-0000-0000-0000000000b2", github_id: 102, login: "globex", name: "Globex" },
  ];
}

function orgIdFromRoute(route: string): string | null {
  const path = route.replace(/^#/, "").replace(/^\/+/, "").split("/");
  if (path[0] === "reports" && path[1] === "org" && path[2]) return path[2];
  return null;
}

export function OrgReportPage(): JSX.Element {
  const route = useRoute();
  const routeOrgId = orgIdFromRoute(route);

  const orgsQuery = useQuery({
    queryKey: ["orgs"],
    queryFn: () => (USE_MOCK ? Promise.resolve(mockOrgs()) : api.listOrgs()),
  });
  const orgs: ReadonlyArray<OrgDto> = orgsQuery.data ?? [];

  const [orgId, setOrgId] = useState<string | null>(routeOrgId);
  const activeOrgId = orgId ?? routeOrgId ?? orgs[0]?.id ?? null;
  const activeOrg = orgs.find((o) => o.id === activeOrgId);

  const [windowState, setWindowState] = useState<WindowState>(defaultWindowState());
  const [lens, setLens] = useState<ScopeMode>("single_org");

  const params = useMemo(
    () => ({
      ...windowStateToParams(windowState),
      scope_mode: lens,
      group_by: "day" as const,
    }),
    [windowState, lens],
  );

  const queries = useQueries({
    queries: ACTIVITY_KINDS.map((k) => ({
      queryKey: ["report-org", activeOrgId, k.key, params],
      enabled: !!activeOrgId,
      queryFn: () => {
        if (!activeOrgId) return Promise.reject(new Error("no org"));
        if (USE_MOCK) return Promise.resolve(mockResponse(k.key));
        return api.getReportOrg(activeOrgId, {
          ...params,
          activity_types: [k.key],
        });
      },
    })),
  });

  const perKind = useMemo(() => {
    const m = new Map<string, { rows: ReadonlyArray<CountRow>; loading: boolean }>();
    queries.forEach((q, i) => {
      const kind = ACTIVITY_KINDS[i]!;
      m.set(kind.key, {
        rows: q.data?.rows ?? [],
        loading: q.isPending,
      });
    });
    return m;
  }, [queries]);
  const tableRows = useMemo(() => buildActivityRows(perKind), [perKind]);

  const firstSettled = queries.find((q) => q.data);
  const dataAsOf: DataAsOf | null = firstSettled?.data?.data_as_of ?? null;
  const anyLoading = queries.some((q) => q.isPending);

  const headline = useMemo(() => {
    if (!activeOrg) return "";
    const top = [...tableRows]
      .filter((r) => r.total > 0)
      .sort((a, b) => b.total - a.total)
      .slice(0, 3);
    if (top.length === 0) {
      return `Org ${activeOrg.name ?? activeOrg.login} recorded no activity in the selected window.`;
    }
    const lensLabel = LENSES.find((l) => l.value === lens)?.label ?? "";
    const parts = top.map((r) => `${r.total} ${r.label.toLowerCase()}`);
    const joined = parts.length === 1
      ? parts[0]
      : `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
    return `Org ${activeOrg.name ?? activeOrg.login} recorded ${joined} (${lensLabel}).`;
  }, [activeOrg, tableRows, lens]);

  function selectOrg(id: string): void {
    setOrgId(id);
    navigate(`/reports/org/${id}`);
  }

  const dropdownId = useId();
  return (
    <div className="grid gap-6">
      <PageHeading
        title="Org activity report"
        description={
          <>
            <code className="font-mono text-xs">GET /reports/org/:org_id</code> · headline + table + trend.
          </>
        }
      />

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Filters</CardTitle>
        </CardHeader>
        <CardContent>
          <div className={FILTER_GRID_CLASS}>
            <div className="grid gap-1.5">
              <Label htmlFor={dropdownId}>Org</Label>
              <Select
                value={activeOrgId ?? ""}
                onValueChange={selectOrg}
                disabled={orgsQuery.isPending || orgs.length === 0}
              >
                <SelectTrigger id={dropdownId} data-testid="org-select">
                  <SelectValue placeholder={orgsQuery.isPending ? "Loading orgs…" : "Select an org"} />
                </SelectTrigger>
                <SelectContent>
                  {orgs.map((o) => (
                    <SelectItem key={o.id} value={o.id}>
                      {o.name ?? o.login}
                      {o.name ? <span className="text-muted-foreground"> · {o.login}</span> : null}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <WindowPicker value={windowState} onChange={setWindowState} />
          </div>
        </CardContent>
      </Card>

      <DataAsOfBanner data={dataAsOf} loading={anyLoading && !dataAsOf} />

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Activity</CardTitle>
        </CardHeader>
        <CardContent>
          <LensTabs value={lens} onChange={setLens}>
            {!activeOrgId ? (
              <p className="text-sm text-muted-foreground">
                Pick an org above to load the report.
              </p>
            ) : (
              <>
                <p
                  data-testid="headline"
                  className="text-sm text-foreground"
                >
                  {anyLoading && !dataAsOf ? "Loading report…" : headline}
                </p>
                <ActivityTable rows={tableRows} />
              </>
            )}
          </LensTabs>
        </CardContent>
      </Card>
    </div>
  );
}
