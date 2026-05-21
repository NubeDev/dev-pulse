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

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Spinner } from "@/components/ui/spinner";

import type { BoardLinkDto, IssueListItem, ProjectDto } from "../api/client.js";
import { PageHeading } from "../components/page-heading.jsx";
import { navigate, projectDetailRoute, projectSelectedIssue, useRoute } from "../routes.js";
import { IssueEditCard } from "../workflow/issues-page.js";

import { LinkBoardDialog } from "./link-board-dialog.js";
import { AddIssuesDialog } from "./add-issues-dialog.js";
import { ProjectRowActions } from "./project-row-actions.js";
import { ProjectReposCard } from "./project-repos-card.js";
import {
  useBoardLinks,
  useDeleteBoardLink,
  useProject,
  useProjectIssues,
  useRemoveIssueFromProject,
} from "./use-projects-data.js";

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
  const links = useBoardLinks(project.id);
  const deleteLink = useDeleteBoardLink(project.id);

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
          <span className="flex items-center gap-3">
            <span data-testid="project-detail-name">{project.name}</span>
            <Badge
              variant={STATUS_VARIANT[project.status]}
              data-testid="project-detail-status"
            >
              {STATUS_LABEL[project.status]}
            </Badge>
          </span>
        }
        description={project.description ?? undefined}
        trailing={<ProjectRowActions project={project} />}
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

      <BoardLinksCard
        projectId={project.id}
        projectOrgId={project.org_id}
        links={links.data ?? []}
        isLoading={links.isPending}
        onUnlink={(linkId) => deleteLink.mutate(linkId)}
        unlinkPending={deleteLink.isPending}
        unlinkError={deleteLink.error?.message ?? null}
      />

      <ProjectReposCard
        projectId={project.id}
        projectOrgId={project.org_id}
      />

      <ProjectIssuesCard project={project} onSelectIssue={openIssue} selectedIssueId={selectedIssueId} />
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

/** §6.3 `Linked GitHub boards` section. Each row carries the
 *  cached board title + the `last_mirror_at` / `last_mirror_error`
 *  aggregate the §7.3 GET surfaces, so the row reads as either:
 *
 *    NubeIO / Rubix Roadmap   ✓ Last sync 14:23:07
 *
 *  or, on a failed mirror:
 *
 *    NubeIO / Rubix Roadmap   ✕ Last sync failed — <message>   [Re-link]
 */
function BoardLinksCard({
  projectId,
  projectOrgId,
  links,
  isLoading,
  onUnlink,
  unlinkPending,
  unlinkError,
}: {
  projectId: string;
  projectOrgId: string;
  links: BoardLinkDto[];
  isLoading: boolean;
  onUnlink: (linkId: string) => void;
  unlinkPending: boolean;
  unlinkError: string | null;
}): JSX.Element {
  const [dialogOpen, setDialogOpen] = useState(false);

  return (
    <Card data-testid="project-board-links">
      <CardHeader className="flex flex-row items-center justify-between">
        <CardTitle className="text-base">Linked GitHub boards</CardTitle>
        <Button
          size="sm"
          onClick={() => setDialogOpen(true)}
          data-testid="project-link-board-button"
        >
          + Link a GitHub board…
        </Button>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        {isLoading && (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Spinner /> Loading links…
          </div>
        )}

        {!isLoading && links.length === 0 && (
          <p
            className="text-sm text-muted-foreground"
            data-testid="project-board-links-empty"
          >
            No GitHub boards linked yet. Linking a board mirrors
            issue Start / Due dates to its date fields whenever a
            project member edits them in dev-pulse.
          </p>
        )}

        {!isLoading &&
          links.map((link) => (
            <BoardLinkRow
              key={link.id}
              link={link}
              onUnlink={() => onUnlink(link.id)}
              unlinkPending={unlinkPending}
            />
          ))}

        {unlinkError && (
          <Alert
            variant="destructive"
            data-testid="project-board-link-error"
          >
            <AlertTitle>Unlink failed</AlertTitle>
            <AlertDescription>{unlinkError}</AlertDescription>
          </Alert>
        )}
      </CardContent>

      <LinkBoardDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        projectId={projectId}
        projectOrgId={projectOrgId}
      />
    </Card>
  );
}

function BoardLinkRow({
  link,
  onUnlink,
  unlinkPending,
}: {
  link: BoardLinkDto;
  onUnlink: () => void;
  unlinkPending: boolean;
}): JSX.Element {
  const title = link.github_board_title ?? "Untitled board";
  const sync = renderMirrorStatus(link);
  return (
    <div
      className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-border bg-muted/20 p-3"
      data-testid="project-board-link-row"
    >
      <div className="flex flex-col gap-0.5">
        <div className="flex items-center gap-2">
          {link.github_board_url ? (
            <a
              href={link.github_board_url}
              target="_blank"
              rel="noreferrer"
              className="text-sm font-medium underline-offset-4 hover:underline"
              data-testid="project-board-link-title"
            >
              {title}
            </a>
          ) : (
            <span className="text-sm font-medium" data-testid="project-board-link-title">
              {title}
            </span>
          )}
          {sync.badge}
        </div>
        <span
          className={`text-xs ${sync.ok ? "text-muted-foreground" : "text-destructive"}`}
          data-testid="project-board-link-sync"
        >
          {sync.line}
        </span>
      </div>
      <Button
        variant="ghost"
        size="sm"
        onClick={onUnlink}
        disabled={unlinkPending}
        data-testid="project-board-link-unlink"
      >
        {unlinkPending ? "Unlinking…" : "Unlink"}
      </Button>
    </div>
  );
}

/** §6.4 mirror-status mock: `✓ Last sync 14:23:07` on the latest
 *  success, `✕ Last sync failed — <message>` on the latest
 *  failure, `· Not yet synced` when neither timestamp is set
 *  (e.g. just-linked board with no edits yet). */
function renderMirrorStatus(link: BoardLinkDto): {
  ok: boolean;
  badge: JSX.Element;
  line: string;
} {
  if (link.last_mirror_error) {
    return {
      ok: false,
      badge: (
        <Badge variant="destructive" className="text-[10px]">
          ✕ failed
        </Badge>
      ),
      line: `Last sync failed — ${link.last_mirror_error}`,
    };
  }
  if (link.last_mirror_at) {
    const t = new Date(link.last_mirror_at);
    return {
      ok: true,
      badge: (
        <Badge variant="secondary" className="text-[10px]">
          ✓ synced
        </Badge>
      ),
      line: `Last sync ${t.toLocaleTimeString()}`,
    };
  }
  return {
    ok: true,
    badge: (
      <Badge variant="outline" className="text-[10px]">
        · pending
      </Badge>
    ),
    line: "Not yet synced — edit a date in dev-pulse to fire the first mirror.",
  };
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
