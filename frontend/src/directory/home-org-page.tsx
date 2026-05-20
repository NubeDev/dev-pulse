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
 *
 * The confirmation surface is a shadcn `Dialog` (not `AlertDialog`) —
 * this isn't a destructive write, it's an idempotent re-assignment,
 * so the affirmative button is a regular primary `Button` and the
 * trigger composes via `<DialogTrigger asChild>`.
 */

import { useMemo, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { Alert, AlertDescription, AlertTitle } from "@nube/starter-ui-kit/components/alert";
import { Button } from "@nube/starter-ui-kit/components/button";
import { Badge } from "@nube/starter-ui-kit/components/badge";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@nube/starter-ui-kit/components/card";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@nube/starter-ui-kit/components/dialog";
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
  const [open, setOpen] = useState(false);
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
    ? selectedUser.org_ids
        .map((id) => orgsById.get(id))
        .filter((o): o is NonNullable<typeof o> => Boolean(o))
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
      setOpen(false);
    },
  });

  const canSubmit =
    userId !== null && orgId !== null && !mutation.isPending;

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
      <CardContent className="grid gap-4">
        <div className="grid grid-cols-2 gap-3">
          <div className="grid gap-1">
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
              <span className="text-[0.8125rem] text-muted-foreground">
                Current home org:{" "}
                <Badge variant="outline">
                  {orgsById.get(selectedUser.home_org)?.login ??
                    selectedUser.home_org.slice(0, 8)}
                </Badge>
              </span>
            )}
          </div>

          <div className="grid gap-1">
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

        <div className="flex items-center gap-2">
          <Dialog
            open={open}
            onOpenChange={(next) => {
              if (!next && mutation.isPending) return;
              setOpen(next);
            }}
          >
            <DialogTrigger asChild>
              <Button
                data-testid="home-org-submit"
                disabled={!canSubmit}
              >
                Set home org…
              </Button>
            </DialogTrigger>
            <DialogContent data-testid="home-org-confirm">
              <DialogHeader>
                <DialogTitle>Confirm home-org assignment</DialogTitle>
                <DialogDescription>
                  Any previous home-org flag for this user will be cleared
                  atomically.
                </DialogDescription>
              </DialogHeader>
              {userId && orgId ? (
                <div className="grid gap-2 py-2 text-sm">
                  <div className="grid grid-cols-[6rem_1fr] items-center gap-2">
                    <span className="text-muted-foreground">User</span>
                    <strong>
                      {usersById.get(userId)?.user.login ?? "user"}
                    </strong>
                  </div>
                  <div className="grid grid-cols-[6rem_1fr] items-center gap-2">
                    <span className="text-muted-foreground">Home org</span>
                    <strong>
                      {orgsById.get(orgId)?.login ?? "org"}
                    </strong>
                  </div>
                </div>
              ) : null}
              <DialogFooter>
                <DialogClose asChild>
                  <Button
                    variant="outline"
                    data-testid="home-org-cancel"
                    disabled={mutation.isPending}
                  >
                    Cancel
                  </Button>
                </DialogClose>
                <Button
                  data-testid="home-org-confirm-submit"
                  disabled={mutation.isPending || !userId || !orgId}
                  onClick={(e) => {
                    e.preventDefault();
                    if (userId && orgId) mutation.mutate({ userId, orgId });
                  }}
                >
                  {mutation.isPending ? "Setting…" : "Confirm"}
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>
          {feedback && (
            <Alert
              variant={feedback.kind === "ok" ? "default" : "destructive"}
              data-testid="home-org-feedback"
              data-kind={feedback.kind}
              aria-live="polite"
              className="py-2"
            >
              <AlertTitle>
                {feedback.kind === "ok" ? "Done" : "Failed"}
              </AlertTitle>
              <AlertDescription>{feedback.message}</AlertDescription>
            </Alert>
          )}
        </div>
      </CardContent>
    </Card>
  );
}
