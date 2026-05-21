/**
 * §6.6 bulk-add-from-triage dialog. Mounted by the triage bulk
 * toolbar; takes a fixed set of pre-selected issue ids plus their
 * org id (derived from the first row — v1 caps bulk operations
 * to a single org because `dp_projects.org_id` is `NOT NULL`).
 *
 * Picks one of the org's active projects, fires a single
 * `POST /projects/{id}/issues`, then surfaces the per-row
 * `BulkAddResult` so the user can see exactly which issues
 * landed and which were skipped (with reason).
 */

import { useEffect, useState } from "react";

import {
  Alert,
  AlertDescription,
  AlertTitle,
} from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";

import {
  BULK_ADD_ISSUE_CAP,
  type BulkAddResult,
} from "../api/client.js";

import {
  useAddIssuesToProject,
  useProjectList,
} from "../projects/use-projects-data.js";

export interface BulkAddToProjectDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Issue ids to attach. Capped client-side at
   *  `BULK_ADD_ISSUE_CAP`; over the cap the dialog disables the
   *  submit button with an explainer. */
  issueIds: string[];
  /** Restrict the picker to one org. v1 single-org constraint —
   *  see `linear-projects-v2.md` §4. */
  orgId: string | null;
  /** Fires once a bulk add succeeds (after the user closes the
   *  result alert). The triage page uses this to clear its
   *  multi-select state. */
  onAdded?: () => void;
}

export function BulkAddToProjectDialog({
  open,
  onOpenChange,
  issueIds,
  orgId,
  onAdded,
}: BulkAddToProjectDialogProps): JSX.Element {
  const projectsQ = useProjectList(
    orgId
      ? { org_id: orgId, status: "active", limit: 200 }
      : { limit: 200 },
  );
  const [projectId, setProjectId] = useState<string>("");
  const [result, setResult] = useState<BulkAddResult | null>(null);
  const add = useAddIssuesToProject(projectId);

  useEffect(() => {
    if (!open) return;
    setProjectId("");
    setResult(null);
    add.reset();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const projects = projectsQ.data?.rows ?? [];
  const chosen = projects.find((p) => p.id === projectId);
  const overCap = issueIds.length > BULK_ADD_ISSUE_CAP;

  const onSubmit = (): void => {
    if (!chosen || overCap || issueIds.length === 0) return;
    add.mutate(
      {
        expected_version: chosen.version,
        issue_ids: issueIds,
      },
      {
        onSuccess: (r) => setResult(r),
      },
    );
  };

  const close = (): void => {
    if (result) onAdded?.();
    onOpenChange(false);
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) close();
        else onOpenChange(o);
      }}
    >
      <DialogContent data-testid="bulk-add-project-dialog">
        <DialogHeader>
          <DialogTitle>Add {issueIds.length} issues to a project</DialogTitle>
          <DialogDescription>
            Pick an active project in this org. Issues already in another
            project are skipped with a clear reason.
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-3">
          <div className="flex flex-col gap-2">
            <Label htmlFor="bulk-add-project">Project</Label>
            {projectsQ.isPending ? (
              <div className="flex items-center gap-2 text-sm text-muted-foreground">
                <Spinner /> Loading projects…
              </div>
            ) : projects.length === 0 ? (
              <p className="text-sm text-muted-foreground">
                No active projects in this org. Create one first from{" "}
                <a className="underline" href="#/projects">
                  #/projects
                </a>
                .
              </p>
            ) : (
              <Select value={projectId} onValueChange={setProjectId}>
                <SelectTrigger
                  id="bulk-add-project"
                  data-testid="bulk-add-project-select"
                >
                  <SelectValue placeholder="Select a project" />
                </SelectTrigger>
                <SelectContent>
                  {projects.map((p) => (
                    <SelectItem key={p.id} value={p.id}>
                      {p.name}{" "}
                      <span className="text-xs text-muted-foreground">
                        ({p.closed_issue_count}/{p.issue_count} closed)
                      </span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          </div>

          {overCap && (
            <Alert variant="destructive">
              <AlertTitle>Selection too large</AlertTitle>
              <AlertDescription>
                The bulk-add endpoint accepts up to {BULK_ADD_ISSUE_CAP}{" "}
                issues per call; you have {issueIds.length}. Trim the
                selection and try again.
              </AlertDescription>
            </Alert>
          )}

          {add.error && (
            <Alert variant="destructive" data-testid="bulk-add-project-error">
              <AlertTitle>Add failed</AlertTitle>
              <AlertDescription>{add.error.message}</AlertDescription>
            </Alert>
          )}

          {result && (
            <Alert data-testid="bulk-add-project-result">
              <AlertTitle>
                Added {result.added.length}, skipped {result.skipped.length}
              </AlertTitle>
              {result.skipped.length > 0 && (
                <AlertDescription>
                  <ul className="mt-2 list-disc pl-5 text-xs">
                    {result.skipped.slice(0, 10).map((s) => (
                      <li key={s.issue_id}>
                        <code className="font-mono">
                          {s.issue_id.slice(0, 8)}
                        </code>
                        : {s.reason}
                      </li>
                    ))}
                    {result.skipped.length > 10 && (
                      <li>…and {result.skipped.length - 10} more</li>
                    )}
                  </ul>
                </AlertDescription>
              )}
            </Alert>
          )}
        </div>

        <DialogFooter>
          <Button
            type="button"
            variant="ghost"
            onClick={close}
            disabled={add.isPending}
          >
            {result ? "Done" : "Cancel"}
          </Button>
          {!result && (
            <Button
              type="button"
              data-testid="bulk-add-project-submit"
              onClick={onSubmit}
              disabled={
                !chosen || add.isPending || overCap || issueIds.length === 0
              }
            >
              {add.isPending ? "Adding…" : `Add ${issueIds.length}`}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
