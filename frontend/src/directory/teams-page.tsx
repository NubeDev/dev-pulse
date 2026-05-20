/**
 * Directory · Teams page — `GET /teams?org_id=…`.
 *
 * dp-rest requires `org_id` on the teams endpoint, so this page
 * gates the table behind an org selector. The list is filtered on
 * the server, not the client — once an org is picked, we fetch
 * once and render.
 *
 * Layout (stage 4 visual rewrite): PageHeading lockup, filter Card
 * with the org selector, results Card with the shadcn-shaped
 * `Table` primitive (local — the kit doesn't ship one).
 */

import { useState } from "react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from "../components/empty.jsx";
import { PageHeading } from "../components/page-heading.jsx";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "../components/table.jsx";
import { useDirectory, useTeamsForOrg } from "./use-directory.js";

export function TeamsPage(): JSX.Element {
  const dir = useDirectory();
  const [orgId, setOrgId] = useState<string | null>(null);
  const teamsState = useTeamsForOrg(orgId);

  return (
    <div className="flex flex-col gap-4 px-4 md:gap-6 lg:px-6">
      <PageHeading
        title="Teams"
        description={
          <>
            <code className="font-mono text-xs">GET /teams?org_id=…</code> ·
            teams are always scoped to a single org, so pick one to load the
            list.
          </>
        }
      />

      <Card>
        <CardHeader>
          <CardTitle className="text-lg font-medium">Filters</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="grid max-w-xs gap-1.5">
            <Label htmlFor="teams-org-select">Org</Label>
            <Select
              value={orgId ?? undefined}
              onValueChange={(v) => setOrgId(v)}
            >
              <SelectTrigger id="teams-org-select" data-testid="teams-org-select">
                <SelectValue
                  placeholder={dir.loading ? "Loading orgs…" : "Select an org"}
                />
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
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-lg font-medium">Results</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-4">
          {dir.orgsError ? (
            <Alert variant="destructive">
              <AlertTitle>Failed to load orgs</AlertTitle>
              <AlertDescription>{dir.orgsError}</AlertDescription>
            </Alert>
          ) : null}
          {teamsState.error ? (
            <Alert variant="destructive" data-testid="teams-error">
              <AlertTitle>Failed to load teams</AlertTitle>
              <AlertDescription>{teamsState.error}</AlertDescription>
            </Alert>
          ) : null}

          {orgId === null ? (
            <p className="text-sm text-muted-foreground">
              Pick an org to see its teams.
            </p>
          ) : teamsState.loading ? (
            <p className="text-sm text-muted-foreground">Loading teams…</p>
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
              <Table data-testid="teams-table">
                <TableHeader className="bg-muted/50">
                  <TableRow>
                    <TableHead>Slug</TableHead>
                    <TableHead>Name</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {[...teamsState.teams]
                    .sort((a, b) => a.slug.localeCompare(b.slug))
                    .map((t) => (
                      <TableRow key={t.id} data-team-id={t.id}>
                        <TableCell>
                          <code className="font-mono text-xs">{t.slug}</code>
                        </TableCell>
                        <TableCell>{t.name}</TableCell>
                      </TableRow>
                    ))}
                </TableBody>
              </Table>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
