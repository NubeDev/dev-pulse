/**
 * Directory · Home-org assignment page.
 *
 * Selects a user, selects an org, opens a confirmation dialog,
 * and `POST /home-org`-es the pair with optimistic update + rollback
 * on failure (see `use-directory.ts` for the optimistic store).
 *
 * Picking only an org that the chosen user is actually a member of
 * is enforced client-side — the server's `set_home_org_for_user`
 * already returns 404 on a non-existent membership, but the UI
 * gates the org dropdown so the obvious bad path doesn't even
 * reach the wire.
 */

import { useMemo, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@nube/starter-ui-kit/components/alert-dialog";
import { Button } from "@nube/starter-ui-kit/components/button";
import { Badge } from "@nube/starter-ui-kit/components/badge";
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

import { api } from "../api/client.js";
import { USE_MOCK } from "./mocks.js";
import { useDirectory } from "./use-directory.js";

export function HomeOrgPage(): JSX.Element {
  const dir = useDirectory();
  const [userId, setUserId] = useState<string | null>(null);
  const [orgId, setOrgId] = useState<string | null>(null);
  const [pending, setPending] = useState<{ userId: string; orgId: string } | null>(null);
  const [feedback, setFeedback] = useState<
    { kind: "ok" | "err"; message: string } | null
  >(null);

  const usersById = useMemo(
    () => new Map(dir.users.map((u) => [u.user.id, u])),
    [dir.users],
  );
  const orgsById = useMemo(
    () => new Map(dir.orgs.map((o) => [o.id, o])),
    [dir.orgs],
  );

  const selectedUser = userId ? usersById.get(userId) ?? null : null;
  const selectableOrgs = selectedUser
    ? selectedUser.org_ids.map((id) => orgsById.get(id)).filter((o): o is NonNullable<typeof o> => Boolean(o))
    : [];

  const mutation = useMutation({
    mutationFn: async (req: { userId: string; orgId: string }) => {
      if (USE_MOCK) {
        // Mock mode: pretend it succeeded after a microtask so the
        // optimistic update has a "settle" point and the dialog
        // closes deterministically.
        await Promise.resolve();
        return { ok: true } as const;
      }
      return api.setHomeOrg({ user_id: req.userId, org_id: req.orgId });
    },
    onMutate: ({ userId, orgId }) => {
      const prev = selectedUser?.home_org ?? null;
      dir.homeOrg.set(userId, orgId);
      return { prev };
    },
    onError: (err, { userId }, ctx) => {
      dir.homeOrg.rollback(userId, ctx?.prev ?? null);
      setFeedback({
        kind: "err",
        message: err instanceof Error ? err.message : String(err),
      });
    },
    onSuccess: (_data, { userId, orgId }) => {
      const userLogin = usersById.get(userId)?.user.login ?? userId.slice(0, 8);
      const orgLogin = orgsById.get(orgId)?.login ?? orgId.slice(0, 8);
      setFeedback({
        kind: "ok",
        message: `Set home org for ${userLogin} → ${orgLogin}.`,
      });
      dir.invalidate();
    },
    onSettled: () => {
      setPending(null);
    },
  });

  const canSubmit =
    userId !== null && orgId !== null && !mutation.isPending && !pending;

  return (
    <Card>
      <CardHeader>
        <CardTitle>Home-org assignment</CardTitle>
        <CardDescription>
          <code>POST /home-org</code> · pick a user, pick one of their
          orgs, confirm. Exactly one membership row per user ends up
          flagged as <code>home_org</code> server-side.
        </CardDescription>
      </CardHeader>
      <CardContent style={{ display: "grid", gap: "1rem" }}>
        <div
          style={{
            display: "grid",
            gap: "0.75rem",
            gridTemplateColumns: "1fr 1fr",
          }}
        >
          <div style={{ display: "grid", gap: "0.25rem" }}>
            <Label htmlFor="home-org-user">User</Label>
            <Select
              value={userId ?? undefined}
              onValueChange={(v) => {
                setUserId(v);
                // Reset org selection — the new user may not be in
                // the previously chosen org.
                setOrgId(null);
                setFeedback(null);
              }}
            >
              <SelectTrigger id="home-org-user" data-testid="home-org-user">
                <SelectValue
                  placeholder={dir.loading ? "Loading users…" : "Select a user"}
                />
              </SelectTrigger>
              <SelectContent>
                {dir.users.map((u) => (
                  <SelectItem key={u.user.id} value={u.user.id}>
                    {u.user.login}
                    {u.user.name ? ` — ${u.user.name}` : ""}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {selectedUser && selectedUser.home_org && (
              <span style={{ fontSize: "0.8125rem", color: "var(--muted-foreground)" }}>
                Current home org:{" "}
                <Badge variant="outline">
                  {orgsById.get(selectedUser.home_org)?.login ??
                    selectedUser.home_org.slice(0, 8)}
                </Badge>
              </span>
            )}
          </div>

          <div style={{ display: "grid", gap: "0.25rem" }}>
            <Label htmlFor="home-org-org">Home org</Label>
            <Select
              value={orgId ?? undefined}
              onValueChange={(v) => {
                setOrgId(v);
                setFeedback(null);
              }}
              disabled={selectedUser === null}
            >
              <SelectTrigger id="home-org-org" data-testid="home-org-org">
                <SelectValue
                  placeholder={
                    selectedUser === null
                      ? "Pick a user first"
                      : selectableOrgs.length === 0
                        ? "User has no memberships"
                        : "Select an org"
                  }
                />
              </SelectTrigger>
              <SelectContent>
                {selectableOrgs.map((o) => (
                  <SelectItem key={o.id} value={o.id}>
                    {o.login}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>

        <div style={{ display: "flex", gap: "0.5rem", alignItems: "center" }}>
          <Button
            data-testid="home-org-submit"
            disabled={!canSubmit}
            onClick={() => {
              if (userId && orgId) setPending({ userId, orgId });
            }}
          >
            Set home org…
          </Button>
          {feedback && (
            <span
              data-testid="home-org-feedback"
              data-kind={feedback.kind}
              role="status"
              aria-live="polite"
              style={{
                fontSize: "0.875rem",
                color: feedback.kind === "ok" ? "oklch(0.45 0.16 145)" : "oklch(0.5 0.2 25)",
              }}
            >
              {feedback.message}
            </span>
          )}
        </div>
      </CardContent>

      <AlertDialog
        open={pending !== null}
        onOpenChange={(open) => {
          if (!open && !mutation.isPending) setPending(null);
        }}
      >
        <AlertDialogContent data-testid="home-org-confirm">
          <AlertDialogHeader>
            <AlertDialogTitle>Confirm home-org assignment</AlertDialogTitle>
            <AlertDialogDescription>
              {pending && (
                <>
                  Set{" "}
                  <strong>
                    {usersById.get(pending.userId)?.user.login ?? "user"}
                  </strong>
                  's home org to{" "}
                  <strong>{orgsById.get(pending.orgId)?.login ?? "org"}</strong>?
                  Any previous home-org flag for this user will be
                  cleared atomically.
                </>
              )}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel
              disabled={mutation.isPending}
              data-testid="home-org-cancel"
              onClick={() => setPending(null)}
            >
              Cancel
            </AlertDialogCancel>
            <AlertDialogAction
              data-testid="home-org-confirm-submit"
              disabled={mutation.isPending}
              onClick={(e) => {
                e.preventDefault();
                if (pending) mutation.mutate(pending);
              }}
            >
              {mutation.isPending ? "Setting…" : "Confirm"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Card>
  );
}
