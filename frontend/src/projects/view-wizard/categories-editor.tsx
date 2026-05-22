/**
 * `<CategoriesEditor>` — reusable list editor for a view's
 * `categories` array. Shared by `<NewViewWizard>` (creation) and
 * `<EditViewDialog>` (edit). Renders one row per category with
 * inline rename, a slug preview, a remove button, and an "Add"
 * input at the bottom that takes Enter.
 *
 * The editor works in DISPLAY-FORM strings (what the user typed)
 * so renames don't lose capitalisation locally. Callers slugify
 * via [`slugifyCategoryKey`] right before submit and pass the
 * slug list to the API.
 */

import { useState } from "react";
import { PlusIcon, XIcon } from "lucide-react";

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
}

export function CategoriesEditor({
  categories,
  onChange,
  helpText,
}: CategoriesEditorProps): JSX.Element {
  const [draft, setDraft] = useState("");

  const add = (): void => {
    const v = draft.trim();
    if (v.length === 0) return;
    const slug = slugifyCategoryKey(v);
    if (slug.length === 0) return;
    // Reject duplicates by slug — multiple display forms that
    // collapse to the same slug would create a single section
    // anyway, so the second add is a no-op.
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
      <Label>Categories</Label>
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

      <div className="flex flex-col gap-1.5">
        {categories.map((c, i) => {
          const slug = slugifyCategoryKey(c);
          return (
            <div
              key={i}
              className="flex items-center gap-2"
              data-testid={`project-view-category-row-${i}`}
            >
              <Input
                value={c}
                onChange={(e) => rename(i, e.target.value)}
                maxLength={50}
                className="flex-1"
                placeholder="Category name…"
                data-testid={`project-view-category-name-${i}`}
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
                onClick={() => remove(i)}
                aria-label={`Remove ${c || "category"}`}
                data-testid={`project-view-category-remove-${i}`}
                className="size-7"
              >
                <XIcon className="size-3.5" />
              </Button>
            </div>
          );
        })}
      </div>

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
