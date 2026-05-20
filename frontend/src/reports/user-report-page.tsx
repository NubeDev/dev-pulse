/**
 * `GET /reports/user/:user_id` page — SCOPE §11.5 "headline + table
 * + trend" shape, three-lens toggle (§8.1), "Data as of" banner per
 * §0.3.
 *
 * Skeleton (shared with team / org / home-org-split):
 *
 *   1. PageHeading lockup (h1 + muted description).
 *   2. Filter Card — User picker, Window, Time zone, Anchor — laid
 *      out in one responsive grid of Label+Select pairs.
 *   3. Data-as-of Alert with the staleness Badge.
 *   4. Results Card — TabsList (three lenses, segmented) over the
 *      activity Table (per-kind totals + sparkline).
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
  ReportResponse,
  ScopeMode,
  UserDto,
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
  const base = kind.length;
  for (let i = 6; i >= 0; i--) {
    const value = Math.max(0, base + ((i * 3 + kind.charCodeAt(0)) % 5) - 1);
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
      headline: new Date(today - 5 * 60_000).toISOString(),
      per_org: {},
      reconciler_latest: new Date(today - 5 * 60_000).toISOString(),
      webhook_latest: new Date(today - 30_000).toISOString(),
    },
  };
}

function mockUsers(): UserDto[] {
  return [
    { id: "11111111-1111-1111-1111-111111111111", github_id: 1, login: "alice", name: "Alice Example", email: "alice@example.com" },
    { id: "22222222-2222-2222-2222-222222222222", github_id: 2, login: "bob", name: "Bob Example", email: "bob@example.com" },
  ];
}

function userIdFromRoute(route: string): string | null {
  const path = route.replace(/^#/, "").replace(/^\/+/, "").split("/");
  if (path[0] === "reports" && path[1] === "user" && path[2]) return path[2];
  return null;
}

export function UserReportPage(): JSX.Element {
  const route = useRoute();
  const routeUserId = userIdFromRoute(route);

  const usersQuery = useQuery({
    queryKey: ["users"],
    queryFn: () => (USE_MOCK ? Promise.resolve(mockUsers()) : api.listUsers()),
  });

  const users: ReadonlyArray<UserDto> = usersQuery.data ?? [];
  const [userId, setUserId] = useState<string | null>(routeUserId);
  const activeUserId = userId ?? routeUserId ?? users[0]?.id ?? null;
  const activeUser = users.find((u) => u.id === activeUserId);

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
      queryKey: ["report-user", activeUserId, k.key, params],
      enabled: !!activeUserId,
      queryFn: () => {
        if (!activeUserId) return Promise.reject(new Error("no user"));
        if (USE_MOCK) return Promise.resolve(mockResponse(k.key));
        return api.getReportUser(activeUserId, {
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
    if (!activeUser) return "";
    const top = [...tableRows]
      .filter((r) => r.total > 0)
      .sort((a, b) => b.total - a.total)
      .slice(0, 3);
    if (top.length === 0) {
      return `${activeUser.login} had no recorded activity in the selected window.`;
    }
    const lensLabel = LENSES.find((l) => l.value === lens)?.label ?? "";
    const parts = top.map((r) => `${r.total} ${r.label.toLowerCase()}`);
    const joined = parts.length === 1
      ? parts[0]
      : `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
    return `${activeUser.name ?? activeUser.login} recorded ${joined} (${lensLabel}).`;
  }, [activeUser, tableRows, lens]);

  function selectUser(id: string): void {
    setUserId(id);
    navigate(`/reports/user/${id}`);
  }

  const dropdownId = useId();
  return (
    <div className="grid gap-6">
      <PageHeading
        title="User activity report"
        description={
          <>
            <code className="font-mono text-xs">GET /reports/user/:user_id</code> · headline + table + trend.
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
              <Label htmlFor={dropdownId}>User</Label>
              <Select
                value={activeUserId ?? ""}
                onValueChange={selectUser}
                disabled={usersQuery.isPending || users.length === 0}
              >
                <SelectTrigger id={dropdownId} data-testid="user-select">
                  <SelectValue placeholder={usersQuery.isPending ? "Loading users…" : "Select a user"} />
                </SelectTrigger>
                <SelectContent>
                  {users.map((u) => (
                    <SelectItem key={u.id} value={u.id}>
                      {u.name ?? u.login}
                      {u.name ? <span className="text-muted-foreground"> · {u.login}</span> : null}
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
            {!activeUserId ? (
              <p className="text-sm text-muted-foreground">
                Pick a user above to load the report.
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
