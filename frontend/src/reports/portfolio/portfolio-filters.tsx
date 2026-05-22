import type { ProjectStatusDto } from "../../api/client.js";
import { Card, CardContent } from "@/components/ui/card";
import { Toggle } from "@/components/ui/toggle";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { navigate } from "../../routes.js";
import {
  FILTER_STATUSES,
  FILTER_STATUS_LABEL,
  STATUS_TONE,
  VALID_STATUSES,
} from "./portfolio-constants.js";
import { buildRoute, currentParams } from "./portfolio-url.js";

export function PortfolioFilterBar({
  statuses,
  hideOverdue,
  route,
}: {
  statuses: ProjectStatusDto[];
  hideOverdue: boolean;
  route: string;
}): JSX.Element {
  const setStatus = (next: string) => {
    const params = currentParams(route);
    params.delete("page");
    if (!next || !VALID_STATUSES.has(next as ProjectStatusDto)) {
      params.delete("status");
    } else {
      params.set("status", next);
    }
    navigate(buildRoute(params));
  };

  const setHideOverdue = (pressed: boolean) => {
    const params = currentParams(route);
    params.delete("page");
    if (pressed) params.set("hide_overdue", "1");
    else params.delete("hide_overdue");
    navigate(buildRoute(params));
  };

  const clearAll = () => {
    const params = currentParams(route);
    params.delete("status");
    params.delete("hide_overdue");
    params.delete("page");
    navigate(buildRoute(params));
  };

  const active = statuses.length === 1 ? statuses[0]! : "";
  const dirty = statuses.length > 0 || hideOverdue;

  return (
    <Card>
      <CardContent className="flex flex-wrap items-center gap-3 py-3">
        <span className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
          Show
        </span>
        <ToggleGroup
          type="single"
          size="sm"
          variant="outline"
          value={active}
          onValueChange={setStatus}
          data-testid="portfolio-status-filter"
        >
          {FILTER_STATUSES.map((s) => (
            <ToggleGroupItem
              key={s}
              value={s}
              aria-label={`Show only ${FILTER_STATUS_LABEL[s]}`}
              data-testid={`portfolio-status-${s}`}
              className="px-2.5 text-xs"
            >
              <span
                aria-hidden
                className={cn(
                  "mr-1.5 size-1.5 rounded-full ring-1 ring-inset",
                  STATUS_TONE[s],
                )}
              />
              {FILTER_STATUS_LABEL[s]}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>

        <span className="mx-1 hidden h-5 w-px bg-border sm:block" />

        <Toggle
          size="sm"
          variant="outline"
          pressed={hideOverdue}
          onPressedChange={setHideOverdue}
          data-testid="portfolio-toggle-hide-overdue"
          className="text-xs"
        >
          Hide overdue
        </Toggle>

        {dirty ? (
          <Button
            variant="ghost"
            size="sm"
            onClick={clearAll}
            className="ml-auto text-xs text-muted-foreground"
            data-testid="portfolio-filter-clear"
          >
            Clear filters
          </Button>
        ) : null}
      </CardContent>
    </Card>
  );
}
