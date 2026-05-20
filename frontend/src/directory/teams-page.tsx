/**
 * Directory · Teams page — `GET /teams?org_id=…`.
 *
 * dp-rest requires `org_id` on the teams endpoint, so this page
 * gates the table behind an org selector. The list is filtered on
 * the server, not the client — once an org is picked, we fetch
 * once and render.
 *
 * Markup: semantic `<table>` with shared `HEADER_CLASS` / `CELL_CLASS`
 * Tailwind constants (the kit doesn't ship a Table primitive).
 */

import { useState } from "react";
import { Alert, AlertDescription, AlertTitle } from "@nube/starter-ui-kit/components/alert";
import {
  Card,
  CardContent,
  CardDescription,
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

import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from "../components/empty.jsx";
import { useDirectory, useTeamsForOrg } from "./use-directory.js";

const HEADER_CLASS =
  "border-b border-border px-3 py-2 text-left text-xs font-semibold uppercase tracking-wider text-muted-foreground";
const CELL_CLASS = "border-b border-border px-3 py-2 align-middle text-sm";

export function TeamsPage(): JSX.Element {
  const dir = useDirectory();
  const [orgId, setOrgId] = useState<string | null>(null);
  const teamsState = useTeamsForOrg(orgId);

  return (
    <Card>
      <CardHeader>
        <CardTitle>Teams</CardTitle>
        <CardDescription>
          <code>GET /teams?org_id=…</code> · teams are always scoped
          to a single org, so pick one to load the list.
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4">
        <div className="grid max-w-xs gap-1">
          <Label htmlFor="teams-org-select">Org</Label>
          <Select
            value={orgId ?? undefined}
            onValueChange={(v) => setOrgId(v)}
          >
            <SelectTrigger id="teams-org-select" data-testid="teams-org-select">
              <SelectValue placeholder={dir.loading ? "Loading orgs…" : "Select an org"} />
            </SelectTrigger>
            <SelectContent>
              {dir.orgs.map((o) => (
                <SelectItem key={o.id} value={o.id}>
                  {o.login}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        {dir.error ? (
          <Alert variant="destructive">
            <AlertTitle>Failed to load orgs</AlertTitle>
            <AlertDescription>{dir.error}</AlertDescription>
          </Alert>
        ) : null}
        {teamsState.error ? (
          <Alert variant="destructive" data-testid="teams-error">
            <AlertTitle>Failed to load teams</AlertTitle>
            <AlertDescription>{teamsState.error}</AlertDescription>
          </Alert>
        ) : null}

        {orgId === null ? (
          <p className="text-muted-foreground">
            Pick an org to see its teams.
          </p>
        ) : teamsState.loading ? (
          <p className="text-muted-foreground">Loading teams…</p>
        ) : teamsState.teams.length === 0 ? (
          <Empty data-testid="teams-empty">
            <EmptyHeader>
              <EmptyTitle>No teams in this org yet</EmptyTitle>
              <EmptyDescription>
                Teams appear here once GitHub returns them for the selected
                org.
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <div className="overflow-hidden rounded-md border border-border bg-card">
            <table data-testid="teams-table" className="w-full border-collapse">
              <thead className="bg-muted">
                <tr>
                  <th className={HEADER_CLASS}>Slug</th>
                  <th className={HEADER_CLASS}>Name</th>
                </tr>
              </thead>
              <tbody>
                {[...teamsState.teams]
                  .sort((a, b) => a.slug.localeCompare(b.slug))
                  .map((t) => (
                    <tr key={t.id} data-team-id={t.id}>
                      <td className={CELL_CLASS}>
                        <code>{t.slug}</code>
                      </td>
                      <td className={CELL_CLASS}>{t.name}</td>
                    </tr>
                  ))}
              </tbody>
            </table>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
