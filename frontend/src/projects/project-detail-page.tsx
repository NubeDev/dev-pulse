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
} from "lucide-react";

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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
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

import type { IssueListItem, ProjectDto } from "../api/client.js";
import { PageHeading } from "../components/page-heading.jsx";
import { navigate, projectDetailRoute, projectDetailRouteWithParams, projectFilter, projectGroupBy, projectSelectedIssue, projectSort, projectViewId, useRoute } from "../routes.js";
import { IssueEditCard } from "../workflow/issues-page.js";

import { LinkBoardDialog } from "./link-board-dialog.js";
import { MilestonesStrip } from "./milestones-strip.js";
import { ManageReposDialog } from "./project-repos-card.js";
import { parseFilterString, serializeFilterChips } from "./project-filter-chips.js";
import { ProjectWorkbench } from "./project-workbench.js";
import {
  useArchiveProject,
  useAdoptProjectMilestone,
  useBoardLinks,
  useDeleteBoardLink,
  usePatchProject,
  useProject,
  useProjectMilestones,
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
  const [editDatesOpen, setEditDatesOpen] = useState(false);
  const links = useBoardLinks(project.id);
  const deleteLink = useDeleteBoardLink(project.id);
  const repoLinks = useProjectRepos(project.id);
  const milestones = useProjectMilestones(project.id);
  const adoptMilestone = useAdoptProjectMilestone(project.id);
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

      <ProjectKpiGrid
        project={project}
        boardCount={links.data?.length ?? project.board_link_count}
        repoCount={repoLinks.data?.length ?? 0}
        onEditTimeline={() => setEditDatesOpen(true)}
      />

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
      className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-4"
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
              <Input
                id="edit-dates-start"
                data-testid="edit-dates-start"
                type="date"
                value={startAt}
                onChange={(e) => setStartAt(e.target.value)}
              />
            </div>
            <div className="flex flex-col gap-2">
              <Label htmlFor="edit-dates-due">Due</Label>
              <Input
                id="edit-dates-due"
                data-testid="edit-dates-due"
                type="date"
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

