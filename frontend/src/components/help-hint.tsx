/**
 * `<HelpHint>` — small `?` button that opens a popover with a
 * short title + body explaining a UI section. Used as inline
 * documentation next to the project page's main sections so
 * newcomers can self-serve without leaving the page.
 *
 * Keep copy concise — the popover is 18rem wide. For long-form
 * material link out via the `learnMoreHref` (rendered as a
 * subtle footer link).
 *
 * Usage:
 *   <HelpHint
 *     title="Milestones"
 *     body={[
 *       "+ New milestone creates one on GitHub and mirrors it here.",
 *       "Use ⋯ to adopt as primary, edit, close, or delete.",
 *     ]}
 *   />
 */
import { HelpCircleIcon } from "lucide-react";

import { cn } from "@/lib/utils";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";

export interface HelpHintProps {
  /** Short headline shown bold at the top of the popover. */
  title: string;
  /** One or more paragraphs. Plain strings render as `<p>`; nodes
   *  render as-is so callers can embed `<code>` / `<strong>`. */
  body: ReadonlyArray<string | JSX.Element>;
  /** Optional CSS classes on the trigger button. */
  className?: string;
  /** Optional accessible label override. Defaults to the title. */
  ariaLabel?: string;
}

export function HelpHint({
  title,
  body,
  className,
  ariaLabel,
}: HelpHintProps): JSX.Element {
  return (
    <Popover>
      <PopoverTrigger
        type="button"
        aria-label={ariaLabel ?? `Help: ${title}`}
        className={cn(
          "inline-flex h-5 w-5 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
          className,
        )}
        data-testid={`help-hint-${slugify(title)}`}
      >
        <HelpCircleIcon className="h-4 w-4" />
      </PopoverTrigger>
      <PopoverContent
        align="start"
        className="text-sm leading-relaxed"
        data-testid={`help-hint-content-${slugify(title)}`}
      >
        <div className="mb-2 font-semibold">{title}</div>
        <div className="flex flex-col gap-2 text-muted-foreground">
          {body.map((para, i) =>
            typeof para === "string" ? <p key={i}>{para}</p> : (
              <div key={i}>{para}</div>
            ),
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}

function slugify(s: string): string {
  return s
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}
