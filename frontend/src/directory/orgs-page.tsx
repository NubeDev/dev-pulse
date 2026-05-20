/**
 * Directory · Orgs page — `GET /orgs` rendered with a derived
 * member-count column.
 *
 * Member counts come from the same per-org `listUsers` fanout the
 * Users page consumes (via `useDirectory()`), so this view shares
 * the react-query cache and updates atomically when an invalidation
 * lands.
 *
 * Markup: semantic `<table>` with shared `HEADER_CLASS` / `CELL_CLASS`
 * Tailwind constants (the kit doesn't ship a Table primitive — see
 * `reports/activity-table.tsx` for the same pattern).
 */

import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@nube/starter-ui-kit/components/card";
import { Alert, AlertDescription, AlertTitle } from "@nube/starter-ui-kit/components/alert";
import { Badge } from "@nube/starter-ui-kit/components/badge";

import { useDirectory } from "./use-directory.js";

const HEADER_CLASS =
  "border-b border-border px-3 py-2 text-left text-xs font-semibold uppercase tracking-wider text-muted-foreground";
const CELL_CLASS = "border-b border-border px-3 py-2 align-middle text-sm";

export function OrgsPage(): JSX.Element {
  const dir = useDirectory();
  const sortedOrgs = [...dir.orgs].sort((a, b) =>
    a.login.localeCompare(b.login),
  );

  return (
    <Card>
      <CardHeader>
        <CardTitle>Orgs</CardTitle>
        <CardDescription>
          <code>GET /orgs</code> · every org dev-pulse has observed.
          Member count is derived from the per-org user fanout.
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4">
        {dir.error ? (
          <Alert variant="destructive" data-testid="orgs-error">
            <AlertTitle>Failed to load orgs</AlertTitle>
            <AlertDescription>{dir.error}</AlertDescription>
          </Alert>
        ) : null}

        {dir.loading && sortedOrgs.length === 0 ? (
          <p className="text-muted-foreground">Loading orgs…</p>
        ) : sortedOrgs.length === 0 ? (
          <p data-testid="orgs-empty" className="text-muted-foreground">
            No orgs tracked yet.
          </p>
        ) : (
          <div className="overflow-hidden rounded-md border border-border bg-card">
            <table data-testid="orgs-table" className="w-full border-collapse">
              <thead className="bg-muted">
                <tr>
                  <th className={HEADER_CLASS}>Login</th>
                  <th className={HEADER_CLASS}>Name</th>
                  <th className={HEADER_CLASS}>Members</th>
                </tr>
              </thead>
              <tbody>
                {sortedOrgs.map((o) => {
                  const count = dir.memberCount.get(o.id) ?? 0;
                  return (
                    <tr key={o.id} data-org-id={o.id}>
                      <td className={CELL_CLASS}>
                        <strong>{o.login}</strong>
                      </td>
                      <td className={CELL_CLASS}>
                        {o.name ? (
                          o.name
                        ) : (
                          <span className="text-muted-foreground">—</span>
                        )}
                      </td>
                      <td className={CELL_CLASS}>
                        <Badge variant="outline" data-testid="org-member-count">
                          {count} {count === 1 ? "member" : "members"}
                        </Badge>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
