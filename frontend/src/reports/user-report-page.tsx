/**
 * `GET /reports/user/:user_id` — built on the shared `ReportShell`
 * (SectionCards + ChartAreaInteractive + DataTable from dashboard-01).
 */

import { useMemo, useState, useId } from "react";
import { useQueries, useQuery } from "@tanstack/react-query";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

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
import { ReportShell, type PerKind } from "./report-shell.jsx";
import {
  WindowPicker,
  FILTER_GRID_CLASS,
  defaultWindowState,
  windowStateToParams,
  type WindowState,
} from "./window-picker.jsx";

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
    const m = new Map<string, PerKind>();
    queries.forEach((q, i) => {
      const kind = ACTIVITY_KINDS[i]!;
      m.set(kind.key, {
        rows: q.data?.rows ?? [],
        loading: q.isPending,
      });
    });
    return m;
  }, [queries]);

  const firstSettled = queries.find((q) => q.data);
  const dataAsOf: DataAsOf | null = firstSettled?.data?.data_as_of ?? null;
  const anyLoading = queries.some((q) => q.isPending);

  function selectUser(id: string): void {
    setUserId(id);
    navigate(`/reports/user/${id}`);
  }

  const dropdownId = useId();
  const subjectLabel = activeUser?.name ?? activeUser?.login ?? null;

  return (
    <ReportShell
      title="User activity"
      description={
        <>
          <code className="font-mono text-xs">GET /reports/user/:user_id</code> ·
          headline · KPI tiles · area chart · per-kind table.
        </>
      }
      ready={!!activeUserId}
      emptyPrompt="Pick a user above to load the report."
      perKind={perKind}
      dataAsOf={dataAsOf}
      dataAsOfLoading={anyLoading && !dataAsOf}
      lens={lens}
      onLensChange={setLens}
      subjectLabel={subjectLabel}
      filters={
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
      }
    />
  );
}
