import { useState } from "react";
import {
  AlertTriangleIcon,
  CheckCircle2Icon,
  RotateCcwIcon,
  SaveIcon,
  SendIcon,
  Loader2Icon,
} from "lucide-react";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { cn } from "@/lib/utils";

import type { ExecSummaryDto } from "../../api/client.js";
import { formatAuDateTime } from "../view-wizard/date-display.js";
import {
  useApproveExecSummary,
  useRevertExecSummary,
  useSubmitExecSummary,
} from "./hooks/use-exec-summary.js";
import { PrintPdfButton } from "./pdf/print-pdf-button.js";
import { SaveVersionDialog } from "./save-version-dialog.js";
import { SECTIONS, StatusBadge, type ExecSummaryPermissions } from "./shared.js";
import { hasPendingChanges } from "./version.js";

/** Server-side gate in [`submit_project_exec_summary`]; kept in sync
 *  manually so the proactive missing-sections hint can tell the user
 *  what stands between them and being able to submit. */
const SUBMIT_THRESHOLD_PERCENT = 80;

export function ExecSummaryHeader({
  projectId,
  data,
  permissions,
  saving,
}: {
  projectId: string;
  data: ExecSummaryDto;
  permissions: ExecSummaryPermissions;
  saving: boolean;
}): JSX.Element {
  const submit = useSubmitExecSummary(projectId);
  const approve = useApproveExecSummary(projectId);
  const revert = useRevertExecSummary(projectId);
  const [saveOpen, setSaveOpen] = useState(false);
  const pending = hasPendingChanges(data);
  const pct = data.completion.percent;
  const status = data.approval.status;
  const missingLabels = SECTIONS
    .filter((s) => data.completion.sections[s.id] === false)
    .map((s) => s.label);
  const showHint = status === "draft" && missingLabels.length > 0;
  const blocked = pct < SUBMIT_THRESHOLD_PERCENT;

  return (
    <Card
      className="gap-3 px-5 py-4"
      data-testid="exec-summary-header"
    >
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="flex min-w-0 flex-col gap-1.5">
          <div className="flex items-center gap-2 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
            Executive summary
            {saving && (
              <span className="inline-flex items-center gap-1">
                <Loader2Icon className="h-3 w-3 animate-spin" /> Saving…
              </span>
            )}
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <StatusBadge status={status} />
            {pending && (
              <span
                className="inline-flex items-center rounded-full border border-amber-300 bg-amber-50 px-2 py-0.5 text-[11px] font-medium uppercase tracking-wide text-amber-900"
                data-testid="exec-summary-pending-changes"
              >
                Pending changes
              </span>
            )}
            <span className="text-xs text-muted-foreground">
              Updated {formatAuDateTime(data.updated_at)}
            </span>
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            size="sm"
            variant={pending ? "default" : "outline"}
            onClick={() => setSaveOpen(true)}
            data-testid="exec-summary-save-version"
          >
            <SaveIcon className="mr-1.5 h-3.5 w-3.5" />
            Save
          </Button>
          <PrintPdfButton projectId={projectId} data={data} />
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={
              !permissions.canSubmit ||
              submit.isPending ||
              status !== "draft"
            }
            onClick={() => submit.mutate()}
          >
            <SendIcon className="mr-1.5 h-3.5 w-3.5" />
            Submit
          </Button>
          {status === "draft" && blocked && permissions.canSubmit && (
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  className="border-amber-300 bg-amber-50 text-amber-900 hover:bg-amber-100"
                  disabled={submit.isPending}
                  data-testid="exec-summary-force-submit"
                >
                  <AlertTriangleIcon className="mr-1.5 h-3.5 w-3.5" />
                  Force submit
                </Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>
                    Force submit this exec summary?
                  </AlertDialogTitle>
                  <AlertDialogDescription asChild>
                    <div className="space-y-2">
                      <p>
                        The summary is{" "}
                        <span className="font-semibold">{pct}%</span>{" "}
                        complete — below the {SUBMIT_THRESHOLD_PERCENT}%
                        threshold normally required to move to review.
                      </p>
                      {missingLabels.length > 0 && (
                        <p>
                          Still incomplete:{" "}
                          <span className="font-medium">
                            {missingLabels.join(", ")}
                          </span>
                          .
                        </p>
                      )}
                      <p>
                        This action will be audit-logged as a forced
                        submission.
                      </p>
                    </div>
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>Cancel</AlertDialogCancel>
                  <AlertDialogAction
                    onClick={() => submit.mutate({ force: true })}
                    className="bg-amber-600 text-white hover:bg-amber-700"
                  >
                    Submit anyway
                  </AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          )}
          <Button
            type="button"
            size="sm"
            disabled={
              !permissions.canApprove ||
              approve.isPending ||
              status !== "in_review"
            }
            onClick={() => approve.mutate()}
          >
            <CheckCircle2Icon className="mr-1.5 h-3.5 w-3.5" />
            Approve
          </Button>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            disabled={
              !permissions.canRevert ||
              revert.isPending ||
              status === "draft"
            }
            onClick={() => revert.mutate()}
          >
            <RotateCcwIcon className="mr-1.5 h-3.5 w-3.5" />
            Revert
          </Button>
        </div>
      </div>

      <div className="flex items-center gap-3">
        <div className="flex-1">
          <Progress
            value={pct}
            className={cn(
              "h-2",
              pct >= 80 &&
                "[&>[data-slot=progress-indicator]]:bg-emerald-500",
            )}
          />
        </div>
        <span className="w-12 text-right text-sm font-semibold tabular-nums">
          {pct}%
        </span>
      </div>

      {showHint && (
        <div
          className={cn(
            "rounded-md border px-3 py-2 text-xs",
            blocked
              ? "border-amber-200 bg-amber-50 text-amber-900"
              : "border-muted bg-muted/40 text-muted-foreground",
          )}
          data-testid="exec-summary-missing-hint"
        >
          {blocked ? (
            <>
              <span className="font-medium">
                Needs {SUBMIT_THRESHOLD_PERCENT}% to submit.
              </span>{" "}
              Finish or mark N/A:{" "}
              <span className="font-medium">{missingLabels.join(", ")}</span>.
            </>
          ) : (
            <>
              Still to fill in:{" "}
              <span className="font-medium">{missingLabels.join(", ")}</span>.
            </>
          )}
        </div>
      )}

      <SaveVersionDialog
        projectId={projectId}
        data={data}
        open={saveOpen}
        onOpenChange={setSaveOpen}
      />
    </Card>
  );
}
