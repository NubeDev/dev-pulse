/**
 * `<ViewsTabStrip>` — PROJECT-VIEW.md §5.4 / §7.1 (Slice 4) saved
 * views strip that lives directly above the workbench [`Toolbar`].
 *
 * v1 scope:
 *
 *   * Pinned **All** tab on the left (the "no view" / ad-hoc state).
 *   * One tab per saved view, ordered by `position ASC`.
 *   * Click activates: `?view=<id>` is written and the ad-hoc
 *     `group` / `filter` / `sort` overrides are cleared in the same
 *     navigation so the view's stored shape takes over. The parent
 *     ([`ProjectWorkbench`]) owns the URL write; this component only
 *     surfaces the intent.
 *   * Dirty state: when a view is active **and** the parent reports
 *     `isDirty`, the active tab renders `● {name} *` plus inline
 *     **Save** / **Discard** buttons that PATCH or clear overrides.
 *   * `+ Save view` button is always visible. When invoked it asks
 *     for a name and creates a view from the *current* effective
 *     shape (group/filter/sort the parent passes in `current`).
 *   * Per-tab `×` (visible on hover) deletes after a confirm.
 *   * Drag-to-reorder via HTML5 DnD; on drop the parent re-orders
 *     server-side and the strip reflects the new positions on the
 *     next response.
 *
 * The strip is **owner-only** in v1 (visibility = `private`) — every
 * row returned by `GET /projects/{id}/views` already belongs to the
 * caller so no extra filtering happens here.
 */
import { useState } from "react";

import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";

import type {
  ProjectViewDto,
  ProjectViewFilterClause,
  ProjectViewWriteBody,
} from "../api/client.js";

/** The effective workbench shape the strip needs both to save new
 *  views from the current toolbar state and to drive the dirty
 *  diff against the active view. */
export interface ViewsTabStripCurrent {
  groupBy: string | null;
  filterClauses: ProjectViewFilterClause[];
  sort: string;
}

export interface ViewsTabStripProps {
  views: ProjectViewDto[];
  /** `null` ⇒ pinned **All** tab is active. */
  activeViewId: string | null;
  /** Only ever `true` when `activeViewId !== null` and the URL
   *  overrides diverge from the saved view's shape. The parent
   *  computes this. */
  isDirty: boolean;
  /** The current effective toolbar state. Used as the seed for
   *  `+ Save view` and for `Save changes` on a dirty active view. */
  current: ViewsTabStripCurrent;
  onSelectView: (viewId: string | null) => void;
  onCreateView: (body: ProjectViewWriteBody) => void;
  onUpdateView: (viewId: string, body: ProjectViewWriteBody) => void;
  onDeleteView: (viewId: string) => void;
  onReorderViews: (orderedIds: string[]) => void;
  /** Clear `group`/`filter`/`sort` overrides while keeping the
   *  active view. The parent rewrites the URL. */
  onDiscardDirty: () => void;
  /** Disabled while any create / update / delete / reorder is in
   *  flight so we don't fire overlapping mutations. */
  busy?: boolean;
}

export function ViewsTabStrip({
  views,
  activeViewId,
  isDirty,
  current,
  onSelectView,
  onCreateView,
  onUpdateView,
  onDeleteView,
  onReorderViews,
  onDiscardDirty,
  busy,
}: ViewsTabStripProps): JSX.Element {
  const [savePromptOpen, setSavePromptOpen] = useState(false);
  const [draftName, setDraftName] = useState("");
  const [dragId, setDragId] = useState<string | null>(null);

  const activeView = activeViewId
    ? views.find((v) => v.id === activeViewId) ?? null
    : null;

  const submitNewView = (): void => {
    const name = draftName.trim();
    if (!name) return;
    onCreateView({
      name,
      group_by: current.groupBy,
      filter_clauses: current.filterClauses,
      sort: current.sort,
    });
    setDraftName("");
    setSavePromptOpen(false);
  };

  const saveDirtyView = (): void => {
    if (!activeView) return;
    onUpdateView(activeView.id, {
      name: activeView.name,
      group_by: current.groupBy,
      filter_clauses: current.filterClauses,
      sort: current.sort,
    });
  };

  const handleDrop = (targetId: string): void => {
    if (!dragId || dragId === targetId) {
      setDragId(null);
      return;
    }
    const ids = views.map((v) => v.id);
    const fromIdx = ids.indexOf(dragId);
    const toIdx = ids.indexOf(targetId);
    if (fromIdx < 0 || toIdx < 0) {
      setDragId(null);
      return;
    }
    const next = [...ids];
    next.splice(fromIdx, 1);
    next.splice(toIdx, 0, dragId);
    setDragId(null);
    onReorderViews(next);
  };

  return (
    <div
      className="flex flex-wrap items-center gap-1 border-b border-border pb-2"
      data-testid="project-views-tab-strip"
    >
      {/* Pinned "All" tab — ad-hoc / no view selected. */}
      <ViewTabButton
        label="All"
        active={activeViewId === null}
        onClick={() => onSelectView(null)}
        testid="project-view-tab-all"
      />

      {views.map((v) => {
        const active = v.id === activeViewId;
        const dirty = active && isDirty;
        return (
          <div
            key={v.id}
            className="flex items-center"
            draggable
            onDragStart={() => setDragId(v.id)}
            onDragOver={(e) => {
              if (dragId) e.preventDefault();
            }}
            onDrop={() => handleDrop(v.id)}
            onDragEnd={() => setDragId(null)}
          >
            <ViewTabButton
              label={dirty ? `● ${v.name} *` : v.name}
              active={active}
              onClick={() => onSelectView(v.id)}
              testid={`project-view-tab-${v.id}`}
            />
            {active && (
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 px-1 text-muted-foreground"
                    aria-label={`Actions for view ${v.name}`}
                    data-testid={`project-view-tab-menu-${v.id}`}
                  >
                    ⋯
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <DropdownMenuItem
                    disabled={busy}
                    onSelect={() => {
                      if (
                        // eslint-disable-next-line no-alert
                        window.confirm(
                          `Delete view "${v.name}"? This can't be undone.`,
                        )
                      ) {
                        onDeleteView(v.id);
                      }
                    }}
                    data-testid={`project-view-tab-delete-${v.id}`}
                  >
                    Delete view
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            )}
          </div>
        );
      })}

      {/* Dirty controls — only when an active view diverges from the URL. */}
      {activeView && isDirty && (
        <div className="ml-2 flex items-center gap-1">
          <Button
            size="sm"
            variant="default"
            onClick={saveDirtyView}
            disabled={busy}
            data-testid="project-view-save-changes"
          >
            Save changes
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={onDiscardDirty}
            disabled={busy}
            data-testid="project-view-discard"
          >
            Discard
          </Button>
        </div>
      )}

      {/* Save-as-new view — visible at all times. The button doubles
       *  as the only entry point for first-time view creation. */}
      <div className="ml-auto flex items-center gap-1">
        {savePromptOpen ? (
          <>
            <Input
              autoFocus
              value={draftName}
              onChange={(e) => setDraftName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submitNewView();
                if (e.key === "Escape") {
                  setSavePromptOpen(false);
                  setDraftName("");
                }
              }}
              placeholder="View name…"
              maxLength={60}
              className="h-7 w-40 text-sm"
              data-testid="project-view-new-name"
            />
            <Button
              size="sm"
              onClick={submitNewView}
              disabled={busy || draftName.trim().length === 0}
              data-testid="project-view-new-confirm"
            >
              Save
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                setSavePromptOpen(false);
                setDraftName("");
              }}
            >
              Cancel
            </Button>
          </>
        ) : (
          <Button
            size="sm"
            variant="outline"
            onClick={() => setSavePromptOpen(true)}
            disabled={busy}
            data-testid="project-view-save-as"
          >
            + Save view
          </Button>
        )}
      </div>
    </div>
  );
}

interface ViewTabButtonProps {
  label: string;
  active: boolean;
  onClick: () => void;
  testid: string;
}

function ViewTabButton({
  label,
  active,
  onClick,
  testid,
}: ViewTabButtonProps): JSX.Element {
  return (
    <button
      type="button"
      onClick={onClick}
      data-testid={testid}
      data-active={active ? "true" : "false"}
      className={
        active
          ? "h-7 rounded-md border border-border bg-background px-3 text-sm font-medium shadow-sm"
          : "h-7 rounded-md border border-transparent px-3 text-sm text-muted-foreground hover:bg-muted/40"
      }
    >
      {label}
    </button>
  );
}
