/**
 * Per-row CRUD action button for the §6.2 projects table.
 *
 * Backend doesn't support hard delete — projects are archived
 * (`POST /projects/{id}/archive`) and restored via PATCH
 * (`status` back to `active`). Both ops are CAS-gated by
 * `expected_version`, so we always pass `project.version`.
 */

import { useState } from "react";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";

import type { ProjectDto } from "../api/client.js";

import { useArchiveProject, usePatchProject } from "./use-projects-data.js";

export function ProjectRowActions({
  project,
}: {
  project: ProjectDto;
}): JSX.Element {
  const [confirmOpen, setConfirmOpen] = useState(false);
  const archive = useArchiveProject(project.id);
  const patch = usePatchProject(project.id);

  const isArchived = project.status === "archived";
  const pending = archive.isPending || patch.isPending;

  const onConfirm = (): void => {
    if (isArchived) {
      patch.mutate(
        { expected_version: project.version, status: "active" },
        { onSuccess: () => setConfirmOpen(false) },
      );
    } else {
      archive.mutate(
        { expected_version: project.version },
        { onSuccess: () => setConfirmOpen(false) },
      );
    }
  };

  return (
    <>
      <Button
        variant="ghost"
        size="sm"
        disabled={pending}
        onClick={(e) => {
          e.stopPropagation();
          setConfirmOpen(true);
        }}
        data-testid={
          isArchived ? "project-restore-button" : "project-archive-button"
        }
      >
        {pending ? <Spinner /> : isArchived ? "Restore" : "Archive"}
      </Button>

      <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <AlertDialogContent onClick={(e) => e.stopPropagation()}>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {isArchived
                ? `Restore "${project.name}"?`
                : `Archive "${project.name}"?`}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {isArchived
                ? "Move this project back to Active. Linked boards and issues are preserved."
                : "Archived projects are hidden from the default views but keep their issue links and board mirrors. You can restore later."}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={pending}>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={onConfirm} disabled={pending}>
              {isArchived ? "Restore" : "Archive"}
            </AlertDialogAction>
          </AlertDialogFooter>
          {(archive.error || patch.error) && (
            <p
              className="text-sm text-destructive"
              data-testid="project-action-error"
            >
              {(archive.error ?? patch.error)?.message}
            </p>
          )}
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
