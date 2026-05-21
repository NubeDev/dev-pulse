/**
 * `<EditMilestoneDialog>` — sibling of [`NewMilestoneDialog`]
 * for the in-place edit of an existing milestone. Backed by
 * `PATCH /projects/{id}/milestones/{milestone_id}`, which
 * forwards the diff to GitHub and re-upserts the local mirror
 * in the same request so the strip refreshes without an extra
 * round-trip.
 *
 * Three editable fields — title, description, due date. The
 * repo isn't surfaced (milestones can't migrate between repos)
 * and the state toggle lives on the strip's overflow menu.
 *
 * Tri-state field semantics (matching the wire shape):
 *   * left blank ⇒ omitted from the patch (server leaves as-is)
 *     for the title (title cannot be cleared on GitHub);
 *   * blanked-out description / due date ⇒ `null` (clear);
 *   * changed text / date ⇒ replacement value.
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
import { DateInput } from "@/components/ui/date-input";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";

import {
  isDpRestError,
  type MilestoneDto,
  type PatchMilestoneRequest,
} from "../api/client.js";
import { useUpdateProjectMilestone } from "./use-projects-data.js";

export interface EditMilestoneDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  projectId: string;
  /** The milestone being edited. May be `null` when the parent
   *  closed the dialog and the row has been removed from the
   *  cache. The dialog renders nothing in that case. */
  milestone: MilestoneDto | null;
}

export function EditMilestoneDialog({
  open,
  onOpenChange,
  projectId,
  milestone,
}: EditMilestoneDialogProps): JSX.Element | null {
  const update = useUpdateProjectMilestone(projectId);

  const [title, setTitle] = useState<string>("");
  const [description, setDescription] = useState<string>("");
  const [dueOn, setDueOn] = useState<string>("");

  // Prefill on (re-)open so editing one milestone, closing, then
  // editing a different one doesn't bleed values from the
  // previous form.
  useEffect(() => {
    if (open && milestone) {
      setTitle(milestone.title);
      setDescription(milestone.description ?? "");
      setDueOn(milestone.due_on ?? "");
      update.reset();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, milestone?.id]);

  if (!milestone) return null;

  const trimmedTitle = title.trim();
  const trimmedDesc = description.trim();
  const hasTitleChange =
    trimmedTitle.length > 0 && trimmedTitle !== milestone.title;
  const hasDescChange = trimmedDesc !== (milestone.description ?? "");
  const hasDueChange = dueOn !== (milestone.due_on ?? "");
  const dirty = hasTitleChange || hasDescChange || hasDueChange;

  const canSubmit =
    dirty && trimmedTitle.length > 0 && !update.isPending;

  const onSubmit = (): void => {
    if (!canSubmit) return;
    const body: PatchMilestoneRequest = {};
    if (hasTitleChange) body.title = trimmedTitle;
    if (hasDescChange) {
      // Empty string ⇒ clear on GitHub.
      body.description = trimmedDesc.length === 0 ? null : trimmedDesc;
    }
    if (hasDueChange) {
      body.due_on = dueOn || null;
    }
    update.mutate(
      { milestoneId: milestone.id, body },
      { onSuccess: () => onOpenChange(false) },
    );
  };

  const updateErr: { title: string; body: string } | null = (() => {
    if (!update.error) return null;
    if (isDpRestError(update.error)) {
      if (update.error.code === "writes_not_available_for_org") {
        return {
          title: "Writes not available",
          body: "Install the dev-pulse GitHub App with Issues: write on the target org, then try again.",
        };
      }
      if (update.error.code === "upstream_validation") {
        return {
          title: "GitHub rejected the change",
          body: update.error.message,
        };
      }
      if (update.error.code === "milestone_not_found") {
        return {
          title: "Milestone gone",
          body: "This milestone was deleted elsewhere. Refresh the page.",
        };
      }
    }
    return { title: "Save failed", body: update.error.message };
  })();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="sm:max-w-lg"
        data-testid="edit-milestone-dialog"
      >
        <DialogHeader>
          <DialogTitle>Edit milestone</DialogTitle>
          <DialogDescription>
            Writes through to GitHub and refreshes the local mirror in
            one step.
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-4 py-2">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="edit-milestone-title">Title</Label>
            <Input
              id="edit-milestone-title"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              maxLength={255}
              data-testid="edit-milestone-title"
            />
          </div>

          <div className="flex flex-col gap-1.5">
            <Label htmlFor="edit-milestone-description">
              Description{" "}
              <span className="text-xs font-normal text-muted-foreground">
                (markdown — blank clears)
              </span>
            </Label>
            <Textarea
              id="edit-milestone-description"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              rows={3}
              data-testid="edit-milestone-description"
            />
          </div>

          <div className="flex flex-col gap-1.5">
            <Label htmlFor="edit-milestone-due">
              Due date{" "}
              <span className="text-xs font-normal text-muted-foreground">
                (blank clears)
              </span>
            </Label>
            <DateInput
              id="edit-milestone-due"
              value={dueOn}
              onChange={(e) => setDueOn(e.target.value)}
              data-testid="edit-milestone-due"
            />
          </div>
        </div>

        {updateErr && (
          <Alert variant="destructive">
            <AlertTitle>{updateErr.title}</AlertTitle>
            <AlertDescription>{updateErr.body}</AlertDescription>
          </Alert>
        )}

        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={update.isPending}
          >
            Cancel
          </Button>
          <Button
            onClick={onSubmit}
            disabled={!canSubmit}
            data-testid="edit-milestone-submit"
          >
            {update.isPending ? "Saving…" : "Save changes"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
