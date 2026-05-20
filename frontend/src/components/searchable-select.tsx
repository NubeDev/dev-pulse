/**
 * `SearchableSelect` — single-value picker with built-in search,
 * sharing the visual vocabulary of `MultiSelect`. Built on Radix
 * `Popover` for the same reasons (predictable measurement, no
 * fight with the menu-item close-on-select default).
 */

import { useMemo, useState } from "react";
import { Check, ChevronDown, Search } from "lucide-react";
import { Popover as PopoverPrimitive } from "radix-ui";

import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

export interface SearchableSelectOption {
  value: string;
  label: string;
  /** Optional muted secondary text (e.g. GitHub login or email). */
  hint?: string;
}

export interface SearchableSelectProps {
  options: ReadonlyArray<SearchableSelectOption>;
  value: string | null;
  onChange: (next: string) => void;
  placeholder?: string;
  disabled?: boolean;
  className?: string;
  id?: string;
  "data-testid"?: string;
  contentMaxHeight?: string;
  /** Placeholder for the embedded search input. */
  searchPlaceholder?: string;
}

export function SearchableSelect({
  options,
  value,
  onChange,
  placeholder = "Select…",
  disabled,
  className,
  id,
  contentMaxHeight = "18rem",
  searchPlaceholder = "Search…",
  ...rest
}: SearchableSelectProps): JSX.Element {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");

  const selected = useMemo(
    () => options.find((o) => o.value === value) ?? null,
    [options, value],
  );

  const visibleOptions = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return options;
    return options.filter(
      (o) =>
        o.label.toLowerCase().includes(q) ||
        (o.hint?.toLowerCase().includes(q) ?? false),
    );
  }, [options, query]);

  return (
    <PopoverPrimitive.Root
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (!o) setQuery("");
      }}
    >
      <PopoverPrimitive.Trigger
        id={id}
        disabled={disabled}
        data-testid={rest["data-testid"]}
        aria-haspopup="listbox"
        className={cn(
          "inline-flex h-9 w-full shrink-0 items-center justify-between gap-2 rounded-md border bg-background px-3 py-2 text-sm font-normal whitespace-nowrap shadow-xs outline-none transition-all",
          "hover:bg-accent hover:text-accent-foreground dark:border-input dark:bg-input/30 dark:hover:bg-input/50",
          "focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50",
          "disabled:pointer-events-none disabled:opacity-50",
          !selected && "text-muted-foreground",
          className,
        )}
      >
        <span className="truncate">
          {selected ? (
            <>
              {selected.label}
              {selected.hint ? (
                <span className="ml-1 text-muted-foreground">
                  · {selected.hint}
                </span>
              ) : null}
            </>
          ) : (
            placeholder
          )}
        </span>
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
          <div className="relative px-1 pb-1 pt-1">
            <Search
              className="pointer-events-none absolute left-3 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground"
              aria-hidden
            />
            <Input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={searchPlaceholder}
              className="h-8 pl-7 text-xs"
              aria-label={searchPlaceholder}
            />
          </div>
          <div className="-mx-1 my-1 h-px bg-border" />
          <div
            role="listbox"
            className="overflow-y-auto"
            style={{ maxHeight: contentMaxHeight }}
          >
            {options.length === 0 ? (
              <p className="px-2 py-3 text-xs text-muted-foreground">
                No options available.
              </p>
            ) : visibleOptions.length === 0 ? (
              <p className="px-2 py-3 text-xs text-muted-foreground">
                No matches for &ldquo;{query}&rdquo;.
              </p>
            ) : (
              visibleOptions.map((o) => {
                const checked = o.value === value;
                return (
                  <button
                    key={o.value}
                    type="button"
                    role="option"
                    aria-selected={checked}
                    onClick={() => {
                      onChange(o.value);
                      setOpen(false);
                    }}
                    className={cn(
                      "flex w-full cursor-default items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm outline-none",
                      "hover:bg-accent hover:text-accent-foreground focus-visible:bg-accent focus-visible:text-accent-foreground",
                    )}
                  >
                    <span className="flex flex-1 items-center justify-between gap-2 truncate">
                      <span className="truncate">{o.label}</span>
                      {o.hint ? (
                        <span className="truncate text-xs text-muted-foreground">
                          {o.hint}
                        </span>
                      ) : null}
                    </span>
                    <Check
                      className={cn(
                        "size-3.5 shrink-0",
                        checked ? "opacity-100" : "opacity-0",
                      )}
                      aria-hidden
                    />
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
