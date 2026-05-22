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
  SparklesIcon,
} from "lucide-react";

import { Button } from "@/components/ui/button";
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

import { iconForName } from "./icon-for-name.js";

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
      name: `${g.short} · ${g.label}`,
      groupBy: null,
      filterClauses: [{ dim: "tag", key: "gate", value: g.key }],
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
              icon={iconForName(v.name)}
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
                    onSelect={() => setDialog({ kind: "edit", view: v })}
                    data-testid={`project-view-tab-rename-${v.id}`}
                  >
                    Edit view…
                  </DropdownMenuItem>
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
  /** Optional `open / total` count rendered as a subdued suffix.
   *  Populated for saved-view tabs from
   *  `ProjectViewDto.{open,total}_issue_count`; omitted on the
   *  pinned All tab (no count is computed server-side for it). */
  count?: { open: number; total: number };
  /** Optional due date (`YYYY-MM-DD`); rendered next to the count
   *  as a relative "Nth week of <Month>" badge. */
  dueDate?: string | null;
}

function ViewTabButton({
  label,
  active,
  onClick,
  testid,
  icon: Icon,
  count,
  dueDate,
}: ViewTabButtonProps): JSX.Element {
  const dueLabel = dueDate ? weekOfMonthLabel(dueDate) : null;
  return (
    <button
      type="button"
      onClick={onClick}
      data-testid={testid}
      data-active={active ? "true" : "false"}
      className={
        active
          ? "inline-flex h-7 items-center gap-1.5 rounded-md border border-primary/40 bg-primary/10 px-3 text-sm font-semibold text-primary shadow-sm"
          : "inline-flex h-7 items-center gap-1.5 rounded-md border border-transparent px-3 text-sm font-medium text-foreground/70 hover:bg-muted/40 hover:text-foreground"
      }
    >
      {Icon ? (
        <Icon
          className={
            active
              ? "size-3.5 shrink-0 text-primary"
              : "size-3.5 shrink-0 text-foreground/60"
          }
          data-testid={`${testid}-icon`}
        />
      ) : null}
      <span>{label}</span>
      {count ? (
        count.total > 0 && count.open === 0 ? (
          // All issues closed — swap the `N/N` badge for a green
          // tick so a glanceable "done" state pops on the strip.
          <span
            className="inline-flex items-center gap-1 rounded bg-emerald-100 px-1.5 text-[11px] tabular-nums text-emerald-700 dark:bg-emerald-950 dark:text-emerald-400"
            data-testid={`${testid}-count`}
            data-complete="true"
            title={`${count.total}/${count.total} closed`}
          >
            <CheckCircle2Icon className="size-3" />
            {count.total}/{count.total}
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
}

function ViewSettingsDialog({
  state,
  current,
  busy,
  onCancel,
  onSubmit,
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
        </div>

        <DialogFooter>
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
