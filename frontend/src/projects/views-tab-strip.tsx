/**
 * `<ViewsTabStrip>` — PROJECT-VIEW.md §5.4 / §7.1 (Slice 4) saved
 * views strip that lives directly above the workbench
 * [`Toolbar`].
 *
 * The strip itself is now thin chrome — every interactive editor
 * (create wizard, edit dialog, category helpers, date-display
 * helpers) lives under [`./view-wizard/`]. The strip just renders
 * tabs, wires drag-and-drop reordering, and opens the right
 * dialog when the `+` icon or per-tab pencil is hit.
 */

import { useEffect, useState } from "react";

import { CheckCircle2Icon, PencilIcon } from "lucide-react";

import { Button } from "@/components/ui/button";

import { gateMetaForName, iconForName } from "./icon-for-name.js";

import {
  EditViewDialog,
  formatAu,
  formatDateDisplay,
  NewViewWizard,
  readCompleted,
  readDateDisplayMode,
  writeCompleted,
  writeDateDisplayMode,
  type DateDisplayMode,
} from "./view-wizard/index.js";

import type {
  ProjectViewDto,
  ProjectViewFilterClause,
  ProjectViewWriteBody,
  TagDto,
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
  current: ViewsTabStripCurrent;
  /** Org of the project — required so the wizard and the edit
   *  dialog can create org-scoped `category:<slug>` tags before
   *  the view PATCH lands. */
  orgId: string;
  /** Cached tag list from the workbench's `useTags()` query.
   *  Forwarded to both dialogs so `ensureCategoryTags` can skip
   *  slugs that already have a backing tag instead of POSTing and
   *  relying on the server's 409 swallow. `null` while the query
   *  is loading — the dialog will fetch as a fallback. */
  existingTags: readonly TagDto[] | null;
  onSelectView: (viewId: string | null) => void;
  onCreateView: (
    body: ProjectViewWriteBody,
    dateDisplay: DateDisplayMode,
  ) => void;
  /** Create many views as one verified unit (the G1–G8 gate
   *  progression). Separate from `onCreateView` because the batch
   *  reconciles against the server until every view exists. */
  onCreateViewBatch: (
    bodies: ProjectViewWriteBody[],
    dateDisplay: DateDisplayMode,
  ) => void;
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

type DialogState =
  | { kind: "closed" }
  | { kind: "create" }
  | { kind: "edit"; view: ProjectViewDto };

export function ViewsTabStrip({
  views,
  activeViewId,
  isDirty,
  current,
  orgId,
  existingTags,
  onSelectView,
  onCreateView,
  onCreateViewBatch,
  onUpdateView,
  onDeleteView,
  onReorderViews,
  onDiscardDirty,
  busy,
}: ViewsTabStripProps): JSX.Element {
  const [dialog, setDialog] = useState<DialogState>({ kind: "closed" });
  const [dragId, setDragId] = useState<string | null>(null);
  const [dragOverId, setDragOverId] = useState<string | null>(null);

  // Per-view due-date display mode (machine-local — see
  // `./view-wizard/date-display.ts`). Hydrated lazily for each
  // view id encountered so changes take effect without a reload
  // and so the tab strip re-renders on toggle.
  const [dateDisplayByView, setDateDisplayByView] = useState<
    Record<string, DateDisplayMode>
  >(() => {
    const seed: Record<string, DateDisplayMode> = {};
    for (const v of views) seed[v.id] = readDateDisplayMode(v.id);
    return seed;
  });
  useEffect(() => {
    setDateDisplayByView((prev) => {
      let changed = false;
      const next = { ...prev };
      for (const v of views) {
        if (!(v.id in next)) {
          next[v.id] = readDateDisplayMode(v.id);
          changed = true;
        }
      }
      return changed ? next : prev;
    });
  }, [views]);
  const setDateDisplayFor = (viewId: string, mode: DateDisplayMode): void => {
    writeDateDisplayMode(viewId, mode);
    setDateDisplayByView((prev) => ({ ...prev, [viewId]: mode }));
  };

  // Per-view "completed" flag (sibling of date-display).
  const [completedByView, setCompletedByView] = useState<
    Record<string, boolean>
  >(() => {
    const seed: Record<string, boolean> = {};
    for (const v of views) seed[v.id] = readCompleted(v.id);
    return seed;
  });
  useEffect(() => {
    setCompletedByView((prev) => {
      let changed = false;
      const next = { ...prev };
      for (const v of views) {
        if (!(v.id in next)) {
          next[v.id] = readCompleted(v.id);
          changed = true;
        }
      }
      return changed ? next : prev;
    });
  }, [views]);
  const setCompletedFor = (viewId: string, completed: boolean): void => {
    writeCompleted(viewId, completed);
    setCompletedByView((prev) => ({ ...prev, [viewId]: completed }));
  };

  const activeView = activeViewId
    ? views.find((v) => v.id === activeViewId) ?? null
    : null;

  const saveDirtyView = (): void => {
    if (!activeView) return;
    onUpdateView(activeView.id, {
      name: activeView.name,
      group_by: current.groupBy,
      filter_clauses: current.filterClauses,
      sort: current.sort,
      start_date: activeView.start_date ?? null,
      due_date: activeView.due_date ?? null,
    });
  };

  const handleDrop = (targetId: string): void => {
    setDragOverId(null);
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
      // Classic browser-style tab row: visible side borders + top
      // border per tab, the active tab's bottom border merges with
      // the strip baseline, inactive tabs share a muted fill so
      // the boundary between adjacent tabs is unambiguous.
      className="flex flex-wrap items-end gap-0.5 border-b border-border pb-0 pt-1"
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
        const isDragging = dragId === v.id;
        const isDropTarget =
          dragId !== null && dragOverId === v.id && dragId !== v.id;
        const ids = views.map((x) => x.id);
        const dropBefore =
          isDropTarget && ids.indexOf(dragId!) > ids.indexOf(v.id);
        const gateMeta = gateMetaForName(v.name);
        return (
          <div
            key={v.id}
            className={[
              "relative flex items-center rounded-md",
              "cursor-grab active:cursor-grabbing",
              isDragging ? "opacity-40" : "",
              isDropTarget
                ? dropBefore
                  ? "shadow-[inset_2px_0_0_0_var(--primary)]"
                  : "shadow-[inset_-2px_0_0_0_var(--primary)]"
                : "",
            ]
              .filter(Boolean)
              .join(" ")}
            draggable
            data-testid={`project-view-tab-drag-${v.id}`}
            onDragStart={(e) => {
              setDragId(v.id);
              e.dataTransfer.setData("text/plain", v.id);
              e.dataTransfer.effectAllowed = "move";
            }}
            onDragOver={(e) => {
              if (!dragId) return;
              e.preventDefault();
              e.dataTransfer.dropEffect = "move";
              if (dragOverId !== v.id) setDragOverId(v.id);
            }}
            onDragLeave={() => {
              if (dragOverId === v.id) setDragOverId(null);
            }}
            onDrop={() => handleDrop(v.id)}
            onDragEnd={() => {
              setDragId(null);
              setDragOverId(null);
            }}
          >
            <ViewTabButton
              label={dirty ? `● ${v.name} *` : v.name}
              icon={iconForName(v.name)}
              iconClass={gateMeta?.iconClass}
              title={gateMeta?.tooltip}
              active={active}
              onClick={() => onSelectView(v.id)}
              testid={`project-view-tab-${v.id}`}
              count={
                v.open_issue_count != null && v.total_issue_count != null
                  ? { open: v.open_issue_count, total: v.total_issue_count }
                  : undefined
              }
              dueDate={v.due_date ?? null}
              dateDisplay={dateDisplayByView[v.id] ?? "week"}
              completed={completedByView[v.id] ?? false}
              onEdit={
                busy ? undefined : () => setDialog({ kind: "edit", view: v })
              }
            />
          </div>
        );
      })}

      {/* Inline `+` icon — opens the create wizard. */}
      <Button
        type="button"
        variant="ghost"
        size="sm"
        onClick={() => setDialog({ kind: "create" })}
        disabled={busy}
        aria-label="New view"
        title="New view"
        className="h-7 w-7 px-0 text-base leading-none text-muted-foreground hover:text-foreground"
        data-testid="project-view-new"
      >
        +
      </Button>

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

      <NewViewWizard
        open={dialog.kind === "create"}
        orgId={orgId}
        existingTags={existingTags}
        current={current}
        busy={busy}
        onCancel={() => setDialog({ kind: "closed" })}
        onSubmit={(body, dateDisplay) => onCreateView(body, dateDisplay)}
        onSubmitBatch={(bodies, dateDisplay) =>
          onCreateViewBatch(bodies, dateDisplay)
        }
      />

      <EditViewDialog
        open={dialog.kind === "edit"}
        view={dialog.kind === "edit" ? dialog.view : null}
        orgId={orgId}
        existingTags={existingTags}
        busy={busy}
        onCancel={() => setDialog({ kind: "closed" })}
        onSubmit={(viewId, body) => {
          onUpdateView(viewId, body);
          setDialog({ kind: "closed" });
        }}
        onDelete={(viewId) => {
          onDeleteView(viewId);
          setDialog({ kind: "closed" });
        }}
        dateDisplay={
          dialog.kind === "edit"
            ? dateDisplayByView[dialog.view.id] ?? "week"
            : undefined
        }
        onChangeDateDisplay={(mode) => {
          if (dialog.kind === "edit") setDateDisplayFor(dialog.view.id, mode);
        }}
        completed={
          dialog.kind === "edit"
            ? completedByView[dialog.view.id] ?? false
            : undefined
        }
        onChangeCompleted={(done) => {
          if (dialog.kind === "edit") setCompletedFor(dialog.view.id, done);
        }}
      />
    </div>
  );
}

interface ViewTabButtonProps {
  label: string;
  active: boolean;
  onClick: () => void;
  testid: string;
  icon?: React.ComponentType<{ className?: string }>;
  iconClass?: string;
  title?: string;
  count?: { open: number; total: number };
  dueDate?: string | null;
  dateDisplay?: DateDisplayMode;
  completed?: boolean;
  onEdit?: () => void;
}

function ViewTabButton({
  label,
  active,
  onClick,
  testid,
  icon: Icon,
  iconClass,
  title,
  count,
  dueDate,
  dateDisplay = "week",
  completed = false,
  onEdit,
}: ViewTabButtonProps): JSX.Element {
  const dueLabel = dueDate ? formatDateDisplay(dueDate, dateDisplay) : null;
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      data-testid={testid}
      data-active={active ? "true" : "false"}
      className={
        active
          ? "relative -mb-px inline-flex h-8 items-center gap-1.5 rounded-t-md border border-border border-b-background bg-background px-3 text-sm font-semibold text-foreground shadow-[0_-1px_0_0_var(--primary)]"
          : "inline-flex h-7 items-center gap-1.5 rounded-t-md border border-border/60 border-b-transparent bg-muted/40 px-3 text-sm font-medium text-foreground/70 hover:bg-muted hover:text-foreground"
      }
    >
      {Icon ? (
        <Icon
          className={
            iconClass
              ? `size-3.5 shrink-0 ${iconClass}`
              : active
                ? "size-3.5 shrink-0 text-foreground"
                : "size-3.5 shrink-0 text-foreground/60"
          }
          data-testid={`${testid}-icon`}
        />
      ) : null}
      <span>{label}</span>
      {count ? (
        completed || (count.total > 0 && count.open === 0) ? (
          <span
            className="inline-flex items-center gap-1 rounded bg-emerald-100 px-1.5 text-[11px] tabular-nums text-emerald-700 dark:bg-emerald-950 dark:text-emerald-400"
            data-testid={`${testid}-count`}
            data-complete="true"
            title={
              completed
                ? `Marked completed (${count.open}/${count.total} open)`
                : `${count.total}/${count.total} closed`
            }
          >
            <CheckCircle2Icon className="size-3" />
            {completed && count.open > 0
              ? "Done"
              : `${count.total}/${count.total}`}
          </span>
        ) : (
          <span
            className="rounded bg-muted px-1.5 text-[11px] tabular-nums text-muted-foreground"
            data-testid={`${testid}-count`}
          >
            {count.open}/{count.total}
          </span>
        )
      ) : null}
      {dueLabel ? (
        <span
          className="rounded bg-muted px-1.5 text-[11px] text-muted-foreground"
          data-testid={`${testid}-due`}
          title={`Due ${formatAu(dueDate!)}`}
        >
          {dueLabel}
        </span>
      ) : null}
      {active && onEdit ? (
        <span
          role="button"
          tabIndex={0}
          aria-label={`Edit view ${label}`}
          title="Edit view"
          data-testid={`${testid}-edit`}
          onClick={(e) => {
            e.stopPropagation();
            onEdit();
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              e.stopPropagation();
              onEdit();
            }
          }}
          className="ml-1 -mr-1 inline-flex size-5 cursor-pointer items-center justify-center rounded text-muted-foreground hover:bg-muted hover:text-foreground"
        >
          <PencilIcon className="size-3" />
        </span>
      ) : null}
    </button>
  );
}
