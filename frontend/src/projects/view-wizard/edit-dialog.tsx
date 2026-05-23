/**
 * `<EditViewDialog>` — single-page edit dialog for a saved view.
 *
 * Categories: a view has zero or more category slugs in
 * `ProjectViewDto.categories`. The dialog ALWAYS surfaces the
 * editor so the user can promote a flat view into a categorised
 * one (or vice versa) without going back through the create
 * wizard. On save we:
 *
 *   * Slugify the editor's display strings, dedupe, and send the
 *     resulting `categories` array on the PATCH.
 *   * Ensure each NEW slug has an org-scoped `category:<slug>`
 *     tag before the PATCH lands, so the auto-tag flow in
 *     `<AddIssuesDialog>` can immediately link new issues.
 *   * Force `group_by = "tag:category"` whenever `categories` is
 *     non-empty (the workbench enforces this too, but doing it
 *     in the body keeps the persisted row self-describing).
 *
 * Removing a slug from the list does NOT remove the underlying
 * tag — issues already tagged keep their category, they just no
 * longer appear in this view's curated section list (they fall
 * into the trailing "Uncategorised" group).
 */

import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";

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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

import { iconForName } from "../icon-for-name.js";
import { workflowKeys } from "../../workflow/use-workflow-data.js";

import type { ProjectViewDto, ProjectViewWriteBody, TagDto } from "../../api/client.js";

import { ensureCategoryTags } from "./category-utils.js";
import {
  CategoriesEditor,
  categoriesToSlugList,
} from "./categories-editor.js";
import { CATEGORISED_GROUP_BY } from "./templates.js";
import {
  formatDateDisplay,
  weekOfMonthLabel,
  type DateDisplayMode,
} from "./date-display.js";

export interface EditViewDialogProps {
  open: boolean;
  view: ProjectViewDto | null;
  /** Org of the project — required so we can ensure org-scoped
   *  tags for any category slugs added by the user. */
  orgId: string;
  /** Cached tag list, so we can skip POSTs for slugs that
   *  already have a backing org-scoped tag. `null` while the
   *  parent's `useTags()` is loading — the helper falls back to
   *  a fresh `listTags` in that case. */
  existingTags: readonly TagDto[] | null;
  busy?: boolean;
  onCancel: () => void;
  onSubmit: (viewId: string, body: ProjectViewWriteBody) => void;
  onDelete: (viewId: string) => void;
  dateDisplay?: DateDisplayMode;
  onChangeDateDisplay?: (mode: DateDisplayMode) => void;
  completed?: boolean;
  onChangeCompleted?: (completed: boolean) => void;
}

export function EditViewDialog({
  open,
  view,
  orgId,
  existingTags,
  busy,
  onCancel,
  onSubmit,
  onDelete,
  dateDisplay,
  onChangeDateDisplay,
  completed,
  onChangeCompleted,
}: EditViewDialogProps): JSX.Element {
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [startDate, setStartDate] = useState("");
  const [dueDate, setDueDate] = useState("");
  const [categories, setCategories] = useState<string[]>([]);
  const [ensuringTags, setEnsuringTags] = useState(false);
  const [ensureError, setEnsureError] = useState<string | null>(null);

  useEffect(() => {
    if (view && open) {
      setName(view.name);
      setStartDate(view.start_date ?? "");
      setDueDate(view.due_date ?? "");
      // Persisted categories are already slugs; keep them as the
      // editor's display values so renames stay reversible.
      setCategories([...view.categories]);
      setEnsureError(null);
      setEnsuringTags(false);
    }
  }, [view, open]);

  const trimmed = name.trim();
  const canSubmit =
    trimmed.length > 0 && trimmed.length <= 60 && !busy && !ensuringTags;
  const PreviewIcon = iconForName(trimmed || "view");

  const submit = async (): Promise<void> => {
    if (!view || !canSubmit) return;
    const slugs = categoriesToSlugList(categories);
    const isCategorised = slugs.length > 0;
    const oldSlugs = new Set(view.categories);
    const newOnly = slugs.filter((s) => !oldSlugs.has(s));

    if (isCategorised && newOnly.length > 0) {
      setEnsuringTags(true);
      setEnsureError(null);
      try {
        await ensureCategoryTags(orgId, newOnly, existingTags ?? undefined);
        // See `wizard-dialog.tsx` for the full story: invalidate
        // the shared tags cache so the workbench picks up the
        // freshly-created `category:<slug>` tag and the add-issue
        // dialog's auto-tag wiring can resolve a non-null id.
        qc.invalidateQueries({ queryKey: workflowKeys.tags() });
      } catch (err) {
        setEnsureError(err instanceof Error ? err.message : String(err));
        setEnsuringTags(false);
        return;
      }
      setEnsuringTags(false);
    }

    onSubmit(view.id, {
      name: trimmed,
      // Categorised views always group by `tag:category`; flat
      // views keep whatever group_by they had.
      group_by: isCategorised ? CATEGORISED_GROUP_BY : view.group_by,
      filter_clauses: view.filter_clauses,
      sort: view.sort,
      start_date: startDate || null,
      due_date: dueDate || null,
      categories: slugs,
    });
  };

  return (
    <Dialog open={open} onOpenChange={(o) => (o ? null : onCancel())}>
      <DialogContent
        className="sm:max-w-xl"
        data-testid="project-view-edit-dialog"
      >
        <DialogHeader>
          <DialogTitle>Edit view</DialogTitle>
          <DialogDescription>
            Rename, adjust the timeline, or manage the categorised
            sections rendered inside this view. Removing a category
            keeps any existing issue tags — they fall into the
            trailing "Uncategorised" section.
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-4 py-2">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="project-view-edit-name">Name</Label>
            <div className="flex items-center gap-2">
              <div
                className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-border bg-muted/40"
                title="Auto-picked from the name"
              >
                <PreviewIcon className="size-4 text-muted-foreground" />
              </div>
              <Input
                id="project-view-edit-name"
                autoFocus
                value={name}
                onChange={(e) => setName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    void submit();
                  }
                }}
                placeholder="View name…"
                maxLength={60}
                data-testid="project-view-edit-name-input"
              />
            </div>
          </div>

          <CategoriesEditor
            categories={categories}
            onChange={setCategories}
            helpText="Each category becomes a collapsible section inside this view. Issues created from a section are auto-tagged with the matching `category:<slug>` tag. Removing a category leaves the tag intact — existing issues keep it."
          />

          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="project-view-edit-start-date">Start date</Label>
              <DateInput
                id="project-view-edit-start-date"
                value={startDate}
                onChange={(e) => setStartDate(e.target.value)}
                data-testid="project-view-edit-start-date"
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="project-view-edit-due-date">Due date</Label>
              <DateInput
                id="project-view-edit-due-date"
                value={dueDate}
                onChange={(e) => setDueDate(e.target.value)}
                data-testid="project-view-edit-due-date"
              />
              {dueDate ? (
                <p className="text-xs text-muted-foreground">
                  {weekOfMonthLabel(dueDate)}
                </p>
              ) : null}
            </div>
          </div>

          {onChangeCompleted ? (
            <div className="flex items-start gap-2 rounded-md border border-border bg-muted/30 p-3">
              <Checkbox
                id="project-view-edit-completed"
                checked={completed ?? false}
                onCheckedChange={(v) => onChangeCompleted(v === true)}
                data-testid="project-view-edit-completed"
                className="mt-0.5"
              />
              <div className="flex flex-col gap-0.5">
                <Label
                  htmlFor="project-view-edit-completed"
                  className="cursor-pointer text-sm font-medium"
                >
                  Mark this view as completed
                </Label>
                <p className="text-xs text-muted-foreground">
                  Forces the green tick on the tab regardless of the
                  open / total issue count.
                </p>
              </div>
            </div>
          ) : null}

          {onChangeDateDisplay ? (
            <div className="flex flex-col gap-1.5">
              <Label>
                Date display
                <span className="ml-1 text-xs font-normal text-muted-foreground">
                  (how the due date appears on this tab)
                </span>
              </Label>
              <div
                role="radiogroup"
                aria-label="Date display"
                className="inline-flex w-fit overflow-hidden rounded-md border border-border"
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
                    >
                      {opt.label}
                    </button>
                  );
                })}
              </div>
              {dueDate ? (
                <p className="text-xs text-muted-foreground">
                  Preview:{" "}
                  <span className="font-medium text-foreground">
                    {formatDateDisplay(dueDate, dateDisplay ?? "week") ??
                      "(hidden)"}
                  </span>
                </p>
              ) : null}
            </div>
          ) : null}

          {ensureError && (
            <p
              className="rounded-md border border-destructive/50 bg-destructive/5 p-2 text-xs text-destructive"
              data-testid="project-view-edit-ensure-error"
            >
              Couldn't create category tags: {ensureError}
            </p>
          )}
        </div>

        <DialogFooter className="sm:justify-between">
          {view && (
            <Button
              variant="ghost"
              onClick={() => {
                // eslint-disable-next-line no-alert
                if (
                  window.confirm(
                    `Delete view "${view.name}"? This can't be undone. Category tags on issues are kept.`,
                  )
                ) {
                  onDelete(view.id);
                }
              }}
              disabled={busy}
              className="text-destructive hover:bg-destructive/10 hover:text-destructive"
              data-testid={`project-view-edit-delete-${view.id}`}
            >
              Delete view
            </Button>
          )}
          <div className="flex items-center gap-2">
            <Button variant="ghost" onClick={onCancel} disabled={busy}>
              Cancel
            </Button>
            <Button
              onClick={() => {
                void submit();
              }}
              disabled={!canSubmit}
              data-testid="project-view-edit-submit"
            >
              {ensuringTags ? "Creating tags…" : "Save"}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
