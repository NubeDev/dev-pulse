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
 * Layout (stage 4 visual rewrite): PageHeading lockup at top, filter
 * Card next, results Card with the shadcn-shaped `Table` primitive
 * (local `components/table.tsx` — the kit doesn't ship one). Search
 * input uses `InputGroup` with a leading magnifier addon.
 */

import { useMemo, useState } from "react";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import {
  InputGroup,
  InputGroupAddon,
  InputGroupInput,
} from "@/components/ui/input-group";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

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

const ALL = "__all__";

const FILTER_GRID_CLASS =
  "grid items-end gap-4 grid-cols-1 sm:grid-cols-[minmax(12rem,1fr)_14rem]";

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
    <div className="flex flex-col gap-4 px-4 md:gap-6 lg:px-6">
      <PageHeading
        title="Users"
        description={
          <>
            <code className="font-mono text-xs">GET /users</code> · search by login,
            name, or email. Memberships and home-org badges come from the
            per-org fanout.
          </>
        }
      />

      <Card>
        <CardHeader>
          <CardTitle className="text-lg font-medium">Filters</CardTitle>
        </CardHeader>
        <CardContent>
          <div className={FILTER_GRID_CLASS}>
            <div className="grid gap-1.5">
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
            <div className="grid gap-1.5">
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
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-lg font-medium">Results</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-4">
          {dir.error ? (
            <Alert variant="destructive" data-testid="users-error">
              <AlertTitle>Failed to load users</AlertTitle>
              <AlertDescription>{dir.error}</AlertDescription>
            </Alert>
          ) : null}

          {dir.loading && dir.users.length === 0 ? (
            <p className="text-sm text-muted-foreground">Loading users…</p>
          ) : visible.length === 0 ? (
            <p data-testid="users-empty" className="text-sm text-muted-foreground">
              No users match the current filters.
            </p>
          ) : (
            <div className="overflow-hidden rounded-md border border-border bg-card">
              <Table data-testid="users-table">
                <TableHeader className="bg-muted/50">
                  <TableRow>
                    <TableHead>Login</TableHead>
                    <TableHead>Name / email</TableHead>
                    <TableHead>Memberships</TableHead>
                    <TableHead>Home org</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
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
                </TableBody>
              </Table>
            </div>
          )}

          <p className="text-xs text-muted-foreground">
            Showing {visible.length} of {dir.users.length} user
            {dir.users.length === 1 ? "" : "s"}.
          </p>
        </CardContent>
      </Card>
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
    <TableRow>
      <TableCell className="font-medium">{login}</TableCell>
      <TableCell>
        <div className="grid">
          {name && <span className="text-sm">{name}</span>}
          {email && (
            <span className="text-xs text-muted-foreground">{email}</span>
          )}
        </div>
      </TableCell>
      <TableCell>
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
      </TableCell>
      <TableCell>
        {homeOrgLogin ? (
          <Badge
            variant="secondary"
            data-testid="user-home-org"
            data-home-org={homeOrgLogin}
          >
            🏠 {homeOrgLogin}
          </Badge>
        ) : (
          <span className="text-muted-foreground">unset</span>
        )}
      </TableCell>
    </TableRow>
  );
}
