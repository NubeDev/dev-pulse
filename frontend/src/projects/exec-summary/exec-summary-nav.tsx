import { CheckIcon } from "lucide-react";

import { cn } from "@/lib/utils";

import type { ExecSummarySectionId } from "../../api/client.js";
import { SECTIONS } from "./shared.js";

export function ExecSummaryNav({
  active,
  onSelect,
  completion,
  skipped,
}: {
  active: ExecSummarySectionId;
  onSelect: (id: ExecSummarySectionId) => void;
  completion: Record<string, boolean>;
  /** Section ids the user has marked N/A. Render the badge instead
   *  of a tick so a skipped section reads differently from one
   *  that's complete via content. */
  skipped: readonly string[];
}): JSX.Element {
  const skippedSet = new Set(skipped);
  return (
    <nav
      className="flex flex-row gap-1.5 overflow-x-auto lg:sticky lg:top-4 lg:flex-col lg:gap-1.5"
      aria-label="Executive summary sections"
    >
      {SECTIONS.map((s) => {
        const isActive = s.id === active;
        const isSkipped = skippedSet.has(s.id);
        // A skipped section is also marked complete by the server,
        // but the nav should signal the difference visually.
        const isComplete = !isSkipped && completion[s.id] === true;
        return (
          <button
            key={s.id}
            type="button"
            onClick={() => onSelect(s.id)}
            data-testid={`exec-summary-nav-${s.id}`}
            aria-current={isActive ? "page" : undefined}
            className={cn(
              "flex items-center gap-3 rounded-md border px-3 py-2 text-left text-sm transition-colors",
              "min-w-[140px] lg:min-w-0 lg:w-full",
              isActive
                ? "border-input bg-accent text-accent-foreground shadow-sm"
                : "border-border bg-background text-foreground hover:bg-accent/40",
            )}
          >
            <span
              className={cn(
                "flex h-6 shrink-0 items-center justify-center rounded-full text-[10px] font-semibold",
                isSkipped ? "w-9 px-1.5" : "w-6",
                isSkipped
                  ? "bg-slate-500 text-white"
                  : isComplete
                    ? "bg-emerald-500 text-white"
                    : isActive
                      ? "bg-foreground text-background"
                      : "bg-muted text-muted-foreground",
              )}
              aria-hidden
            >
              {isSkipped ? (
                "N/A"
              ) : isComplete ? (
                <CheckIcon className="h-3.5 w-3.5" />
              ) : (
                s.step
              )}
            </span>
            <span className="flex min-w-0 flex-col">
              <span className="truncate text-sm font-medium">{s.label}</span>
              <span className="hidden truncate text-[11px] text-muted-foreground lg:block">
                {s.description}
              </span>
            </span>
          </button>
        );
      })}
    </nav>
  );
}
