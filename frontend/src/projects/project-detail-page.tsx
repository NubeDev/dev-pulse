/**
 * `#/projects/{id}` — single-project detail page (§6.3).
 *
 * Stage 9 (slice B) builds the minimum surface needed to host the
 * Link-a-board dialog + the per-link mirror status rows: header
 * with name + status pill, a Meta block (Start · Due · Lead · %
 * closed), and a `Linked GitHub boards` section. The §6.3 issue
 * list lives in a later slice — we deliberately scaffold the page
 * around the slice-B affordances so the file isn't a no-op once
 * stage 9 lands.
 *
 * The mirror-status row mirrors §6.4 verbatim:
 *   `Last sync: 14:23:07 ✓` on success, or
 *   `Last sync: failed — <message>` on failure (with a
 *   `[Re-link]` follow-up that unlinks + re-opens the dialog).
 */

import { useState } from "react";
import { SettingsIcon } from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Spinner } from "@/components/ui/spinner";

import type { IssueListItem, ProjectDto } from "../api/client.js";
import { PageHeading } from "../components/page-heading.jsx";
import { navigate, projectDetailRoute, projectSelectedIssue, useRoute } from "../routes.js";
import { IssueEditCard } from "../workflow/issues-page.js";

import { LinkBoardDialog } from "./link-board-dialog.js";
import { AddIssuesDialog } from "./add-issues-dialog.js";
import { ManageReposDialog } from "./project-repos-card.js";
import {
  useArchiveProject,
  useBoardLinks,
  useDeleteBoardLink,
  usePatchProject,
  useProject,
  useProjectIssues,
  useProjectRepos,
  useRemoveIssueFromProject,
} from "./use-projects-data.js";
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

export interface ProjectDetailPageProps {
  projectId: string;
}

const STATUS_LABEL: Record<ProjectDto["status"], string> = {
  active: "Active",
  backlog: "Backlog",
  done: "Done",
  archived: "Archived",
};

const STATUS_VARIANT: Record<
  ProjectDto["status"],
  "default" | "secondary" | "outline"
> = {
  active: "default",
  backlog: "secondary",
  done: "outline",
  archived: "outline",
};

export function ProjectDetailPage({
  projectId,
}: ProjectDetailPageProps): JSX.Element {
  const project = useProject(projectId);

  if (project.isPending) {
    return (
      <div className="px-4 lg:px-6">
        <div
          className="flex items-center gap-2 py-4 text-sm text-muted-foreground"
          data-testid="project-detail-loading"
        >
          <Spinner /> Loading project…
        </div>
      </div>
    );
  }

  if (project.isError) {
    return (
      <div className="px-4 lg:px-6">
        <Alert variant="destructive" data-testid="project-detail-error">
          <AlertTitle>Couldn't load project</AlertTitle>
          <AlertDescription>{project.error.message}</AlertDescription>
        </Alert>
      </div>
    );
  }

  if (!project.data) {
    return (
      <div className="px-4 lg:px-6">
        <Alert data-testid="project-detail-missing">
          <AlertTitle>Project not found</AlertTitle>
          <AlertDescription>
            This project either doesn't exist or you don't have
            access to it.{" "}
            <a className="underline" href="#/projects">
              Back to projects
            </a>
            .
          </AlertDescription>
        </Alert>
      </div>
    );
  }

  return (
    <ProjectDetailBody project={project.data} />
  );
}

function ProjectDetailBody({ project }: { project: ProjectDto }): JSX.Element {
  const route = useRoute();
  const selectedIssueId = projectSelectedIssue(route);
  const [linkBoardOpen, setLinkBoardOpen] = useState(false);
  const [manageReposOpen, setManageReposOpen] = useState(false);
  const [archiveConfirmOpen, setArchiveConfirmOpen] = useState(false);
  const links = useBoardLinks(project.id);
  const deleteLink = useDeleteBoardLink(project.id);
  const repoLinks = useProjectRepos(project.id);
  const archive = useArchiveProject(project.id);
  const patch = usePatchProject(project.id);
  const isArchived = project.status === "archived";
  const archivePending = archive.isPending || patch.isPending;

  const onArchiveConfirm = (): void => {
    if (isArchived) {
      patch.mutate(
        { expected_version: project.version, status: "active" },
        { onSuccess: () => setArchiveConfirmOpen(false) },
      );
    } else {
      archive.mutate(
        { expected_version: project.version },
        { onSuccess: () => setArchiveConfirmOpen(false) },
      );
    }
  };

  const pctClosed =
    project.issue_count > 0
      ? Math.round((project.closed_issue_count / project.issue_count) * 100)
      : 0;

  const openIssue = (id: string | null): void => {
    navigate(projectDetailRoute(project.id, id));
  };

  return (
    <div className="flex gap-4 px-4 lg:px-6" data-testid="project-detail">
    <div className="flex min-w-0 flex-1 flex-col gap-4">
      <PageHeading
        title={
          <span className="flex flex-wrap items-center gap-2">
            <span data-testid="project-detail-name">{project.name}</span>
            <Badge
              variant={STATUS_VARIANT[project.status]}
              data-testid="project-detail-status"
            >
              {STATUS_LABEL[project.status]}
            </Badge>
            {(repoLinks.data ?? []).map((r) => (
              <Badge
                key={r.repo_id}
                variant="outline"
                className="border-emerald-300 bg-emerald-50 font-mono text-xs font-normal text-emerald-800 dark:border-emerald-800/60 dark:bg-emerald-950/40 dark:text-emerald-200"
                data-testid="project-detail-repo-tag"
                title={`Repo · linked ${new Date(r.added_at).toLocaleString("en-AU")}`}
              >
                {r.repo_name}
              </Badge>
            ))}
            {(links.data ?? []).map((b) => (
              <Badge
                key={b.id}
                variant="outline"
                className="border-sky-300 bg-sky-50 text-xs font-normal text-sky-800 dark:border-sky-800/60 dark:bg-sky-950/40 dark:text-sky-200"
                data-testid="project-detail-board-tag"
                title="GitHub Project board"
              >
                {b.github_board_title ?? "Untitled board"}
              </Badge>
            ))}
          </span>
        }
        description={project.description ?? undefined}
        trailing={
          <div className="flex items-center gap-2">
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline" size="sm">
                  <SettingsIcon className="mr-1.5 h-4 w-4" /> Settings
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-56">
                <DropdownMenuLabel>Boards</DropdownMenuLabel>
                {(links.data ?? []).map((link) => (
                  <DropdownMenuItem
                    key={link.id}
                    className="flex items-center justify-between"
                    onSelect={(e) => { e.preventDefault(); deleteLink.mutate(link.id); }}
                  >
                    <span className="truncate text-xs">{link.github_board_title ?? "Untitled"}</span>
                    <span className="ml-2 shrink-0 text-[10px] text-muted-foreground">Unlink</span>
                  </DropdownMenuItem>
                ))}
                <DropdownMenuItem
                  onSelect={(e) => {
                    // Radix: preventDefault keeps the menu from auto-closing
                    // in the same tick the Dialog mounts, otherwise the
                    // menu's DismissableLayer fires on the trailing mouseup
                    // and immediately closes the freshly-opened Dialog.
                    e.preventDefault();
                    setLinkBoardOpen(true);
                  }}
                >
                  + Link a board…
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuLabel>Repos</DropdownMenuLabel>
                <DropdownMenuItem
                  onSelect={(e) => {
                    e.preventDefault();
                    setManageReposOpen(true);
                  }}
                  data-testid="project-settings-manage-repos"
                >
                  Manage repos…
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuLabel>Actions</DropdownMenuLabel>
                <DropdownMenuItem
                  disabled={archivePending}
                  onSelect={(e) => {
                    e.preventDefault();
                    setArchiveConfirmOpen(true);
                  }}
                  data-testid={isArchived ? "project-restore-button" : "project-archive-button"}
                  className={isArchived ? "" : "text-destructive focus:text-destructive"}
                >
                  {isArchived ? "Restore" : "Archive project"}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        }
      />

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Meta</CardTitle>
        </CardHeader>
        <CardContent>
          <dl className="grid grid-cols-2 gap-x-6 gap-y-2 text-sm md:grid-cols-4">
            <MetaCell label="Start" value={fmtDate(project.start_at)} />
            <MetaCell label="Due" value={fmtDate(project.due_at)} />
            <MetaCell
              label="Issues"
              value={`${project.closed_issue_count}/${project.issue_count} closed (${pctClosed}%)`}
              testId="project-detail-progress"
            />
            <MetaCell
              label="Linked boards"
              value={String(project.board_link_count)}
              testId="project-detail-board-count"
            />
          </dl>
        </CardContent>
      </Card>

      <ProjectIssuesCard project={project} onSelectIssue={openIssue} selectedIssueId={selectedIssueId} />

      <LinkBoardDialog
        open={linkBoardOpen}
        onOpenChange={setLinkBoardOpen}
        projectId={project.id}
        projectOrgId={project.org_id}
      />

      <ManageReposDialog
        open={manageReposOpen}
        onOpenChange={setManageReposOpen}
        projectId={project.id}
        projectOrgId={project.org_id}
      />

      <AlertDialog open={archiveConfirmOpen} onOpenChange={setArchiveConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {isArchived ? `Restore "${project.name}"?` : `Archive "${project.name}"?`}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {isArchived
                ? "Move this project back to Active. Linked boards and issues are preserved."
                : "Archived projects are hidden from the default views but keep their issue links and board mirrors. You can restore later."}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={archivePending}>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={onArchiveConfirm} disabled={archivePending}>
              {isArchived ? "Restore" : "Archive"}
            </AlertDialogAction>
          </AlertDialogFooter>
          {(archive.error || patch.error) && (
            <p className="text-sm text-destructive" data-testid="project-action-error">
              {(archive.error ?? patch.error)?.message}
            </p>
          )}
        </AlertDialogContent>
      </AlertDialog>
    </div>

    {selectedIssueId && (
      <aside className="hidden w-[400px] shrink-0 flex-col border-l border-border xl:flex" data-testid="project-issue-detail">
        <header className="flex items-center justify-between border-b border-border px-4 py-2">
          <span className="text-sm font-medium">Issue detail</span>
          <Button variant="ghost" size="sm" onClick={() => openIssue(null)}>✕</Button>
        </header>
        <div className="flex-1 overflow-y-auto p-4">
          <IssueEditCard issueId={selectedIssueId} />
        </div>
      </aside>
    )}
    </div>
  );
}

function MetaCell({
  label,
  value,
  testId,
}: {
  label: string;
  value: string;
  testId?: string;
}): JSX.Element {
  return (
    <div className="flex flex-col">
      <dt className="text-xs font-medium uppercase text-muted-foreground">
        {label}
      </dt>
      <dd className="font-mono text-sm" data-testid={testId}>
        {value}
      </dd>
    </div>
  );
}

function fmtDate(s: string | null | undefined): string {
  if (!s) return "—";
  return new Date(s).toLocaleDateString("en-AU");
}

// ---------------------------------------------------------------------------
// §6.3 Issues card — paginated membership list with `[+ Add issues]`
// and per-row remove. Uses the existing IssueListItem shape (the
// same row the workflow surface uses), giving the user a single
// click through to the triage detail pane via the
// `workflowIssuesRoute({ issueId })` deep link.
// ---------------------------------------------------------------------------

function ProjectIssuesCard({ project, onSelectIssue, selectedIssueId }: { project: ProjectDto; onSelectIssue: (id: string | null) => void; selectedIssueId: string | null }): JSX.Element {
  const [dialogOpen, setDialogOpen] = useState(false);
  const issues = useProjectIssues(project.id, { state: "all", limit: 100 });
  const remove = useRemoveIssueFromProject(project.id);

  const rows = issues.data?.rows ?? [];

  return (
    <Card data-testid="project-issues">
      <CardHeader className="flex flex-row items-center justify-between">
        <CardTitle className="text-base">
          Issues{" "}
          <span className="ml-2 text-sm font-normal text-muted-foreground">
            ({project.closed_issue_count}/{project.issue_count} closed)
          </span>
        </CardTitle>
        <Button
          size="sm"
          onClick={() => setDialogOpen(true)}
          data-testid="project-add-issues-button"
        >
          + Add issues
        </Button>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        {issues.isPending && (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Spinner /> Loading issues…
          </div>
        )}
        {issues.isError && (
          <Alert variant="destructive">
            <AlertTitle>Couldn't load issues</AlertTitle>
            <AlertDescription>{issues.error.message}</AlertDescription>
          </Alert>
        )}
        {!issues.isPending && !issues.isError && rows.length === 0 && (
          <p
            className="py-6 text-center text-sm text-muted-foreground"
            data-testid="project-issues-empty"
          >
            No issues in this project yet. Click [+ Add issues] to attach work from the workflow surface.
          </p>
        )}
        {rows.map((row) => (
          <ProjectIssueRow
            key={row.id}
            row={row}
            selected={row.id === selectedIssueId}
            onSelect={() => onSelectIssue(row.id)}
            onRemove={() =>
              remove.mutate({
                issueId: row.id,
                expectedVersion: project.version,
              })
            }
            removePending={remove.isPending}
          />
        ))}
        {remove.error && (
          <Alert variant="destructive">
            <AlertTitle>Remove failed</AlertTitle>
            <AlertDescription>{remove.error.message}</AlertDescription>
          </Alert>
        )}
      </CardContent>

      <AddIssuesDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        project={project}
      />
    </Card>
  );
}

function ProjectIssueRow({
  row,
  selected,
  onSelect,
  onRemove,
  removePending,
}: {
  row: IssueListItem;
  selected: boolean;
  onSelect: () => void;
  onRemove: () => void;
  removePending: boolean;
}): JSX.Element {
  return (
    <div
      className={`flex cursor-pointer items-center gap-3 rounded-md border px-3 py-2 text-sm ${
        selected ? "border-primary bg-accent/40" : "border-border bg-muted/10 hover:bg-accent/20"
      }`}
      data-testid="project-issue-row"
      onClick={onSelect}
    >
      <Badge
        variant={row.state === "open" ? "default" : "secondary"}
        className="shrink-0 px-1.5 py-0 text-[10px] uppercase"
      >
        {row.state}
      </Badge>
      <span className="shrink-0 font-mono text-xs text-muted-foreground tabular-nums">
        {row.repo_slug ?? "—"}#{row.number}
      </span>
      <span className="flex-1 truncate">
        {row.title}
      </span>
      <Button
        variant="ghost"
        size="sm"
        onClick={(e) => { e.stopPropagation(); onRemove(); }}
        disabled={removePending}
        data-testid="project-issue-remove"
        title="Remove from project"
      >
        Remove
      </Button>
    </div>
  );
}
