/**
 * `<DateDisplayPicker>` — the three-way segmented control that
 * chooses how a saved view's due-date badge renders on its tab
 * (`Hide` / `Week of month` / `Date (DD:Mon:YY)`).
 *
 * Extracted from `<EditViewDialog>` so the create wizard and the
 * edit dialog share one control. The mode itself is machine-local
 * (persisted via `writeDateDisplayMode`, keyed by view id) — this
 * component is purely presentational: it renders the current
 * `value`, reports changes through `onChange`, and shows an
 * optional live preview when a `dueDate` is supplied.
 */

import { Label } from "@/components/ui/label";

import { formatDateDisplay, type DateDisplayMode } from "./date-display.js";

const OPTIONS: Array<{ value: DateDisplayMode; label: string }> = [
  { value: "hide", label: "Hide" },
  { value: "week", label: "Week of month" },
  { value: "date", label: "Date (DD:Mon:YY)" },
];

export interface DateDisplayPickerProps {
  value: DateDisplayMode;
  onChange: (mode: DateDisplayMode) => void;
  /** When set, a live "Preview: …" line is shown beneath the
   *  toggle so the user sees the formatted badge before saving. */
  dueDate?: string | null;
  /** Parenthetical helper after the label. Defaults to the
   *  edit-dialog wording. */
  hint?: string;
}

export function DateDisplayPicker({
  value,
  onChange,
  dueDate,
  hint = "how the due date appears on this tab",
}: DateDisplayPickerProps): JSX.Element {
  return (
    <div className="flex flex-col gap-1.5">
      <Label>
        Date display
        <span className="ml-1 text-xs font-normal text-muted-foreground">
          ({hint})
        </span>
      </Label>
      <div
        role="radiogroup"
        aria-label="Date display"
        className="inline-flex w-fit overflow-hidden rounded-md border border-border"
      >
        {OPTIONS.map((opt, i) => {
          const selected = value === opt.value;
          return (
            <button
              key={opt.value}
              type="button"
              role="radio"
              aria-checked={selected}
              onClick={() => onChange(opt.value)}
              data-testid={`date-display-${opt.value}`}
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
            {formatDateDisplay(dueDate, value) ?? "(hidden)"}
          </span>
        </p>
      ) : null}
    </div>
  );
}
