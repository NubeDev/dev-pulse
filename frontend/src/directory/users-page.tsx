/**
 * Directory · Users page.
 *
 * - Search box (filters by login / name / email, case-insensitive).
 * - Org filter dropdown — "all orgs" + one option per org.
 * - One row per user, showing every org they're a member of plus a
 *   home-org badge derived from the optimistic store (see
 *   `use-directory.ts` for the why).
 *
 * Source data comes from `useDirectory()`, which fans out
 * `GET /users?org_id=…` across every org.
 */

import { useMemo, useState } from "react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@nube/starter-ui-kit/components/card";
import { Badge } from "@nube/starter-ui-kit/components/badge";
import { Input } from "@nube/starter-ui-kit/components/input";
import { Label } from "@nube/starter-ui-kit/components/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@nube/starter-ui-kit/components/select";

import { useDirectory } from "./use-directory.js";

const ALL = "__all__";

export function UsersPage(): JSX.Element {
  const dir = useDirectory();
  const [query, setQuery] = useState("");
  const [orgFilter, setOrgFilter] = useState<string>(ALL);

  const orgsById = useMemo(
    () => new Map(dir.orgs.map((o) => [o.id, o])),
    [dir.orgs],
  );

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    return dir.users.filter((u) => {
      if (orgFilter !== ALL && !u.org_ids.includes(orgFilter)) return false;
      if (q.length === 0) return true;
      const name = u.user.name?.toLowerCase() ?? "";
      const email = u.user.email?.toLowerCase() ?? "";
      return (
        u.user.login.toLowerCase().includes(q) ||
        name.includes(q) ||
        email.includes(q)
      );
    });
  }, [dir.users, orgFilter, query]);

  return (
    <Card>
      <CardHeader>
        <CardTitle>Users</CardTitle>
        <CardDescription>
          <code>GET /users</code> · search by login, name, or email.
          Membership column is derived from the per-org fanout; the
          home-org badge reflects the latest <code>POST /home-org</code>{" "}
          this session.
        </CardDescription>
      </CardHeader>
      <CardContent style={{ display: "grid", gap: "1rem" }}>
        <div
          style={{
            display: "grid",
            gap: "0.75rem",
            gridTemplateColumns: "minmax(12rem, 1fr) 14rem",
            alignItems: "end",
          }}
        >
          <div style={{ display: "grid", gap: "0.25rem" }}>
            <Label htmlFor="users-search">Search</Label>
            <Input
              id="users-search"
              data-testid="users-search"
              placeholder="login, name, email…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
          <div style={{ display: "grid", gap: "0.25rem" }}>
            <Label htmlFor="users-org-filter">Filter by org</Label>
            <Select
              value={orgFilter}
              onValueChange={(v) => setOrgFilter(v)}
            >
              <SelectTrigger id="users-org-filter" data-testid="users-org-filter">
                <SelectValue placeholder="All orgs" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ALL}>All orgs</SelectItem>
                {dir.orgs.map((o) => (
                  <SelectItem key={o.id} value={o.id}>
                    {o.login}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>

        {dir.error ? (
          <p data-testid="users-error" style={{ color: "oklch(0.5 0.2 25)" }}>
            Failed to load users: {dir.error}
          </p>
        ) : null}

        {dir.loading && dir.users.length === 0 ? (
          <p style={{ color: "var(--muted-foreground)" }}>Loading users…</p>
        ) : visible.length === 0 ? (
          <p data-testid="users-empty" style={{ color: "var(--muted-foreground)" }}>
            No users match the current filters.
          </p>
        ) : (
          <div
            data-testid="users-table"
            role="table"
            style={{
              display: "grid",
              gap: "0.25rem",
              gridTemplateColumns:
                "minmax(8rem, 1fr) minmax(12rem, 1.5fr) minmax(10rem, 1.5fr) minmax(8rem, 1fr)",
              alignItems: "center",
              fontSize: "0.875rem",
            }}
          >
            <HeaderCell>Login</HeaderCell>
            <HeaderCell>Name / email</HeaderCell>
            <HeaderCell>Memberships</HeaderCell>
            <HeaderCell>Home org</HeaderCell>
            {visible.map((u) => (
              <UserRow
                key={u.user.id}
                login={u.user.login}
                name={u.user.name ?? null}
                email={u.user.email ?? null}
                orgLogins={u.org_ids.map(
                  (id) => orgsById.get(id)?.login ?? id.slice(0, 8),
                )}
                homeOrgLogin={
                  u.home_org !== null
                    ? orgsById.get(u.home_org)?.login ?? u.home_org.slice(0, 8)
                    : null
                }
              />
            ))}
          </div>
        )}

        <p style={{ color: "var(--muted-foreground)", fontSize: "0.8125rem" }}>
          Showing {visible.length} of {dir.users.length} user
          {dir.users.length === 1 ? "" : "s"}.
        </p>
      </CardContent>
    </Card>
  );
}

function HeaderCell({ children }: { children: React.ReactNode }): JSX.Element {
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

function UserRow({
  login,
  name,
  email,
  orgLogins,
  homeOrgLogin,
}: {
  login: string;
  name: string | null;
  email: string | null;
  orgLogins: ReadonlyArray<string>;
  homeOrgLogin: string | null;
}): JSX.Element {
  return (
    <>
      <Cell>
        <strong>{login}</strong>
      </Cell>
      <Cell>
        <div style={{ display: "grid" }}>
          {name && <span>{name}</span>}
          {email && (
            <span style={{ color: "var(--muted-foreground)", fontSize: "0.8125rem" }}>
              {email}
            </span>
          )}
        </div>
      </Cell>
      <Cell>
        {orgLogins.length === 0 ? (
          <span style={{ color: "var(--muted-foreground)" }}>—</span>
        ) : (
          <div style={{ display: "flex", flexWrap: "wrap", gap: "0.25rem" }}>
            {orgLogins.map((og) => (
              <Badge key={og} variant="outline" data-testid="user-membership">
                {og}
              </Badge>
            ))}
          </div>
        )}
      </Cell>
      <Cell>
        {homeOrgLogin ? (
          <Badge data-testid="user-home-org" data-home-org={homeOrgLogin}>
            🏠 {homeOrgLogin}
          </Badge>
        ) : (
          <span style={{ color: "var(--muted-foreground)" }}>unset</span>
        )}
      </Cell>
    </>
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
