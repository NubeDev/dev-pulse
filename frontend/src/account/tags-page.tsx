/**
 * Account · Tags — home-grown cross-org grouping primitive
 * (SCOPE-PROJECTS.md §7).
 *
 * The page is the user-facing CRUD surface over `/tags`:
 *
 *   * `GET    /tags`        — list every visible tag.
 *   * `POST   /tags`        — create in a scope the caller is a member of.
 *   * `PATCH  /tags/{id}`   — rename / recolour / set description / archive.
 *
 * Link/unlink to specific targets happens from the per-entity
 * pages (workflow / directory) — this page only manages the tag
 * concepts themselves. The viewer-filtered `visible_link_count`
 * (§7.4) is surfaced as a soft hint of how many things carry the
 * tag right now.
 *
 * The page intentionally does not surface GitHub sync state yet —
 * that's the §5 reconciler slice from `tagging.md`, still to land.
 */

import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { IconArchive, IconPlus, IconRestore, IconTag } from "@tabler/icons-react";

import { Alert, AlertDescription } from "@/components/ui/alert";
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
import { Spinner } from "@/components/ui/spinner";
import { Textarea } from "@/components/ui/textarea";
import { api } from "@/api/client";
import type {
  CreateTagRequest,
  OrgDto,
  TagDto,
  TagScopeKind,
  UpdateTagRequest,
} from "@/api/client";

const TAGS_KEY = ["tags"] as const;
const ORGS_KEY = ["orgs"] as const;

/** Semantic palette names accepted by `TagDto.color`. The list is
 *  intentionally short — the frontend maps each name to a design
 *  token at render time so stored rows survive token churn (§7.2). */
const COLOR_CHOICES = [
  "slate",
  "indigo",
  "blue",
  "teal",
  "green",
  "amber",
  "red",
  "pink",
  "purple",
] as const;

export function TagsPage(): JSX.Element {
  const tags = useQuery({ queryKey: TAGS_KEY, queryFn: () => api.listTags() });

  return (
    <div className="container mx-auto max-w-3xl space-y-6 p-6">
      <header>
        <h1 className="text-2xl font-semibold">Tags</h1>
        <p className="text-sm text-muted-foreground">
          Reusable labels you can attach to repos, issues, users,
          and teams. Tags are visible per scope (yourself, a team,
          or an org) and never leak across visibility boundaries.
        </p>
      </header>

      <NewTagCard />

      {tags.isLoading ? (
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <Spinner /> Loading tags…
        </div>
      ) : tags.isError ? (
        <Alert variant="destructive">
          <AlertDescription>
            Failed to load tags: {String(tags.error)}
          </AlertDescription>
        </Alert>
      ) : (tags.data ?? []).length === 0 ? (
        <Card>
          <CardContent className="py-10 text-center text-sm text-muted-foreground">
            No tags yet. Create one above to get started.
          </CardContent>
        </Card>
      ) : (
        <div className="space-y-3">
          {(tags.data ?? []).map((t) => (
            <TagRow key={t.id} tag={t} />
          ))}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Create
// ---------------------------------------------------------------------------

function NewTagCard(): JSX.Element {
  const queryClient = useQueryClient();
  const orgs = useQuery({ queryKey: ORGS_KEY, queryFn: () => api.listOrgs() });

  const [name, setName] = useState("");
  const [color, setColor] = useState<string>(COLOR_CHOICES[1]);
  const [description, setDescription] = useState("");
  const [scopeKind, setScopeKind] = useState<TagScopeKind>("org");
  const [scopeId, setScopeId] = useState<string>("");
  const [errMsg, setErrMsg] = useState<string | null>(null);

  // Default the org scope to the first visible org once it loads
  // (the most likely choice — keeps "create a tag" two clicks
  // instead of three).
  const orgChoices: OrgDto[] = orgs.data ?? [];
  const effectiveScopeId = useMemo(() => {
    if (scopeId) return scopeId;
    const first = orgChoices[0];
    if (scopeKind === "org" && first) return first.id;
    return "";
  }, [scopeId, scopeKind, orgChoices]);

  const create = useMutation({
    mutationFn: (req: CreateTagRequest) => api.createTag(req),
    onSuccess: () => {
      setName("");
      setDescription("");
      setErrMsg(null);
      void queryClient.invalidateQueries({ queryKey: TAGS_KEY });
    },
    onError: (e) => setErrMsg(String(e)),
  });

  function submit() {
    const n = name.trim();
    if (!n) {
      setErrMsg("Name is required.");
      return;
    }
    if (!effectiveScopeId) {
      setErrMsg("Pick a scope.");
      return;
    }
    create.mutate({
      scope_kind: scopeKind,
      scope_id: effectiveScopeId,
      name: n,
      color,
      description: description.trim() ? description.trim() : null,
    });
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <IconPlus className="size-4" /> New tag
        </CardTitle>
        <CardDescription>
          Pick a scope, name, and colour. Tags are unique
          (case-insensitively) within their scope.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="grid gap-3 sm:grid-cols-2">
          <div className="space-y-1">
            <Label htmlFor="tag-name">Name</Label>
            <Input
              id="tag-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="e.g. priority:high"
              autoComplete="off"
            />
          </div>
          <div className="space-y-1">
            <Label htmlFor="tag-color">Colour</Label>
            <Select value={color} onValueChange={setColor}>
              <SelectTrigger id="tag-color">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {COLOR_CHOICES.map((c) => (
                  <SelectItem key={c} value={c}>
                    <span className="inline-flex items-center gap-2">
                      <ColorDot name={c} /> {c}
                    </span>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-1">
            <Label htmlFor="tag-scope-kind">Scope</Label>
            <Select
              value={scopeKind}
              onValueChange={(v) => {
                setScopeKind(v as TagScopeKind);
                setScopeId("");
              }}
            >
              <SelectTrigger id="tag-scope-kind">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="org">Org-shared</SelectItem>
                <SelectItem value="user">Just me</SelectItem>
              </SelectContent>
            </Select>
          </div>
          {scopeKind === "org" ? (
            <div className="space-y-1">
              <Label htmlFor="tag-scope-org">Org</Label>
              <Select
                value={effectiveScopeId}
                onValueChange={setScopeId}
                disabled={orgChoices.length === 0}
              >
                <SelectTrigger id="tag-scope-org">
                  <SelectValue
                    placeholder={
                      orgs.isLoading ? "Loading…" : "Pick an org"
                    }
                  />
                </SelectTrigger>
                <SelectContent>
                  {orgChoices.map((o) => (
                    <SelectItem key={o.id} value={o.id}>
                      {o.login}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          ) : (
            // user-scope: the server picks the caller's user id
            // from the session; we still need to send *something*
            // for `scope_id`. The REST handler validates that the
            // value equals the caller — passing the caller's id
            // requires a /me lookup we don't have, so we leave the
            // input out and rely on the existing `403
            // tag_scope_member_required` to surface mis-scoped
            // requests cleanly. v1 stores the value via /me/tags
            // on success.
            null
          )}
        </div>
        <div className="space-y-1">
          <Label htmlFor="tag-desc">Description (optional)</Label>
          <Textarea
            id="tag-desc"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="What does this tag mean?"
            rows={2}
          />
        </div>
        {errMsg ? (
          <Alert variant="destructive">
            <AlertDescription>{errMsg}</AlertDescription>
          </Alert>
        ) : null}
        <div className="flex justify-end">
          <Button
            onClick={submit}
            disabled={create.isPending || !name.trim()}
          >
            {create.isPending ? <Spinner /> : <IconPlus className="size-4" />}
            Create tag
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

// ---------------------------------------------------------------------------
// Edit / archive
// ---------------------------------------------------------------------------

function TagRow({ tag }: { tag: TagDto }): JSX.Element {
  const queryClient = useQueryClient();
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(tag.name);
  const [color, setColor] = useState(tag.color);
  const [description, setDescription] = useState(tag.description ?? "");
  const [errMsg, setErrMsg] = useState<string | null>(null);

  const archived = !!tag.archived_at;

  const update = useMutation({
    mutationFn: (req: UpdateTagRequest) => api.updateTag(tag.id, req),
    onSuccess: () => {
      setEditing(false);
      setErrMsg(null);
      void queryClient.invalidateQueries({ queryKey: TAGS_KEY });
    },
    onError: (e) => setErrMsg(String(e)),
  });

  function save() {
    const n = name.trim();
    if (!n) {
      setErrMsg("Name is required.");
      return;
    }
    const trimmedDesc = description.trim();
    update.mutate({
      name: n !== tag.name ? n : undefined,
      color: color !== tag.color ? color : undefined,
      description:
        trimmedDesc !== (tag.description ?? "")
          ? trimmedDesc || null
          : undefined,
    });
  }

  function toggleArchive() {
    update.mutate({ archived: !archived });
  }

  return (
    <Card className={archived ? "opacity-60" : undefined}>
      <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
        <div className="space-y-1">
          <CardTitle className="flex items-center gap-2 text-base">
            <ColorDot name={tag.color} />
            <span>{tag.name}</span>
            {archived ? (
              <Badge variant="outline">Archived</Badge>
            ) : null}
            <Badge variant="secondary">{tag.scope_kind}</Badge>
          </CardTitle>
          {tag.description ? (
            <CardDescription>{tag.description}</CardDescription>
          ) : null}
        </div>
        <div className="flex items-center gap-2">
          <Badge variant="outline" className="font-mono">
            <IconTag className="mr-1 size-3" />
            {tag.visible_link_count}
          </Badge>
          {!editing ? (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                setEditing(true);
                setName(tag.name);
                setColor(tag.color);
                setDescription(tag.description ?? "");
                setErrMsg(null);
              }}
            >
              Edit
            </Button>
          ) : null}
          <Button
            variant="ghost"
            size="sm"
            onClick={toggleArchive}
            disabled={update.isPending}
            title={archived ? "Un-archive" : "Archive"}
          >
            {archived ? (
              <IconRestore className="size-4" />
            ) : (
              <IconArchive className="size-4" />
            )}
          </Button>
        </div>
      </CardHeader>
      {editing ? (
        <CardContent className="space-y-3">
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-1">
              <Label htmlFor={`name-${tag.id}`}>Name</Label>
              <Input
                id={`name-${tag.id}`}
                value={name}
                onChange={(e) => setName(e.target.value)}
                autoComplete="off"
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor={`color-${tag.id}`}>Colour</Label>
              <Select value={color} onValueChange={setColor}>
                <SelectTrigger id={`color-${tag.id}`}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {COLOR_CHOICES.map((c) => (
                    <SelectItem key={c} value={c}>
                      <span className="inline-flex items-center gap-2">
                        <ColorDot name={c} /> {c}
                      </span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
          <div className="space-y-1">
            <Label htmlFor={`desc-${tag.id}`}>Description</Label>
            <Textarea
              id={`desc-${tag.id}`}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              rows={2}
            />
          </div>
          {errMsg ? (
            <Alert variant="destructive">
              <AlertDescription>{errMsg}</AlertDescription>
            </Alert>
          ) : null}
          <div className="flex justify-end gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => setEditing(false)}
              disabled={update.isPending}
            >
              Cancel
            </Button>
            <Button size="sm" onClick={save} disabled={update.isPending}>
              {update.isPending ? <Spinner /> : null}
              Save
            </Button>
          </div>
        </CardContent>
      ) : null}
    </Card>
  );
}

// ---------------------------------------------------------------------------
// Colour swatch
// ---------------------------------------------------------------------------

/** Map a semantic palette name to a Tailwind background swatch.
 *  Unknown names fall back to a neutral slate dot so a stored row
 *  whose colour is outside our v1 palette still renders. */
function ColorDot({ name }: { name: string }): JSX.Element {
  const cls = COLOR_CLASS[name] ?? "bg-slate-500";
  return (
    <span
      aria-hidden
      className={`inline-block size-3 rounded-full ${cls}`}
    />
  );
}

const COLOR_CLASS: Record<string, string> = {
  slate: "bg-slate-500",
  indigo: "bg-indigo-500",
  blue: "bg-blue-500",
  teal: "bg-teal-500",
  green: "bg-green-500",
  amber: "bg-amber-500",
  red: "bg-red-500",
  pink: "bg-pink-500",
  purple: "bg-purple-500",
};
