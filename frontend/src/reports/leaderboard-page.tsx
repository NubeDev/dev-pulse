/**
 * `GET /reports/org/:org_id?group_by=user&activity_types=…` rendered
 * as a ranked-user table — pragmatic leaderboard view built on the
 * existing per-org count endpoint. The full `dp-reports::leaderboard`
 * SQL machinery (PR #9) ships the library but doesn't expose a REST
 * route yet; this page uses the count-by-user reducer that already
 * runs server-side.
 */

import { useId, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

import { api } from "../api/client.js";
import type { OrgDto, UserDto } from "../api/client.js";

import { ACTIVITY_KINDS } from "./activity-types.js";
import { DataAsOfBanner } from "./data-as-of.jsx";
import {
  WindowPicker,
  FILTER_GRID_CLASS,
  defaultWindowState,
  windowStateToParams,
} from "./window-picker.jsx";

export function LeaderboardPage(): JSX.Element {
  const orgSelectId = useId();
  const kindSelectId = useId();

  const orgsQuery = useQuery({
    queryKey: ["orgs"],
    queryFn: () => api.listOrgs(),
  });
  const orgs: ReadonlyArray<OrgDto> = orgsQuery.data ?? [];

  const usersQuery = useQuery({
    queryKey: ["users"],
    queryFn: () => api.listUsers(),
  });
  const usersById = useMemo(() => {
    const m = new Map<string, UserDto>();
    (usersQuery.data ?? []).forEach((u) => m.set(u.id, u));
    return m;
  }, [usersQuery.data]);

  const [orgId, setOrgId] = useState<string | null>(null);
  const activeOrgId = orgId ?? orgs[0]?.id ?? null;
  const activeOrg = orgs.find((o) => o.id === activeOrgId);

  const [windowState, setWindowState] = useState(defaultWindowState());
  const [kind, setKind] = useState<string>("commit");

  const params = useMemo(
    () => ({
      ...windowStateToParams(windowState),
      scope_mode: "single_org" as const,
      group_by: "user" as const,
      activity_types: [kind],
    }),
    [windowState, kind],
  );

  const reportQuery = useQuery({
    queryKey: ["leaderboard", activeOrgId, kind, params],
    enabled: !!activeOrgId,
    queryFn: () => {
      if (!activeOrgId) return Promise.reject(new Error("no org"));
      return api.getReportOrg(activeOrgId, params);
    },
  });

  const rankedRows = useMemo(() => {
    const rows = reportQuery.data?.rows ?? [];
    return [...rows]
      .filter((r) => r.count > 0)
      .sort((a, b) => b.count - a.count);
  }, [reportQuery.data]);

  const kindLabel =
    ACTIVITY_KINDS.find((k) => k.key === kind)?.label ?? kind;

  return (
    <div className="flex flex-1 flex-col gap-4 p-4 md:p-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Leaderboard</h1>
        <p className="text-sm text-muted-foreground">
          <code className="font-mono text-xs">
            GET /reports/org/:org_id?group_by=user
          </code>{" "}
          · ranked contributors for one org / window / activity kind.
        </p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Filters</CardTitle>
        </CardHeader>
        <CardContent>
          <div className={FILTER_GRID_CLASS}>
            <div className="grid gap-1.5">
              <Label htmlFor={orgSelectId}>Org</Label>
              <Select
                value={activeOrgId ?? ""}
                onValueChange={(v) => setOrgId(v)}
                disabled={orgsQuery.isPending || orgs.length === 0}
              >
                <SelectTrigger id={orgSelectId} data-testid="leaderboard-org-select">
                  <SelectValue
                    placeholder={orgsQuery.isPending ? "Loading orgs…" : "Select an org"}
                  />
                </SelectTrigger>
                <SelectContent>
                  {orgs.map((o) => (
                    <SelectItem key={o.id} value={o.id}>
                      {o.name ?? o.login}
                      {o.name ? (
                        <span className="text-muted-foreground"> · {o.login}</span>
                      ) : null}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="grid gap-1.5">
              <Label htmlFor={kindSelectId}>Activity type</Label>
              <Select value={kind} onValueChange={setKind}>
                <SelectTrigger id={kindSelectId} data-testid="leaderboard-kind-select">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {ACTIVITY_KINDS.map((k) => (
                    <SelectItem key={k.key} value={k.key}>
                      {k.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <WindowPicker value={windowState} onChange={setWindowState} />
          </div>
        </CardContent>
      </Card>

      <DataAsOfBanner
        data={reportQuery.data?.data_as_of ?? null}
        loading={reportQuery.isPending}
      />

      <Card>
        <CardHeader>
          <CardTitle>
            Top contributors · {kindLabel}
            {activeOrg ? (
              <span className="text-muted-foreground">
                {" "}
                · {activeOrg.name ?? activeOrg.login}
              </span>
            ) : null}
          </CardTitle>
        </CardHeader>
        <CardContent>
          {!activeOrgId ? (
            <p className="text-sm text-muted-foreground">
              Pick an org above to load the leaderboard.
            </p>
          ) : reportQuery.isPending ? (
            <p className="text-sm text-muted-foreground">Loading…</p>
          ) : reportQuery.isError ? (
            <p className="text-sm text-destructive">
              Failed to load leaderboard: {String(reportQuery.error)}
            </p>
          ) : rankedRows.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              No {kindLabel.toLowerCase()} recorded for this org in the selected
              window.
            </p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-12">#</TableHead>
                  <TableHead>User</TableHead>
                  <TableHead className="text-right">{kindLabel}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {rankedRows.map((r, i) => {
                  const u = usersById.get(r.key);
                  const display = u
                    ? u.name
                      ? `${u.name} (${u.login})`
                      : u.login
                    : r.key;
                  return (
                    <TableRow key={r.key}>
                      <TableCell className="text-muted-foreground">
                        {i + 1}
                      </TableCell>
                      <TableCell>{display}</TableCell>
                      <TableCell className="text-right font-mono">
                        {r.count}
                      </TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
