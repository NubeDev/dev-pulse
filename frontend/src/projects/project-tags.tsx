/**
 * Project tags — display chips + an inline attach/detach/create
 * editor, shared by the portfolio "Tags" column and the project
 * detail page.
 *
 * Backed by the home-grown tag system (SCOPE-PROJECTS §7), extended
 * with a `project` link kind (migration 0049). Attach/detach goes
 * through the existing `POST/DELETE /tags/{id}/links` batch routes;
 * "create new" is a `POST /tags` (org scope) followed by a link.
 * Rename / recolour / archive of the tag *definition* stays on the
 * Account → Tags page — this surface only manages which tags a
 * project carries.
 */

import { useMemo, useState } from "react";
import type { JSX } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckIcon, PlusIcon, TagIcon, XIcon } from "lucide-react";

import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import { cn } from "@/lib/utils";
import { api } from "../api/client.js";
import type { TagDto } from "../api/client.js";
import { LABEL_PALETTE } from "@/components/label-chip";

/** What the display chips need — a structural subset of `TagDto`
 *  that both the portfolio row (`TagChip`) and the detail page
 *  (`TagDto`) satisfy. */
export interface ProjectTag {
  id: string;
  name: string;
  color: string;
}

const CREATE_COLORS = [
  "indigo",
  "blue",
  "teal",
  "green",
  "amber",
  "red",
  "pink",
  "purple",
  "slate",
] as const;

/** A single tag pill. Colour resolved from the semantic palette
 *  (shared with the issue label chips); unknown colours fall back to
 *  a muted border. */
export function TagPill({
  name,
  color,
  className,
}: {
  name: string;
  color: string;
  className?: string;
}): JSX.Element {
  const cls = LABEL_PALETTE[color] ?? "bg-transparent text-muted-foreground border-border";
  return (
    <span
      className={cn(
        "inline-flex max-w-[10rem] shrink-0 items-center truncate rounded-full border px-1.5 py-0 text-[10px] font-medium leading-4",
        cls,
        className,
      )}
      data-testid="project-tag-pill"
      title={name}
    >
      {name}
    </span>
  );
}

export interface ProjectTagsControlProps {
  projectId: string;
  /** Org the project belongs to — the scope new tags are created in
   *  and the org whose tags are offered for linking. */
  orgId: string;
  /** Tags to render as chips (portfolio passes `row.tags`; the detail
   *  page passes its `listProjectTags` query data). */
  tags: ReadonlyArray<ProjectTag>;
  /** Invoked after any attach/detach/create so the caller can
   *  invalidate whatever query feeds `tags` (e.g. the portfolio
   *  report). The control already invalidates its own
   *  `["project-tags", projectId]` editor query. */
  onChanged?: () => void;
  /** `true` ⇒ dense layout for a table cell (cap chips, "+N"
   *  overflow). `false` ⇒ full wrap for the detail page. */
  compact?: boolean;
  "data-testid"?: string;
}

export function ProjectTagsControl({
  projectId,
  orgId,
  tags,
  onChanged,
  compact = false,
  ...rest
}: ProjectTagsControlProps): JSX.Element {
  const [open, setOpen] = useState(false);
  const max = compact ? 3 : 50;
  const visible = tags.slice(0, max);
  const overflow = tags.length - visible.length;

  return (
    <div
      className="flex flex-wrap items-center gap-1"
      data-testid={rest["data-testid"] ?? "project-tags"}
      onClick={(e) => e.stopPropagation()}
    >
      {visible.map((t) => (
        <TagPill key={t.id} name={t.name} color={t.color} />
      ))}
      {overflow > 0 ? (
        <span
          className="text-[10px] text-muted-foreground"
          title={tags.slice(max).map((t) => t.name).join(", ")}
        >
          +{overflow}
        </span>
      ) : null}
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          {tags.length === 0 ? (
            // Empty state: a clearly labelled affordance so "where do
            // I add tags?" is obvious. (A bare icon was too easy to
            // miss.) Compact (portfolio cell) keeps it terse.
            <Button
              variant="outline"
              size="sm"
              className="h-6 gap-1 px-2 text-xs font-normal text-muted-foreground hover:text-foreground"
              title="Add tags to this project"
              data-testid="project-tags-edit"
            >
              <TagIcon className="size-3" />
              {compact ? "Tag" : "Add tags"}
            </Button>
          ) : (
            <Button
              variant="ghost"
              size="icon"
              className="size-6 text-muted-foreground hover:text-foreground"
              aria-label="Add or remove tags"
              title="Add or remove tags"
              data-testid="project-tags-edit"
            >
              <PlusIcon className="size-3.5" />
            </Button>
          )}
        </PopoverTrigger>
        <PopoverContent
          align="start"
          className="w-72 p-0"
          data-testid="project-tags-popover"
        >
          <TagEditorBody
            projectId={projectId}
            orgId={orgId}
            open={open}
            onChanged={onChanged}
          />
        </PopoverContent>
      </Popover>
    </div>
  );
}

function TagEditorBody({
  projectId,
  orgId,
  open,
  onChanged,
}: {
  projectId: string;
  orgId: string;
  open: boolean;
  onChanged?: () => void;
}): JSX.Element {
  const qc = useQueryClient();
  const [query, setQuery] = useState("");
  const [creating, setCreating] = useState(false);
  const [newColor, setNewColor] = useState<string>(CREATE_COLORS[0]);
  const [err, setErr] = useState<string | null>(null);

  // Org tags available to link, and the project's currently-linked
  // set. The linked query only runs while the popover is open so a
  // table of N rows doesn't fire N requests on mount.
  const allTags = useQuery({
    queryKey: ["tags"],
    queryFn: () => api.listTags(),
    enabled: open,
    staleTime: 30_000,
  });
  const linked = useQuery({
    queryKey: ["project-tags", projectId],
    queryFn: () => api.listProjectTags(projectId),
    enabled: open,
    staleTime: 10_000,
  });
  // Orgs the operator belongs to — the fan-out target for the
  // "create for all orgs" affordance. Tags have no global scope
  // (`user | team | org` only), so "all orgs" means one org-scoped
  // tag per org. Membership-scoped (not every observed org) so the
  // per-org POST never trips authz on an org we can't write to.
  const myOrgs = useQuery({
    queryKey: ["my-orgs"],
    queryFn: () => api.listMyOrgs(),
    enabled: open,
    staleTime: 60_000,
  });

  const linkedIds = useMemo(
    () => new Set((linked.data ?? []).map((t) => t.id)),
    [linked.data],
  );

  const orgTags = useMemo(
    () =>
      (allTags.data ?? []).filter(
        (t) =>
          t.scope_kind === "org" &&
          t.scope_id === orgId &&
          !t.archived_at,
      ),
    [allTags.data, orgId],
  );

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return orgTags;
    return orgTags.filter((t) => t.name.toLowerCase().includes(q));
  }, [orgTags, query]);

  const invalidate = () => {
    void qc.invalidateQueries({ queryKey: ["project-tags", projectId] });
    onChanged?.();
  };

  const toggle = useMutation({
    mutationFn: async (tag: TagDto) => {
      const body = { items: [{ kind: "project" as const, target_id: projectId }] };
      if (linkedIds.has(tag.id)) {
        await api.unlinkTagTargets(tag.id, body);
      } else {
        await api.linkTagTargets(tag.id, body);
      }
    },
    onSuccess: invalidate,
    onError: (e) => setErr(String(e)),
  });

  const create = useMutation({
    mutationFn: async (name: string) => {
      const tag = await api.createTag({
        scope_kind: "org",
        scope_id: orgId,
        name,
        color: newColor,
      });
      await api.linkTagTargets(tag.id, {
        items: [{ kind: "project", target_id: projectId }],
      });
    },
    onSuccess: () => {
      setQuery("");
      setCreating(false);
      setErr(null);
      void qc.invalidateQueries({ queryKey: ["tags"] });
      invalidate();
    },
    onError: (e) => setErr(String(e)),
  });

  // Fan the same tag (name + colour) into every org the operator
  // belongs to. The current org's tag is created + linked to this
  // project authoritatively (errors surface); the other orgs are
  // best-effort so a pre-existing same-named tag in one org (409)
  // doesn't abort the rest.
  const createAll = useMutation({
    mutationFn: async (name: string) => {
      const orgs = myOrgs.data ?? [];
      const others = orgs.filter((o) => o.id !== orgId);
      const tag = await api.createTag({
        scope_kind: "org",
        scope_id: orgId,
        name,
        color: newColor,
      });
      await api.linkTagTargets(tag.id, {
        items: [{ kind: "project", target_id: projectId }],
      });
      await Promise.allSettled(
        others.map((o) =>
          api.createTag({
            scope_kind: "org",
            scope_id: o.id,
            name,
            color: newColor,
          }),
        ),
      );
    },
    onSuccess: () => {
      setQuery("");
      setCreating(false);
      setErr(null);
      void qc.invalidateQueries({ queryKey: ["tags"] });
      invalidate();
    },
    onError: (e) => setErr(String(e)),
  });

  const trimmed = query.trim();
  const exactMatch = orgTags.some(
    (t) => t.name.toLowerCase() === trimmed.toLowerCase(),
  );
  const busy = toggle.isPending || create.isPending || createAll.isPending;
  const orgCount = myOrgs.data?.length ?? 0;

  return (
    <div className="flex flex-col">
      <div className="border-b p-2">
        <Input
          autoFocus
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Filter or create a tag…"
          className="h-8"
          data-testid="project-tags-search"
        />
      </div>
      <div className="max-h-56 overflow-y-auto p-1">
        {allTags.isLoading || linked.isLoading ? (
          <div className="flex items-center gap-2 p-2 text-xs text-muted-foreground">
            <Spinner /> Loading tags…
          </div>
        ) : filtered.length === 0 && !trimmed ? (
          <p className="p-2 text-xs text-muted-foreground">
            No org tags yet. Type a name to create one.
          </p>
        ) : (
          filtered.map((t) => {
            const on = linkedIds.has(t.id);
            return (
              <button
                key={t.id}
                type="button"
                disabled={busy}
                onClick={() => toggle.mutate(t)}
                className="flex w-full items-center justify-between gap-2 rounded-sm px-2 py-1.5 text-left text-xs hover:bg-accent disabled:opacity-50"
                data-testid={`project-tags-option-${t.id}`}
                data-checked={on ? "true" : "false"}
              >
                <span className="flex min-w-0 items-center gap-2">
                  <TagPill name={t.name} color={t.color} />
                </span>
                {on ? <CheckIcon className="size-3.5 shrink-0" /> : null}
              </button>
            );
          })
        )}
      </div>

      {/* Create-new affordance — appears when the query doesn't match
          an existing org tag exactly. */}
      {trimmed && !exactMatch ? (
        <div className="border-t p-2">
          <div className="mb-1.5 flex flex-wrap items-center gap-1">
            {CREATE_COLORS.map((c) => (
              <button
                key={c}
                type="button"
                aria-label={`Colour ${c}`}
                onClick={() => setNewColor(c)}
                className={cn(
                  "size-4 rounded-full border",
                  LABEL_PALETTE[c],
                  newColor === c ? "ring-2 ring-foreground/50" : "",
                )}
                data-testid={`project-tags-color-${c}`}
              />
            ))}
          </div>
          <Button
            size="sm"
            className="h-7 w-full text-xs"
            disabled={busy}
            onClick={() => create.mutate(trimmed)}
            data-testid="project-tags-create"
          >
            {create.isPending ? (
              <Spinner />
            ) : (
              <PlusIcon className="size-3.5" />
            )}
            Create "{trimmed}" in this org
          </Button>
          {/* Fan-out create: same tag in every org the operator
              belongs to. Hidden when there's only the current org to
              avoid offering a no-op. */}
          {orgCount > 1 ? (
            <Button
              variant="outline"
              size="sm"
              className="mt-1 h-7 w-full text-xs"
              disabled={busy}
              onClick={() => createAll.mutate(trimmed)}
              data-testid="project-tags-create-all-orgs"
              title={`Creates "${trimmed}" in all ${orgCount} orgs you belong to`}
            >
              {createAll.isPending ? (
                <Spinner />
              ) : (
                <PlusIcon className="size-3.5" />
              )}
              Create for all {orgCount} orgs
            </Button>
          ) : null}
        </div>
      ) : null}

      {err ? (
        <div className="flex items-start gap-1 border-t p-2 text-[11px] text-red-600 dark:text-red-400">
          <XIcon className="mt-0.5 size-3 shrink-0" />
          <span className="break-words">{err}</span>
        </div>
      ) : null}
    </div>
  );
}
