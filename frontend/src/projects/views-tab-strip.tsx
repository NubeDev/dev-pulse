/**
 * `<ViewsTabStrip>` — PROJECT-VIEW.md §5.4 / §7.1 (Slice 4) saved
 * views strip that lives directly above the workbench [`Toolbar`].
 *
 * Add-view affordance amendment (May 2026): the right-aligned
 * `+ Save view` button is gone. In its place there is a compact
 * `+` icon **inline** at the tail of the tab row — adjacent to the
 * pinned `All` tab when no views exist, or to the last saved view
 * otherwise. Clicking it opens a dialog (`<ViewSettingsDialog>`)
 * with: **name**, **start date**, **due date** — the latter two
 * editable AU-format pickers ([`DateInput`]). The same dialog is
 * reused (with the row pre-filled) by the per-tab `⋯ → Edit view…`
 * action; the old inline rename input is retired.
 *
 * Dates are stored as `YYYY-MM-DD` on the wire and rendered in the
 * UI as AU `dd/mm/yyyy` plus a relative "Nth week of <Month>" badge
 * for the due date (see [`weekOfMonthLabel`]).
 */
import { useEffect, useState } from "react";

import {
  AlertOctagonIcon,
  BoxesIcon,
  CheckCircle2Icon,
  FlagIcon,
  ListChecksIcon,
  PencilIcon,
  SparklesIcon,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { DateInput } from "@/components/ui/date-input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

import { gateMetaForName, iconForName } from "./icon-for-name.js";

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
   *  the add-view dialog and for `Save changes` on a dirty active
   *  view. */
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

type DialogState =
  | { kind: "closed" }
  | { kind: "create" }
  | { kind: "edit"; view: ProjectViewDto };

// ---------------------------------------------------------------------------
// View templates — frontend-only seeds for the create dialog.
//
// Each template either seeds the dialog with a single view definition
// (most templates), or expands into a *batch* of views written back-
// to-back (the gate-progression template fans out into 8 tabs, one
// per gate). Nothing here writes directly — the picker only re-seeds
// the dialog's local state or hands a batch to the parent's `onBulk`
// callback. Hidden in edit mode.
// ---------------------------------------------------------------------------

/** Single-view seed used by every non-batch template and by Custom. */
interface ViewTemplateSeed {
  name: string;
  groupBy: string | null;
  filterClauses: ProjectViewFilterClause[];
}

/** Catalogue entry rendered as a tile in the picker. */
interface ViewTemplate {
  id: string;
  label: string;
  description: string;
  /** Lucide icon rendered at ~20px in the picker tile. */
  Icon: React.ComponentType<{ className?: string }>;
  /** `single`: pre-fills the form, user clicks Create.
   *  `batch`: writes N views immediately, then closes the dialog.
   *  `custom`: uses the current toolbar shape, no auto-fill. */
  kind: "single" | "batch" | "custom";
  /** Populated when `kind === "single"`. */
  seed?: ViewTemplateSeed;
  /** Populated when `kind === "batch"`. Each entry becomes one view. */
  batch?: ViewTemplateSeed[];
}

/** The 8 gates from PROJECT-VIEW.md §5.1 ordinal list, expanded into
 *  a per-gate tab. Each filters `tag:gate:<key>` so the resulting
 *  tab shows only that gate's issues. Names match the user-visible
 *  short labels from the design (G1–G8). */
const GATES: Array<{ key: string; short: string; label: string }> = [
  { key: "g1-executive-summary", short: "G1", label: "Executive Summary" },
  { key: "g2-poc", short: "G2", label: "Proof of Concept" },
  { key: "g3-mvp-build", short: "G3", label: "MVP Build" },
  { key: "g4-client-acceptance", short: "G4", label: "Client Acceptance" },
  { key: "g5-product-refinement", short: "G5", label: "Product Refinement" },
  { key: "g6-production-ready", short: "G6", label: "Production Ready" },
  { key: "g7-go-to-market", short: "G7", label: "Go-To-Market" },
  { key: "g8-scale-support", short: "G8", label: "Scale & Support" },
];

const VIEW_TEMPLATES: ViewTemplate[] = [
  {
    id: "gate-progression",
    label: "Gate progression (G1–G8)",
    description:
      "Creates 8 tabs — one per gate from Executive Summary through Scale & Support. Each tab filters to its gate so progress is glanceable.",
    Icon: FlagIcon,
    kind: "batch",
    batch: GATES.map((g) => ({
      // Stored name is just the short code (`G1`, `G2`, …) so the
      // tab stays compact; the full label is surfaced as the tab's
      // hover tooltip and the icon's accent colour via
      // [`gateMetaForName`].
      //
      // Auto-filters disabled for now (May 2026): the per-gate
      // `tag:gate:<key>` clause was creating noisy filter chips
      // the user couldn't easily clear. Templates seed the tabs
      // with no filters; users can add their own via the toolbar
      // and Save changes on the dirty view.
      name: g.short,
      groupBy: null,
      filterClauses: [],
    })),
  },
  {
    id: "by-category",
    label: "By category",
    description:
      "One tab grouped by the `category` tag — firmware / hardware / backend / app are side-by-side. Best when issues are categorised.",
    Icon: BoxesIcon,
    kind: "single",
    seed: {
      name: "By category",
      groupBy: "tag:category",
      filterClauses: [],
    },
  },
  {
    id: "status-and-blocked",
    label: "Open vs closed / blocked",
    description:
      "Groups by open/closed and surfaces a `blocked`-labelled sub-bucket so stalled work is one click away.",
    Icon: ListChecksIcon,
    kind: "single",
    seed: {
      name: "Open vs closed",
      groupBy: "status",
      filterClauses: [],
    },
  },
  {
    id: "blocked-only",
    label: "Blocked only",
    description:
      "Single tab filtered to `label:blocked`, grouped by gate so you see where work has stalled across the gate progression.",
    Icon: AlertOctagonIcon,
    kind: "single",
    seed: {
      name: "Blocked",
      groupBy: "tag:gate",
      filterClauses: [{ dim: "label", value: "blocked" }],
    },
  },
  {
    id: "custom",
    label: "Custom",
    description:
      "Starts from the current group / filter / sort. The tab's icon is auto-picked from its name (rename to change it).",
    Icon: SparklesIcon,
    kind: "custom",
  },
];

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
  const [dialog, setDialog] = useState<DialogState>({ kind: "closed" });
  const [dragId, setDragId] = useState<string | null>(null);
  const [dragOverId, setDragOverId] = useState<string | null>(null);
  // Per-view due-date display mode (machine-local, see helpers
  // at the bottom of this file). Hydrated lazily from localStorage
  // for each view id encountered so changes take effect without
  // a reload and so the tab strip re-renders on toggle.
  const [dateDisplayByView, setDateDisplayByView] = useState<
    Record<string, DateDisplayMode>
  >(() => {
    const seed: Record<string, DateDisplayMode> = {};
    for (const v of views) {
      seed[v.id] = readDateDisplayMode(v.id);
    }
    return seed;
  });
  // Ensure any newly-arrived views are reflected once they show
  // up in the list (e.g. just-created tab from the dialog).
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
  // Per-view "completed" flag (machine-local, same scope as
  // dateDisplay). Forces the green-tick badge on the tab and
  // overrides the open/total derived state.
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

  const handleDialogSubmit = (body: ProjectViewWriteBody): void => {
    if (dialog.kind === "create") {
      onCreateView(body);
    } else if (dialog.kind === "edit") {
      onUpdateView(dialog.view.id, body);
    }
    setDialog({ kind: "closed" });
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
      // The strip now reads as a classic browser-style tab row:
      // every tab has visible side borders and a top border, the
      // active tab "sits on" the strip's baseline (its bottom
      // border is the same colour as the page background so it
      // appears to merge with the content below), and inactive
      // tabs share a subtle muted fill so the boundary between
      // adjacent tabs is unambiguous.
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
              // Firefox requires dataTransfer payload to start the drag.
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
                  ? {
                      open: v.open_issue_count,
                      total: v.total_issue_count,
                    }
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

      {/* Inline `+` icon — sits at the tail of the tab row, immediately
       *  after the pinned `All` tab (when no saved views exist) or
       *  after the last saved view. Single entry point for new views. */}
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

      <ViewSettingsDialog
        state={dialog}
        current={current}
        busy={busy}
        onCancel={() => setDialog({ kind: "closed" })}
        onSubmit={handleDialogSubmit}
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
  /** Lucide icon rendered to the left of the label. Auto-picked
   *  from the view name by [`iconForName`] for saved views;
   *  omitted on the pinned All tab. */
  icon?: React.ComponentType<{ className?: string }>;
  /** Optional tailwind class applied to the leading lucide icon
   *  so gate tabs (G1…G8) can each carry a distinct accent
   *  without dyeing the whole tab chrome. */
  iconClass?: string;
  /** Native `title` attribute — used for the gate tabs to show
   *  the full gate label (e.g. "Executive Summary") on hover
   *  while the visible label stays the short code. */
  title?: string;
  /** Optional `open / total` count rendered as a subdued suffix.
   *  Populated for saved-view tabs from
   *  `ProjectViewDto.{open,total}_issue_count`; omitted on the
   *  pinned All tab (no count is computed server-side for it). */
  count?: { open: number; total: number };
  /** Optional due date (`YYYY-MM-DD`); rendered next to the count
   *  as a relative "Nth week of <Month>" badge. */
  dueDate?: string | null;
  /** How to render the due date badge — see [`DateDisplayMode`].
   *  Defaults to `"week"` when omitted. */
  dateDisplay?: DateDisplayMode;
  /** When `true` the tab forces the green-tick badge regardless
   *  of `count`, signalling the user explicitly marked the view
   *  as done (e.g. gate completed) without waiting for every
   *  underlying issue to close. */
  completed?: boolean;
  /** When provided, an inline pencil-edit affordance is rendered
   *  at the trailing edge of the tab (only visible while the tab
   *  is active). Click is isolated from the tab's own onClick so
   *  selecting the pencil doesn't also re-trigger view selection. */
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
          ? // Active tab: solid background flush with the panel
            // below, top + sides bordered, bottom edge masked by
            // a -1px offset so it merges with the strip baseline.
            "relative -mb-px inline-flex h-8 items-center gap-1.5 rounded-t-md border border-border border-b-background bg-background px-3 text-sm font-semibold text-foreground shadow-[0_-1px_0_0_var(--primary)]"
          : // Inactive tab: muted fill so each tab is its own
            // pill; hover lifts it toward the active treatment.
            "inline-flex h-7 items-center gap-1.5 rounded-t-md border border-border/60 border-b-transparent bg-muted/40 px-3 text-sm font-medium text-foreground/70 hover:bg-muted hover:text-foreground"
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
          // All issues closed (or the user explicitly marked the
          // view as completed) — swap the `N/N` badge for a green
          // tick so a glanceable "done" state pops on the strip.
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
        // Inline pencil-edit affordance — rendered inside the tab
        // so it visually belongs to it. Using a `<span role="button">`
        // (not a nested <button>, which is invalid HTML) and
        // stopping propagation so the outer tab's onClick doesn't
        // also re-fire.
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

// ---------------------------------------------------------------------------
// View settings dialog (shared by create + edit)
// ---------------------------------------------------------------------------

interface ViewSettingsDialogProps {
  state: DialogState;
  current: ViewsTabStripCurrent;
  busy?: boolean;
  onCancel: () => void;
  onSubmit: (body: ProjectViewWriteBody) => void;
  /** Invoked when the user confirms deletion from the edit dialog
   *  footer. Only wired in edit mode; ignored on create. */
  onDelete?: (viewId: string) => void;
  /** Current per-view date-display preference; only meaningful
   *  in edit mode (the value is read from the parent's local
   *  cache, not from the view DTO). */
  dateDisplay?: DateDisplayMode;
  /** Called when the user changes the display mode. Persistence
   *  is the parent's concern; this dialog is presentational. */
  onChangeDateDisplay?: (mode: DateDisplayMode) => void;
  /** Current completed flag (machine-local). Only meaningful in
   *  edit mode. */
  completed?: boolean;
  /** Called when the user toggles the completed checkbox. */
  onChangeCompleted?: (completed: boolean) => void;
}

function ViewSettingsDialog({
  state,
  current,
  busy,
  onCancel,
  onSubmit,
  onDelete,
  dateDisplay,
  onChangeDateDisplay,
  completed,
  onChangeCompleted,
}: ViewSettingsDialogProps): JSX.Element {
  const open = state.kind !== "closed";
  const editing = state.kind === "edit" ? state.view : null;
  const [name, setName] = useState("");
  const [startDate, setStartDate] = useState("");
  const [dueDate, setDueDate] = useState("");
  // Template seed (create mode only). When set it overrides the
  // current toolbar shape on submit; the user can still rename / set
  // dates without losing the seed.
  const [template, setTemplate] = useState<ViewTemplate | null>(null);

  // Reset the form whenever the dialog opens or the target view
  // changes. The dialog stays mounted (it's a portal) so the local
  // state would otherwise leak across reuses.
  useEffect(() => {
    if (state.kind === "edit") {
      setName(state.view.name);
      setStartDate(state.view.start_date ?? "");
      setDueDate(state.view.due_date ?? "");
      setTemplate(null);
    } else if (state.kind === "create") {
      setName("");
      setStartDate("");
      setDueDate("");
      setTemplate(null);
    }
  }, [state]);

  const applyTemplate = (t: ViewTemplate): void => {
    setTemplate(t);
    if (t.kind === "single" && t.seed) {
      // Only auto-fill the name when the user hasn't started typing
      // one — avoids clobbering an in-flight rename if the user
      // double-clicks a template.
      if (name.trim().length === 0) {
        setName(t.seed.name);
      }
    } else if (t.kind === "batch") {
      // Name + dates aren't used in batch mode (each view gets the
      // batch entry's own name + the shared dates). Clear the name
      // so the empty-state guidance reads "8 tabs will be created".
      setName("");
    }
    // custom: no auto-fill; user types a name and the icon is
    // derived from it.
  };

  const trimmed = name.trim();
  const isBatch = template?.kind === "batch";
  const canSubmit = isBatch
    ? !busy && (template?.batch?.length ?? 0) > 0
    : trimmed.length > 0 && trimmed.length <= 60 && !busy;

  // Live icon preview: auto-derived from whatever the user has
  // typed. Drives both the inline preview row and the tab itself
  // once the view is saved (same `iconForName` is called there).
  const PreviewIcon = iconForName(trimmed || "view");

  const submit = (): void => {
    if (!canSubmit) return;
    if (editing) {
      onSubmit({
        name: trimmed,
        group_by: editing.group_by,
        filter_clauses: editing.filter_clauses,
        sort: editing.sort,
        start_date: startDate || null,
        due_date: dueDate || null,
      });
      return;
    }
    if (template?.kind === "batch" && template.batch) {
      // Fan-out: one POST per entry. Each batch entry's name carries
      // an icon-friendly keyword (e.g. "G3 · MVP Build") so
      // `iconForName` picks a sensible glyph at render time.
      for (const entry of template.batch) {
        onSubmit({
          name: entry.name,
          group_by: entry.groupBy,
          filter_clauses: entry.filterClauses,
          sort: current.sort,
          start_date: startDate || null,
          due_date: dueDate || null,
        });
      }
      return;
    }
    if (template?.kind === "single" && template.seed) {
      onSubmit({
        name: trimmed || template.seed.name,
        group_by: template.seed.groupBy,
        filter_clauses: template.seed.filterClauses,
        sort: current.sort,
        start_date: startDate || null,
        due_date: dueDate || null,
      });
      return;
    }
    // Custom or no template — use the current toolbar shape.
    onSubmit({
      name: trimmed,
      group_by: current.groupBy,
      filter_clauses: current.filterClauses,
      sort: current.sort,
      start_date: startDate || null,
      due_date: dueDate || null,
    });
  };

  return (
    <Dialog open={open} onOpenChange={(o) => (o ? null : onCancel())}>
      <DialogContent
        className="sm:max-w-xl"
        data-testid="project-view-settings-dialog"
      >
        <DialogHeader>
          <DialogTitle>{editing ? "Edit view" : "New view"}</DialogTitle>
          <DialogDescription>
            {editing
              ? "Rename this view or adjust its timeline. The tab icon is auto-picked from the name. Dates use AU dd/mm/yyyy."
              : "Pick a template or build a custom view. The tab icon is auto-picked from the view name (e.g. 'bug' → bug icon, 'gate' → flag). Dates are optional and use AU dd/mm/yyyy."}
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-4 py-2">
          {/* Template picker — create mode only. */}
          {!editing && (
            <div className="flex flex-col gap-1.5">
              <Label>Start from a template</Label>
              <div
                className="grid grid-cols-2 gap-2"
                data-testid="project-view-templates"
              >
                {VIEW_TEMPLATES.map((t) => {
                  const selected = template?.id === t.id;
                  return (
                    <button
                      key={t.id}
                      type="button"
                      onClick={() => applyTemplate(t)}
                      data-testid={`project-view-template-${t.id}`}
                      data-selected={selected ? "true" : "false"}
                      className={
                        selected
                          ? "flex items-start gap-2 rounded-md border border-primary bg-primary/5 p-2 text-left ring-1 ring-primary"
                          : "flex items-start gap-2 rounded-md border border-border p-2 text-left hover:bg-muted/40"
                      }
                    >
                      <t.Icon className="mt-0.5 size-5 shrink-0 text-muted-foreground" />
                      <div className="flex flex-col gap-0.5">
                        <span className="text-sm font-medium">{t.label}</span>
                        <span className="text-xs text-muted-foreground">
                          {t.description}
                        </span>
                      </div>
                    </button>
                  );
                })}
              </div>
              {isBatch ? (
                <p
                  className="text-xs text-muted-foreground"
                  data-testid="project-view-batch-hint"
                >
                  {template?.batch?.length ?? 0} tabs will be created in one
                  click. Each gets the shared dates below; each tab's icon
                  is auto-picked from its name.
                </p>
              ) : (
                <p className="text-xs text-muted-foreground">
                  Templates pre-fill group / filter. You can still edit them
                  after saving via the tab's ⋯ menu.
                </p>
              )}
            </div>
          )}

          {/* Name + live icon preview — hidden in batch mode (each
              batch entry has its own pre-baked name). */}
          {!isBatch && (
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="project-view-name">Name</Label>
              <div className="flex items-center gap-2">
                <div
                  className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-border bg-muted/40"
                  data-testid="project-view-name-icon"
                  title="Auto-picked from the name"
                >
                  <PreviewIcon className="size-4 text-muted-foreground" />
                </div>
                <Input
                  id="project-view-name"
                  autoFocus
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") submit();
                  }}
                  placeholder="View name…"
                  maxLength={60}
                  data-testid="project-view-name-input"
                />
              </div>
              <p className="text-xs text-muted-foreground">
                Tip: words like <code>bug</code>, <code>gate</code>,{" "}
                <code>blocked</code>, <code>urgent</code>, <code>release</code>{" "}
                pick matching icons.
              </p>
            </div>
          )}

          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="project-view-start-date">Start date</Label>
              <DateInput
                id="project-view-start-date"
                value={startDate}
                onChange={(e) => setStartDate(e.target.value)}
                data-testid="project-view-start-date"
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="project-view-due-date">Due date</Label>
              <DateInput
                id="project-view-due-date"
                value={dueDate}
                onChange={(e) => setDueDate(e.target.value)}
                data-testid="project-view-due-date"
              />
              {dueDate ? (
                <p
                  className="text-xs text-muted-foreground"
                  data-testid="project-view-due-date-preview"
                >
                  {weekOfMonthLabel(dueDate)}
                </p>
              ) : null}
            </div>
          </div>

          {editing && onChangeCompleted ? (
            <div className="flex items-start gap-2 rounded-md border border-border bg-muted/30 p-3">
              <Checkbox
                id="project-view-completed"
                checked={completed ?? false}
                onCheckedChange={(v) => onChangeCompleted(v === true)}
                data-testid="project-view-completed"
                className="mt-0.5"
              />
              <div className="flex flex-col gap-0.5">
                <Label
                  htmlFor="project-view-completed"
                  className="cursor-pointer text-sm font-medium"
                >
                  Mark this view as completed
                </Label>
                <p className="text-xs text-muted-foreground">
                  Forces the green tick on the tab regardless of the
                  open / total issue count. Useful for gates you
                  consider done even if a few cleanup tickets remain.
                </p>
              </div>
            </div>
          ) : null}

          {editing && onChangeDateDisplay ? (
            <div className="flex flex-col gap-1.5">
              <Label>
                Date display
                <span className="ml-1 text-xs font-normal text-muted-foreground">
                  (how the due date appears on this tab)
                </span>
              </Label>
              {/* Segmented button group instead of a Select so the
               *  picker can't pop over the Completed card or the
               *  Save button — three mutually exclusive options
               *  fit cleanly inline. */}
              <div
                role="radiogroup"
                aria-label="Date display"
                className="inline-flex w-fit overflow-hidden rounded-md border border-border"
                data-testid="project-view-date-display"
              >
                {(
                  [
                    { value: "hide", label: "Hide" },
                    { value: "week", label: "Week of month" },
                    { value: "date", label: "Date (DD:Mon:YY)" },
                  ] as Array<{ value: DateDisplayMode; label: string }>
                ).map((opt, i) => {
                  const selected = (dateDisplay ?? "week") === opt.value;
                  return (
                    <button
                      key={opt.value}
                      type="button"
                      role="radio"
                      aria-checked={selected}
                      onClick={() => onChangeDateDisplay(opt.value)}
                      className={[
                        "px-3 py-1.5 text-xs font-medium transition-colors",
                        i > 0 ? "border-l border-border" : "",
                        selected
                          ? "bg-foreground text-background"
                          : "bg-background text-foreground/70 hover:bg-muted",
                      ]
                        .filter(Boolean)
                        .join(" ")}
                      data-testid={`project-view-date-display-${opt.value}`}
                    >
                      {opt.label}
                    </button>
                  );
                })}
              </div>
              <p className="text-xs text-muted-foreground">
                {dateDisplay === "week" || dateDisplay === undefined
                  ? "Week of month — e.g. “2nd week of June”. Year is appended when the due date isn’t this year."
                  : dateDisplay === "date"
                    ? "DD:Mon:YY — abbreviated month so it doesn’t read like a clock time."
                    : "Badge hidden on the tab."}
              </p>
              {dueDate ? (
                <p
                  className="text-xs text-muted-foreground"
                  data-testid="project-view-date-display-preview"
                >
                  Preview:{" "}
                  <span className="font-medium text-foreground">
                    {formatDateDisplay(dueDate, dateDisplay ?? "week") ??
                      "(hidden)"}
                  </span>
                </p>
              ) : null}
            </div>
          ) : null}
        </div>

        <DialogFooter className={editing ? "sm:justify-between" : undefined}>
          {editing && onDelete ? (
            <Button
              variant="ghost"
              onClick={() => {
                // eslint-disable-next-line no-alert
                if (window.confirm(`Delete view "${editing.name}"? This can't be undone.`)) {
                  onDelete(editing.id);
                }
              }}
              disabled={busy}
              className="text-destructive hover:bg-destructive/10 hover:text-destructive"
              data-testid={`project-view-settings-delete-${editing.id}`}
            >
              Delete view
            </Button>
          ) : (
            <span />
          )}
          <div className="flex items-center gap-2">
            <Button variant="ghost" onClick={onCancel} disabled={busy}>
              Cancel
            </Button>
            <Button
              onClick={submit}
              disabled={!canSubmit}
              data-testid="project-view-settings-submit"
            >
              {editing
                ? "Save"
                : isBatch
                  ? `Create ${template?.batch?.length ?? 0} tabs`
                  : "Create"}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ---------------------------------------------------------------------------
// Date helpers
// ---------------------------------------------------------------------------

const MONTHS = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];

const ORDINALS = ["1st", "2nd", "3rd", "4th", "5th"];

const MONTH_ABBR = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
];

/** Map a `YYYY-MM-DD` calendar date to a "Nth week of <Month>" label,
 *  rounded to the closest week within that month. Week boundaries
 *  are 1-7, 8-14, 15-21, 22-28, 29-end (so a 5-week bucket is only
 *  rendered when the date falls in the tail of a long month). */
export function weekOfMonthLabel(iso: string): string {
  const m = iso.match(/^(\d{4})-(\d{2})-(\d{2})$/);
  if (!m) return "";
  const month = Number(m[2]);
  const day = Number(m[3]);
  if (month < 1 || month > 12 || day < 1) return "";
  const weekIdx = Math.min(Math.floor((day - 1) / 7), 4);
  return `${ORDINALS[weekIdx]} week of ${MONTHS[month - 1]}`;
}

/** Render a `YYYY-MM-DD` as AU `dd/mm/yyyy`. */
function formatAu(iso: string): string {
  const m = iso.match(/^(\d{4})-(\d{2})-(\d{2})$/);
  if (!m) return iso;
  return `${m[3]}/${m[2]}/${m[1]}`;
}

// ---------------------------------------------------------------------------
// Per-view due-date display preference.
//
// Three modes selectable from the edit dialog:
//   - "hide"  — no badge on the tab.
//   - "week"  — "Nth week of <Month>" (default). When the due
//     year differs from the current year the year is appended so
//     a 2027 due date doesn't read as "next month".
//   - "date"  — compact `DD:Mon:YY` (abbreviated month, e.g.
//     `13:Jun:26`) so the badge doesn't get mistaken for a
//     time-of-day stamp like 13:06:26.
//
// Persisted in localStorage keyed by view id so the choice is
// machine-local but survives reloads, no backend migration needed.
// ---------------------------------------------------------------------------

export type DateDisplayMode = "hide" | "week" | "date";

const DATE_DISPLAY_DEFAULT: DateDisplayMode = "week";
const DATE_DISPLAY_LS_PREFIX = "dp.projectView.dateDisplay.";

function isDateDisplayMode(v: unknown): v is DateDisplayMode {
  return v === "hide" || v === "week" || v === "date";
}

/** Read a saved display mode for `viewId`. Returns the default
 *  ("week") on first access / corrupt value / SSR. */
export function readDateDisplayMode(viewId: string): DateDisplayMode {
  if (typeof window === "undefined") return DATE_DISPLAY_DEFAULT;
  try {
    const raw = window.localStorage.getItem(DATE_DISPLAY_LS_PREFIX + viewId);
    return isDateDisplayMode(raw) ? raw : DATE_DISPLAY_DEFAULT;
  } catch {
    return DATE_DISPLAY_DEFAULT;
  }
}

/** Persist `mode` for `viewId`. A no-op (silently) when storage
 *  is unavailable (private mode quota, SSR). */
export function writeDateDisplayMode(
  viewId: string,
  mode: DateDisplayMode,
): void {
  if (typeof window === "undefined") return;
  try {
    if (mode === DATE_DISPLAY_DEFAULT) {
      window.localStorage.removeItem(DATE_DISPLAY_LS_PREFIX + viewId);
    } else {
      window.localStorage.setItem(DATE_DISPLAY_LS_PREFIX + viewId, mode);
    }
  } catch {
    // ignore
  }
}

/** Format `iso` (`YYYY-MM-DD`) per `mode`. Returns `null` when
 *  the badge should be hidden. The "week" mode appends the year
 *  whenever the due year differs from the current calendar year
 *  so a 2027 date doesn't get confused with this year's June. */
export function formatDateDisplay(
  iso: string,
  mode: DateDisplayMode,
): string | null {
  if (mode === "hide") return null;
  const m = iso.match(/^(\d{4})-(\d{2})-(\d{2})$/);
  if (!m) return null;
  if (mode === "date") {
    // DD:Mon:YY — abbreviated month so the badge doesn't read
    // like a clock time (e.g. "13:Jun:26" instead of "13:06:26").
    const monthIdx = Number(m[2]) - 1;
    const mon =
      monthIdx >= 0 && monthIdx < 12
        ? MONTH_ABBR[monthIdx]
        : m[2];
    return `${m[3]}:${mon}:${m[1]!.slice(2)}`;
  }
  // mode === "week"
  const base = weekOfMonthLabel(iso);
  if (!base) return null;
  const dueYear = Number(m[1]);
  const thisYear = new Date().getFullYear();
  return dueYear === thisYear ? base : `${base} ${dueYear}`;
}

// ---------------------------------------------------------------------------
// Per-view "completed" flag.
//
// Lets the user mark a gate / view as done independently of the
// open/total issue count. Persisted in localStorage (same scope
// as the date-display mode) so the choice is machine-local but
// survives reloads. When set, the tab renders the green-tick
// badge regardless of the underlying issue counts.
// ---------------------------------------------------------------------------

const COMPLETED_LS_PREFIX = "dp.projectView.completed.";

export function readCompleted(viewId: string): boolean {
  if (typeof window === "undefined") return false;
  try {
    return window.localStorage.getItem(COMPLETED_LS_PREFIX + viewId) === "1";
  } catch {
    return false;
  }
}

export function writeCompleted(viewId: string, completed: boolean): void {
  if (typeof window === "undefined") return;
  try {
    if (completed) {
      window.localStorage.setItem(COMPLETED_LS_PREFIX + viewId, "1");
    } else {
      window.localStorage.removeItem(COMPLETED_LS_PREFIX + viewId);
    }
  } catch {
    // ignore
  }
}
