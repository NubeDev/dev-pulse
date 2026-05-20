/**
 * `MultiSelect` — generic checkbox popover used by the leaderboard
 * filters for org / user / activity-type multi-selection.
 *
 * Built on Radix `Popover` (not `DropdownMenu`) so the menu stays
 * open between clicks without fighting the menu-item "select closes
 * the menu" default. Each row is a plain `<button>` with a manual
 * checkbox icon; clicking calls `onChange` directly.
 */

import { useMemo, useState } from "react";
import { Check, ChevronDown } from "lucide-react";
import { Popover as PopoverPrimitive } from "radix-ui";

import { cn } from "@/lib/utils";

export interface MultiSelectOption {
  value: string;
  label: string;
  /** Optional muted secondary text (e.g. GitHub login). */
  hint?: string;
}

export interface MultiSelectProps {
  options: ReadonlyArray<MultiSelectOption>;
  value: ReadonlyArray<string>;
  onChange: (next: string[]) => void;
  placeholder?: string;
  /** Override the summary label when at least one option is selected.
   *  Receives the resolved option list. */
  summary?: (selected: ReadonlyArray<MultiSelectOption>) => string;
  disabled?: boolean;
  className?: string;
  id?: string;
  "data-testid"?: string;
  /** Optional max-height for the scroll area (default `18rem`). */
  contentMaxHeight?: string;
}

export function MultiSelect({
  options,
  value,
  onChange,
  placeholder = "Select…",
  summary,
  disabled,
  className,
  id,
  contentMaxHeight = "18rem",
  ...rest
}: MultiSelectProps): JSX.Element {
  const [open, setOpen] = useState(false);
  const selectedSet = useMemo(() => new Set(value), [value]);
  const selectedOptions = useMemo(
    () => options.filter((o) => selectedSet.has(o.value)),
    [options, selectedSet],
  );

  function toggle(v: string): void {
    const next = selectedSet.has(v)
      ? value.filter((x) => x !== v)
      : [...value, v];
    onChange(next);
  }

  const label =
    selectedOptions.length === 0
      ? placeholder
      : summary
        ? summary(selectedOptions)
        : selectedOptions.length === 1
          ? (selectedOptions[0]?.label ?? placeholder)
          : `${selectedOptions.length} selected`;

  const allSelected =
    options.length > 0 && selectedOptions.length === options.length;

  return (
    <PopoverPrimitive.Root open={open} onOpenChange={setOpen}>
      <PopoverPrimitive.Trigger
        id={id}
        disabled={disabled}
        data-testid={rest["data-testid"]}
        aria-haspopup="listbox"
        className={cn(
          // shadcn outline-button styling, applied directly so radix
          // owns the trigger ref (Button doesn't forward refs, which
          // breaks popper measurement when used with `asChild`).
          "inline-flex h-9 w-full shrink-0 items-center justify-between gap-2 rounded-md border bg-background px-3 py-2 text-sm font-normal whitespace-nowrap shadow-xs outline-none transition-all",
          "hover:bg-accent hover:text-accent-foreground dark:border-input dark:bg-input/30 dark:hover:bg-input/50",
          "focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50",
          "disabled:pointer-events-none disabled:opacity-50",
          selectedOptions.length === 0 && "text-muted-foreground",
          className,
        )}
      >
        <span className="truncate">{label}</span>
        <ChevronDown className="size-4 shrink-0 opacity-50" aria-hidden />
      </PopoverPrimitive.Trigger>
      <PopoverPrimitive.Portal>
        <PopoverPrimitive.Content
          align="start"
          sideOffset={4}
          className={cn(
            "z-50 rounded-md border bg-popover p-1 text-popover-foreground shadow-md",
            "min-w-(--radix-popover-trigger-width) w-(--radix-popover-trigger-width)",
          )}
          style={{ minWidth: "14rem" }}
        >
          <div className="flex items-center justify-between gap-2 px-2 pb-1 pt-1">
            <span className="text-xs uppercase tracking-wider text-muted-foreground">
              {selectedOptions.length} / {options.length} selected
            </span>
            {options.length > 0 ? (
              <button
                type="button"
                className="rounded px-1.5 py-0.5 text-xs text-muted-foreground hover:text-foreground"
                onClick={() =>
                  onChange(allSelected ? [] : options.map((o) => o.value))
                }
              >
                {allSelected ? "Clear" : "All"}
              </button>
            ) : null}
          </div>
          <div className="-mx-1 my-1 h-px bg-border" />
          <div
            role="listbox"
            aria-multiselectable
            className="overflow-y-auto"
            style={{ maxHeight: contentMaxHeight }}
          >
            {options.length === 0 ? (
              <p className="px-2 py-3 text-xs text-muted-foreground">
                No options available.
              </p>
            ) : (
              options.map((o) => {
                const checked = selectedSet.has(o.value);
                return (
                  <button
                    key={o.value}
                    type="button"
                    role="option"
                    aria-selected={checked}
                    onClick={() => toggle(o.value)}
                    className={cn(
                      "flex w-full cursor-default items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm outline-none",
                      "hover:bg-accent hover:text-accent-foreground focus-visible:bg-accent focus-visible:text-accent-foreground",
                    )}
                  >
                    <span
                      aria-hidden
                      className={cn(
                        "flex size-4 shrink-0 items-center justify-center rounded-sm border",
                        checked
                          ? "border-primary bg-primary text-primary-foreground"
                          : "border-input",
                      )}
                    >
                      {checked ? <Check className="size-3" /> : null}
                    </span>
                    <span className="flex flex-1 items-center justify-between gap-2 truncate">
                      <span className="truncate">{o.label}</span>
                      {o.hint ? (
                        <span className="truncate text-xs text-muted-foreground">
                          {o.hint}
                        </span>
                      ) : null}
                    </span>
                  </button>
                );
              })
            )}
          </div>
        </PopoverPrimitive.Content>
      </PopoverPrimitive.Portal>
    </PopoverPrimitive.Root>
  );
}
