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

import { type ReactNode, useEffect, useState } from "react";
import {
  CheckCircle2Icon,
  CircleDotIcon,
  LinkIcon,
  PencilIcon,
  SettingsIcon,
  UserIcon,
} from "lucide-react";
import { Popover as PopoverPrimitive } from "radix-ui";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
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
import { cn } from "@/lib/utils";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Spinner } from "@/components/ui/spinner";

import { useQuery } from "@tanstack/react-query";

import { api } from "../api/client.js";
import type { IssueListItem, MilestoneDto, OrgDto, PatchProjectRequest, ProjectDto } from "../api/client.js";
import { PageHeading } from "../components/page-heading.jsx";
import { HelpHint } from "@/components/help-hint";
import { navigate, projectDetailRoute, projectDetailRouteWithParams, projectFilter, projectGroupBy, projectSelectedIssue, projectSort, projectViewId, useRoute } from "../routes.js";
import { IssueEditCard } from "../workflow/issues-page.js";
import { UserPicker } from "../components/user-picker.js";

import { LinkBoardDialog } from "./link-board-dialog.js";
import { MilestonesStrip } from "./milestones-strip.js";
import { NewMilestoneDialog } from "./new-milestone-dialog.js";
import { EditMilestoneDialog } from "./edit-milestone-dialog.js";
import { ManageReposDialog } from "./project-repos-card.js";
import { parseFilterString, serializeFilterChips } from "./project-filter-chips.js";
import { ProjectWorkbench } from "./project-workbench.js";
import {
  useArchiveProject,
  useAdoptProjectMilestone,
  useBoardLinks,
  useDeleteBoardLink,
  useDeleteProjectMilestone,
  useDeleteProjectView,
  usePatchProject,
  useProject,
  useProjectMilestones,
  useProjectRepos,
  useProjectViews,
  useRemoveIssueFromProject,
  useUpdateProjectMilestone,
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
  const [editDatesOpen, setEditDatesOpen] = useState(false);
  const [editDetailsOpen, setEditDetailsOpen] = useState(false);
  const [deleteAllViewsOpen, setDeleteAllViewsOpen] = useState(false);
  const [bulkDeleteBusy, setBulkDeleteBusy] = useState(false);
  const [bulkDeleteError, setBulkDeleteError] = useState<string | null>(null);
  const [newMilestoneOpen, setNewMilestoneOpen] = useState(false);
  const [editMilestone, setEditMilestone] = useState<MilestoneDto | null>(null);
  const [deleteMilestoneTarget, setDeleteMilestoneTarget] =
    useState<MilestoneDto | null>(null);
  const links = useBoardLinks(project.id);
  const deleteLink = useDeleteBoardLink(project.id);
  const repoLinks = useProjectRepos(project.id);
  const projectViews = useProjectViews(project.id);
  const deleteProjectView = useDeleteProjectView(project.id);
  const milestones = useProjectMilestones(project.id);
  const adoptMilestone = useAdoptProjectMilestone(project.id);
  const updateMilestone = useUpdateProjectMilestone(project.id);
  const deleteMilestone = useDeleteProjectMilestone(project.id);
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

  /** Bulk-delete every saved view on this project. The REST API
   *  has no "delete all" endpoint, so we fan out to the per-view
   *  DELETE in parallel and surface the first error (if any). The
   *  underlying mutation is idempotent so a partial failure can
   *  safely be retried by hitting the menu item again. */
  const onDeleteAllViewsConfirm = async (): Promise<void> => {
    const all = projectViews.data ?? [];
    if (all.length === 0) {
      setDeleteAllViewsOpen(false);
      return;
    }
    setBulkDeleteBusy(true);
    setBulkDeleteError(null);
    try {
      await Promise.all(
        all.map((v) => deleteProjectView.mutateAsync(v.id)),
      );
      setDeleteAllViewsOpen(false);
    } catch (err) {
      setBulkDeleteError(
        err instanceof Error ? err.message : "Failed to delete views",
      );
    } finally {
      setBulkDeleteBusy(false);
    }
  };

  const openIssue = (id: string | null): void => {
    // Preserve the active view + any group/filter/sort overrides
    // when opening/closing the issue detail panel. Without this
    // `projectDetailRoute` strips every query param except `issue`,
    // which silently kicks the user back to the pinned "All" tab
    // and loses their toolbar state.
    navigate(
      projectDetailRouteWithParams(project.id, {
        issueId: id,
        view: projectViewId(route),
        group: projectGroupBy(route),
        filter: projectFilter(route),
        sort: projectSort(route),
      }),
    );
  };

  const orgsQ = useQuery<OrgDto[]>({
    queryKey: ["orgs"],
    queryFn: () => api.listOrgs(),
    staleTime: 60_000,
  });
  const orgLogin = (orgsQ.data ?? []).find((o) => o.id === project.org_id)?.login;

  return (
    <div className="flex gap-4 px-4 lg:px-6" data-testid="project-detail">
    <div className="flex min-w-0 flex-1 flex-col gap-4">
      <PageHeading
        title={
          <span className="flex flex-wrap items-center gap-x-2 gap-y-1">
            {orgLogin ? (
              <a
                href={`https://github.com/${orgLogin}`}
                target="_blank"
                rel="noreferrer"
                className="text-muted-foreground text-base font-normal hover:text-foreground hover:underline"
                title={`Open @${orgLogin} on GitHub`}
              >
                @{orgLogin}
              </a>
            ) : null}
            <span data-testid="project-detail-name">{project.name}</span>
            <Button
              variant="ghost"
              size="icon"
              className="size-6 text-muted-foreground hover:text-foreground"
              title="Edit project name and description"
              data-testid="project-detail-edit-details"
              onClick={() => setEditDetailsOpen(true)}
            >
              <PencilIcon className="size-3.5" />
            </Button>
            <HelpHint
              title="Project page"
              body={[
                "Top tiles: Progress (closed / total), Timeline (start + due, click ✎ to edit), Issues (open / closed), Lead (click to assign), and Linked Surfaces (GitHub boards + repos linked to this project).",
                "Milestones strip: GitHub milestones on the linked repos. Create / Edit / Close / Delete write through to GitHub via the dev-pulse App or a personal access token.",
                "Workbench: tabs are Saved Views (per-user). Toolbar groups, filters and sorts the issue list; use + Add issue to attach work from the Triage queue.",
                "Settings ▾: link / unlink GitHub boards, manage which repos this project draws from, and archive or restore the project.",
              ]}
            />
            <Badge
              variant={STATUS_VARIANT[project.status]}
              data-testid="project-detail-status"
            >
              {STATUS_LABEL[project.status]}
            </Badge>
          </span>
        }
        description={
          <span className="flex flex-col gap-1.5">
            {(repoLinks.data ?? []).length > 0 || (links.data ?? []).length > 0 ? (
              <span className="flex flex-wrap items-center gap-2">
                {(repoLinks.data ?? []).map((r) => (
                  <a
                    key={r.repo_id}
                    href={`https://github.com/${r.repo_org_login}/${r.repo_name}`}
                    target="_blank"
                    rel="noreferrer"
                    className="inline-flex items-center"
                  >
                    <Badge
                      variant="outline"
                      className="border-emerald-300 bg-emerald-50 font-mono text-xs font-normal text-emerald-800 hover:bg-emerald-100 dark:border-emerald-800/60 dark:bg-emerald-950/40 dark:text-emerald-200 dark:hover:bg-emerald-950/60"
                      data-testid="project-detail-repo-tag"
                      title={`Open ${r.repo_org_login}/${r.repo_name} on GitHub`}
                    >
                      {r.repo_name}
                    </Badge>
                  </a>
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
            ) : null}
            {project.description ? (
              <span
                className="line-clamp-2 max-w-3xl break-words text-sm text-muted-foreground"
                title={project.description}
                data-testid="project-detail-description"
              >
                {project.description}
              </span>
            ) : null}
          </span>
        }
        trailing={
          <div className="flex items-center gap-2">
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="outline"
                  size="icon"
                  className="size-8"
                  aria-label="Project settings"
                  title="Settings"
                >
                  <SettingsIcon className="size-4" />
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
                  onSelect={(e) => {
                    e.preventDefault();
                    setEditDetailsOpen(true);
                  }}
                  data-testid="project-settings-edit-details"
                >
                  Edit details…
                </DropdownMenuItem>
                <DropdownMenuItem
                  disabled={(projectViews.data?.length ?? 0) === 0}
                  onSelect={(e) => {
                    e.preventDefault();
                    setBulkDeleteError(null);
                    setDeleteAllViewsOpen(true);
                  }}
                  data-testid="project-settings-delete-all-views"
                  className="text-destructive focus:text-destructive"
                >
                  Delete all views…
                  {projectViews.data && projectViews.data.length > 0 ? (
                    <span className="ml-auto text-[10px] text-muted-foreground">
                      {projectViews.data.length}
                    </span>
                  ) : null}
                </DropdownMenuItem>
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

      <ProjectKpiGrid
        project={project}
        boardCount={links.data?.length ?? project.board_link_count}
        repoCount={repoLinks.data?.length ?? 0}
        onEditTimeline={() => setEditDatesOpen(true)}
      />

      {/* Page-level "no linked repos" warning — same intent as the
       *  in-dialog alert on NewMilestoneDialog, but surfaced up
       *  front so the user notices the missing prerequisite before
       *  they try to create a milestone or rely on repo-derived
       *  data. Hidden once at least one repo is linked. */}
      {!repoLinks.isPending && (repoLinks.data?.length ?? 0) === 0 && (
        <Alert
          variant="destructive"
          data-testid="project-no-repos-warning"
        >
          <AlertTitle>No linked repos</AlertTitle>
          <AlertDescription className="flex flex-col items-start gap-2">
            <span>
              Link a repo to this project so dev-pulse can mirror
              issues, milestones, and board status from GitHub.
              Until then milestones and most surfaces stay empty.
            </span>
            <Button
              size="sm"
              variant="outline"
              onClick={() => setManageReposOpen(true)}
              data-testid="project-no-repos-link"
            >
              Link a repo…
            </Button>
          </AlertDescription>
        </Alert>
      )}

      <MilestonesStrip
        milestones={milestones.data ?? []}
        primaryMilestoneId={project.primary_milestone_id ?? null}
        onAdoptPrimary={(mid) => adoptMilestone.mutate(mid)}
        onFilterToMilestone={(mid) => {
          // Append/replace the milestone chip on the current
          // workbench filter. One milestone chip max: dropping
          // any pre-existing milestone:* keeps the URL idempotent
          // (clicking twice on different cards swaps the chip
          // rather than stacking unsatisfiable AND-clauses).
          const current = parseFilterString(projectFilter(route));
          const next = current.filter((c) => c.dim !== "milestone");
          next.push({ dim: "milestone", value: mid });
          const serialised = serializeFilterChips(next);
          navigate(
            projectDetailRouteWithParams(project.id, {
              issueId: selectedIssueId,
              view: projectViewId(route),
              group: projectGroupBy(route),
              filter: serialised.length > 0 ? serialised : null,
              sort: projectSort(route),
            }),
          );
        }}
        adoptBusy={adoptMilestone.isPending}
        isPending={milestones.isPending}
        onCreateMilestone={() => setNewMilestoneOpen(true)}
        onEditMilestone={(m) => setEditMilestone(m)}
        onToggleMilestoneState={(m) =>
          updateMilestone.mutate({
            milestoneId: m.id,
            body: { state: m.state === "closed" ? "open" : "closed" },
          })
        }
        onDeleteMilestone={(m) => setDeleteMilestoneTarget(m)}
        writeBusy={updateMilestone.isPending || deleteMilestone.isPending}
      />

      <ProjectWorkbench
        project={project}
        selectedIssueId={selectedIssueId}
        onSelectIssue={openIssue}
        renderRow={(row, selected) => (
          <ProjectIssueRowWired
            key={row.id}
            project={project}
            row={row}
            selected={selected}
            onSelect={() => openIssue(row.id)}
            activeViewId={projectViewId(route)}
          />
        )}
      />

      <LinkBoardDialog
        open={linkBoardOpen}
        onOpenChange={setLinkBoardOpen}
        projectId={project.id}
        projectOrgId={project.org_id}
      />

      <NewMilestoneDialog
        open={newMilestoneOpen}
        onOpenChange={setNewMilestoneOpen}
        projectId={project.id}
        onRequestLinkRepo={() => setManageReposOpen(true)}
      />

      <EditMilestoneDialog
        open={editMilestone !== null}
        onOpenChange={(open) => {
          if (!open) setEditMilestone(null);
        }}
        projectId={project.id}
        milestone={editMilestone}
      />

      <AlertDialog
        open={deleteMilestoneTarget !== null}
        onOpenChange={(open) => {
          if (!open) setDeleteMilestoneTarget(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete milestone?</AlertDialogTitle>
            <AlertDialogDescription>
              This deletes <strong>{deleteMilestoneTarget?.title}</strong>{" "}
              on GitHub and removes the local mirror. Issues currently
              attached to the milestone remain — only the milestone
              itself is removed. This cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deleteMilestone.isPending}>
              Cancel
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                const target = deleteMilestoneTarget;
                if (!target) return;
                deleteMilestone.mutate(target.id, {
                  onSuccess: () => setDeleteMilestoneTarget(null),
                });
              }}
              disabled={deleteMilestone.isPending}
              data-testid="project-milestone-delete-confirm"
            >
              {deleteMilestone.isPending ? "Deleting…" : "Delete"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <ManageReposDialog
        open={manageReposOpen}
        onOpenChange={setManageReposOpen}
        projectId={project.id}
        projectOrgId={project.org_id}
      />

      <EditDatesDialog
        open={editDatesOpen}
        onOpenChange={setEditDatesOpen}
        project={project}
      />

      <EditDetailsDialog
        open={editDetailsOpen}
        onOpenChange={setEditDetailsOpen}
        project={project}
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

      <AlertDialog
        open={deleteAllViewsOpen}
        onOpenChange={(open) => {
          if (bulkDeleteBusy) return;
          setDeleteAllViewsOpen(open);
          if (!open) setBulkDeleteError(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              Delete all saved views?
            </AlertDialogTitle>
            <AlertDialogDescription>
              {`This will permanently remove ${projectViews.data?.length ?? 0} saved view${
                (projectViews.data?.length ?? 0) === 1 ? "" : "s"
              } on "${project.name}". The project's issues and linked surfaces are unaffected. This can't be undone.`}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={bulkDeleteBusy}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={(e) => {
                e.preventDefault();
                void onDeleteAllViewsConfirm();
              }}
              disabled={bulkDeleteBusy}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
              data-testid="project-settings-delete-all-views-confirm"
            >
              {bulkDeleteBusy ? "Deleting…" : "Delete all"}
            </AlertDialogAction>
          </AlertDialogFooter>
          {bulkDeleteError && (
            <p className="text-sm text-destructive" data-testid="project-settings-delete-all-views-error">
              {bulkDeleteError}
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

function fmtDate(s: string | null | undefined): string {
  if (!s) return "—";
  return new Date(s).toLocaleDateString("en-AU");
}

// ---------------------------------------------------------------------------
// §6.3 KPI grid — replaces the old plain "Meta" dl with four
// at-a-glance tiles: progress (with bar), timeline (with relative
// due pill), issue mix (open vs closed), and linked surfaces
// (boards + repos). Computed entirely from the data already on
// the page — no extra round trips.
// ---------------------------------------------------------------------------

function ProjectKpiGrid({
  project,
  boardCount,
  repoCount,
  onEditTimeline,
}: {
  project: ProjectDto;
  boardCount: number;
  repoCount: number;
  onEditTimeline: () => void;
}): JSX.Element {
  const total = project.issue_count;
  const closed = project.closed_issue_count;
  const open = Math.max(0, total - closed);
  const pct = total > 0 ? Math.round((closed / total) * 100) : 0;
  const complete = total > 0 && closed === total;
  const due = relativeDue(project.due_at);

  return (
    <div
      className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-5"
      data-testid="project-detail-kpis"
    >
      <KpiTile
        label="Progress"
        icon={
          complete ? (
            <CheckCircle2Icon className="h-4 w-4 text-emerald-600" />
          ) : (
            <CircleDotIcon className="h-4 w-4 text-muted-foreground" />
          )
        }
      >
        <div className="flex items-baseline gap-2">
          <span
            className={cn(
              "text-2xl font-semibold tabular-nums",
              complete && "text-emerald-600",
            )}
            data-testid="project-detail-progress"
          >
            {pct}%
          </span>
          <span className="text-xs text-muted-foreground">
            {closed} / {total} closed
          </span>
        </div>
        <Progress
          value={pct}
          className={cn(
            "mt-3 h-1.5",
            complete && "[&>[data-slot=progress-indicator]]:bg-emerald-500",
          )}
        />
      </KpiTile>

      <KpiTile
        label="Timeline"
        icon={
          <button
            type="button"
            onClick={onEditTimeline}
            className="inline-flex h-6 w-6 items-center justify-center rounded text-muted-foreground hover:bg-muted hover:text-foreground"
            aria-label="Edit start and due dates"
            data-testid="project-detail-edit-dates"
          >
            <PencilIcon className="h-3.5 w-3.5" />
          </button>
        }
      >
        <div className="flex flex-col gap-1">
          <span className="text-2xl font-semibold tabular-nums">
            {fmtDate(project.due_at)}
          </span>
          <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
            <span>Started {fmtDate(project.start_at)}</span>
            {due && (
              <Badge
                variant="outline"
                className={cn(
                  "h-5 px-1.5 text-[10px] font-medium",
                  due.tone === "overdue" &&
                    "border-red-300 bg-red-50 text-red-700 dark:border-red-900/60 dark:bg-red-950/40 dark:text-red-300",
                  due.tone === "soon" &&
                    "border-amber-300 bg-amber-50 text-amber-700 dark:border-amber-900/60 dark:bg-amber-950/40 dark:text-amber-300",
                  due.tone === "ok" &&
                    "border-emerald-300 bg-emerald-50 text-emerald-700 dark:border-emerald-900/60 dark:bg-emerald-950/40 dark:text-emerald-300",
                )}
              >
                {due.label}
              </Badge>
            )}
          </div>
        </div>
      </KpiTile>

      <KpiTile
        label="Issues"
        icon={<CircleDotIcon className="h-4 w-4 text-muted-foreground" />}
      >
        <div className="flex items-baseline gap-2">
          <span className="text-2xl font-semibold tabular-nums">{open}</span>
          <span className="text-xs text-muted-foreground">open</span>
        </div>
        <div
          className="mt-2 flex h-1.5 w-full overflow-hidden rounded-full bg-muted"
          aria-hidden
        >
          {total > 0 && (
            <>
              <div
                className="bg-emerald-500"
                style={{ width: `${(closed / total) * 100}%` }}
                title={`${closed} closed`}
              />
              <div
                className="bg-sky-500"
                style={{ width: `${(open / total) * 100}%` }}
                title={`${open} open`}
              />
            </>
          )}
        </div>
        <div className="mt-1 flex justify-between text-[10px] text-muted-foreground">
          <span>{closed} closed</span>
          <span>{total} total</span>
        </div>
      </KpiTile>

      <LeadKpiTile project={project} />

      <KpiTile
        label="Linked surfaces"
        icon={<LinkIcon className="h-4 w-4 text-muted-foreground" />}
      >
        <div className="flex items-baseline gap-4">
          <div className="flex items-baseline gap-1.5">
            <span
              className="text-2xl font-semibold tabular-nums"
              data-testid="project-detail-board-count"
            >
              {boardCount}
            </span>
            <span className="text-xs text-muted-foreground">
              {boardCount === 1 ? "board" : "boards"}
            </span>
          </div>
          <div className="flex items-baseline gap-1.5">
            <span className="text-2xl font-semibold tabular-nums">
              {repoCount}
            </span>
            <span className="text-xs text-muted-foreground">
              {repoCount === 1 ? "repo" : "repos"}
            </span>
          </div>
        </div>
      </KpiTile>
    </div>
  );
}

function KpiTile({
  label,
  icon,
  children,
}: {
  label: string;
  icon: ReactNode;
  children: ReactNode;
}): JSX.Element {
  return (
    <Card className="gap-2 py-4">
      <CardHeader className="px-4">
        <div className="flex items-center justify-between">
          <CardTitle className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
            {label}
          </CardTitle>
          {icon}
        </div>
      </CardHeader>
      <CardContent className="px-4">{children}</CardContent>
    </Card>
  );
}

/**
 * `<LeadKpiTile>` — clickable Lead pill on the project KPI grid.
 *
 * Opens a popover with a `UserPicker` scoped to the project's
 * org. Setting picks a `dp_users.id`; the "— Unassigned —"
 * sentinel clears `lead_user_id`. Writes go through the same
 * §8.2 CAS PATCH path as every other project mutation so a
 * concurrent edit can't silently win.
 */
function LeadKpiTile({ project }: { project: ProjectDto }): JSX.Element {
  const patch = usePatchProject(project.id);
  const [open, setOpen] = useState(false);
  // Resolve the current lead_user_id → display name from the
  // org-scoped member list. Reuses the same `["users", orgId]`
  // cache key as `UserPicker` so opening the popover doesn't
  // re-fetch.
  const usersQuery = useQuery({
    queryKey: ["users", project.org_id],
    queryFn: () => api.listUsers(project.org_id),
    staleTime: 60_000,
  });
  const leadUser = project.lead_user_id
    ? usersQuery.data?.find((u) => u.id === project.lead_user_id)
    : undefined;
  const leadLabel = project.lead_user_id
    ? (leadUser
        ? (leadUser.name && leadUser.name.trim().length > 0
            ? leadUser.name
            : leadUser.login)
        : "…")
    : "Unassigned";
  const leadHint = leadUser ? `@${leadUser.login}` : null;

  const handleChange = (next: string | null): void => {
    if (next === (project.lead_user_id ?? null)) {
      setOpen(false);
      return;
    }
    patch.mutate(
      {
        expected_version: project.version,
        // `null` clears, UUID assigns. Wrapping in an extra
        // `Some(...)` layer is the wire convention for "this
        // field is intentionally being written" vs left alone.
        lead_user_id: next,
      },
      { onSuccess: () => setOpen(false) },
    );
  };

  return (
    <Card className="gap-2 py-4">
      <CardHeader className="px-4">
        <div className="flex items-center justify-between">
          <CardTitle className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
            Lead
          </CardTitle>
          <UserIcon className="h-4 w-4 text-muted-foreground" />
        </div>
      </CardHeader>
      <CardContent className="px-4">
        <PopoverPrimitive.Root open={open} onOpenChange={setOpen}>
          <PopoverPrimitive.Trigger
            type="button"
            disabled={patch.isPending}
            data-testid="project-detail-edit-lead"
            aria-label="Set project lead"
            className={cn(
              "flex w-full items-center gap-2 rounded-md px-1 py-1 text-left outline-none",
              "hover:bg-accent focus-visible:bg-accent",
              "disabled:opacity-50",
            )}
          >
            <span className="flex min-w-0 flex-1 flex-col">
              <span
                className={cn(
                  "truncate text-lg font-semibold",
                  !project.lead_user_id && "text-muted-foreground",
                )}
                data-testid="project-detail-lead-label"
              >
                {leadLabel}
              </span>
              {leadHint && (
                <span className="truncate text-xs text-muted-foreground">
                  {leadHint}
                </span>
              )}
            </span>
            <PencilIcon className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
          </PopoverPrimitive.Trigger>
          <PopoverPrimitive.Portal>
            <PopoverPrimitive.Content
              align="start"
              sideOffset={4}
              className={cn(
                "z-50 w-[20rem] rounded-md border bg-popover p-3 text-popover-foreground shadow-md",
              )}
            >
              <div className="mb-2 text-xs font-medium text-muted-foreground">
                Project lead
              </div>
              <UserPicker
                orgId={project.org_id}
                value={project.lead_user_id ?? null}
                onChange={handleChange}
                disabled={patch.isPending}
                data-testid="project-detail-lead-picker"
              />
              {patch.isError && (
                <p
                  className="mt-2 text-xs text-destructive"
                  data-testid="project-detail-lead-error"
                >
                  {patch.error.message}
                </p>
              )}
            </PopoverPrimitive.Content>
          </PopoverPrimitive.Portal>
        </PopoverPrimitive.Root>
      </CardContent>
    </Card>
  );
}

/** Renders a project's due_at as a coloured pill: "due in 6d",
 *  "due today", "5d overdue", or `null` when no date is set. */
function relativeDue(
  due: string | null | undefined,
): { label: string; tone: "ok" | "soon" | "overdue" } | null {
  if (!due) return null;
  const target = new Date(due).getTime();
  if (Number.isNaN(target)) return null;
  const now = Date.now();
  const oneDay = 86_400_000;
  const days = Math.round((target - now) / oneDay);
  if (days < 0) {
    return { label: `${Math.abs(days)}d overdue`, tone: "overdue" };
  }
  if (days === 0) {
    return { label: "due today", tone: "soon" };
  }
  if (days <= 7) {
    return { label: `due in ${days}d`, tone: "soon" };
  }
  return { label: `due in ${days}d`, tone: "ok" };
}

// ---------------------------------------------------------------------------
// §6.3 issue row — the dense card shape rendered by both the flat
// list and the §5.1 sectioned views (PROJECT-VIEW.md). The
// workbench owns layout + grouping; this row owns its remove
// affordance + the click-through to the detail pane.
// ---------------------------------------------------------------------------

function ProjectIssueRowWired({
  project,
  row,
  selected,
  onSelect,
  activeViewId,
}: {
  project: ProjectDto;
  row: IssueListItem;
  selected: boolean;
  onSelect: () => void;
  /** When set, Remove scopes the detach to the active saved-view
   *  tab's membership only (issue stays on the project). When
   *  null/undefined (the "All" tab), Remove detaches from the
   *  project itself — the historical behaviour. */
  activeViewId?: string | null;
}): JSX.Element {
  const remove = useRemoveIssueFromProject(project.id);
  return (
    <ProjectIssueRow
      row={row}
      selected={selected}
      onSelect={onSelect}
      onRemove={() =>
        remove.mutate({
          issueId: row.id,
          expectedVersion: activeViewId ? null : project.version,
          viewId: activeViewId ?? undefined,
        })
      }
      removePending={remove.isPending}
    />
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
  const isLocal = row.is_local === true;
  return (
    <div
      className={`flex cursor-pointer items-center gap-3 rounded-md border px-3 py-2 text-sm ${
        selected
          ? "border-primary bg-accent/40"
          : isLocal
            // Local-only rows get a warm amber tint and a dashed
            // border so they read as "note, not a GitHub issue"
            // at a glance — distinct from the muted bg used for
            // GitHub-backed rows.
            ? "border-dashed border-amber-500/50 bg-amber-500/5 hover:bg-amber-500/10"
            : "border-border bg-muted/10 hover:bg-accent/20"
      }`}
      data-testid="project-issue-row"
      data-local={isLocal ? "true" : undefined}
      onClick={onSelect}
    >
      <Badge
        variant={row.state === "open" ? "default" : "secondary"}
        className="shrink-0 px-1.5 py-0 text-[10px] uppercase"
      >
        {row.state}
      </Badge>
      {isLocal ? (
        // Local-only rows: no repo badge, no GitHub deep-link —
        // there is no GitHub-side issue to link to.
        <Badge
          variant="outline"
          className="shrink-0 border-amber-500/60 px-1.5 py-0 text-[10px] uppercase text-amber-700 dark:text-amber-300"
          title="Local-only note (not synced to GitHub)"
        >
          local
        </Badge>
      ) : (
        <span className="shrink-0 font-mono text-xs text-muted-foreground tabular-nums">
          {row.repo_slug ? (
            <a
              href={`https://github.com/${row.repo_slug}/issues/${row.number}`}
              target="_blank"
              rel="noreferrer"
              className="hover:text-foreground hover:underline"
              onClick={(e) => e.stopPropagation()}
              title={`Open ${row.repo_slug}#${row.number} on GitHub`}
            >
              {row.repo_slug}#{row.number}
            </a>
          ) : (
            <>—#{row.number}</>
          )}
        </span>
      )}
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

// ---------------------------------------------------------------------------
// Edit-dates dialog — small inline editor for the Timeline KPI
// tile. Mirrors the new-project dialog's date handling
// (`<input type="date">` ⇒ `YYYY-MM-DDT00:00:00Z`). Empty input
// clears the field server-side (`null`). CAS via `expected_version`.
// ---------------------------------------------------------------------------

function EditDatesDialog({
  open,
  onOpenChange,
  project,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  project: ProjectDto;
}): JSX.Element {
  const patch = usePatchProject(project.id);
  const [startAt, setStartAt] = useState("");
  const [dueAt, setDueAt] = useState("");

  // Seed the inputs whenever the dialog re-opens — picking up the
  // latest server values rather than whatever was typed last time.
  useEffect(() => {
    if (!open) {
      patch.reset();
      return;
    }
    setStartAt(project.start_at ? project.start_at.slice(0, 10) : "");
    setDueAt(project.due_at ? project.due_at.slice(0, 10) : "");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, project.start_at, project.due_at]);

  const toIso = (v: string): string | null =>
    v ? `${v}T00:00:00Z` : null;

  const onSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    patch.mutate(
      {
        expected_version: project.version,
        start_at: toIso(startAt),
        due_at: toIso(dueAt),
      },
      { onSuccess: () => onOpenChange(false) },
    );
  };

  const rangeError =
    startAt && dueAt && new Date(startAt) > new Date(dueAt)
      ? "Start must be on or before Due."
      : null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="sm:max-w-md"
        data-testid="edit-dates-dialog"
      >
        <DialogHeader>
          <DialogTitle>Edit timeline</DialogTitle>
          <DialogDescription>
            Set the project's start and due dates. Leave a field blank
            to clear it.
          </DialogDescription>
        </DialogHeader>

        <form className="flex flex-col gap-4" onSubmit={onSubmit}>
          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-2">
              <Label htmlFor="edit-dates-start">Start</Label>
              <DateInput
                id="edit-dates-start"
                data-testid="edit-dates-start"
                value={startAt}
                onChange={(e) => setStartAt(e.target.value)}
              />
            </div>
            <div className="flex flex-col gap-2">
              <Label htmlFor="edit-dates-due">Due</Label>
              <DateInput
                id="edit-dates-due"
                data-testid="edit-dates-due"
                value={dueAt}
                onChange={(e) => setDueAt(e.target.value)}
              />
            </div>
          </div>

          {rangeError && (
            <p className="text-xs text-destructive">{rangeError}</p>
          )}

          {patch.isError && (
            <Alert variant="destructive" data-testid="edit-dates-error">
              <AlertTitle>Save failed</AlertTitle>
              <AlertDescription>{patch.error.message}</AlertDescription>
            </Alert>
          )}

          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={() => onOpenChange(false)}
              disabled={patch.isPending}
            >
              Cancel
            </Button>
            <Button
              type="submit"
              data-testid="edit-dates-submit"
              disabled={patch.isPending || rangeError !== null}
            >
              {patch.isPending ? "Saving…" : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/**
 * Edit the project's name and description. Sends `PATCH
 * /projects/{id}` under §8.2 CAS — only fields the user actually
 * changed go on the wire so an unrelated concurrent edit to the
 * lead / dates / status won't be clobbered. A 409 stale-version
 * surfaces as the standard mutation error here; the user can
 * close and reopen the dialog to pick up the new row.
 */
function EditDetailsDialog({
  open,
  onOpenChange,
  project,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  project: ProjectDto;
}): JSX.Element {
  const patch = usePatchProject(project.id);
  const [name, setName] = useState(project.name);
  const [description, setDescription] = useState(project.description ?? "");
  const [leadUserId, setLeadUserId] = useState<string | null>(
    project.lead_user_id ?? null,
  );

  useEffect(() => {
    if (!open) {
      patch.reset();
      return;
    }
    setName(project.name);
    setDescription(project.description ?? "");
    setLeadUserId(project.lead_user_id ?? null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, project.name, project.description, project.lead_user_id]);

  const trimmedName = name.trim();
  const nameError = trimmedName.length === 0 ? "Name is required." : null;

  const onSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    if (nameError) return;
    const body: PatchProjectRequest = { expected_version: project.version };
    if (trimmedName !== project.name) body.name = trimmedName;
    const trimmedDesc = description.trim();
    const currentDesc = project.description ?? "";
    if (trimmedDesc !== currentDesc) {
      // `null` clears the column; empty string would round-trip
      // an empty description (server treats both as "no value"
      // but `null` is the documented clear-form).
      body.description = trimmedDesc.length === 0 ? null : trimmedDesc;
    }
    const currentLead = project.lead_user_id ?? null;
    if (leadUserId !== currentLead) {
      body.lead_user_id = leadUserId;
    }
    // Nothing changed — just close.
    if (
      body.name === undefined &&
      body.description === undefined &&
      body.lead_user_id === undefined
    ) {
      onOpenChange(false);
      return;
    }
    patch.mutate(body, { onSuccess: () => onOpenChange(false) });
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        // Hard-cap via inline style so a runaway grid child (long
        // textarea token) cannot push the dialog wider than its
        // `sm:max-w-lg` cap. `overflow-hidden` + `min-w-0` is the
        // belt-and-braces for the grid layout the upstream
        // `DialogContent` uses.
        style={{ maxWidth: "32rem" }}
        className="sm:max-w-lg min-w-0 overflow-hidden"
        data-testid="edit-details-dialog"
      >
        <DialogHeader>
          <DialogTitle>Edit project details</DialogTitle>
          <DialogDescription>
            Update the project's name and description. Changes save
            through the §8.2 CAS path — a concurrent edit elsewhere
            surfaces as a stale-version error you can retry.
          </DialogDescription>
        </DialogHeader>

        <form className="flex min-w-0 flex-col gap-4" onSubmit={onSubmit}>
          <div className="flex flex-col gap-2">
            <Label htmlFor="edit-details-name">Name</Label>
            <Input
              id="edit-details-name"
              data-testid="edit-details-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              autoFocus
              maxLength={200}
            />
            {nameError && (
              <p className="text-xs text-destructive">{nameError}</p>
            )}
          </div>

          <div className="flex min-w-0 flex-col gap-2">
            <Label htmlFor="edit-details-description">Description</Label>
            <Textarea
              id="edit-details-description"
              data-testid="edit-details-description"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              rows={5}
              placeholder="What is this project for?"
              // The shadcn `Textarea` ships `field-sizing-content`
              // (auto-grows to fit content) + lives inside the
              // `grid`-laid `DialogContent`, whose grid items
              // default to `min-width: auto`. A single long
              // unbreakable token would push the form past the
              // dialog's `sm:max-w-lg` cap. Inline style wins over
              // the upstream utility class regardless of tailwind-
              // merge ordering; `wrap-anywhere` + `w-full` ensures
              // the textarea itself cannot exceed its parent.
              style={{ fieldSizing: "fixed" } as React.CSSProperties}
              className="block w-full max-w-full resize-y overflow-auto break-all [overflow-wrap:anywhere]"
            />
            <p className="text-xs text-muted-foreground">
              Plain text. Leave blank to clear.
            </p>
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="edit-details-lead">Lead</Label>
            <UserPicker
              id="edit-details-lead"
              data-testid="edit-details-lead"
              orgId={project.org_id}
              value={leadUserId}
              onChange={setLeadUserId}
              placeholder="Unassigned"
            />
            <p className="text-xs text-muted-foreground">
              The GitHub user accountable for this project. Pick
              from members of {project.org_id ? "this org" : "the org"}.
            </p>
          </div>

          {patch.isError && (
            <Alert variant="destructive" data-testid="edit-details-error">
              <AlertTitle>Save failed</AlertTitle>
              <AlertDescription>{patch.error.message}</AlertDescription>
            </Alert>
          )}

          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={() => onOpenChange(false)}
              disabled={patch.isPending}
            >
              Cancel
            </Button>
            <Button
              type="submit"
              data-testid="edit-details-submit"
              disabled={patch.isPending || nameError !== null}
            >
              {patch.isPending ? "Saving…" : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

