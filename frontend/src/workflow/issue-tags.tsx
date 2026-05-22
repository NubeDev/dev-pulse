/**
 * Issue tag UI — read chips for the table row and an add/remove
 * picker for the issue detail sheet.
 *
 * Backed by:
 *
 * * `IssueDto.tags` — the viewer-visible tag chip list embedded
 *   by `attach_issue_tags` in [`dp-rest/src/issues_read.rs`]
 *   (tagging.md §7.4). The list and detail handlers both populate
 *   it, so the row chips and the picker stay in sync without a
 *   second round-trip.
 * * `useTags()` — every tag the viewer can *see*, used as the
 *   "Add a tag" source. The picker filters to the viewer's
 *   **scope-member** subset (only tags the caller can also link)
 *   by excluding archived rows; the backend ultimately gates this
 *   with `403 tag_scope_member_required`.
 * * `useLinkTagTargets` / `useUnlinkTagTargets` — the existing
 *   batch link/unlink mutations. We invalidate the whole
 *   `workflow` key on success so the row chips reflect the new
 *   state.
 *
 * No bespoke schema or endpoint — this is purely a wiring slice
 * over the §7 routes already shipped.
 */

import { useMemo, useState } from "react";
import { IconPlus, IconTag, IconX } from "@tabler/icons-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { LABEL_PALETTE } from "@/components/label-chip";

import type { IssueDto, IssueTagDto, TagDto } from "../api/client.js";
import {
  useLinkTagTargets,
  useTags,
  useUnlinkTagTargets,
} from "./use-workflow-data.js";

/**
 * Read-only chip strip rendered inside the issues table row. Each
 * chip carries an `<IconTag>` glyph so DP tags are visually
 * distinct from GitHub labels (which share the same palette but
 * no leading icon). Wrapped in a `<span>` rather than a button so
 * clicking it still falls through to the row's `onClick` (opens
 * the peek panel).
 */
export function IssueTagsRow({
  tags,
  max = 3,
}: {
  tags: ReadonlyArray<IssueTagDto>;
  max?: number;
}): JSX.Element | null {
  if (tags.length === 0) return null;
  const visible = tags.slice(0, max);
  const overflow = tags.length - visible.length;
  return (
    <span
      className="inline-flex shrink-0 items-center gap-1"
      data-testid="issue-tags-row"
    >
      {visible.map((t) => (
        <span
          key={t.id}
          className={`inline-flex shrink-0 items-center gap-0.5 rounded-full border px-1.5 py-0 text-[10px] font-medium leading-4 ${
            LABEL_PALETTE[t.color] ??
            "bg-transparent text-muted-foreground border-border"
          }`}
          data-testid="issue-tag-chip"
          data-tag-id={t.id}
          title={`${t.name} (${t.scope_kind}-scope tag)`}
        >
          <IconTag className="size-2.5" />
          {t.name}
        </span>
      ))}
      {overflow > 0 && (
        <span
          className="inline-flex shrink-0 items-center rounded-full border border-border bg-transparent px-1.5 py-0 text-[10px] font-medium leading-4 text-muted-foreground"
          data-testid="issue-tag-overflow"
          title={tags
            .slice(max)
            .map((t) => t.name)
            .join(", ")}
        >
          +{overflow}
        </span>
      )}
    </span>
  );
}

/**
 * Add/remove picker for the issue detail sheet. Renders the
 * attached chips with an `×` remove button and an "Add tag"
 * popover listing every visible (non-archived) tag the caller
 * has access to. Already-attached entries are disabled in the
 * popover list so the user cannot link the same tag twice (the
 * backend would 422 the duplicate anyway).
 */
export function IssueTagsEditor({ issue }: { issue: IssueDto }): JSX.Element {
  const attached = issue.tags ?? [];
  const attachedIds = useMemo(
    () => new Set(attached.map((t) => t.id)),
    [attached],
  );

  const tagsQuery = useTags();
  const allVisible = useMemo<TagDto[]>(
    () =>
      (tagsQuery.data ?? [])
          .filter((t) => !t.archived_at)
          // Stable alpha order; the picker is small so we render
          // the full list (no virtualisation).
          .sort((a, b) => a.name.localeCompare(b.name)),
    [tagsQuery.data],
  );

  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return allVisible;
    return allVisible.filter((t) => t.name.toLowerCase().includes(q));
  }, [allVisible, query]);

  return (
    <div
      className="flex flex-col gap-2 border-t border-border pt-4"
      data-testid="issue-tags-editor"
    >
      <div className="flex items-center justify-between">
        <Label className="flex items-center gap-1.5">
          <IconTag className="size-4" />
          Tags
        </Label>
        <Popover
          open={open}
          onOpenChange={(o) => {
            setOpen(o);
            if (!o) setQuery("");
          }}
        >
          <PopoverTrigger asChild>
            <Button
              type="button"
              variant="outline"
              size="sm"
              data-testid="issue-tags-add-trigger"
            >
              <IconPlus className="mr-1 size-4" />
              Add tag
            </Button>
          </PopoverTrigger>
          <PopoverContent
            className="w-72 p-2"
            align="end"
            data-testid="issue-tags-add-popover"
          >
            <Input
              autoFocus
              placeholder="Search tags…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              data-testid="issue-tags-search"
            />
            <div className="mt-2 max-h-64 overflow-y-auto">
              {tagsQuery.isLoading && (
                <p className="px-2 py-3 text-sm text-muted-foreground">
                  Loading…
                </p>
              )}
              {!tagsQuery.isLoading && filtered.length === 0 && (
                <p className="px-2 py-3 text-sm text-muted-foreground">
                  {allVisible.length === 0
                    ? "No tags yet. Create one in Account · Tags."
                    : "No matches."}
                </p>
              )}
              {filtered.map((t) => {
                const already = attachedIds.has(t.id);
                return (
                  <TagAddRow
                    key={t.id}
                    issueId={issue.id}
                    tag={t}
                    already={already}
                    onAdded={() => setOpen(false)}
                  />
                );
              })}
            </div>
          </PopoverContent>
        </Popover>
      </div>

      {attached.length === 0 ? (
        <p className="text-sm italic text-muted-foreground">
          No tags. Use "Add tag" to attach one.
        </p>
      ) : (
        <div className="flex flex-wrap gap-1.5">
          {attached.map((t) => (
            <AttachedTagChip key={t.id} issueId={issue.id} tag={t} />
          ))}
        </div>
      )}
    </div>
  );
}

function TagAddRow({
  issueId,
  tag,
  already,
  onAdded,
}: {
  issueId: string;
  tag: TagDto;
  already: boolean;
  onAdded: () => void;
}): JSX.Element {
  const link = useLinkTagTargets(tag.id);
  const swatch =
    LABEL_PALETTE[tag.color] ??
    "bg-transparent text-muted-foreground border-border";
  return (
    <button
      type="button"
      disabled={already || link.isPending}
      onClick={() =>
        link.mutate(
          { items: [{ kind: "issue", target_id: issueId }] },
          { onSuccess: () => onAdded() },
        )
      }
      className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm hover:bg-accent disabled:cursor-default disabled:opacity-50"
      data-testid="issue-tags-add-row"
      data-tag-id={tag.id}
      data-already={already ? "true" : undefined}
    >
      <span
        className={`inline-flex shrink-0 items-center gap-0.5 rounded-full border px-1.5 py-0 text-[10px] font-medium leading-4 ${swatch}`}
      >
        <IconTag className="size-2.5" />
        {tag.name}
      </span>
      <span className="text-xs text-muted-foreground">
        {tag.scope_kind}-scope
      </span>
      {already && (
        <span className="ml-auto text-xs text-muted-foreground">attached</span>
      )}
    </button>
  );
}

function AttachedTagChip({
  issueId,
  tag,
}: {
  issueId: string;
  tag: IssueTagDto;
}): JSX.Element {
  const unlink = useUnlinkTagTargets(tag.id);
  const swatch =
    LABEL_PALETTE[tag.color] ??
    "bg-transparent text-muted-foreground border-border";
  return (
    <span
      className={`inline-flex items-center gap-1 rounded-full border px-1.5 py-0.5 text-[11px] font-medium leading-4 ${swatch}`}
      data-testid="issue-tag-attached"
      data-tag-id={tag.id}
    >
      <IconTag className="size-2.5" />
      {tag.name}
      <button
        type="button"
        title={`Remove tag ${tag.name}`}
        disabled={unlink.isPending}
        onClick={() =>
          unlink.mutate({ items: [{ kind: "issue", target_id: issueId }] })
        }
        className="ml-0.5 inline-flex size-3.5 items-center justify-center rounded-full hover:bg-foreground/10 disabled:opacity-50"
        data-testid="issue-tag-remove"
      >
        <IconX className="size-2.5" />
      </button>
    </span>
  );
}
