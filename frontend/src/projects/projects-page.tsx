/**
 * §6.2 Projects list page.
 *
 * Surfaces the per-org projects roster:
 *
 *   - Status filter sourced from `projectsStatusOf(route)` so
 *     `#/projects?status=active` round-trips through copy-paste
 *     and the §6.1 sidebar deep links land on the right view.
 *   - Search bar (case-insensitive substring on name) wired to
 *     the `q` query param.
 *   - Status-grouped rows (Active / Backlog / Done / Archived)
 *     when "All" is selected; a single flat table when a status
 *     filter is in effect.
 *   - `[+ New project]` button mounts `NewProjectDialog`; on
 *     success the page navigates to the new project's detail
 *     route so the user can start adding issues immediately.
 *
 * Backed by `useProjectList` (`GET /projects?status=&q=&limit=`).
 * For "All" we issue a single high-cap fetch and group client-side
 * — the §6 sidebar caps make a multi-hundred row list unlikely;
 * a follow-up can per-status fetch if the n is ever large enough
 * to matter.
 */

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Progress } from "@/components/ui/progress";
import { Spinner } from "@/components/ui/spinner";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

import { api, type OrgDto, type ProjectDto, type ProjectStatusDto } from "../api/client.js";
import { PageHeading } from "../components/page-heading.jsx";
import { ProjectRowActions } from "./project-row-actions.js";
import {
  navigate,
  projectDetailRoute,
  projectsStatusOf,
  useRoute,
} from "../routes.js";

import { NewProjectDialog } from "./new-project-dialog.js";
import { useProjectList } from "./use-projects-data.js";

const STATUS_ORDER: ProjectStatusDto[] = [
  "active",
  "backlog",
  "done",
  "archived",
];

const STATUS_LABEL: Record<ProjectStatusDto, string> = {
  active: "Active",
  backlog: "Backlog",
  done: "Done",
  archived: "Archived",
};

const STATUS_VARIANT: Record<
  ProjectStatusDto,
  "default" | "secondary" | "outline"
> = {
  active: "default",
  backlog: "secondary",
  done: "outline",
  archived: "outline",
};

export function ProjectsPage(): JSX.Element {
  const route = useRoute();
  const status = projectsStatusOf(route);
  const [search, setSearch] = useState("");
  const [dialogOpen, setDialogOpen] = useState(false);

  // Server filter mirrors the URL `?status=` exactly; when `null`
  // (the "All" view) we omit the filter and fetch every status.
  const list = useProjectList({
    status: status ?? undefined,
    q: search.trim() || undefined,
    // §6 sidebar caps suggest the per-org project n is small; a
    // 200-row fetch is well within `MAX_LIST_LIMIT` and avoids a
    // second pagination surface on day one.
    limit: 200,
  });

  const orgsQ = useQuery<OrgDto[]>({
    queryKey: ["orgs"],
    queryFn: () => api.listOrgs(),
    staleTime: 60_000,
  });
  const orgMap = useMemo(() => {
    const m = new Map<string, string>();
    for (const o of orgsQ.data ?? []) m.set(o.id, o.login);
    return m;
  }, [orgsQ.data]);

  const rows = list.data?.rows ?? [];
  const grouped = useMemo<Record<ProjectStatusDto, ProjectDto[]>>(() => {
    const acc: Record<ProjectStatusDto, ProjectDto[]> = {
      active: [],
      backlog: [],
      done: [],
      archived: [],
    };
    for (const r of rows) acc[r.status].push(r);
    return acc;
  }, [rows]);

  return (
    <div
      className="flex flex-col gap-4 px-4 lg:px-6"
      data-testid="projects-page"
    >
      <PageHeading
        title="Projects"
        description="Cross-repo planning. Group issues by goal, track progress, and (optionally) mirror dates to a GitHub Projects v2 board."
        trailing={
          <Button
            onClick={() => setDialogOpen(true)}
            data-testid="projects-new-button"
          >
            + New project
          </Button>
        }
      />

      <Card>
        <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-2">
          <CardTitle className="text-base">
            {status ? STATUS_LABEL[status] : "All projects"}{" "}
            <span
              className="ml-2 text-sm font-normal text-muted-foreground"
              data-testid="projects-page-count"
            >
              {list.data ? `(${list.data.total})` : ""}
            </span>
          </CardTitle>
          <div className="flex items-center gap-2">
            <Input
              data-testid="projects-search"
              placeholder="Search by name…"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="w-56"
            />
          </div>
        </CardHeader>
        <CardContent>
          {list.isPending && (
            <div
              className="flex items-center gap-2 py-4 text-sm text-muted-foreground"
              data-testid="projects-loading"
            >
              <Spinner /> Loading projects…
            </div>
          )}

          {list.isError && (
            <Alert variant="destructive" data-testid="projects-error">
              <AlertTitle>Couldn't load projects</AlertTitle>
              <AlertDescription>{list.error.message}</AlertDescription>
            </Alert>
          )}

          {!list.isPending && !list.isError && rows.length === 0 && (
            <p
              className="py-8 text-center text-sm text-muted-foreground"
              data-testid="projects-empty"
            >
              {search.trim()
                ? "No projects match that search."
                : status
                  ? `No ${STATUS_LABEL[status].toLowerCase()} projects yet.`
                  : "No projects yet. Click [+ New project] to create one."}
            </p>
          )}

          {!list.isPending && !list.isError && rows.length > 0 && (
            <div className="flex flex-col gap-6" data-testid="projects-list">
              {status ? (
                <ProjectTable rows={rows} orgMap={orgMap} />
              ) : (
                STATUS_ORDER.map((s) =>
                  grouped[s].length > 0 ? (
                    <section key={s} data-testid={`projects-section-${s}`}>
                      <h2 className="mb-2 flex items-center gap-2 text-sm font-medium uppercase tracking-wide text-muted-foreground">
                        {STATUS_LABEL[s]}
                        <Badge variant={STATUS_VARIANT[s]} className="px-1.5">
                          {grouped[s].length}
                        </Badge>
                      </h2>
                      <ProjectTable rows={grouped[s]} orgMap={orgMap} />
                    </section>
                  ) : null,
                )
              )}
            </div>
          )}
        </CardContent>
      </Card>

      <NewProjectDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        onCreated={(p) => navigate(projectDetailRoute(p.id))}
      />
    </div>
  );
}

function ProjectTable({ rows, orgMap }: { rows: ProjectDto[]; orgMap: Map<string, string> }): JSX.Element {
  return (
    <Table className="table-fixed">
      <TableHeader>
        <TableRow>
          <TableHead className="w-[40%]">Name</TableHead>
          <TableHead className="w-[12%]">Org</TableHead>
          <TableHead className="w-[10%]">Status</TableHead>
          <TableHead className="w-[18%]">Progress</TableHead>
          <TableHead className="w-[12%]">Due</TableHead>
          <TableHead className="w-[8%] text-right">Issues</TableHead>
          <TableHead className="w-[60px]"></TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {rows.map((p) => {
          const pct =
            p.issue_count > 0
              ? Math.round((p.closed_issue_count / p.issue_count) * 100)
              : 0;
          return (
            <TableRow
              key={p.id}
              data-testid="projects-row"
              className="cursor-pointer"
              onClick={() => navigate(projectDetailRoute(p.id))}
            >
              <TableCell className="font-medium">
                <a
                  className="hover:underline"
                  href={projectDetailRoute(p.id)}
                  onClick={(e) => e.stopPropagation()}
                >
                  {p.name}
                </a>
                {p.description && (
                  <span className="ml-2 text-xs text-muted-foreground line-clamp-1">
                    {p.description}
                  </span>
                )}
              </TableCell>
              <TableCell className="text-xs text-muted-foreground">
                {orgMap.get(p.org_id) ?? "—"}
              </TableCell>
              <TableCell>
                <Badge variant={STATUS_VARIANT[p.status]}>
                  {STATUS_LABEL[p.status]}
                </Badge>
              </TableCell>
              <TableCell>
                <div className="flex items-center gap-2">
                  <Progress value={pct} className="h-2" />
                  <span className="text-xs tabular-nums text-muted-foreground">
                    {pct}%
                  </span>
                </div>
              </TableCell>
              <TableCell className="font-mono text-xs">
                {p.due_at ? new Date(p.due_at).toLocaleDateString("en-AU") : "—"}
              </TableCell>
              <TableCell className="text-right tabular-nums">
                {p.closed_issue_count}/{p.issue_count}
              </TableCell>
              <TableCell
                className="text-right"
                onClick={(e) => e.stopPropagation()}
              >
                <ProjectRowActions project={p} />
              </TableCell>
            </TableRow>
          );
        })}
      </TableBody>
    </Table>
  );
}
