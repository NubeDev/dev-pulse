/**
 * `<CategoriesEditor>` — reusable list editor for a view's
 * `categories` array. Shared by:
 *   - `<NewViewWizard>` (creation)
 *   - `<EditViewDialog>` (edit)
 *   - `<CategoriesManagerDialog>` (live-PATCH manager from the
 *     workbench toolbar)
 *
 * Each row is sortable via dnd-kit; renders a drag handle, an
 * inline rename, the canonical slug preview, and a remove button.
 * The "Add" input at the bottom takes Enter.
 *
 * The editor works in DISPLAY-FORM strings (what the user typed)
 * so renames don't lose capitalisation locally. Callers slugify
 * via [`slugifyCategoryKey`] right before submit and pass the
 * slug list to the API.
 */

import { useState } from "react";
import { GripVerticalIcon, PlusIcon, XIcon } from "lucide-react";
import {
  DndContext,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import { restrictToVerticalAxis } from "@dnd-kit/modifiers";
import {
  SortableContext,
  arrayMove,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

import { CATEGORY_TAG_KEY, slugifyCategoryKey } from "./category-utils.js";

export interface CategoriesEditorProps {
  categories: string[];
  onChange: (next: string[]) => void;
  /** Optional override for the section heading — `<EditViewDialog>`
   *  uses "Categories" with a help line about renaming carrying
   *  existing issues over. */
  helpText?: string;
  /** Hide the "Categories" label (the manager dialog has its own
   *  header). */
  hideLabel?: boolean;
}

export function CategoriesEditor({
  categories,
  onChange,
  helpText,
  hideLabel,
}: CategoriesEditorProps): JSX.Element {
  const [draft, setDraft] = useState("");

  // Row id = array index. Two reasons we *don't* include the
  // value text (the previous `${i}:${value}` scheme):
  //   1. React would unmount and remount the row's <Input> on
  //      every keystroke (the key changed), so the input lost
  //      focus and the parent's state update fired against a
  //      remounted element — the visible symptom was "only the
  //      first character is kept" when renaming.
  //   2. dnd-kit needs the id stable within a drag session, not
  //      across reorders.
  // Tradeoff: after a reorder, the row at index N keeps the same
  // id as the row that used to be there. dnd-kit's exit animation
  // is therefore a position swap rather than a follow-the-item
  // tween. Acceptable.
  const itemIds = categories.map((_, i) => `row-${i}`);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );

  const handleDragEnd = (e: DragEndEvent): void => {
    const { active, over } = e;
    if (!over || active.id === over.id) return;
    const from = itemIds.indexOf(String(active.id));
    const to = itemIds.indexOf(String(over.id));
    if (from < 0 || to < 0) return;
    onChange(arrayMove(categories, from, to));
  };

  const add = (): void => {
    const v = draft.trim();
    if (v.length === 0) return;
    const slug = slugifyCategoryKey(v);
    if (slug.length === 0) return;
    if (categories.some((c) => slugifyCategoryKey(c) === slug)) {
      setDraft("");
      return;
    }
    onChange([...categories, v]);
    setDraft("");
  };

  const remove = (idx: number): void => {
    onChange(categories.filter((_, i) => i !== idx));
  };

  const rename = (idx: number, value: string): void => {
    onChange(categories.map((c, i) => (i === idx ? value : c)));
  };

  return (
    <div className="flex flex-col gap-2">
      {!hideLabel && <Label>Categories</Label>}
      <p className="text-xs text-muted-foreground">
        {helpText ??
          (<>
            Each category becomes a collapsible section inside the
            view. Names normalise to lowercase slugs (e.g.
            "Go-To-Market" → <code>go-to-market</code>) and the
            backing <code>{CATEGORY_TAG_KEY}:&lt;slug&gt;</code> tag is
            created on the project's org.
          </>)}
      </p>

      <DndContext
        sensors={sensors}
        collisionDetection={closestCenter}
        modifiers={[restrictToVerticalAxis]}
        onDragEnd={handleDragEnd}
      >
        <SortableContext items={itemIds} strategy={verticalListSortingStrategy}>
          <div className="flex flex-col gap-1.5">
            {categories.map((c, i) => (
              <SortableCategoryRow
                key={itemIds[i]!}
                id={itemIds[i]!}
                index={i}
                value={c}
                onRename={(v) => rename(i, v)}
                onRemove={() => remove(i)}
              />
            ))}
          </div>
        </SortableContext>
      </DndContext>

      <div className="flex items-center gap-2">
        <Input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              add();
            }
          }}
          maxLength={50}
          placeholder="Add a category and press Enter…"
          data-testid="project-view-category-draft"
        />
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={add}
          disabled={draft.trim().length === 0}
          data-testid="project-view-category-add"
        >
          <PlusIcon className="mr-1 size-3.5" /> Add
        </Button>
      </div>
    </div>
  );
}

interface SortableCategoryRowProps {
  id: string;
  index: number;
  value: string;
  onRename: (next: string) => void;
  onRemove: () => void;
}

function SortableCategoryRow({
  id,
  index,
  value,
  onRename,
  onRemove,
}: SortableCategoryRowProps): JSX.Element {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id });

  const slug = slugifyCategoryKey(value);

  return (
    <div
      ref={setNodeRef}
      style={{
        transform: CSS.Transform.toString(transform),
        transition,
        opacity: isDragging ? 0.4 : 1,
      }}
      className="flex items-center gap-2"
      data-testid={`project-view-category-row-${index}`}
    >
      <button
        type="button"
        className="flex size-7 shrink-0 cursor-grab items-center justify-center rounded text-muted-foreground hover:bg-accent/40 active:cursor-grabbing"
        aria-label={`Reorder ${value || "category"}`}
        data-testid={`project-view-category-drag-${index}`}
        {...attributes}
        {...listeners}
      >
        <GripVerticalIcon className="size-3.5" />
      </button>
      <Input
        value={value}
        onChange={(e) => onRename(e.target.value)}
        maxLength={50}
        className="flex-1"
        placeholder="Category name…"
        data-testid={`project-view-category-name-${index}`}
      />
      <span
        className="w-32 truncate font-mono text-[11px] text-muted-foreground"
        title={slug}
      >
        {slug || "—"}
      </span>
      <Button
        type="button"
        variant="ghost"
        size="icon"
        onClick={onRemove}
        aria-label={`Remove ${value || "category"}`}
        data-testid={`project-view-category-remove-${index}`}
        className="size-7"
      >
        <XIcon className="size-3.5" />
      </Button>
    </div>
  );
}

/** Project the user-typed display strings down to the canonical
 *  ordered slug list the API expects. Duplicates (same slug, any
 *  case) are deduped first-wins; empty rows are dropped. */
export function categoriesToSlugList(displays: readonly string[]): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const raw of displays) {
    const slug = slugifyCategoryKey(raw.trim());
    if (slug.length === 0 || seen.has(slug)) continue;
    seen.add(slug);
    out.push(slug);
  }
  return out;
}
