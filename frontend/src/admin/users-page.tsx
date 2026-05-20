/**
 * Admin · Users page — GDPR controls.
 *
 *   - Export:    `GET /admin/users/:id/export` is fetched as the raw
 *                response body, wrapped in a `Blob`, and pushed
 *                through an anchor click so the browser saves it as
 *                `user-<login-or-id>-export.json`. We deliberately
 *                bypass `api.exportUser` (which would parse + zod-
 *                validate the whole envelope into memory) — the
 *                server already streams chunked JSON, and the user
 *                wants the bytes, not a typed object.
 *
 *                While the download is in flight we surface a shadcn
 *                `Progress` indicator next to the button. The dp-rest
 *                endpoint doesn't ship a content-length header, so
 *                the progress is a two-step "working / done" visual
 *                cue (33% on click → 100% on settle) rather than a
 *                byte-accurate readout.
 *
 *   - Anonymise: `POST /admin/users/:id/anonymise` is irreversible
 *                (the server cascades scrubs across membership,
 *                event, and audit rows), so the button opens a
 *                shadcn `AlertDialog` (destructive variant) that
 *                requires the user to retype the login as
 *                confirmation. Cancelling closes the dialog without
 *                firing the request.
 *
 * Both actions read the list of users from the same `GET /users`
 * query the directory section uses — no per-page user fanout, the
 * admin tool is single-org-agnostic.
 */

import { useMemo, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
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
import { Progress } from "@/components/ui/progress";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

import { api, StarterError } from "../api/client.js";
import type { UserDto } from "../api/client.js";
import { PageHeading } from "../components/page-heading.jsx";
import { MOCK_USERS, USE_MOCK, mockUserExport } from "./mocks.js";

interface Feedback {
  kind: "ok" | "err";
  message: string;
}

export function AdminUsersPage(): JSX.Element {
  const [userId, setUserId] = useState<string | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [typedLogin, setTypedLogin] = useState("");
  const [feedback, setFeedback] = useState<Feedback | null>(null);

  const usersQuery = useQuery({
    queryKey: ["users"],
    queryFn: () => (USE_MOCK ? Promise.resolve([...MOCK_USERS]) : api.listUsers()),
  });
  const users = usersQuery.data ?? [];
  const usersById = useMemo(
    () => new Map(users.map((u) => [u.id, u])),
    [users],
  );
  const selected = userId ? usersById.get(userId) ?? null : null;

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
        message: `Anonymised ${user.login}. Subsequent reads of this user will return the redacted sentinel.`,
      });
      setConfirmOpen(false);
      setTypedLogin("");
      // The user row still exists, just scrubbed — leave the
      // selection in place so the operator can confirm via export.
    },
    onError: (err) => {
      setFeedback({
        kind: "err",
        message: err instanceof Error ? err.message : String(err),
      });
    },
  });

  const canConfirmAnonymise =
    selected !== null &&
    typedLogin.trim() === selected.login &&
    !anonymiseMut.isPending;

  // The export endpoint doesn't ship a content-length header, so the
  // Progress bar is a two-step "working / done" affordance rather
  // than a true byte-accurate readout. 33% while in flight, 100% on
  // success, hidden again on the next interaction.
  const exportProgress = exportMut.isPending
    ? 33
    : exportMut.isSuccess
      ? 100
      : null;

  return (
    <div className="flex flex-col gap-4 px-4 md:gap-6 lg:px-6">
      <PageHeading
        title="User GDPR controls"
        description={
          <>
            Export or anonymise a single user.{" "}
            <code className="font-mono text-xs">POST /admin/users/:id/anonymise</code>{" "}
            is irreversible — confirmation required.
          </>
        }
      />

      <Card>
      <CardHeader>
        <CardTitle className="text-lg font-medium">Pick a user</CardTitle>
        <CardDescription>
          Export streams the user's full record as JSON; anonymise scrubs
          identifying fields and cascades the redaction.
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4">
        <div className="grid max-w-lg gap-1.5">
          <Label htmlFor="admin-user">User</Label>
          <Select
            value={userId ?? undefined}
            onValueChange={(v) => {
              setUserId(v);
              setFeedback(null);
            }}
          >
            <SelectTrigger id="admin-user" data-testid="admin-user-select">
              <SelectValue
                placeholder={usersQuery.isPending ? "Loading users…" : "Select a user"}
              />
            </SelectTrigger>
            <SelectContent>
              {users.map((u) => (
                <SelectItem key={u.id} value={u.id}>
                  {u.login}{u.name ? ` — ${u.name}` : ""}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <div className="grid gap-2">
          <div className="flex flex-wrap items-center gap-2">
            <Button
              data-testid="admin-export"
              disabled={selected === null || exportMut.isPending}
              onClick={() => {
                if (selected) {
                  setFeedback(null);
                  exportMut.mutate(selected);
                }
              }}
            >
              {exportMut.isPending ? "Preparing download…" : "Export user data"}
            </Button>
            <Button
              data-testid="admin-anonymise"
              variant="destructive"
              disabled={selected === null}
              onClick={() => {
                if (selected) {
                  setFeedback(null);
                  setTypedLogin("");
                  setConfirmOpen(true);
                }
              }}
            >
              Anonymise user…
            </Button>
          </div>
          {exportProgress !== null ? (
            <Progress
              value={exportProgress}
              data-testid="admin-export-progress"
              className="h-2 max-w-[18rem]"
            />
          ) : null}
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
      </CardContent>

      <AlertDialog
        open={confirmOpen}
        onOpenChange={(open) => {
          if (!open && !anonymiseMut.isPending) {
            setConfirmOpen(false);
            setTypedLogin("");
          }
        }}
      >
        <AlertDialogContent data-testid="anonymise-confirm">
          <AlertDialogHeader>
            <AlertDialogTitle>Anonymise this user?</AlertDialogTitle>
            <AlertDialogDescription>
              This is irreversible. <code>POST /admin/users/:id/anonymise</code>{" "}
              scrubs the user's identifying fields and cascades the redaction
              through every membership, event, and audit row referencing them.
              {selected ? (
                <>
                  {" "}Type <strong>{selected.login}</strong> below to confirm.
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
              placeholder={selected?.login ?? ""}
              autoComplete="off"
            />
          </div>
          <AlertDialogFooter>
            <AlertDialogCancel
              data-testid="anonymise-cancel"
              disabled={anonymiseMut.isPending}
              onClick={() => {
                setConfirmOpen(false);
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
                if (selected && canConfirmAnonymise) anonymiseMut.mutate(selected);
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

/** Stash the bytes in a Blob URL, click a hidden anchor, revoke the
 *  URL on the next tick.  Works in every modern browser without
 *  pulling in a third-party file-saver lib. */
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
  // Defer revoke so Safari has time to start the download.
  setTimeout(() => URL.revokeObjectURL(url), 1_000);
}
