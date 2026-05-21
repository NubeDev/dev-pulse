/**
 * `<FilterChipBar>` — the §5.2 / §5.4 AND-combined filter
 * surface that lives in the [`ProjectWorkbench`] toolbar.
 *
 * v1 scope (PROJECT-VIEW.md Slice 3, §5.2):
 *
 *   * Chips render as `[dim:value ×]`.
 *   * `+ Add` opens a two-step picker — pick a dim, then either
 *     pick the value from a list (status) or type it (assignee,
 *     label, tag). Tag dims surface every kv key the project's
 *     `group-by-options` endpoint reported.
 *   * Wire form is the same `;`-separated chip string the
 *     `?filter=` URL param uses (§5.4 — `;` is unsafe-free for
 *     tag values and UUIDs).
 *
 * Deliberately *not* a true typeahead this slice: we don't have a
 * per-project value-suggestion endpoint yet, and the tag-key /
 * label / assignee sets the user usually wants come from labels
 * already visible in the issues list, not a separate index. The
 * design (§5.2) reserves room for typeahead values; this v1
 * just keeps the parser stable so a future typeahead can swap in
 * without a wire change.
 */

import { useMemo, useState } from "react";
import { PlusIcon, XIcon } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";

import type { GroupByOption } from "../api/client.js";

/** Closed-vocabulary dims this slice exposes. Mirrors the server
 *  parser in `crates/dp-rest/src/project_issues.rs::parse_filter`. */
export type FilterDim = "status" | "assignee" | "label" | "tag" | "milestone";

/** A single parsed chip. `tag` chips carry both `key` and `value`.
 *  `milestone` chips carry the `dp_milestones.id` UUID in `value`. */
export interface FilterChip {
  dim: FilterDim;
  /** Only set when `dim === "tag"`. */
  key?: string;
  value: string;
}

/** Parse the wire `?filter=` string into chip objects. Mirrors the
 *  server parser bit-for-bit so the same string round-trips. */
export function parseFilterString(raw: string | null): FilterChip[] {
  if (!raw) return [];
  const out: FilterChip[] = [];
  for (const chunk of raw.split(";")) {
    const trimmed = chunk.trim();
    if (!trimmed) continue;
    const colon = trimmed.indexOf(":");
    if (colon < 0) continue;
    const dim = trimmed.slice(0, colon).trim();
    const value = trimmed.slice(colon + 1).trim();
    if (!value) continue;
    if (dim === "status" && (value === "open" || value === "closed")) {
      out.push({ dim: "status", value });
    } else if (dim === "assignee") {
      out.push({ dim: "assignee", value });
    } else if (dim === "label") {
      out.push({ dim: "label", value });
    } else if (dim === "milestone") {
      // Loose shape check only — the server is the authority on
      // UUID validity (it returns 400 with `invalid_filter`).
      out.push({ dim: "milestone", value });
    } else if (dim === "tag") {
      const inner = value.indexOf(":");
      if (inner < 0) continue;
      const key = value.slice(0, inner).trim();
      const v = value.slice(inner + 1).trim();
      if (!key || !v) continue;
      out.push({ dim: "tag", key, value: v });
    }
    // Unknown dims silently dropped on the client — the URL might
    // be from a future build. The server is still authoritative.
  }
  return out;
}

/** Inverse of [`parseFilterString`]. */
export function serializeFilterChips(chips: FilterChip[]): string {
  return chips
    .map((c) =>
      c.dim === "tag"
        ? `tag:${c.key}:${c.value}`
        : `${c.dim}:${c.value}`,
    )
    .join(";");
}

/** Stable identity for React `key` / chip equality. */
function chipId(c: FilterChip): string {
  return c.dim === "tag" ? `tag:${c.key}:${c.value}` : `${c.dim}:${c.value}`;
}

export interface FilterChipBarProps {
  /** Current chip list (already parsed). */
  chips: FilterChip[];
  /** Dim catalogue from `GET /projects/{id}/group-by-options`. We
   *  use the `tag:<key>` entries to populate the tag-key picker. */
  groupOptions: GroupByOption[];
  /** Adopted-milestone catalogue. The Add menu seeds the
   *  Milestone… submenu from this, and chip labels resolve
   *  `milestone:<uuid>` against it. Omit when the surface has no
   *  milestone data — the dim then renders the raw UUID and the
   *  Add submenu is suppressed. */
  milestoneOptions?: { id: string; title: string }[];
  /** Receives the new chip list after add / remove. The caller
   *  serialises and writes to the URL hash. */
  onChange: (next: FilterChip[]) => void;
}

export function FilterChipBar({
  chips,
  groupOptions,
  milestoneOptions,
  onChange,
}: FilterChipBarProps): JSX.Element {
  const tagKeys = useMemo(
    () =>
      groupOptions
        .filter((o) => o.id.startsWith("tag:"))
        .map((o) => o.id.slice("tag:".length)),
    [groupOptions],
  );
  const milestoneTitleById = useMemo(() => {
    const m = new Map<string, string>();
    for (const opt of milestoneOptions ?? []) m.set(opt.id, opt.title);
    return m;
  }, [milestoneOptions]);

  const removeChip = (target: FilterChip): void => {
    const targetId = chipId(target);
    onChange(chips.filter((c) => chipId(c) !== targetId));
  };

  const addChip = (chip: FilterChip): void => {
    const id = chipId(chip);
    if (chips.some((c) => chipId(c) === id)) return;
    onChange([...chips, chip]);
  };

  return (
    <div
      className="flex flex-wrap items-center gap-1.5"
      data-testid="project-filter-chips"
    >
      <span className="text-muted-foreground">Filter:</span>
      {chips.map((c) => (
        <Badge
          key={chipId(c)}
          variant="secondary"
          className="gap-1 pl-2 pr-1 font-mono text-[11px]"
          data-testid="project-filter-chip"
        >
          <span>
            {c.dim === "tag"
              ? `${c.key}:${c.value}`
              : c.dim === "milestone"
                ? `milestone:${milestoneTitleById.get(c.value) ?? c.value}`
                : `${c.dim}:${c.value}`}
          </span>
          <button
            type="button"
            onClick={() => removeChip(c)}
            className="rounded-sm p-0.5 hover:bg-accent"
            aria-label={`Remove ${chipId(c)} filter`}
          >
            <XIcon className="h-3 w-3" />
          </button>
        </Badge>
      ))}
      <AddChipMenu
        tagKeys={tagKeys}
        milestoneOptions={milestoneOptions ?? []}
        onAdd={addChip}
      />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Add-chip surface — two-step: dim picker → value picker / input.
// ---------------------------------------------------------------------------

type AddStep =
  | { kind: "idle" }
  | { kind: "value"; dim: "assignee" | "label" }
  | { kind: "tag-key" }
  | { kind: "tag-value"; key: string }
  | { kind: "milestone" };

function AddChipMenu({
  tagKeys,
  milestoneOptions,
  onAdd,
}: {
  tagKeys: string[];
  milestoneOptions: { id: string; title: string }[];
  onAdd: (chip: FilterChip) => void;
}): JSX.Element {
  const [menuOpen, setMenuOpen] = useState(false);
  const [step, setStep] = useState<AddStep>({ kind: "idle" });
  const [draft, setDraft] = useState("");

  const reset = (): void => {
    setStep({ kind: "idle" });
    setDraft("");
  };

  const close = (): void => {
    setMenuOpen(false);
    reset();
  };

  const submitDraft = (): void => {
    const value = draft.trim();
    if (!value) return;
    if (step.kind === "value") {
      onAdd({ dim: step.dim, value });
    } else if (step.kind === "tag-value") {
      onAdd({ dim: "tag", key: step.key, value });
    }
    close();
  };

  return (
    <DropdownMenu
      open={menuOpen}
      onOpenChange={(o) => {
        setMenuOpen(o);
        if (!o) reset();
      }}
    >
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="sm"
          className="h-6 gap-1 px-2 text-xs"
          data-testid="project-filter-add"
        >
          <PlusIcon className="h-3 w-3" /> Add
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-56">
        {step.kind === "idle" && (
          <>
            <DropdownMenuLabel>Filter by</DropdownMenuLabel>
            <DropdownMenuItem
              onSelect={(e) => {
                e.preventDefault();
                onAdd({ dim: "status", value: "open" });
                close();
              }}
            >
              Status · Open
            </DropdownMenuItem>
            <DropdownMenuItem
              onSelect={(e) => {
                e.preventDefault();
                onAdd({ dim: "status", value: "closed" });
                close();
              }}
            >
              Status · Closed
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              onSelect={(e) => {
                e.preventDefault();
                setStep({ kind: "value", dim: "assignee" });
              }}
            >
              Assignee…
            </DropdownMenuItem>
            <DropdownMenuItem
              onSelect={(e) => {
                e.preventDefault();
                setStep({ kind: "value", dim: "label" });
              }}
            >
              Label…
            </DropdownMenuItem>
            {tagKeys.length > 0 && (
              <DropdownMenuItem
                onSelect={(e) => {
                  e.preventDefault();
                  setStep({ kind: "tag-key" });
                }}
              >
                Tag…
              </DropdownMenuItem>
            )}
            {milestoneOptions.length > 0 && (
              <DropdownMenuItem
                onSelect={(e) => {
                  e.preventDefault();
                  setStep({ kind: "milestone" });
                }}
                data-testid="project-filter-add-milestone"
              >
                Milestone…
              </DropdownMenuItem>
            )}
          </>
        )}

        {step.kind === "milestone" && (
          <>
            <DropdownMenuLabel>Milestone</DropdownMenuLabel>
            {milestoneOptions.map((m) => (
              <DropdownMenuItem
                key={m.id}
                onSelect={(e) => {
                  e.preventDefault();
                  onAdd({ dim: "milestone", value: m.id });
                  close();
                }}
                data-testid={`project-filter-milestone-option-${m.id}`}
              >
                {m.title}
              </DropdownMenuItem>
            ))}
          </>
        )}

        {step.kind === "tag-key" && (
          <>
            <DropdownMenuLabel>Tag key</DropdownMenuLabel>
            {tagKeys.map((k) => (
              <DropdownMenuItem
                key={k}
                onSelect={(e) => {
                  e.preventDefault();
                  setStep({ kind: "tag-value", key: k });
                }}
              >
                {k}
              </DropdownMenuItem>
            ))}
          </>
        )}

        {(step.kind === "value" || step.kind === "tag-value") && (
          <div className="flex flex-col gap-2 p-2">
            <span className="text-xs font-medium text-muted-foreground">
              {step.kind === "tag-value"
                ? `Tag · ${step.key} value`
                : step.dim === "assignee"
                  ? "Assignee login"
                  : "Label"}
            </span>
            <Input
              autoFocus
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  submitDraft();
                }
              }}
              className="h-7 text-xs"
              data-testid="project-filter-value-input"
            />
            <div className="flex items-center justify-end gap-2">
              <Button
                variant="ghost"
                size="sm"
                className="h-6 px-2 text-xs"
                onClick={close}
              >
                Cancel
              </Button>
              <Button
                size="sm"
                className="h-6 px-2 text-xs"
                disabled={!draft.trim()}
                onClick={submitDraft}
              >
                Add
              </Button>
            </div>
          </div>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
