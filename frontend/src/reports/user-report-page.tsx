/**
 * `GET /reports/user/:user_id` page — SCOPE §11.5 "headline + table
 * + trend" shape, three-lens toggle (§8.1), "Data as of" banner per
 * §0.3.
 *
 * The layout:
 *
 *   [user dropdown]  [Data as of …]
 *   [window picker]
 *   [tabs: SingleOrg | AllOrgsCombined | PerOrgSplit]
 *     - headline sentence
 *     - sortable activity-table (per-kind totals + sparkline trend)
 *
 * Per-row data: one `getReportUser` query per activity kind, fired
 * in parallel via `useQueries` with `group_by=day` so the row's
 * trend column has bucketed data. The freshness banner shares the
 * `data_as_of` from whichever query lands first — they all read the
 * same store snapshot so the timestamps are consistent.
 *
 * Mock-data smoke: when `VITE_USE_MOCK_REPORTS=1` (set in tests /
 * Storybook-style harness), the queries are short-circuited to a
 * deterministic fixture so the page still renders fully without
 * dp-server running. The real `useQueries` shape is preserved so
 * production wiring is one env-flag flip away.
 */

import { useMemo, useState, useId } from "react";
import { useQueries, useQuery } from "@tanstack/react-query";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@nube/starter-ui-kit/components/card";
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
  defaultWindowState,
  windowStateToParams,
  type WindowState,
} from "./window-picker.jsx";

const USE_MOCK = import.meta.env.VITE_USE_MOCK_REPORTS === "1";

/** Deterministic mock — used by the stage-4 smoke harness so the
 *  page renders without dp-server. The shape mirrors a real
 *  `getReportUser` response with `group_by=day` over 7 daily
 *  buckets. */
function mockResponse(kind: string): ReportResponse<CountRow[]> {
  const day = 86_400_000;
  const today = Date.UTC(2026, 4, 20); // 2026-05-20, matches harness clock.
  const rows: CountRow[] = [];
  // Seed each kind with a different base so the table is non-trivial.
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

/** Parse `#/reports/user/<uuid>` -> `<uuid>` (or `null` for a bare
 *  `#/reports`). */
function userIdFromRoute(route: string): string | null {
  const path = route.replace(/^#/, "").replace(/^\/+/, "").split("/");
  // ["reports", "user", "<uuid>"] -> path[2]
  if (path[0] === "reports" && path[1] === "user" && path[2]) return path[2];
  return null;
}

export function UserReportPage(): JSX.Element {
  const route = useRoute();
  const routeUserId = userIdFromRoute(route);

  // User dropdown population. The dropdown is the source of truth
  // for the active user — the route updates as a side-effect so a
  // refresh keeps the same user selected.
  const usersQuery = useQuery({
    queryKey: ["users"],
    queryFn: () => (USE_MOCK ? Promise.resolve(mockUsers()) : api.listUsers()),
  });

  const users: ReadonlyArray<UserDto> = usersQuery.data ?? [];
  const [userId, setUserId] = useState<string | null>(routeUserId);
  const activeUserId = userId ?? routeUserId ?? users[0]?.id ?? null;
  const activeUser = users.find((u) => u.id === activeUserId);

  // Window + lens state.
  const [windowState, setWindowState] = useState<WindowState>(defaultWindowState());
  const [lens, setLens] = useState<ScopeMode>("single_org");

  // Per-activity-kind queries — one `useQuery` per kind, fanned out
  // through `useQueries` so the table can stream in as each kind
  // resolves.
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

  // Build the table rows from the per-kind query results.
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

  // Pick the freshness banner from the first resolved query — every
  // dp-rest report reads the same `data_as_of()` snapshot per request
  // so the timestamps are consistent across the fanout.
  const firstSettled = queries.find((q) => q.data);
  const dataAsOf: DataAsOf | null = firstSettled?.data?.data_as_of ?? null;
  const anyLoading = queries.some((q) => q.isPending);

  // Headline sentence — top three kinds by count.
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

  // User-dropdown change — also push to the route so deep links work.
  function selectUser(id: string): void {
    setUserId(id);
    navigate(`/reports/user/${id}`);
  }

  const dropdownId = useId();
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
            <CardTitle>User activity report</CardTitle>
            <CardDescription>
              <code>GET /reports/user/:user_id</code> · SCOPE §11.5 headline + table + trend.
            </CardDescription>
          </div>
          <DataAsOfBanner data={dataAsOf} loading={anyLoading && !dataAsOf} />
        </div>
      </CardHeader>
      <CardContent style={{ display: "grid", gap: "1rem" }}>
        <div style={{ display: "grid", gap: "0.375rem", maxWidth: "24rem" }}>
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
                  {u.name ? <span style={{ color: "var(--muted-foreground)" }}> · {u.login}</span> : null}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <WindowPicker value={windowState} onChange={setWindowState} />

        <LensTabs value={lens} onChange={setLens}>
          {!activeUserId ? (
            <p style={{ color: "var(--muted-foreground)" }}>
              Pick a user above to load the report.
            </p>
          ) : (
            <>
              <p
                data-testid="headline"
                style={{
                  fontSize: "1rem",
                  margin: "0 0 1rem",
                  color: "var(--foreground)",
                }}
              >
                {anyLoading && !dataAsOf ? "Loading report…" : headline}
              </p>
              <ActivityTable rows={tableRows} />
            </>
          )}
        </LensTabs>
      </CardContent>
    </Card>
  );
}
