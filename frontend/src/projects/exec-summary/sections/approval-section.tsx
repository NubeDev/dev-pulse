import { CheckCircle2Icon, ClockIcon, RotateCcwIcon, SendIcon } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";

import type { ExecSummaryDto } from "../../../api/client.js";
import { useExecSummaryAutosave } from "../hooks/use-exec-summary.js";
import {
  useApproveExecSummary,
  useRevertExecSummary,
  useSubmitExecSummary,
} from "../hooks/use-exec-summary.js";
import { PlainTextareaField, TextField } from "../form-fields.js";
import { StatusBadge, type ExecSummaryPermissions } from "../shared.js";

export function ApprovalSection({
  projectId,
  data,
  permissions,
}: {
  projectId: string;
  data: ExecSummaryDto;
  permissions: ExecSummaryPermissions;
}): JSX.Element {
  const { patch } = useExecSummaryAutosave(projectId);
  const submit = useSubmitExecSummary(projectId);
  const approve = useApproveExecSummary(projectId);
  const revert = useRevertExecSummary(projectId);
  const a = data.approval;

  return (
    <div className="flex flex-col gap-6">
      <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
        <StatusCard
          label="Current status"
          icon={
            a.status === "approved" ? (
              <CheckCircle2Icon className="h-4 w-4 text-emerald-600" />
            ) : (
              <ClockIcon className="h-4 w-4 text-muted-foreground" />
            )
          }
        >
          <StatusBadge status={a.status} />
        </StatusCard>
        <StatusCard
          label="Submitted at"
          icon={<ClockIcon className="h-4 w-4 text-muted-foreground" />}
        >
          <span className="text-sm tabular-nums">
            {a.submitted_at ? fmtDateTime(a.submitted_at) : "—"}
          </span>
        </StatusCard>
        <StatusCard
          label="Approved at"
          icon={
            <CheckCircle2Icon
              className={
                a.approved_at
                  ? "h-4 w-4 text-emerald-600"
                  : "h-4 w-4 text-muted-foreground"
              }
            />
          }
        >
          <span className="text-sm tabular-nums">
            {a.approved_at ? fmtDateTime(a.approved_at) : "—"}
          </span>
        </StatusCard>
      </div>

      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <TextField
          id="es-reviewer"
          label="Reviewer"
          value={a.reviewer}
          onCommit={(reviewer) => patch({ approval: { reviewer } })}
          placeholder="Name or @login"
        />
        <TextField
          id="es-approver"
          label="Approver"
          value={a.approver}
          onCommit={(approver) => patch({ approval: { approver } })}
          placeholder="Project lead"
        />
      </div>

      <PlainTextareaField
        id="es-review-notes"
        label="Review notes"
        value={a.review_notes}
        onCommit={(review_notes) => patch({ approval: { review_notes } })}
      />
      <PlainTextareaField
        id="es-approval-notes"
        label="Approval notes"
        value={a.approval_notes}
        onCommit={(approval_notes) => patch({ approval: { approval_notes } })}
      />

      <div className="flex flex-wrap items-center gap-2">
        <Button
          type="button"
          variant="default"
          size="sm"
          disabled={
            !permissions.canSubmit ||
            submit.isPending ||
            a.status !== "draft"
          }
          onClick={() => submit.mutate()}
        >
          <SendIcon className="mr-1.5 h-3.5 w-3.5" />
          Submit for review
        </Button>
        <Button
          type="button"
          variant="default"
          size="sm"
          className="bg-emerald-600 text-white hover:bg-emerald-700"
          disabled={
            !permissions.canApprove ||
            approve.isPending ||
            a.status !== "in_review"
          }
          onClick={() => approve.mutate()}
        >
          <CheckCircle2Icon className="mr-1.5 h-3.5 w-3.5" />
          Approve
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={
            !permissions.canRevert ||
            revert.isPending ||
            a.status === "draft"
          }
          onClick={() => revert.mutate()}
        >
          <RotateCcwIcon className="mr-1.5 h-3.5 w-3.5" />
          Revert to draft
        </Button>
      </div>

      {(submit.error || approve.error || revert.error) && (
        <Alert variant="destructive">
          <AlertTitle>Action failed</AlertTitle>
          <AlertDescription>
            {(submit.error ?? approve.error ?? revert.error)?.message}
          </AlertDescription>
        </Alert>
      )}
    </div>
  );
}

function StatusCard({
  label,
  icon,
  children,
}: {
  label: string;
  icon: React.ReactNode;
  children: React.ReactNode;
}): JSX.Element {
  return (
    <Card className="gap-2 py-3">
      <CardContent className="flex items-start justify-between gap-3 px-4">
        <div className="flex min-w-0 flex-col gap-1">
          <span className="text-[10px] font-medium uppercase tracking-wide text-muted-foreground">
            {label}
          </span>
          {children}
        </div>
        {icon}
      </CardContent>
    </Card>
  );
}

function fmtDateTime(iso: string): string {
  try {
    return new Date(iso).toLocaleString();
  } catch {
    return iso;
  }
}
