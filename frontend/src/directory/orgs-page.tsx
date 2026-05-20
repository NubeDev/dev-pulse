/**
 * Directory · Orgs page — `GET /orgs` rendered with a derived
 * member-count column.
 *
 * Member counts come from the same per-org `listUsers` fanout the
 * Users page consumes (via `useDirectory()`), so this view shares
 * the react-query cache and updates atomically when an invalidation
 * lands.
 */

import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@nube/starter-ui-kit/components/card";
import { Badge } from "@nube/starter-ui-kit/components/badge";

import { useDirectory } from "./use-directory.js";

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
      <CardContent style={{ display: "grid", gap: "1rem" }}>
        {dir.error ? (
          <p data-testid="orgs-error" style={{ color: "oklch(0.5 0.2 25)" }}>
            Failed to load orgs: {dir.error}
          </p>
        ) : null}

        {dir.loading && sortedOrgs.length === 0 ? (
          <p style={{ color: "var(--muted-foreground)" }}>Loading orgs…</p>
        ) : sortedOrgs.length === 0 ? (
          <p data-testid="orgs-empty" style={{ color: "var(--muted-foreground)" }}>
            No orgs tracked yet.
          </p>
        ) : (
          <div
            data-testid="orgs-table"
            role="table"
            style={{
              display: "grid",
              gap: "0.25rem",
              gridTemplateColumns:
                "minmax(8rem, 1fr) minmax(12rem, 1.5fr) minmax(8rem, auto)",
              alignItems: "center",
              fontSize: "0.875rem",
            }}
          >
            <Header>Login</Header>
            <Header>Name</Header>
            <Header>Members</Header>
            {sortedOrgs.map((o) => {
              const count = dir.memberCount.get(o.id) ?? 0;
              return (
                <Row key={o.id} data-org-id={o.id}>
                  <Cell><strong>{o.login}</strong></Cell>
                  <Cell>
                    {o.name ? (
                      o.name
                    ) : (
                      <span style={{ color: "var(--muted-foreground)" }}>—</span>
                    )}
                  </Cell>
                  <Cell>
                    <Badge variant="outline" data-testid="org-member-count">
                      {count} {count === 1 ? "member" : "members"}
                    </Badge>
                  </Cell>
                </Row>
              );
            })}
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

function Row({
  children,
  ...rest
}: {
  children: React.ReactNode;
} & React.HTMLAttributes<HTMLDivElement>): JSX.Element {
  // The CSS grid above lays each cell directly, so this wrapper
  // is just a logical fragment — but exposing `data-org-id` etc.
  // via a single-row span is convenient for tests.
  return (
    <div role="row" style={{ display: "contents" }} {...rest}>
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
