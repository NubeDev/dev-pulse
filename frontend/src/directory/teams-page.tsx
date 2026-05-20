/**
 * Directory · Teams page — `GET /teams?org_id=…`.
 *
 * dp-rest requires `org_id` on the teams endpoint, so this page
 * gates the table behind an org selector. The list is filtered on
 * the server, not the client — once an org is picked, we fetch
 * once and render.
 */

import { useState } from "react";
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

import { useDirectory, useTeamsForOrg } from "./use-directory.js";

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
      <CardContent style={{ display: "grid", gap: "1rem" }}>
        <div style={{ display: "grid", gap: "0.25rem", maxWidth: "20rem" }}>
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
          <p style={{ color: "oklch(0.5 0.2 25)" }}>
            Failed to load orgs: {dir.error}
          </p>
        ) : null}
        {teamsState.error ? (
          <p data-testid="teams-error" style={{ color: "oklch(0.5 0.2 25)" }}>
            Failed to load teams: {teamsState.error}
          </p>
        ) : null}

        {orgId === null ? (
          <p style={{ color: "var(--muted-foreground)" }}>
            Pick an org to see its teams.
          </p>
        ) : teamsState.loading ? (
          <p style={{ color: "var(--muted-foreground)" }}>Loading teams…</p>
        ) : teamsState.teams.length === 0 ? (
          <p data-testid="teams-empty" style={{ color: "var(--muted-foreground)" }}>
            No teams in this org yet.
          </p>
        ) : (
          <div
            data-testid="teams-table"
            role="table"
            style={{
              display: "grid",
              gap: "0.25rem",
              gridTemplateColumns: "minmax(8rem, 1fr) minmax(12rem, 1.5fr)",
              alignItems: "center",
              fontSize: "0.875rem",
            }}
          >
            <Header>Slug</Header>
            <Header>Name</Header>
            {[...teamsState.teams]
              .sort((a, b) => a.slug.localeCompare(b.slug))
              .map((t) => (
                <div key={t.id} role="row" style={{ display: "contents" }} data-team-id={t.id}>
                  <Cell><code>{t.slug}</code></Cell>
                  <Cell>{t.name}</Cell>
                </div>
              ))}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function Header({ children }: { children: React.ReactNode }): JSX.Element {
  return (
    <div
      role="columnheader"
      style={{
        padding: "0.5rem 0.625rem",
        fontWeight: 600,
        borderBottom: "1px solid var(--border)",
        color: "var(--muted-foreground)",
        fontSize: "0.8125rem",
        textTransform: "uppercase",
        letterSpacing: "0.02em",
      }}
    >
      {children}
    </div>
  );
}

function Cell({ children }: { children: React.ReactNode }): JSX.Element {
  return (
    <div
      role="cell"
      style={{
        padding: "0.625rem",
        borderBottom: "1px solid var(--border)",
      }}
    >
      {children}
    </div>
  );
}
