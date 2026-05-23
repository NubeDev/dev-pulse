import {
  CheckCircle2Icon,
  RotateCcwIcon,
  SendIcon,
  Loader2Icon,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { cn } from "@/lib/utils";

import type { ExecSummaryDto } from "../../api/client.js";
import {
  useApproveExecSummary,
  useRevertExecSummary,
  useSubmitExecSummary,
} from "./hooks/use-exec-summary.js";
import { StatusBadge, type ExecSummaryPermissions } from "./shared.js";

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
  const pct = data.completion.percent;
  const status = data.approval.status;

  return (
    <div
      className="rounded-2xl bg-[#071923] px-5 py-4 text-slate-100 shadow-sm"
      data-testid="exec-summary-header"
    >
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="flex min-w-0 flex-col gap-1.5">
          <div className="flex items-center gap-2 text-xs uppercase tracking-wide text-slate-400">
            Executive summary
            {saving && (
              <span className="inline-flex items-center gap-1 text-slate-400">
                <Loader2Icon className="h-3 w-3 animate-spin" /> Saving…
              </span>
            )}
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <StatusBadge status={status} />
            <span className="text-xs text-slate-400">
              Updated {new Date(data.updated_at).toLocaleString()}
            </span>
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            size="sm"
            variant="secondary"
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
          <Button
            type="button"
            size="sm"
            className="bg-emerald-600 text-white hover:bg-emerald-700"
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
            variant="outline"
            className="border-slate-600 bg-transparent text-slate-100 hover:bg-slate-800"
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

      <div className="mt-4 flex items-center gap-3">
        <div className="flex-1">
          <Progress
            value={pct}
            className={cn(
              "h-2 bg-slate-800",
              pct >= 80 &&
                "[&>[data-slot=progress-indicator]]:bg-emerald-500",
              pct < 80 &&
                pct > 0 &&
                "[&>[data-slot=progress-indicator]]:bg-sky-500",
            )}
          />
        </div>
        <span className="w-12 text-right text-sm font-semibold tabular-nums">
          {pct}%
        </span>
      </div>
    </div>
  );
}
