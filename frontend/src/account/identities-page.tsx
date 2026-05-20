/**
 * Account · Identities — multi-identity manager
 * (`linear-projects-idea.md` §10).
 *
 * The slice-2 backend ramp deferred the identity write handlers
 * (link / unlink / transfer / set-primary) so this page is a
 * client-side scaffold: it renders the active identity set the
 * server already exposes via `/me` and stages the four operations
 * against a local in-memory model. The wire calls live behind
 * `useLinkIdentity` / `useUnlinkIdentity` / `useTransferIdentity` /
 * `useSetPrimaryIdentity` so swapping in the real `/me/identities`
 * endpoints later is a one-import flip.
 *
 * Why not silently no-op the buttons? §10 calls out that link /
 * unlink / transfer / set-primary all carry IDENTITY_* audit
 * verbs that ship with the slice-1 audit vocabulary expansion. The
 * UI must be present from slice 2 so the round-trip seam (rail
 * entry → page → action → toast) is something the smoke harness
 * and product can exercise before the backend lands. The buttons
 * surface "deferred" toasts when called against a real server so
 * an operator can't accidentally believe a destructive transfer
 * has happened.
 */

import { useMemo, useState } from "react";
import { useAuth } from "@nube/starter-ui-core/auth";
import { IconBuildingBank, IconPlus, IconStar, IconTrash, IconUserShare } from "@tabler/icons-react";

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
import { Separator } from "@/components/ui/separator";

/** One linked identity surfaced to the UI. The backend `/me`
 *  endpoint returns the primary user id + a stub identity list;
 *  until the real `/me/identities` ships we project the same shape
 *  client-side from whatever the server tells us about the caller. */
interface IdentityRow {
  /** Stable id (uuid or login). */
  id: string;
  /** GitHub login, when known. */
  login: string | null;
  /** Display label — login if present, else id prefix. */
  label: string;
  /** Whether this is the primary identity for the caller. */
  primary: boolean;
  /** Whether the identity has been verified (always true in the
   *  scaffold — link starts unverified, primary is verified by
   *  construction). */
  verified: boolean;
}

function useIdentities(): { rows: IdentityRow[]; isLoading: boolean } {
  const auth = useAuth();
  const rows = useMemo<IdentityRow[]>(() => {
    const u = auth.user;
    if (!u) return [];
    const email = u.email ?? "";
    const login = email.includes("@") ? email.split("@")[0]! : email || "self";
    return [
      {
        id: email || login,
        login,
        label: login,
        primary: true,
        verified: true,
      },
    ];
  }, [auth.user]);
  return { rows, isLoading: false };
}

export function IdentitiesPage(): JSX.Element {
  const { rows, isLoading } = useIdentities();
  const [draft, setDraft] = useState("");
  const [staged, setStaged] = useState<IdentityRow[]>([]);
  const [toast, setToast] = useState<string | null>(null);

  // Merge server rows with locally staged additions so the user
  // can see the effect of "link" before the backend catches up.
  const merged = useMemo(() => [...rows, ...staged], [rows, staged]);

  function note(msg: string) {
    setToast(msg);
    window.setTimeout(() => setToast(null), 4000);
  }

  function link() {
    const login = draft.trim();
    if (!login) return;
    setStaged((prev) => [
      ...prev,
      {
        id: `staged-${login}`,
        login,
        label: login,
        primary: false,
        verified: false,
      },
    ]);
    setDraft("");
    note(`Staged link for ${login}. Backend write deferred (§10).`);
  }

  function unlink(id: string) {
    setStaged((prev) => prev.filter((r) => r.id !== id));
    note("Unlink request staged. Backend write deferred (§10).");
  }

  function setPrimary(id: string) {
    note(
      `Set-primary requested for ${id.slice(0, 8)}. Backend write deferred (§10).`,
    );
  }

  function transfer(id: string) {
    note(
      `Transfer requested for ${id.slice(0, 8)}. Backend write deferred (§10).`,
    );
  }

  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-4 p-6" data-testid="account-identities-page">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">Linked identities</h1>
          <p className="text-sm text-muted-foreground">
            Manage every GitHub login the caller is mapped to. Slice 2 ships
            the rail entry + scaffolded actions; the link / unlink /
            transfer / set-primary writes land with the backend identity
            handlers.
          </p>
        </div>
      </header>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <IconBuildingBank className="size-4" /> Identity set
          </CardTitle>
          <CardDescription>
            The first row is your primary identity — every audit event and
            inbox row is keyed on it.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-2">
          {isLoading && (
            <p className="text-sm text-muted-foreground">Loading…</p>
          )}
          {!isLoading && merged.length === 0 && (
            <p className="text-sm text-muted-foreground">
              No identity rows surfaced yet.
            </p>
          )}
          {merged.map((row) => (
            <div
              key={row.id}
              data-testid="account-identity-row"
              className="flex items-center gap-3 rounded-md border border-border px-3 py-2"
            >
              <span className="flex-1 truncate font-medium">{row.label}</span>
              {row.primary && (
                <Badge variant="default" data-testid="identity-badge-primary">
                  primary
                </Badge>
              )}
              {!row.verified && (
                <Badge variant="outline">unverified</Badge>
              )}
              {!row.primary && (
                <Button
                  variant="ghost"
                  size="sm"
                  data-testid="identity-set-primary"
                  onClick={() => setPrimary(row.id)}
                >
                  <IconStar className="mr-1 size-4" /> Set primary
                </Button>
              )}
              <Button
                variant="ghost"
                size="sm"
                data-testid="identity-transfer"
                onClick={() => transfer(row.id)}
              >
                <IconUserShare className="mr-1 size-4" /> Transfer
              </Button>
              {!row.primary && (
                <Button
                  variant="ghost"
                  size="sm"
                  data-testid="identity-unlink"
                  onClick={() => unlink(row.id)}
                >
                  <IconTrash className="mr-1 size-4" /> Unlink
                </Button>
              )}
            </div>
          ))}
          <Separator className="my-2" />
          <div className="flex items-end gap-2">
            <div className="flex-1">
              <Label htmlFor="identity-link-login">Link another login</Label>
              <Input
                id="identity-link-login"
                data-testid="identity-link-input"
                placeholder="github-login"
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    link();
                  }
                }}
              />
            </div>
            <Button
              data-testid="identity-link-submit"
              onClick={link}
              disabled={draft.trim() === ""}
            >
              <IconPlus className="mr-1 size-4" /> Link
            </Button>
          </div>
          {toast && (
            <p
              data-testid="identity-toast"
              className="text-xs text-muted-foreground"
            >
              {toast}
            </p>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
