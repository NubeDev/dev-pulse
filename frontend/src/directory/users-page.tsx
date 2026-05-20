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
 *
 * Markup: shadcn `InputGroup` wraps the search input with a leading
 * search icon, and the result list renders as a semantic `<table>`
 * with shared `HEADER_CLASS` / `CELL_CLASS` Tailwind constants (the
 * kit doesn't ship a Table primitive — see `reports/activity-table.tsx`).
 */

import { useMemo, useState } from "react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@nube/starter-ui-kit/components/card";
import { Alert, AlertDescription, AlertTitle } from "@nube/starter-ui-kit/components/alert";
import { Badge } from "@nube/starter-ui-kit/components/badge";
import {
  InputGroup,
  InputGroupAddon,
  InputGroupInput,
} from "@nube/starter-ui-kit/components/input-group";
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

const HEADER_CLASS =
  "border-b border-border px-3 py-2 text-left text-xs font-semibold uppercase tracking-wider text-muted-foreground";
const CELL_CLASS = "border-b border-border px-3 py-2 align-middle text-sm";

/** Inline magnifier — we don't pull a 3rd-party icon dep in just for
 *  this one decoration. Sized to InputGroupAddon's default 16px. */
function SearchIcon(): JSX.Element {
  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className="size-4"
    >
      <circle cx="11" cy="11" r="7" />
      <path d="m20 20-3.5-3.5" />
    </svg>
  );
}

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
      <CardContent className="grid gap-4">
        <div className="grid items-end gap-3 grid-cols-[minmax(12rem,1fr)_14rem]">
          <div className="grid gap-1">
            <Label htmlFor="users-search">Search</Label>
            <InputGroup>
              <InputGroupAddon>
                <SearchIcon />
              </InputGroupAddon>
              <InputGroupInput
                id="users-search"
                data-testid="users-search"
                placeholder="login, name, email…"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </InputGroup>
          </div>
          <div className="grid gap-1">
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
          <Alert variant="destructive" data-testid="users-error">
            <AlertTitle>Failed to load users</AlertTitle>
            <AlertDescription>{dir.error}</AlertDescription>
          </Alert>
        ) : null}

        {dir.loading && dir.users.length === 0 ? (
          <p className="text-muted-foreground">Loading users…</p>
        ) : visible.length === 0 ? (
          <p data-testid="users-empty" className="text-muted-foreground">
            No users match the current filters.
          </p>
        ) : (
          <div className="overflow-hidden rounded-md border border-border bg-card">
            <table data-testid="users-table" className="w-full border-collapse">
              <thead className="bg-muted">
                <tr>
                  <th className={HEADER_CLASS}>Login</th>
                  <th className={HEADER_CLASS}>Name / email</th>
                  <th className={HEADER_CLASS}>Memberships</th>
                  <th className={HEADER_CLASS}>Home org</th>
                </tr>
              </thead>
              <tbody>
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
              </tbody>
            </table>
          </div>
        )}

        <p className="text-[0.8125rem] text-muted-foreground">
          Showing {visible.length} of {dir.users.length} user
          {dir.users.length === 1 ? "" : "s"}.
        </p>
      </CardContent>
    </Card>
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
    <tr>
      <td className={CELL_CLASS}>
        <strong>{login}</strong>
      </td>
      <td className={CELL_CLASS}>
        <div className="grid">
          {name && <span>{name}</span>}
          {email && (
            <span className="text-[0.8125rem] text-muted-foreground">
              {email}
            </span>
          )}
        </div>
      </td>
      <td className={CELL_CLASS}>
        {orgLogins.length === 0 ? (
          <span className="text-muted-foreground">—</span>
        ) : (
          <div className="flex flex-wrap gap-1">
            {orgLogins.map((og) => (
              <Badge key={og} variant="outline" data-testid="user-membership">
                {og}
              </Badge>
            ))}
          </div>
        )}
      </td>
      <td className={CELL_CLASS}>
        {homeOrgLogin ? (
          <Badge data-testid="user-home-org" data-home-org={homeOrgLogin}>
            🏠 {homeOrgLogin}
          </Badge>
        ) : (
          <span className="text-muted-foreground">unset</span>
        )}
      </td>
    </tr>
  );
}
