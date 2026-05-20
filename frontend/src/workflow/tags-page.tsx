/**
 * Tag manager — SCOPE-PROJECTS §7.
 *
 * Surfaces:
 *
 * - **List** of visible tags. Each row carries the **viewer-filtered
 *   link count** the §7.4 contract enforces ("the count the viewer
 *   would see if they expanded the tag, not the true count"). We
 *   surface this as a Badge with an explicit `(visible)` suffix in
 *   the tooltip so an operator who knows the true count is larger
 *   doesn't think the system is lying.
 *
 * - **Create tag** dialog with §7.4 default scope behaviour:
 *   - viewer is a member of exactly one visible org ⇒ default
 *     `scope_kind = "org"` with that org's id pre-filled.
 *   - viewer is a member of multiple orgs ⇒ prompt for `scope_kind`
 *     and `scope_id` (no silent default — §7.4 product framing).
 *   - viewer is in zero orgs ⇒ fall back to `scope_kind = "user"`
 *     with `scope_id = viewer_id`.
 *   The §1 product framing (cross-org grouping for managers) is the
 *   reason `user`-scope is *opt-in* on the form, not the default.
 *
 * - **Archive** is the only retirement path (§7.4 — no hard delete).
 *   The PATCH submits `{ archived: true }`; archived tags are shown
 *   in a separate collapsed list so the audit story (when did
 *   Phoenix retire?) is one click away.
 */

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { IconTags } from "@tabler/icons-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

import { api } from "../api/client.js";
import type { OrgDto, TagDto, TagScopeKind } from "../api/client.js";
import { PageHeading } from "../components/page-heading.jsx";
import { useAuth } from "@nube/starter-ui-core/auth";

import { useCreateTag, useTags, useUpdateTag } from "./use-workflow-data.js";
import { USE_MOCK, MOCK_ORG_NUBE, MOCK_USER_VIEWER } from "./mocks.js";

const SCOPE_COLORS = ["indigo", "teal", "red", "amber", "violet", "emerald", "slate"];

export function TagsPage(): JSX.Element {
  const tags = useTags();
  const orgs = useViewerOrgs();
  const active = (tags.data ?? []).filter((t) => !t.archived_at);
  const archived = (tags.data ?? []).filter((t) => t.archived_at);
  return (
    <div className="flex flex-col gap-6 px-4 lg:px-6" data-testid="tags-page">
      <PageHeading
        title="Tags"
        description="Cross-org project grouping. Tag any combination of repos, issues, users, and teams; counts here are the viewer-filtered visible set, not the true total (§7.4)."
        trailing={
          <CreateTagDialog orgs={orgs} />
        }
      />
      {tags.isLoading ? (
        <Alert><AlertDescription>Loading tags…</AlertDescription></Alert>
      ) : tags.isError ? (
        <Alert variant="destructive">
          <AlertTitle>Could not load tags</AlertTitle>
          <AlertDescription>
            {tags.error instanceof Error ? tags.error.message : "Unknown error"}
          </AlertDescription>
        </Alert>
      ) : (
        <>
          <TagList tags={active} orgs={orgs} />
          {archived.length > 0 && (
            <details className="rounded border border-border/50 bg-muted/30 p-3">
              <summary className="cursor-pointer text-sm text-muted-foreground">
                Archived ({archived.length})
              </summary>
              <div className="pt-3">
                <TagList tags={archived} orgs={orgs} archived />
              </div>
            </details>
          )}
        </>
      )}
    </div>
  );
}

function TagList({
  tags,
  orgs,
  archived = false,
}: {
  tags: TagDto[];
  orgs: OrgDto[];
  archived?: boolean;
}): JSX.Element {
  if (tags.length === 0) {
    return (
      <Card>
        <CardContent className="py-6 text-sm text-muted-foreground">
          No {archived ? "archived " : ""}tags yet.
        </CardContent>
      </Card>
    );
  }
  return (
    <ul className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
      {tags.map((t) => (
        <TagCard key={t.id} tag={t} orgs={orgs} />
      ))}
    </ul>
  );
}

function TagCard({ tag, orgs }: { tag: TagDto; orgs: OrgDto[] }): JSX.Element {
  const update = useUpdateTag(tag.id);
  const scopeLabel = describeScope(tag, orgs);
  return (
    <Card data-testid={`tag-card-${tag.id}`} className="flex h-full flex-col">
      <CardHeader className="flex-row items-center gap-2">
        <IconTags
          className="size-4"
          style={{ color: paletteColor(tag.color) }}
          aria-hidden
        />
        <CardTitle className="flex-1 truncate text-base">{tag.name}</CardTitle>
        <Badge
          variant="outline"
          title={`${tag.visible_link_count} visible link(s). Counts are viewer-filtered (§7.4) — the true total may be higher.`}
          data-testid={`tag-link-count-${tag.id}`}
        >
          {tag.visible_link_count} visible
        </Badge>
      </CardHeader>
      <CardContent className="flex flex-1 flex-col gap-3 text-sm">
        <p className="text-xs text-muted-foreground">{scopeLabel}</p>
        {tag.description && <p>{tag.description}</p>}
        <div className="mt-auto flex gap-2">
          <Button
            asChild
            variant="outline"
            size="sm"
          >
            <a href={`#/workflow/tags?id=${tag.id}`}>Open</a>
          </Button>
          {!tag.archived_at && (
            <Button
              variant="ghost"
              size="sm"
              disabled={update.isPending}
              onClick={() => update.mutate({ archived: true })}
            >
              Archive
            </Button>
          )}
          {tag.archived_at && (
            <Button
              variant="ghost"
              size="sm"
              disabled={update.isPending}
              onClick={() => update.mutate({ archived: false })}
            >
              Restore
            </Button>
          )}
        </div>
      </CardContent>
    </Card>
  );
}

function describeScope(tag: TagDto, orgs: OrgDto[]): string {
  switch (tag.scope_kind) {
    case "user":
      return "Personal (user-scope)";
    case "team":
      return "Team-shared";
    case "org": {
      const o = orgs.find((x) => x.id === tag.scope_id);
      return o ? `Org · ${o.login}` : "Org-shared";
    }
  }
}

function paletteColor(name: string): string {
  switch (name) {
    case "indigo": return "#6366f1";
    case "red": return "#ef4444";
    case "teal": return "#14b8a6";
    case "amber": return "#f59e0b";
    case "violet": return "#8b5cf6";
    case "emerald": return "#10b981";
    case "slate": return "#64748b";
    default: return "#64748b";
  }
}

// ---------------------------------------------------------------------------
// Create-tag dialog with §7.4 default-scope logic.
// ---------------------------------------------------------------------------

function CreateTagDialog({ orgs }: { orgs: OrgDto[] }): JSX.Element {
  const [open, setOpen] = useState(false);
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button data-testid="create-tag-button">New tag</Button>
      </DialogTrigger>
      <DialogContent data-testid="create-tag-dialog">
        <DialogHeader>
          <DialogTitle>Create tag</DialogTitle>
        </DialogHeader>
        <CreateTagForm orgs={orgs} onDone={() => setOpen(false)} />
      </DialogContent>
    </Dialog>
  );
}

function CreateTagForm({
  orgs,
  onDone,
}: {
  orgs: OrgDto[];
  onDone: () => void;
}): JSX.Element {
  const auth = useAuth();
  // `auth.user.subject` is the stable user identifier the rest of
  // dp-rest keys off; in mocks we substitute a fixed UUID.
  const viewerId = auth.user?.subject ?? (USE_MOCK ? MOCK_USER_VIEWER : "");
  // §7.4: default to `org` when the viewer is in exactly one visible
  // org, prompt when they're in several, fall back to `user` when
  // they're in zero. `user`-scope is the opt-in, never the default
  // here.
  const defaultScope = useMemo<{
    scope_kind: TagScopeKind;
    scope_id: string;
    promptForScope: boolean;
  }>(() => {
    if (orgs.length === 1) {
      return { scope_kind: "org", scope_id: orgs[0]!.id, promptForScope: false };
    }
    if (orgs.length === 0) {
      return { scope_kind: "user", scope_id: viewerId, promptForScope: false };
    }
    // Multiple orgs — prompt; pre-fill the first one so the form is
    // never in an invalid state, but flag `promptForScope` so the
    // operator must confirm rather than accept silently.
    return { scope_kind: "org", scope_id: orgs[0]!.id, promptForScope: true };
  }, [orgs, viewerId]);

  const [scopeKind, setScopeKind] = useState<TagScopeKind>(defaultScope.scope_kind);
  const [scopeId, setScopeId] = useState(defaultScope.scope_id);
  const [name, setName] = useState("");
  const [color, setColor] = useState(SCOPE_COLORS[0]!);
  const [description, setDescription] = useState("");
  const create = useCreateTag();

  const onSubmit = (ev: React.FormEvent): void => {
    ev.preventDefault();
    create.mutate(
      {
        scope_kind: scopeKind,
        scope_id: scopeKind === "user" ? viewerId : scopeId,
        name,
        color,
        description: description || undefined,
      },
      {
        onSuccess: () => onDone(),
      },
    );
  };

  return (
    <form className="flex flex-col gap-3" onSubmit={onSubmit}>
      {defaultScope.promptForScope && (
        <Alert data-testid="create-tag-scope-prompt">
          <AlertTitle>Pick a scope</AlertTitle>
          <AlertDescription>
            You belong to {orgs.length} orgs. Tags default to <code>org</code>{" "}
            so collaborators can see them — confirm the org below or switch
            to <code>user</code> for a personal tag (§7.4).
          </AlertDescription>
        </Alert>
      )}
      <div className="grid grid-cols-[1fr_1fr] gap-3">
        <div className="flex flex-col gap-1">
          <Label>Scope</Label>
          <Select value={scopeKind} onValueChange={(v) => setScopeKind(v as TagScopeKind)}>
            <SelectTrigger data-testid="scope-kind-select">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="org">Org</SelectItem>
              <SelectItem value="team">Team</SelectItem>
              <SelectItem value="user">User (private)</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div className="flex flex-col gap-1">
          <Label>{scopeKind === "org" ? "Org" : scopeKind === "team" ? "Team" : "Owner"}</Label>
          {scopeKind === "org" ? (
            <Select value={scopeId} onValueChange={setScopeId}>
              <SelectTrigger data-testid="scope-id-select">
                <SelectValue placeholder="Select org" />
              </SelectTrigger>
              <SelectContent>
                {orgs.map((o) => (
                  <SelectItem key={o.id} value={o.id}>
                    {o.login}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          ) : scopeKind === "user" ? (
            <Input value={viewerId} disabled />
          ) : (
            <Input
              value={scopeId}
              onChange={(e) => setScopeId(e.target.value)}
              placeholder="team UUID"
            />
          )}
        </div>
      </div>
      <div className="flex flex-col gap-1">
        <Label>Name</Label>
        <Input value={name} onChange={(e) => setName(e.target.value)} required />
      </div>
      <div className="flex flex-col gap-1">
        <Label>Colour</Label>
        <Select value={color} onValueChange={setColor}>
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {SCOPE_COLORS.map((c) => (
              <SelectItem key={c} value={c}>{c}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="flex flex-col gap-1">
        <Label>Description (optional)</Label>
        <Input value={description} onChange={(e) => setDescription(e.target.value)} />
      </div>
      {create.isError && (
        <Alert variant="destructive">
          <AlertDescription>
            {create.error instanceof Error ? create.error.message : "Could not create tag"}
          </AlertDescription>
        </Alert>
      )}
      <DialogFooter>
        <Button type="submit" disabled={create.isPending || !name.trim()}>
          {create.isPending ? "Creating…" : "Create"}
        </Button>
      </DialogFooter>
    </form>
  );
}

/** Viewer's visible orgs — read from `/orgs` (the directory surface
 *  already filters by visibility). Used to drive the §7.4 default
 *  scope. */
function useViewerOrgs() {
  const query = useQuery<OrgDto[]>({
    queryKey: ["workflow", "viewer-orgs"],
    queryFn: () =>
      USE_MOCK
        ? Promise.resolve([
            { id: MOCK_ORG_NUBE, github_id: 1, login: "NubeIO", name: "Nube" },
          ])
        : api.listOrgs(),
  });
  return query.data ?? [];
}
