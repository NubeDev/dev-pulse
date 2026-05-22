import { useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import {
  Card,
  CardContent,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  FolderKanbanIcon,
  CheckCircle2Icon,
  AlertTriangleIcon,
  TrophyIcon,
} from "lucide-react";

import { api } from "../../api/client.js";
import { PageHeading } from "../../components/page-heading.jsx";
import { Skeleton } from "../../components/skeleton.jsx";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from "../../components/empty.jsx";
import { NewProjectDialog } from "../../projects/new-project-dialog.js";
import { navigate, projectDetailRoute, useRoute } from "../../routes.js";

import { KpiTile } from "./portfolio-kpis.js";
import { PortfolioFilterBar } from "./portfolio-filters.js";
import { PortfolioCharts } from "./portfolio-charts.js";
import { PortfolioTable, PaginationFooter } from "./portfolio-table.js";
import { PortfolioGantt } from "./portfolio-gantt.js";
import { PortfolioGroupedTables } from "./portfolio-grouped.js";
import { parseQuery, currentParams, pageFromParams } from "./portfolio-url.js";

export function ProjectPortfolioPage(): JSX.Element {
  const route = useRoute();
  const request = useMemo(() => parseQuery(route), [route]);
  const qc = useQueryClient();
  const [newOpen, setNewOpen] = useState(false);

  const query = useQuery({
    queryKey: ["report-project-portfolio", request],
    queryFn: () => api.getReportProjectPortfolio(request),
  });

  const resp = query.data;
  const loading = query.isPending;
  const error = query.error?.message ?? null;

  const nowMs = resp ? new Date(resp.now).getTime() : Date.now();

  return (
    <div className="flex flex-col gap-4 px-4 md:gap-6 lg:px-6">
      <PageHeading
        title="Project portfolio"
        description={
          <>
            <code className="font-mono text-xs">
              POST /reports/project-portfolio
            </code>{" "}
            · which projects are on track, slipping, or done across every
            org you can see.
          </>
        }
        trailing={
          <Button
            onClick={() => setNewOpen(true)}
            data-testid="portfolio-new-project"
          >
            + New project
          </Button>
        }
      />

      <NewProjectDialog
        open={newOpen}
        onOpenChange={setNewOpen}
        onCreated={(p) => {
          qc.invalidateQueries({ queryKey: ["report-project-portfolio"] });
          navigate(projectDetailRoute(p.id));
        }}
      />

      <PortfolioFilterBar
        statuses={request.statuses ?? []}
        hideOverdue={!!request.hide_overdue}
        route={route}
      />

      {error ? (
        <Alert variant="destructive" data-testid="portfolio-error">
          <AlertDescription>
            Failed to load portfolio: {error}
          </AlertDescription>
        </Alert>
      ) : null}

      <div
        className="grid grid-cols-2 gap-3 md:grid-cols-4"
        data-testid="portfolio-kpis"
      >
        <KpiTile
          label="Total"
          value={resp?.kpis.total_projects ?? "—"}
          hint={
            resp ? `${resp.total} matching across all pages` : undefined
          }
          icon={FolderKanbanIcon}
          tone="neutral"
        />
        <KpiTile
          label="On track"
          value={resp?.kpis.on_track ?? "—"}
          hint={
            resp
              ? `${resp.kpis.avg_progress_pct}% avg progress`
              : undefined
          }
          icon={CheckCircle2Icon}
          tone="good"
        />
        <KpiTile
          label="Overdue"
          value={resp?.kpis.overdue ?? "—"}
          hint={
            resp
              ? `${resp.kpis.total_issues_overdue} open issues overdue`
              : undefined
          }
          icon={AlertTriangleIcon}
          tone={(resp?.kpis.overdue ?? 0) > 0 ? "bad" : "neutral"}
        />
        <KpiTile
          label="Completed"
          value={resp?.kpis.completed ?? "—"}
          hint={
            resp
              ? `${resp.kpis.total_issues_open} open issues remaining`
              : undefined
          }
          icon={TrophyIcon}
          tone="good"
        />
      </div>

      {resp && resp.kpis.total_projects > 0 ? (
        <PortfolioCharts kpis={resp.kpis} rows={resp.rows} />
      ) : null}

      <Tabs defaultValue="table" className="gap-3">
        <TabsList>
          <TabsTrigger value="table" data-testid="portfolio-tab-table">
            Table
          </TabsTrigger>
          <TabsTrigger value="gantt" data-testid="portfolio-tab-gantt">
            Gantt
          </TabsTrigger>
        </TabsList>
        <TabsContent value="table">
          {loading ? (
            <Card>
              <CardContent className="p-4">
                <Skeleton className="h-8 w-full" />
                <Skeleton className="mt-2 h-8 w-full" />
                <Skeleton className="mt-2 h-8 w-full" />
              </CardContent>
            </Card>
          ) : resp && resp.rows.length === 0 ? (
            <Card>
              <CardContent className="p-0">
                <Empty data-testid="portfolio-empty">
                  <EmptyHeader>
                    <EmptyTitle>No projects to show</EmptyTitle>
                  </EmptyHeader>
                  <EmptyDescription>
                    Either no projects match the current filters, or you don't
                    have any projects yet.
                  </EmptyDescription>
                </Empty>
              </CardContent>
            </Card>
          ) : resp ? (
            (request.statuses ?? []).length === 1 ? (
              <Card>
                <CardContent className="p-0">
                  <PortfolioTable
                    rows={resp.rows}
                    nowMs={nowMs}
                    sort={request.sort ?? "due_asc_nulls_last"}
                    route={route}
                  />
                </CardContent>
              </Card>
            ) : (
              <PortfolioGroupedTables
                rows={resp.rows}
                nowMs={nowMs}
                sort={request.sort ?? "due_asc_nulls_last"}
                route={route}
              />
            )
          ) : null}
        </TabsContent>
        <TabsContent value="gantt">
          <Card>
            <CardContent className="p-2">
              {loading ? (
                <Skeleton className="h-[400px] w-full" />
              ) : resp && resp.rows.length > 0 ? (
                <PortfolioGantt rows={resp.rows} />
              ) : (
                <Empty data-testid="portfolio-gantt-empty">
                  <EmptyHeader>
                    <EmptyTitle>Nothing to plot</EmptyTitle>
                  </EmptyHeader>
                  <EmptyDescription>
                    No projects with timeline data in the current filter.
                  </EmptyDescription>
                </Empty>
              )}
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>

      {resp && resp.total > resp.limit ? (
        <PaginationFooter
          page={pageFromParams(currentParams(route))}
          pageSize={resp.limit}
          total={resp.total}
          route={route}
        />
      ) : null}
    </div>
  );
}
