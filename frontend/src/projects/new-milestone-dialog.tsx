/**
 * `<NewMilestoneDialog>` — PROJECT-VIEW.md milestones two-way
 * sync, smallest slice. Creates a milestone on one of the
 * project's linked repos via
 * `POST /projects/{id}/milestones`, which writes through to
 * GitHub and mirrors the row into `dp_milestones`.
 *
 * Three controls:
 *   - Repo dropdown    ← `useProjectRepos(projectId)`. Required.
 *                        Hidden when the project has exactly one
 *                        linked repo (auto-selected).
 *   - Title input      ← required, trimmed before submit.
 *   - Description box  ← optional, markdown, blank == omitted.
 *   - Due date input   ← optional native `<input type="date">`.
 *
 * Submit disables itself while the mutation is in flight. On
 * `403 writes_not_available_for_org` the dialog surfaces the
 * standard upstream-unavailable alert and the caller's install
 * banner explains the fix. Tests pass `enabled=false` via the
 * `open` prop to keep the picker from firing on every render.
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";

import { isDpRestError } from "../api/client.js";
import {
  useCreateProjectMilestone,
  useProjectRepos,
} from "./use-projects-data.js";

export interface NewMilestoneDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  projectId: string;
}

export function NewMilestoneDialog({
  open,
  onOpenChange,
  projectId,
}: NewMilestoneDialogProps): JSX.Element {
  const repoLinks = useProjectRepos(projectId);
  const create = useCreateProjectMilestone(projectId);

  const repos = repoLinks.data ?? [];
  const [repoId, setRepoId] = useState<string>("");
  const [title, setTitle] = useState<string>("");
  const [description, setDescription] = useState<string>("");
  const [dueOn, setDueOn] = useState<string>("");

  // Reset form on (re-)open so previous attempts don't bleed.
  // Auto-select the repo when the project has exactly one linked
  // — the dropdown is hidden in that case so the user shouldn't
  // see a blank required field.
  useEffect(() => {
    if (open) {
      setTitle("");
      setDescription("");
      setDueOn("");
      setRepoId(repos.length === 1 ? repos[0]!.repo_id : "");
      create.reset();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, repos.length]);

  const canSubmit =
    !!repoId && title.trim().length > 0 && !create.isPending;

  const onSubmit = (): void => {
    if (!canSubmit) return;
    create.mutate(
      {
        repo_id: repoId,
        title: title.trim(),
        description: description.trim() || null,
        due_on: dueOn || null,
      },
      { onSuccess: () => onOpenChange(false) },
    );
  };

  const createErr: { title: string; body: string } | null = (() => {
    if (!create.error) return null;
    if (isDpRestError(create.error)) {
      if (create.error.code === "writes_not_available_for_org") {
        return {
          title: "Writes not available",
          body: "Install the dev-pulse GitHub App with Issues: write on the target org, then try again.",
        };
      }
      if (create.error.code === "repo_not_linked") {
        return {
          title: "Repo not linked",
          body: "That repo is no longer linked to this project. Refresh the page and try again.",
        };
      }
      if (create.error.code === "upstream_validation") {
        return {
          title: "GitHub rejected the milestone",
          body: create.error.message,
        };
      }
    }
    return { title: "Create failed", body: create.error.message };
  })();

  const noRepos = !repoLinks.isPending && repos.length === 0;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="sm:max-w-lg"
        data-testid="new-milestone-dialog"
      >
        <DialogHeader>
          <DialogTitle>New milestone</DialogTitle>
          <DialogDescription>
            Creates the milestone on GitHub and mirrors it back into
            dev-pulse in one step.
          </DialogDescription>
        </DialogHeader>

        {noRepos && (
          <Alert variant="destructive">
            <AlertTitle>No linked repos</AlertTitle>
            <AlertDescription>
              Link a repo to this project before creating a milestone.
            </AlertDescription>
          </Alert>
        )}

        {!noRepos && (
          <div className="flex flex-col gap-4 py-2">
            {repos.length > 1 && (
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="new-milestone-repo">Repo</Label>
                <Select value={repoId} onValueChange={setRepoId}>
                  <SelectTrigger
                    id="new-milestone-repo"
                    data-testid="new-milestone-repo"
                  >
                    <SelectValue placeholder="Pick a repo" />
                  </SelectTrigger>
                  <SelectContent>
                    {repos.map((r) => (
                      <SelectItem key={r.repo_id} value={r.repo_id}>
                        {r.repo_name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            )}

            <div className="flex flex-col gap-1.5">
              <Label htmlFor="new-milestone-title">Title</Label>
              <Input
                id="new-milestone-title"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                placeholder="e.g. v0.3 Beta"
                maxLength={255}
                data-testid="new-milestone-title"
              />
            </div>

            <div className="flex flex-col gap-1.5">
              <Label htmlFor="new-milestone-description">
                Description{" "}
                <span className="text-xs font-normal text-muted-foreground">
                  (optional, markdown)
                </span>
              </Label>
              <Textarea
                id="new-milestone-description"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                rows={3}
                data-testid="new-milestone-description"
              />
            </div>

            <div className="flex flex-col gap-1.5">
              <Label htmlFor="new-milestone-due">
                Due date{" "}
                <span className="text-xs font-normal text-muted-foreground">
                  (optional)
                </span>
              </Label>
              <DateInput
                id="new-milestone-due"
                value={dueOn}
                onChange={(e) => setDueOn(e.target.value)}
                data-testid="new-milestone-due"
              />
            </div>
          </div>
        )}

        {createErr && (
          <Alert variant="destructive">
            <AlertTitle>{createErr.title}</AlertTitle>
            <AlertDescription>{createErr.body}</AlertDescription>
          </Alert>
        )}

        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={create.isPending}
          >
            Cancel
          </Button>
          <Button
            onClick={onSubmit}
            disabled={!canSubmit || noRepos}
            data-testid="new-milestone-submit"
          >
            {create.isPending ? "Creating…" : "Create milestone"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
