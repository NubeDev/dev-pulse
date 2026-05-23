/**
 * Cross-cutting helpers used by the exec-summary surface — status
 * badge styling, section metadata, completion-derived nav state,
 * and the permissions shape passed down from the host page.
 */

import { createContext, useContext } from "react";

import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

import type { ExecSummaryStatus, ExecSummarySectionId } from "../../api/client.js";

export interface ExecSummaryPermissions {
  /** Can edit any section. */
  canEdit: boolean;
  /** Can transition `draft → in_review`. */
  canSubmit: boolean;
  /** Can transition `in_review → approved`. Project lead only. */
  canApprove: boolean;
  /** Can transition any → `draft`. Project lead only. */
  canRevert: boolean;
}

export interface SectionMeta {
  id: ExecSummarySectionId;
  /** 1-based step number rendered when the section is incomplete. */
  step: number;
  label: string;
  description: string;
}

export const SECTIONS: readonly SectionMeta[] = [
  {
    id: "summary",
    step: 1,
    label: "Summary",
    description: "Product identifiers, objective, problem, value, criteria.",
  },
  {
    id: "scope",
    step: 2,
    label: "Scope",
    description: "What's in, what's out, assumptions, dependencies.",
  },
  {
    id: "requirements",
    step: 3,
    label: "Requirements",
    description: "Functional + non-functional requirements and protocols.",
  },
  {
    id: "hardware",
    step: 4,
    label: "Hardware",
    description: "Features, physical shape, mounting, environment.",
  },
  {
    id: "commercial",
    step: 5,
    label: "Commercial",
    description: "Pricing, margin, channel, target market, volume.",
  },
  {
    id: "documents",
    step: 6,
    label: "Documents",
    description: "Briefs, BOMs, datasheets and other supporting files.",
  },
  {
    id: "approval",
    step: 7,
    label: "Approval",
    description: "Reviewer, approver, state machine, timestamps.",
  },
  {
    id: "changelog",
    step: 8,
    label: "Change log",
    description: "Append-only history of revisions.",
  },
];

const STATUS_LABEL: Record<ExecSummaryStatus, string> = {
  draft: "Draft",
  in_review: "In review",
  approved: "Approved",
};

const STATUS_CLASS: Record<ExecSummaryStatus, string> = {
  draft:
    "border-slate-300 bg-slate-100 text-slate-700 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200",
  in_review:
    "border-amber-300 bg-amber-50 text-amber-800 dark:border-amber-800/60 dark:bg-amber-950/40 dark:text-amber-200",
  approved:
    "border-emerald-300 bg-emerald-50 text-emerald-800 dark:border-emerald-800/60 dark:bg-emerald-950/40 dark:text-emerald-200",
};

/**
 * Image uploader shared with every embedded markdown editor on
 * this surface. The page sets it once via
 * `<ExecSummaryImageUploaderContext.Provider>`; `MarkdownField`
 * reads it through `useExecSummaryImageUploader()`. Lets
 * paste/drop in any section's markdown body push through the same
 * reference-image endpoint without each section having to plumb
 * the prop.
 */
export type ExecSummaryImageUploader = (file: File) => Promise<string>;

export const ExecSummaryImageUploaderContext =
  createContext<ExecSummaryImageUploader | null>(null);

export function useExecSummaryImageUploader(): ExecSummaryImageUploader | null {
  return useContext(ExecSummaryImageUploaderContext);
}

export function StatusBadge({
  status,
  className,
}: {
  status: ExecSummaryStatus;
  className?: string;
}): JSX.Element {
  return (
    <Badge
      variant="outline"
      className={cn("font-medium", STATUS_CLASS[status], className)}
    >
      {STATUS_LABEL[status]}
    </Badge>
  );
}
