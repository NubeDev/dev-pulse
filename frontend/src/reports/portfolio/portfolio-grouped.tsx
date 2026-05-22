import type {
  PortfolioSort,
  ProjectPortfolioRow,
  ProjectStatusDto,
} from "../../api/client.js";
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import {
  FILTER_STATUSES,
  FILTER_STATUS_LABEL,
  STATUS_TONE,
} from "./portfolio-constants.js";
import { PortfolioTable } from "./portfolio-table.js";

export function PortfolioGroupedTables({
  rows,
  nowMs,
  sort,
  route,
}: {
  rows: ProjectPortfolioRow[];
  nowMs: number;
  sort: PortfolioSort;
  route: string;
}): JSX.Element {
  const groups: Record<ProjectStatusDto, ProjectPortfolioRow[]> = {
    active: [],
    backlog: [],
    done: [],
    archived: [],
  };
  for (const r of rows) groups[r.status].push(r);

  const open = (["active", "backlog"] as ProjectStatusDto[]).filter(
    (s) => groups[s].length > 0,
  );

  return (
    <Accordion
      type="multiple"
      defaultValue={open}
      className="flex flex-col gap-2"
      data-testid="portfolio-grouped"
    >
      {FILTER_STATUSES.map((s) => {
        const groupRows = groups[s];
        const empty = groupRows.length === 0;
        return (
          <AccordionItem
            key={s}
            value={s}
            className={cn(
              "rounded-lg border bg-card",
              empty && "opacity-60",
            )}
            data-testid={`portfolio-group-${s}`}
          >
            <AccordionTrigger
              className="px-4 py-3 hover:no-underline"
              disabled={empty}
            >
              <div className="flex flex-1 items-center justify-between gap-4 pr-2">
                <div className="flex items-center gap-2">
                  <span
                    className={cn(
                      "inline-flex items-center rounded-md px-2 py-0.5 text-xs font-semibold uppercase tracking-wide ring-1 ring-inset",
                      STATUS_TONE[s],
                    )}
                  >
                    {FILTER_STATUS_LABEL[s]}
                  </span>
                  <Badge
                    variant="secondary"
                    className="px-1.5 tabular-nums"
                    data-testid={`portfolio-group-count-${s}`}
                  >
                    {groupRows.length}
                  </Badge>
                </div>
                {!empty ? (
                  <span className="text-xs font-normal text-muted-foreground">
                    {groupSummary(s, groupRows, nowMs)}
                  </span>
                ) : null}
              </div>
            </AccordionTrigger>
            <AccordionContent className="p-0">
              {!empty ? (
                <PortfolioTable
                  rows={groupRows}
                  nowMs={nowMs}
                  sort={sort}
                  route={route}
                />
              ) : null}
            </AccordionContent>
          </AccordionItem>
        );
      })}
    </Accordion>
  );
}

function groupSummary(
  status: ProjectStatusDto,
  rows: ProjectPortfolioRow[],
  nowMs: number,
): string {
  if (status === "active" || status === "backlog") {
    const upcoming = rows
      .map((r) => r.due_at)
      .filter((d): d is string => !!d)
      .map((d) => Date.parse(d))
      .filter((t) => !Number.isNaN(t))
      .sort((a, b) => a - b)[0];
    if (!upcoming) return "no due dates";
    const days = Math.round((upcoming - nowMs) / 86_400_000);
    return days >= 0 ? `next due in ${days}d` : `${-days}d overdue`;
  }
  const overdue = rows.reduce((n, r) => n + (r.issue_overdue_count ?? 0), 0);
  if (overdue > 0) return `${overdue} open overdue`;
  return `${rows.length} project${rows.length === 1 ? "" : "s"}`;
}
