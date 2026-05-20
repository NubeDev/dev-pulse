/**
 * Directory · Orgs page — `GET /orgs` rendered with a derived
 * member-count column.
 *
 * Member counts come from the same per-org `listUsers` fanout the
 * Users page consumes (via `useDirectory()`), so this view shares
 * the react-query cache and updates atomically when an invalidation
 * lands.
 *
 * Layout (stage 4 visual rewrite): PageHeading lockup, then a results
 * Card with the shadcn-shaped `Table` primitive (local — the kit
 * doesn't ship one).
 */

import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";

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
import { useDirectory } from "./use-directory.js";

export function OrgsPage(): JSX.Element {
  const dir = useDirectory();
  const sortedOrgs = [...dir.orgs].sort((a, b) =>
    a.login.localeCompare(b.login),
  );

  return (
    <div className="flex flex-col gap-4 px-4 md:gap-6 lg:px-6">
      <PageHeading
        title="Orgs"
        description={
          <>
            <code className="font-mono text-xs">GET /orgs</code> · every org
            dev-pulse has observed. Member count is derived from the per-org
            user fanout.
          </>
        }
      />

      <Card>
        <CardHeader>
          <CardTitle className="text-lg font-medium">Results</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-4">
          {dir.error ? (
            <Alert variant="destructive" data-testid="orgs-error">
              <AlertTitle>Failed to load orgs</AlertTitle>
              <AlertDescription>{dir.error}</AlertDescription>
            </Alert>
          ) : null}

          {dir.loading && sortedOrgs.length === 0 ? (
            <p className="text-sm text-muted-foreground">Loading orgs…</p>
          ) : sortedOrgs.length === 0 ? (
            <Empty data-testid="orgs-empty">
              <EmptyHeader>
                <EmptyTitle>No orgs tracked yet</EmptyTitle>
                <EmptyDescription>
                  Run a fetch or webhook to seed the first one.
                </EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : (
            <div className="overflow-hidden rounded-md border border-border bg-card">
              <Table data-testid="orgs-table">
                <TableHeader className="bg-muted/50">
                  <TableRow>
                    <TableHead>Login</TableHead>
                    <TableHead>Name</TableHead>
                    <TableHead>Members</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {sortedOrgs.map((o) => {
                    const count = dir.memberCount.get(o.id) ?? 0;
                    return (
                      <TableRow key={o.id} data-org-id={o.id}>
                        <TableCell className="font-medium">{o.login}</TableCell>
                        <TableCell>
                          {o.name ? (
                            o.name
                          ) : (
                            <span className="text-muted-foreground">—</span>
                          )}
                        </TableCell>
                        <TableCell>
                          <Badge variant="outline" data-testid="org-member-count">
                            {count} {count === 1 ? "member" : "members"}
                          </Badge>
                        </TableCell>
                      </TableRow>
                    );
                  })}
                </TableBody>
              </Table>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
