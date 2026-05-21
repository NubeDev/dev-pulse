// Shared label chip — renders a GitHub label string as a small
// coloured pill. Colour is looked up by name in a `tagColorByName`
// map (built from `TagDto[]` upstream); unknown labels fall back to
// a muted-border default so the chip is honest about "no tag row
// exists yet for this label" rather than guessing a colour.
//
// tagging.md §9.6 step 2: "Triage row renders up to 3 label chips
// between the title and the assignees, with `+N more` overflow.
// Colour is resolved from the org-scope tag of matching name
// (§7.1 — same chip the tag surface renders); labels without a
// corresponding tag row fall back to the muted-border default."
import type { JSX } from "react";

/** Semantic palette → Tailwind classes (bg + text + border).
 *  Kept in sync with the palette accepted by `TagDto.color` on
 *  `frontend/src/account/tags-page.tsx`. Background uses the
 *  `-100` swatch + `-800` text for legibility on both light and
 *  dark surfaces; dark-mode variants flip to deeper backgrounds. */
export const LABEL_PALETTE: Record<string, string> = {
  slate:
    "bg-slate-100 text-slate-800 border-slate-200 dark:bg-slate-800/60 dark:text-slate-100 dark:border-slate-700",
  indigo:
    "bg-indigo-100 text-indigo-800 border-indigo-200 dark:bg-indigo-900/40 dark:text-indigo-100 dark:border-indigo-800",
  blue:
    "bg-blue-100 text-blue-800 border-blue-200 dark:bg-blue-900/40 dark:text-blue-100 dark:border-blue-800",
  teal:
    "bg-teal-100 text-teal-800 border-teal-200 dark:bg-teal-900/40 dark:text-teal-100 dark:border-teal-800",
  green:
    "bg-green-100 text-green-800 border-green-200 dark:bg-green-900/40 dark:text-green-100 dark:border-green-800",
  amber:
    "bg-amber-100 text-amber-900 border-amber-200 dark:bg-amber-900/40 dark:text-amber-100 dark:border-amber-800",
  red:
    "bg-red-100 text-red-800 border-red-200 dark:bg-red-900/40 dark:text-red-100 dark:border-red-800",
  pink:
    "bg-pink-100 text-pink-800 border-pink-200 dark:bg-pink-900/40 dark:text-pink-100 dark:border-pink-800",
  purple:
    "bg-purple-100 text-purple-800 border-purple-200 dark:bg-purple-900/40 dark:text-purple-100 dark:border-purple-800",
};

/** Muted-border fallback for labels without a matching tag row.
 *  Honest signal that "we don't know the colour yet" — pull-side
 *  reconciler (§5.1) will populate `dp_tags` and the chip will
 *  pick up the real palette on the next render. */
const LABEL_FALLBACK =
  "bg-transparent text-muted-foreground border-border";

export interface LabelChipProps {
  /** Raw GitHub label name (case-preserved for display). */
  name: string;
  /** Semantic palette name (e.g. `"blue"`). When `undefined` or
   *  not present in [`LABEL_PALETTE`], the chip uses the muted
   *  fallback. */
  color?: string;
  /** When set, the chip renders as a `<button>` and invokes this
   *  handler on click. The wrapping `<button>` stops event
   *  propagation so the chip is independently clickable inside a
   *  row whose container also handles clicks (e.g. the triage row
   *  navigates to the issue on click). */
  onClick?: (name: string) => void;
  /** When `true`, render with a subtle "active filter" outline so
   *  the user sees which labels are currently scoping the list. */
  active?: boolean;
}

export function LabelChip({
  name,
  color,
  onClick,
  active,
}: LabelChipProps): JSX.Element {
  const cls = (color && LABEL_PALETTE[color]) ?? LABEL_FALLBACK;
  const ring = active ? "ring-1 ring-foreground/40" : "";
  const interactive = onClick ? "cursor-pointer hover:brightness-110" : "";
  const base = `inline-flex shrink-0 items-center rounded-full border px-1.5 py-0 text-[10px] font-medium leading-4 ${cls} ${ring} ${interactive}`.trim();
  if (onClick) {
    return (
      <button
        type="button"
        className={base}
        data-testid="label-chip"
        data-label={name}
        data-active={active ? "true" : undefined}
        title={active ? `Remove filter: ${name}` : `Filter by label: ${name}`}
        onClick={(e) => {
          e.stopPropagation();
          onClick(name);
        }}
      >
        {name}
      </button>
    );
  }
  return (
    <span
      className={base}
      data-testid="label-chip"
      data-label={name}
      data-active={active ? "true" : undefined}
      title={name}
    >
      {name}
    </span>
  );
}

export interface LabelChipListProps {
  /** Raw label name list, as it arrives from
   *  `IssueListItem.labels`. Case-preserved. */
  labels: ReadonlyArray<string>;
  /** Name → colour lookup. Keys are **lowercased** by the caller
   *  (matches the v1 normalisation rule in tagging.md §3). Misses
   *  fall through to the muted fallback. */
  colorByName: ReadonlyMap<string, string>;
  /** Max chips to render inline before collapsing the rest into a
   *  `+N more` pill. Defaults to 3 per tagging.md §9.6 step 2. */
  max?: number;
  /** Optional className applied to the wrapping flex container. */
  className?: string;
  /** Forwarded to each [`LabelChip`]. Lower-cased filter set used
   *  to mark chips as active. */
  activeLabels?: ReadonlySet<string>;
  /** Forwarded to each [`LabelChip`]. When provided, chips become
   *  clickable filter toggles. */
  onLabelClick?: (name: string) => void;
}

export function LabelChipList({
  labels,
  colorByName,
  max = 3,
  className,
  activeLabels,
  onLabelClick,
}: LabelChipListProps): JSX.Element | null {
  if (labels.length === 0) return null;
  const visible = labels.slice(0, max);
  const overflow = labels.length - visible.length;
  return (
    <span
      className={`inline-flex shrink-0 items-center gap-1 ${className ?? ""}`}
      data-testid="label-chip-list"
    >
      {visible.map((name) => (
        <LabelChip
          key={name}
          name={name}
          color={colorByName.get(name.toLowerCase())}
          onClick={onLabelClick}
          active={activeLabels?.has(name.toLowerCase())}
        />
      ))}
      {overflow > 0 && (
        <span
          className="inline-flex shrink-0 items-center rounded-full border border-border bg-transparent px-1.5 py-0 text-[10px] font-medium leading-4 text-muted-foreground"
          data-testid="label-chip-overflow"
          title={labels.slice(max).join(", ")}
        >
          +{overflow}
        </span>
      )}
    </span>
  );
}
