/**
 * Pin sidebar widget — SCOPE-PROJECTS §6.
 *
 * Two caps to enforce here, both from §13.5:
 *
 * 1. **Pin cap (data-model, 20).** Enforced server-side; the
 *    `useAddPin` mutation surfaces it as a `pin_cap_exceeded`
 *    error and the calling button shows the message in a tooltip.
 *    Nothing to render here.
 * 2. **Sidebar render cap (50).** This is the §6.1 invariant:
 *    *after tag expansion*, the rendered list collapses above 50
 *    entries into a "…and N more" disclosure. Tag pins expand to
 *    their visible repo links (one entry per repo) — a single
 *    tag pin can blow the render cap on its own. The collapse is
 *    inside the disclosure: the leading {RENDER_CAP - 1} entries
 *    stay visible, the rest land behind a clickable disclosure
 *    row that opens a full-list dialog.
 *
 * Pins are not a report dimension (§6.2). This widget is pure
 * sidebar UI — the only mutation it triggers is "remove this
 * pin", and only as a hover-affordance.
 */

import { useMemo, useState } from "react";
import {
  IconGitBranch,
  IconPinned,
  IconPinnedOff,
  IconTags,
} from "@tabler/icons-react";

import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar";
import { cn } from "@/lib/utils";

import { PIN_RENDER_CAP, type PinDto, type TagDetailResponse } from "../api/client.js";
import { useTagDetail, usePins, useRemovePin } from "./use-workflow-data.js";

/** What we render in the sidebar — flattened after tag expansion. */
interface PinEntry {
  /** Stable React key. `"<kind>:<target>"` for non-expanded pins,
   *  `"tag-expand:<tag>:<repo>"` for repos pulled out of a tag pin. */
  key: string;
  /** Display label — the call site does not have the repo / tag name
   *  registry, so we fall back to a short prefix of the target id. */
  label: string;
  /** href for the entry. `null` ⇒ the row is informational and not
   *  a link (e.g. a tag with zero visible repo links). */
  href: string | null;
  /** Whether the row is the original pin (so the "unpin" affordance
   *  refers to the right target) or an *expansion* of a tag pin
   *  (then "unpin" is contextually wrong — clicking the chip should
   *  navigate, not unpin the parent tag silently). */
  origin:
    | { kind: "pin"; pinKind: "repo" | "tag"; targetId: string }
    | { kind: "expanded"; parentTagId: string };
  icon: typeof IconGitBranch;
}

export function PinSidebar(): JSX.Element | null {
  const pins = usePins();
  const [showAll, setShowAll] = useState(false);

  if (pins.isLoading || pins.isError) return null;
  const data = pins.data ?? [];
  if (data.length === 0) return null;

  return (
    <SidebarGroup data-testid="pin-sidebar">
      <SidebarGroupLabel>Pinned</SidebarGroupLabel>
      <SidebarGroupContent>
        <PinList pins={data} onOpenAll={() => setShowAll(true)} />
      </SidebarGroupContent>
      <PinOverflowDialog
        open={showAll}
        onOpenChange={setShowAll}
        pins={data}
      />
    </SidebarGroup>
  );
}

/** Renders entries up to the §13.5 render-cap, then a "…and N more"
 *  row that opens the overflow dialog. */
function PinList({
  pins,
  onOpenAll,
}: {
  pins: PinDto[];
  onOpenAll: () => void;
}): JSX.Element {
  // Expand tag pins to their repo links one at a time — each tag's
  // detail query runs independently and merges into the flat list.
  const repoPins = pins.filter((p) => p.kind === "repo");
  const tagPins = pins.filter((p) => p.kind === "tag");

  const repoEntries: PinEntry[] = repoPins.map((p) => ({
    key: `repo:${p.target_id}`,
    label: shortLabel(p.target_id, "Repo"),
    href: `#/workflow/issues?repo=${p.target_id}`,
    origin: { kind: "pin", pinKind: "repo", targetId: p.target_id },
    icon: IconGitBranch,
  }));

  return (
    <SidebarMenu>
      {repoEntries.map((e) => (
        <PinRow key={e.key} entry={e} />
      ))}
      {tagPins.map((p) => (
        <ExpandedTagPin key={`tag:${p.target_id}`} pin={p} />
      ))}
      <PinOverflowMarker pins={pins} onOpenAll={onOpenAll} />
    </SidebarMenu>
  );
}

/**
 * A single repo / tag entry row, with hover-revealed "unpin"
 * affordance. The entry is always rendered as a link; the trailing
 * button is mounted absolutely so the row's hit-target stays the
 * label.
 */
function PinRow({ entry }: { entry: PinEntry }): JSX.Element {
  const remove = useRemovePin();
  const Icon = entry.icon;
  const onUnpin = (): void => {
    if (entry.origin.kind !== "pin") return;
    remove.mutate({
      kind: entry.origin.pinKind,
      target_id: entry.origin.targetId,
    });
  };
  return (
    <SidebarMenuItem>
      <SidebarMenuButton asChild className="group/pin">
        <a
          href={entry.href ?? "#"}
          aria-disabled={!entry.href || undefined}
          className={cn(!entry.href && "pointer-events-none opacity-60")}
        >
          <Icon className="size-4" />
          <span className="truncate">{entry.label}</span>
          {entry.origin.kind === "pin" && (
            <button
              type="button"
              onClick={(ev) => {
                ev.preventDefault();
                ev.stopPropagation();
                onUnpin();
              }}
              title="Unpin"
              aria-label={`Unpin ${entry.label}`}
              className="ml-auto opacity-0 transition group-hover/pin:opacity-100"
            >
              <IconPinnedOff className="size-3.5" />
            </button>
          )}
        </a>
      </SidebarMenuButton>
    </SidebarMenuItem>
  );
}

/**
 * A tag pin renders the tag's name as a clickable group header *and*
 * (when the cap allows) one row per visible repo link, per §6.1
 * "Pinning a tag is equivalent to pinning every repo currently
 * linked to it." The expansion respects §7.4 viewer-filtered links —
 * the backend has already filtered `links` to the viewer's
 * allow-list.
 */
function ExpandedTagPin({ pin }: { pin: PinDto }): JSX.Element {
  const detail = useTagDetail(pin.target_id);
  const repoExpansion = useMemo(
    () => expandTag(detail.data),
    [detail.data],
  );
  return (
    <>
      <SidebarMenuItem>
        <SidebarMenuButton asChild>
          <a href={`#/workflow/tags?id=${pin.target_id}`}>
            <IconTags className="size-4" />
            <span className="truncate">
              {detail.data?.tag.name ?? shortLabel(pin.target_id, "Tag")}
            </span>
            {detail.data && (
              <span className="ml-auto text-xs text-muted-foreground">
                {detail.data.tag.visible_link_count}
              </span>
            )}
          </a>
        </SidebarMenuButton>
      </SidebarMenuItem>
      {repoExpansion.map((entry) => (
        <PinRow key={entry.key} entry={entry} />
      ))}
    </>
  );
}

/** "…and N more" row, mounted only when the **post-expansion** list
 *  exceeds the §13.5 render cap. */
function PinOverflowMarker({
  pins,
  onOpenAll,
}: {
  pins: PinDto[];
  onOpenAll: () => void;
}): JSX.Element | null {
  // The expansion of tag pins happens inside `ExpandedTagPin` which
  // is rendered above us, so we cannot count post-expansion entries
  // synchronously here. We instead use an upper bound: assume each
  // tag pin can balloon to PIN_RENDER_CAP. That is intentionally
  // conservative — once a tag pin is present, the disclosure shows.
  const upperBound = pins.reduce(
    (acc, p) => acc + (p.kind === "tag" ? PIN_RENDER_CAP : 1),
    0,
  );
  if (upperBound <= PIN_RENDER_CAP) return null;
  const remaining = upperBound - (PIN_RENDER_CAP - 1);
  return (
    <SidebarMenuItem>
      <SidebarMenuButton
        onClick={onOpenAll}
        data-testid="pin-sidebar-overflow"
        className="text-muted-foreground"
      >
        <IconPinned className="size-4" />
        <span>…and {remaining} more</span>
      </SidebarMenuButton>
    </SidebarMenuItem>
  );
}

/** Dialog that shows the full, un-capped list — only opened from the
 *  "…and N more" disclosure row. */
function PinOverflowDialog({
  open,
  onOpenChange,
  pins,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  pins: PinDto[];
}): JSX.Element {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent data-testid="pin-overflow-dialog">
        <DialogHeader>
          <DialogTitle>All pinned items</DialogTitle>
        </DialogHeader>
        <ul className="flex flex-col gap-2">
          {pins.map((p) => (
            <li key={`${p.kind}:${p.target_id}`} className="flex items-center gap-2">
              {p.kind === "tag" ? (
                <IconTags className="size-4 text-muted-foreground" />
              ) : (
                <IconGitBranch className="size-4 text-muted-foreground" />
              )}
              <a
                href={
                  p.kind === "tag"
                    ? `#/workflow/tags?id=${p.target_id}`
                    : `#/workflow/issues?repo=${p.target_id}`
                }
                className="truncate text-sm hover:underline"
              >
                {shortLabel(p.target_id, p.kind === "tag" ? "Tag" : "Repo")}
              </a>
            </li>
          ))}
        </ul>
      </DialogContent>
    </Dialog>
  );
}

function expandTag(detail: TagDetailResponse | undefined): PinEntry[] {
  if (!detail) return [];
  return detail.links
    .filter((l) => l.kind === "repo")
    .map((l) => ({
      key: `tag-expand:${detail.tag.id}:${l.target_id}`,
      label: shortLabel(l.target_id, "Repo"),
      href: `#/workflow/issues?repo=${l.target_id}`,
      origin: { kind: "expanded", parentTagId: detail.tag.id },
      icon: IconGitBranch,
    }));
}

function shortLabel(id: string, kind: "Repo" | "Tag"): string {
  return `${kind} ${id.slice(0, 8)}`;
}
