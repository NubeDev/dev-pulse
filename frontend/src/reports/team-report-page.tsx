/**
 * `GET /reports/team/:team_id` — built on the shared `ReportShell`.
 * Two-step selector (org → team).
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
  OrgDto,
  ReportResponse,
  ScopeMode,
  TeamDto,
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
  const base = kind.length * 3;
  for (let i = 6; i >= 0; i--) {
    const value = Math.max(0, base + ((i * 5 + kind.charCodeAt(0)) % 9) - 2);
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
      headline: new Date(today - 4 * 60_000).toISOString(),
      per_org: {},
      reconciler_latest: new Date(today - 4 * 60_000).toISOString(),
      webhook_latest: new Date(today - 20_000).toISOString(),
    },
  };
}

function mockOrgs(): OrgDto[] {
  return [
    { id: "00000000-0000-0000-0000-0000000000a1", github_id: 101, login: "acme", name: "Acme" },
    { id: "00000000-0000-0000-0000-0000000000b2", github_id: 102, login: "globex", name: "Globex" },
  ];
}

function mockTeams(orgId: string): TeamDto[] {
  return [
    { id: "00000000-0000-0000-0000-0000000000t1", org_id: orgId, github_id: 201, slug: "platform", name: "Platform" },
    { id: "00000000-0000-0000-0000-0000000000t2", org_id: orgId, github_id: 202, slug: "product", name: "Product" },
  ];
}

function teamIdFromRoute(route: string): string | null {
  const path = route.replace(/^#/, "").replace(/^\/+/, "").split("/");
  if (path[0] === "reports" && path[1] === "team" && path[2]) return path[2];
  return null;
}

export function TeamReportPage(): JSX.Element {
  const route = useRoute();
  const routeTeamId = teamIdFromRoute(route);

  const orgsQuery = useQuery({
    queryKey: ["orgs"],
    queryFn: () => (USE_MOCK ? Promise.resolve(mockOrgs()) : api.listOrgs()),
  });
  const orgs: ReadonlyArray<OrgDto> = orgsQuery.data ?? [];

  const [orgId, setOrgId] = useState<string | null>(null);
  const activeOrgId = orgId ?? orgs[0]?.id ?? null;

  const teamsQuery = useQuery({
    queryKey: ["teams", activeOrgId],
    enabled: !!activeOrgId,
    queryFn: () => {
      if (!activeOrgId) return Promise.resolve<TeamDto[]>([]);
      return USE_MOCK ? Promise.resolve(mockTeams(activeOrgId)) : api.listTeams(activeOrgId);
    },
  });
  const teams: ReadonlyArray<TeamDto> = teamsQuery.data ?? [];

  const [teamId, setTeamId] = useState<string | null>(routeTeamId);
  const activeTeamId = teamId ?? routeTeamId ?? teams[0]?.id ?? null;
  const activeTeam = teams.find((t) => t.id === activeTeamId);

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
      queryKey: ["report-team", activeTeamId, k.key, params],
      enabled: !!activeTeamId,
      queryFn: () => {
        if (!activeTeamId) return Promise.reject(new Error("no team"));
        if (USE_MOCK) return Promise.resolve(mockResponse(k.key));
        return api.getReportTeam(activeTeamId, {
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

  function selectTeam(id: string): void {
    setTeamId(id);
    navigate(`/reports/team/${id}`);
  }
  function selectOrg(id: string): void {
    setOrgId(id);
    setTeamId(null);
  }

  const orgDropdownId = useId();
  const teamDropdownId = useId();
  const subjectLabel = activeTeam ? `Team ${activeTeam.name}` : null;

  return (
    <ReportShell
      title="Team activity"
      description={
        <>
          <code className="font-mono text-xs">GET /reports/team/:team_id</code> ·
          headline · KPI tiles · area chart · per-kind table.
        </>
      }
      ready={!!activeTeamId}
      emptyPrompt="Pick an org and team above to load the report."
      perKind={perKind}
      dataAsOf={dataAsOf}
      dataAsOfLoading={anyLoading && !dataAsOf}
      lens={lens}
      onLensChange={setLens}
      subjectLabel={subjectLabel}
      filters={
        <div className={FILTER_GRID_CLASS}>
          <div className="grid min-w-0 gap-1.5">
            <Label htmlFor={orgDropdownId}>Org</Label>
            <Select
              value={activeOrgId ?? ""}
              onValueChange={selectOrg}
              disabled={orgsQuery.isPending || orgs.length === 0}
            >
              <SelectTrigger className="w-full" id={orgDropdownId} data-testid="team-org-select">
                <SelectValue placeholder={orgsQuery.isPending ? "Loading orgs…" : "Select an org"} />
              </SelectTrigger>
              <SelectContent>
                {orgs.map((o) => (
                  <SelectItem key={o.id} value={o.id}>
                    {o.name ?? o.login}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="grid min-w-0 gap-1.5">
            <Label htmlFor={teamDropdownId}>Team</Label>
            <Select
              value={activeTeamId ?? ""}
              onValueChange={selectTeam}
              disabled={teamsQuery.isPending || teams.length === 0}
            >
              <SelectTrigger className="w-full" id={teamDropdownId} data-testid="team-select">
                <SelectValue placeholder={teamsQuery.isPending ? "Loading teams…" : "Select a team"} />
              </SelectTrigger>
              <SelectContent>
                {teams.map((t) => (
                  <SelectItem key={t.id} value={t.id}>
                    {t.name}
                    <span className="text-muted-foreground"> · {t.slug}</span>
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
