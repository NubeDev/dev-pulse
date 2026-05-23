/**
 * "Mark N/A" toggle for an exec-summary section.
 *
 * Pushes a wholesale replacement of `skipped_sections` through the
 * standard autosave hook so the change rides the same PATCH path as
 * any field edit. The server OR's the skip flag into completion, so
 * toggling immediately moves the % bar and unblocks `submit` for
 * projects where the section legitimately doesn't apply (e.g. a
 * firmware-only project skipping Hardware).
 */

import { CircleSlashIcon } from "lucide-react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

import type { ExecSummaryDto, ExecSummarySectionId } from "../../api/client.js";
import { useExecSummaryAutosave } from "./hooks/use-exec-summary.js";

export function SectionNaToggle({
  projectId,
  data,
  sectionId,
}: {
  projectId: string;
  data: ExecSummaryDto;
  sectionId: ExecSummarySectionId;
}): JSX.Element {
  const { patch, flush } = useExecSummaryAutosave(projectId);
  // Approval + Change log are state-machine surfaces — "N/A" on
  // them doesn't make sense. Hide rather than disable so the button
  // doesn't look broken.
  if (sectionId === "approval" || sectionId === "changelog") {
    return <span aria-hidden />;
  }

  const skipped = data.skipped_sections.includes(sectionId);
  const onToggle = (): void => {
    const next = skipped
      ? data.skipped_sections.filter((id) => id !== sectionId)
      : [...data.skipped_sections, sectionId];
    patch({ skipped_sections: next });
    // Flush immediately: this is a deliberate user action, not a
    // keystroke. The header completion bar should update without
    // waiting on the autosave debounce.
    flush();
  };

  return (
    <Button
      type="button"
      variant={skipped ? "default" : "outline"}
      size="sm"
      onClick={onToggle}
      title={
        skipped
          ? "Section marked N/A — click to mark applicable again."
          : "Mark this section as not applicable to this project."
      }
      className={cn(
        "h-7 gap-1.5 text-xs",
        skipped && "bg-slate-600 text-white hover:bg-slate-700",
      )}
      data-testid={`exec-summary-na-toggle-${sectionId}`}
    >
      <CircleSlashIcon className="h-3.5 w-3.5" />
      {skipped ? "Marked N/A" : "Mark N/A"}
    </Button>
  );
}
