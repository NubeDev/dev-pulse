/**
 * `<CategoriesManagerDialog>` — popup launched from the workbench
 * toolbar's settings icon when the active view is categorised.
 *
 * Lets the user add / rename / reorder (drag-and-drop) / delete
 * categories on the active view. Each change PATCHes the view
 * immediately — there's no Save / Cancel buffer; closing the
 * dialog just closes.
 *
 * Tag side-effects:
 *   - Adding a category ensures the matching `category:<slug>`
 *     org tag exists (idempotent), so issues created from the
 *     new section get auto-tagged correctly.
 *   - Removing a category leaves the underlying tag intact —
 *     issues already tagged keep their category and fall into
 *     the trailing "Uncategorised" section in this view.
 *
 * The list editor is `<CategoriesEditor>` (the same component the
 * create wizard and edit dialog use); the only thing this file
 * owns is the popup chrome + live-PATCH wiring.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { CheckIcon, PlusIcon, Trash2Icon } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Spinner } from "@/components/ui/spinner";

import type {
  ProjectViewDto,
  ProjectViewWriteBody,
  TagDto,
} from "../api/client.js";
import { workflowKeys } from "../workflow/use-workflow-data.js";

import {
  CategoriesEditor,
  categoriesToSlugList,
} from "./view-wizard/categories-editor.js";
import {
  CATEGORISED_GROUP_BY,
  CATEGORY_CHIPS,
  CATEGORY_PACKS,
  ensureCategoryTags,
  slugifyCategoryKey,
} from "./view-wizard/index.js";

export interface CategoriesManagerDialogProps {
  open: boolean;
  /** The view whose categories are being managed. `null` while
   *  closed or before the list resolves. */
  view: ProjectViewDto | null;
  /** Org of the project — required so we can ensure org-scoped
   *  `category:<slug>` tags before each PATCH lands. */
  orgId: string;
  /** Cached tag list, so we can skip POSTs for slugs that already
   *  have a backing tag. */
  existingTags: readonly TagDto[] | null;
  busy?: boolean;
  onClose: () => void;
  /** Persists the new shape. Mirrors `<EditViewDialog>` — the
   *  callsite plumbs this through to `useUpdateProjectView`. */
  onSubmit: (viewId: string, body: ProjectViewWriteBody) => void;
}

export function CategoriesManagerDialog({
  open,
  view,
  orgId,
  existingTags,
  busy,
  onClose,
  onSubmit,
}: CategoriesManagerDialogProps): JSX.Element {
  const qc = useQueryClient();
  const [categories, setCategories] = useState<string[]>([]);
  const [ensuringTags, setEnsuringTags] = useState(false);
  const [pendingSave, setPendingSave] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Latest slug list we successfully sent to the server. Used to
  // suppress redundant PATCHes (e.g. a rename whose slug didn't
  // actually change after normalisation).
  const lastSentRef = useRef<string>("");
  // Pending debounce timer for the PATCH. Renames fire many
  // change events as the user types — we coalesce them into a
  // single PATCH after the user stops typing for 500ms.
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Latest snapshot we want to persist; the debounce callback
  // reads this off the ref so the closure always sees the
  // freshest list even if multiple changes queue up.
  const pendingRef = useRef<string[] | null>(null);

  // Seed the editor when the dialog opens or the user switches to
  // a different view. Critically, we DO NOT re-seed on every
  // `view` ref change — tanstack-query invalidates the views list
  // after each PATCH, which produces a new `view` object even
  // when the user is mid-edit. Re-seeding from that would clobber
  // in-progress keystrokes (e.g. typing "hardware" → "hardware1"
  // and the seed would snap the input back to "hardware" the
  // moment the first debounced PATCH lands). Keying on `view.id`
  // means we only resync when the user genuinely targets a new
  // view; live edits stay live until the dialog closes.
  const viewId = view?.id ?? null;
  useEffect(() => {
    if (!open || !view) return;
    setCategories([...view.categories]);
    setError(null);
    setEnsuringTags(false);
    setPendingSave(false);
    lastSentRef.current = view.categories.join("|");
    pendingRef.current = null;
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- see comment above; `view` ref change is INTENTIONALLY ignored
  }, [open, viewId]);

  // Cancel any in-flight debounce when the component unmounts so
  // a closing dialog doesn't fire a stale PATCH a beat later.
  useEffect(() => {
    return () => {
      if (timerRef.current !== null) clearTimeout(timerRef.current);
    };
  }, []);

  /** Persist the pending snapshot to the server. Skips when the
   *  slug list matches the last successful send. */
  const flush = useCallback(async (): Promise<void> => {
    if (!view) return;
    const next = pendingRef.current;
    pendingRef.current = null;
    if (next === null) return;
    const slugs = categoriesToSlugList(next);
    const key = slugs.join("|");
    if (key === lastSentRef.current) {
      setPendingSave(false);
      return;
    }

    const oldSlugs = new Set(view.categories);
    const newOnly = slugs.filter((s) => !oldSlugs.has(s));

    if (newOnly.length > 0) {
      setEnsuringTags(true);
      setError(null);
      try {
        await ensureCategoryTags(orgId, newOnly, existingTags ?? undefined);
        qc.invalidateQueries({ queryKey: workflowKeys.tags() });
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
        setEnsuringTags(false);
        setPendingSave(false);
        return;
      }
      setEnsuringTags(false);
    }

    lastSentRef.current = key;
    onSubmit(view.id, {
      name: view.name,
      group_by: slugs.length > 0 ? CATEGORISED_GROUP_BY : view.group_by,
      filter_clauses: view.filter_clauses,
      sort: view.sort,
      start_date: view.start_date ?? null,
      due_date: view.due_date ?? null,
      categories: slugs,
    });
    setPendingSave(false);
  }, [view, orgId, existingTags, qc, onSubmit]);

  /** Editor mutates `categories` for any change (add, remove,
   *  rename, reorder). UI reflects the change immediately; the
   *  PATCH is debounced so a burst of keystrokes coalesces into
   *  one network round-trip. */
  const handleChange = (next: string[]): void => {
    setCategories(next);
    pendingRef.current = next;
    setPendingSave(true);
    if (timerRef.current !== null) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => {
      timerRef.current = null;
      void flush();
    }, 500);
  };

  /** Slug index of the current list. Used by the chip grid to
   *  light up chips that are already present. Recomputed only
   *  when `categories` changes. */
  const presentSlugs = useMemo<Set<string>>(
    () => new Set(categories.map((c) => slugifyCategoryKey(c))),
    [categories],
  );

  /** Quick-add a pre-baked pack of categories (Engineering /
   *  Quality & ops / Launch). Merges into the existing list,
   *  dedupe by slug so re-clicking a pack is a no-op. */
  const applyPack = (pack: readonly string[]): void => {
    const seen = new Set(presentSlugs);
    const next = [...categories];
    for (const c of pack) {
      const slug = slugifyCategoryKey(c);
      if (slug.length === 0 || seen.has(slug)) continue;
      next.push(c);
      seen.add(slug);
    }
    if (next.length === categories.length) return;
    handleChange(next);
  };

  /** "Add all" — force the canonical CATEGORY_CHIPS order. Unlike
   *  `applyPack`, this REORDERS existing standard categories to
   *  match the chip list and appends any custom (non-standard)
   *  categories at the end so the user's bespoke entries aren't
   *  destroyed. Display strings already in the list (matched by
   *  slug) keep the user's casing / typing. */
  const applyAllInOrder = (): void => {
    const canonical = CATEGORY_CHIPS.map((c) => c.display);
    const canonicalSlugs = new Set(
      canonical.map((c) => slugifyCategoryKey(c)),
    );
    // Preserve user-typed display form for standard categories
    // they've already added (e.g. "hardware-renamed") by indexing
    // existing entries by slug.
    const existingByStandardSlug = new Map<string, string>();
    const customRows: string[] = [];
    for (const c of categories) {
      const slug = slugifyCategoryKey(c);
      if (canonicalSlugs.has(slug)) {
        existingByStandardSlug.set(slug, c);
      } else if (slug.length > 0) {
        customRows.push(c);
      }
    }
    const next: string[] = [];
    for (const display of canonical) {
      const slug = slugifyCategoryKey(display);
      next.push(existingByStandardSlug.get(slug) ?? display);
    }
    for (const c of customRows) next.push(c);
    // No-op if the resulting list is identical to current.
    if (
      next.length === categories.length &&
      next.every((c, i) => c === categories[i])
    ) {
      return;
    }
    handleChange(next);
  };

  /** "Delete all" — wipe every category off the view. Asks for
   *  confirmation because this is destructive (the section shape
   *  disappears) and not obvious from a stray click. The backing
   *  `category:<slug>` org tags survive — issues already tagged
   *  keep their tags. */
  const deleteAll = (): void => {
    if (categories.length === 0) return;
    // eslint-disable-next-line no-alert
    if (
      !window.confirm(
        `Remove all ${categories.length} categories from this view? Issues already tagged keep their tags — you can re-add categories at any time.`,
      )
    ) {
      return;
    }
    handleChange([]);
  };

  /** Toggle a single quick-add chip: if not present, append; if
   *  already present (by slug), remove that entry. Makes the chip
   *  row act like a multi-select picker. */
  const toggleChip = (display: string): void => {
    const slug = slugifyCategoryKey(display);
    if (slug.length === 0) return;
    if (presentSlugs.has(slug)) {
      handleChange(
        categories.filter((c) => slugifyCategoryKey(c) !== slug),
      );
    } else {
      handleChange([...categories, display]);
    }
  };

  /** "Done" forces a flush before closing so a pending debounce
   *  isn't dropped on dialog dismiss. */
  const handleClose = (): void => {
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
      void flush();
    }
    onClose();
  };

  return (
    <Dialog open={open} onOpenChange={(o) => (o ? null : handleClose())}>
      <DialogContent
        className="sm:max-w-xl"
        data-testid="categories-manager-dialog"
      >
        <DialogHeader>
          <DialogTitle>Manage categories</DialogTitle>
          <DialogDescription>
            Drag to reorder. Edit a name to rename. Removing a
            category keeps any existing tags — issues already
            tagged stay tagged and fall into the "Uncategorised"
            section.
          </DialogDescription>
        </DialogHeader>

        {view ? (
          <div className="flex flex-col gap-3 py-2">
            <div className="flex flex-col gap-3 rounded-md border border-dashed border-border p-3">
              <div className="flex flex-col gap-2">
                <div className="flex items-baseline justify-between gap-2">
                  <Label className="text-xs">Packs</Label>
                  <span className="text-[11px] text-muted-foreground">
                    Add a curated set in one click
                  </span>
                </div>
                <div
                  className="flex flex-wrap gap-1.5"
                  data-testid="project-view-category-packs"
                >
                  <Button
                    type="button"
                    size="sm"
                    variant="default"
                    onClick={applyAllInOrder}
                    data-testid="project-view-category-pack-all"
                    title={CATEGORY_CHIPS.map((c) => c.display).join(", ")}
                  >
                    <PlusIcon className="mr-1 size-3.5" />
                    Add all
                  </Button>
                  {CATEGORY_PACKS.map((pack) => (
                    <Button
                      key={pack.id}
                      type="button"
                      size="sm"
                      variant="outline"
                      onClick={() => applyPack(pack.categories)}
                      data-testid={`project-view-category-pack-${pack.id}`}
                      title={pack.categories.join(", ")}
                    >
                      <PlusIcon className="mr-1 size-3.5" />
                      {pack.label}
                    </Button>
                  ))}
                </div>
              </div>

              <div className="h-px bg-border" aria-hidden />

              <div className="flex flex-col gap-2">
                <div className="flex items-baseline justify-between gap-2">
                  <Label className="text-xs">Individual categories</Label>
                  <span className="text-[11px] text-muted-foreground">
                    Click to add — click again to remove
                  </span>
                </div>
                <div
                  className="flex flex-wrap gap-1.5"
                  data-testid="project-view-category-chips"
                >
                  {CATEGORY_CHIPS.map((chip) => {
                    const isPresent = presentSlugs.has(
                      slugifyCategoryKey(chip.display),
                    );
                    return (
                      <Button
                        key={chip.id}
                        type="button"
                        size="sm"
                        variant={isPresent ? "default" : "outline"}
                        onClick={() => toggleChip(chip.display)}
                        data-testid={`project-view-category-chip-${chip.id}`}
                        data-state={isPresent ? "on" : "off"}
                        aria-pressed={isPresent}
                      >
                        {isPresent ? (
                          <CheckIcon className="mr-1 size-3.5" />
                        ) : (
                          <PlusIcon className="mr-1 size-3.5" />
                        )}
                        {chip.display}
                      </Button>
                    );
                  })}
                </div>
              </div>
            </div>

            <CategoriesEditor
              categories={categories}
              onChange={handleChange}
              hideLabel
              helpText="Changes save automatically. Drag the handle on the left to reorder."
            />

            {(ensuringTags || busy || pendingSave) && (
              <p
                className="flex items-center gap-2 text-xs text-muted-foreground"
                data-testid="categories-manager-saving"
              >
                <Spinner />{" "}
                {ensuringTags || busy ? "Saving…" : "Unsaved changes…"}
              </p>
            )}

            {error && (
              <p
                className="rounded-md border border-destructive/50 bg-destructive/5 p-2 text-xs text-destructive"
                data-testid="categories-manager-error"
              >
                {error}
              </p>
            )}
          </div>
        ) : (
          <p className="py-6 text-center text-sm text-muted-foreground">
            No view selected.
          </p>
        )}

        <DialogFooter className="sm:justify-between">
          {view && categories.length > 0 ? (
            <Button
              type="button"
              variant="ghost"
              onClick={deleteAll}
              data-testid="categories-manager-delete-all"
              className="text-destructive hover:bg-destructive/10 hover:text-destructive"
            >
              <Trash2Icon className="mr-1 size-3.5" />
              Delete all
            </Button>
          ) : (
            <span />
          )}
          <Button
            type="button"
            onClick={handleClose}
            data-testid="categories-manager-close"
          >
            Done
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
