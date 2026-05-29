/**
 * Admin · Users — operator user management
 * (DOCS/SCOPE-AUTHZ-USERS.md §4.3).
 *
 * Replaces the earlier single-user GDPR shell with a per-row table:
 *
 *   - Login / Email columns from `GET /users`.
 *   - Role <Select> per row; on change, optimistic
 *     `PUT /admin/users/:id/role` with rollback on error.
 *     Disabled when the row is the current admin (self-protection;
 *     server-side guard is the authority).
 *   - Linked GitHub logins as chips per row, fetched via a
 *     React-Query fan-out keyed on the rendered slice of user ids.
 *     For ≤50 users this is one render-pass batch; per-row laziness
 *     is a follow-up if/when this gets noisy.
 *   - Export / Anonymise actions per row — the existing AlertDialog
 *     retype-to-confirm flow is preserved verbatim.
 *
 * A role filter (`All / Reader / Writer / Admin`) and login search
 * narrow the list client-side. The login search matches both
 * `login` and `name`.
 */

import { useMemo, useState } from "react";
import { useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { useAuth } from "@nube/starter-ui-core/auth";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
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

import { api, StarterError } from "../api/client.js";
import type { UserDto, UserRole } from "../api/client.js";
import { PageHeading } from "../components/page-heading.jsx";
import {
  MOCK_USERS,
  USE_MOCK,
  mockListUserIdentities,
  mockSetUserRole,
  mockUserExport,
} from "./mocks.js";

interface Feedback {
  kind: "ok" | "err";
  message: string;
}

type RoleFilter = "all" | UserRole;

export function AdminUsersPage(): JSX.Element {
  const auth = useAuth();
  const selfId = auth.user?.subject ?? null;
  const queryClient = useQueryClient();

  const [search, setSearch] = useState("");
  const [roleFilter, setRoleFilter] = useState<RoleFilter>("all");
  const [feedback, setFeedback] = useState<Feedback | null>(null);
  const [confirmTarget, setConfirmTarget] = useState<UserDto | null>(null);
  const [typedLogin, setTypedLogin] = useState("");

  const usersQuery = useQuery({
    queryKey: ["users"],
    queryFn: () => (USE_MOCK ? Promise.resolve([...MOCK_USERS]) : api.listUsers()),
  });
  const users = usersQuery.data ?? [];

  // Identity fan-out per rendered user. Cached under
  // `["admin-user-identities", id]` so a follow-up role mutation
  // doesn't invalidate the (orthogonal) identities cache.
  const identityQueries = useQueries({
    queries: users.map((u) => ({
      queryKey: ["admin-user-identities", u.id],
      queryFn: () =>
        USE_MOCK
          ? Promise.resolve(mockListUserIdentities(u.id))
          : api.listUserIdentities(u.id),
      // Identities don't churn — a 5-minute stale window keeps the
      // fan-out cheap on subsequent renders.
      staleTime: 5 * 60 * 1000,
    })),
  });

  // `useMutation` per row would be cleaner but blows the rules-of-hooks
  // budget; one mutation that takes `(user, role)` and updates the
  // shared cache is the in-bounds shape.
  const roleMut = useMutation({
    mutationFn: async ({ user, role }: { user: UserDto; role: UserRole }) => {
      if (USE_MOCK) {
        await new Promise((r) => setTimeout(r, 30));
        return mockSetUserRole(user.id, role);
      }
      return api.setUserRole(user.id, role);
    },
    onMutate: async ({ user, role }) => {
      // Optimistic write into the `["users"]` cache so the Select
      // reflects the change immediately; the rollback path restores
      // the snapshot on error.
      await queryClient.cancelQueries({ queryKey: ["users"] });
      const prev = queryClient.getQueryData<UserDto[]>(["users"]);
      if (prev) {
        queryClient.setQueryData<UserDto[]>(
          ["users"],
          prev.map((u) => (u.id === user.id ? { ...u, role } : u)),
        );
      }
      return { prev };
    },
    onError: (err, _vars, ctx) => {
      if (ctx?.prev) queryClient.setQueryData(["users"], ctx.prev);
      setFeedback({
        kind: "err",
        message: err instanceof Error ? err.message : String(err),
      });
    },
    onSuccess: (updated) => {
      // Server is the source of truth — fold the canonical row in.
      const cur = queryClient.getQueryData<UserDto[]>(["users"]);
      if (cur) {
        queryClient.setQueryData<UserDto[]>(
          ["users"],
          cur.map((u) => (u.id === updated.id ? updated : u)),
        );
      }
      setFeedback({
        kind: "ok",
        message: `${updated.login} is now ${updated.role}.`,
      });
    },
  });

  const exportMut = useMutation({
    mutationFn: async (user: UserDto) => {
      if (USE_MOCK) {
        const payload = mockUserExport(user.id);
        triggerDownload(
          new Blob([JSON.stringify(payload, null, 2)], { type: "application/json" }),
          downloadName(user),
        );
        return { bytes: 0 };
      }
      const res = await api.client.fetch(
        `${api.client.baseUrl}/admin/users/${encodeURIComponent(user.id)}/export`,
        { credentials: "include", headers: api.client.headers },
      );
      if (!res.ok) throw await StarterError.fromResponse(res);
      const blob = await res.blob();
      triggerDownload(blob, downloadName(user));
      return { bytes: blob.size };
    },
    onSuccess: (data, user) => {
      setFeedback({
        kind: "ok",
        message:
          data.bytes > 0
            ? `Exported ${user.login} (${data.bytes.toLocaleString()} bytes).`
            : `Exported ${user.login}.`,
      });
    },
    onError: (err) => {
      setFeedback({
        kind: "err",
        message: err instanceof Error ? err.message : String(err),
      });
    },
  });

  const anonymiseMut = useMutation({
    mutationFn: async (user: UserDto) => {
      if (USE_MOCK) {
        await new Promise((r) => setTimeout(r, 30));
        return { ok: true } as const;
      }
      return api.anonymiseUser(user.id);
    },
    onSuccess: (_data, user) => {
      setFeedback({
        kind: "ok",
        message: `Anonymised ${user.login}. Subsequent reads return the redacted sentinel.`,
      });
      setConfirmTarget(null);
      setTypedLogin("");
      void queryClient.invalidateQueries({ queryKey: ["users"] });
    },
    onError: (err) => {
      setFeedback({
        kind: "err",
        message: err instanceof Error ? err.message : String(err),
      });
    },
  });

  const filtered = useMemo(() => {
    const needle = search.trim().toLowerCase();
    return users.filter((u) => {
      if (roleFilter !== "all" && u.role !== roleFilter) return false;
      if (!needle) return true;
      return (
        u.login.toLowerCase().includes(needle) ||
        (u.name ?? "").toLowerCase().includes(needle) ||
        (u.email ?? "").toLowerCase().includes(needle)
      );
    });
  }, [users, search, roleFilter]);

  const canConfirmAnonymise =
    confirmTarget !== null &&
    typedLogin.trim() === confirmTarget.login &&
    !anonymiseMut.isPending;

  return (
    <div className="flex flex-col gap-4 px-4 md:gap-6 lg:px-6">
      <PageHeading
        title="Users"
        description={
          <>
            Operator role management plus GDPR export / anonymise.{" "}
            <code className="font-mono text-xs">PUT /admin/users/:id/role</code>
            {" "}is gated on <code className="font-mono text-xs">users:admin</code>.
          </>
        }
      />

      <Card>
        <CardHeader>
          <CardTitle className="text-lg font-medium">Directory</CardTitle>
          <CardDescription>
            One row per dev-pulse user. Role changes apply immediately; the
            server refuses self-demotion (you cannot drop your own admin tier).
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-4">
          {usersQuery.error ? (
            <Alert variant="destructive" data-testid="admin-users-load-error">
              <AlertTitle>Failed to load users</AlertTitle>
              <AlertDescription>
                {usersQuery.error instanceof Error
                  ? usersQuery.error.message
                  : String(usersQuery.error)}
              </AlertDescription>
            </Alert>
          ) : null}
          <div className="flex flex-wrap items-end gap-3">
            <div className="grid gap-1.5">
              <Label htmlFor="admin-users-search">Search</Label>
              <Input
                id="admin-users-search"
                data-testid="admin-users-search"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="login, name, or email"
                className="w-64"
              />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="admin-users-role-filter">Role</Label>
              <Select
                value={roleFilter}
                onValueChange={(v) => setRoleFilter(v as RoleFilter)}
              >
                <SelectTrigger
                  id="admin-users-role-filter"
                  data-testid="admin-users-role-filter"
                  className="w-40"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">All</SelectItem>
                  <SelectItem value="reader">Reader</SelectItem>
                  <SelectItem value="writer">Writer</SelectItem>
                  <SelectItem value="admin">Admin</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          {feedback ? (
            <Alert
              variant={feedback.kind === "ok" ? "default" : "destructive"}
              data-testid="admin-users-feedback"
              data-kind={feedback.kind}
              aria-live="polite"
            >
              <AlertTitle>
                {feedback.kind === "ok" ? "Done" : "Action failed"}
              </AlertTitle>
              <AlertDescription>{feedback.message}</AlertDescription>
            </Alert>
          ) : null}

          <div className="overflow-x-auto">
            <Table data-testid="admin-users-table">
              <TableHeader>
                <TableRow>
                  <TableHead>Login</TableHead>
                  <TableHead>Email</TableHead>
                  <TableHead>Role</TableHead>
                  <TableHead>Linked GitHub logins</TableHead>
                  <TableHead className="text-right">Actions</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {filtered.map((u) => {
                  const idQ = identityQueries[users.indexOf(u)];
                  const identities = idQ?.data?.identities ?? [];
                  const isSelf = selfId !== null && u.id === selfId;
                  return (
                    <TableRow key={u.id} data-testid={`admin-users-row-${u.id}`}>
                      <TableCell className="font-mono">{u.login}</TableCell>
                      <TableCell className="text-muted-foreground">
                        {u.email ?? "—"}
                      </TableCell>
                      <TableCell>
                        <Select
                          value={u.role}
                          onValueChange={(v) =>
                            roleMut.mutate({ user: u, role: v as UserRole })
                          }
                          disabled={isSelf || roleMut.isPending}
                        >
                          <SelectTrigger
                            data-testid={`admin-users-role-${u.id}`}
                            className="w-32"
                          >
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="reader">Reader</SelectItem>
                            <SelectItem value="writer">Writer</SelectItem>
                            <SelectItem value="admin">Admin</SelectItem>
                          </SelectContent>
                        </Select>
                      </TableCell>
                      <TableCell>
                        {idQ?.isPending ? (
                          <span className="text-xs text-muted-foreground">
                            …
                          </span>
                        ) : identities.length === 0 ? (
                          <span className="text-xs text-muted-foreground">
                            —
                          </span>
                        ) : (
                          <div className="flex flex-wrap gap-1">
                            {identities.map((i) => (
                              <Badge
                                key={i.id}
                                variant={i.is_primary ? "default" : "secondary"}
                                title={`${i.provider} · linked ${i.linked_at}`}
                              >
                                {i.display_name ?? i.id}
                              </Badge>
                            ))}
                          </div>
                        )}
                      </TableCell>
                      <TableCell className="text-right">
                        <div className="inline-flex gap-1">
                          <Button
                            size="sm"
                            variant="outline"
                            data-testid={`admin-users-export-${u.id}`}
                            disabled={exportMut.isPending}
                            onClick={() => exportMut.mutate(u)}
                          >
                            Export
                          </Button>
                          <Button
                            size="sm"
                            variant="destructive"
                            data-testid={`admin-users-anonymise-${u.id}`}
                            onClick={() => {
                              setFeedback(null);
                              setTypedLogin("");
                              setConfirmTarget(u);
                            }}
                          >
                            Anonymise
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  );
                })}
                {filtered.length === 0 ? (
                  <TableRow>
                    <TableCell
                      colSpan={5}
                      className="text-center text-sm text-muted-foreground"
                    >
                      {usersQuery.isPending
                        ? "Loading users…"
                        : users.length === 0
                          ? "No users in dp_users yet. Run a GitHub sync, or re-run `dev-pulse create-admin` to mirror the seeded admin row."
                          : "No users match the current filter."}
                    </TableCell>
                  </TableRow>
                ) : null}
              </TableBody>
            </Table>
          </div>
        </CardContent>

        <AlertDialog
          open={confirmTarget !== null}
          onOpenChange={(open) => {
            if (!open && !anonymiseMut.isPending) {
              setConfirmTarget(null);
              setTypedLogin("");
            }
          }}
        >
          <AlertDialogContent data-testid="anonymise-confirm">
            <AlertDialogHeader>
              <AlertDialogTitle>Anonymise this user?</AlertDialogTitle>
              <AlertDialogDescription>
                This is irreversible. <code>POST /admin/users/:id/anonymise</code>{" "}
                scrubs identifying fields and cascades the redaction through every
                membership, event, and audit row referencing them.
                {confirmTarget ? (
                  <>
                    {" "}Type <strong>{confirmTarget.login}</strong> below to confirm.
                  </>
                ) : null}
              </AlertDialogDescription>
            </AlertDialogHeader>
            <div className="grid gap-1.5 py-1">
              <Label htmlFor="anonymise-confirm-login">Confirm login</Label>
              <Input
                id="anonymise-confirm-login"
                data-testid="anonymise-confirm-login"
                value={typedLogin}
                onChange={(e) => setTypedLogin(e.target.value)}
                placeholder={confirmTarget?.login ?? ""}
                autoComplete="off"
              />
            </div>
            <AlertDialogFooter>
              <AlertDialogCancel
                data-testid="anonymise-cancel"
                disabled={anonymiseMut.isPending}
                onClick={() => {
                  setConfirmTarget(null);
                  setTypedLogin("");
                }}
              >
                Cancel
              </AlertDialogCancel>
              <AlertDialogAction
                data-testid="anonymise-confirm-submit"
                variant="destructive"
                disabled={!canConfirmAnonymise}
                onClick={(e) => {
                  e.preventDefault();
                  if (confirmTarget && canConfirmAnonymise) {
                    anonymiseMut.mutate(confirmTarget);
                  }
                }}
              >
                {anonymiseMut.isPending ? "Anonymising…" : "Anonymise"}
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </Card>
    </div>
  );
}

function downloadName(user: UserDto): string {
  const safe = (user.login ?? user.id).replace(/[^a-z0-9._-]/gi, "_");
  return `dev-pulse-user-${safe}-export.json`;
}

function triggerDownload(blob: Blob, filename: string): void {
  if (typeof document === "undefined" || typeof URL === "undefined") return;
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.rel = "noopener";
  a.style.display = "none";
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  setTimeout(() => URL.revokeObjectURL(url), 1_000);
}
