import type { ProjectStatusDto, TagMatch } from "../../api/client.js";
import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { Card, CardContent } from "@/components/ui/card";
import { Toggle } from "@/components/ui/toggle";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { api } from "../../api/client.js";
import { MultiSelect } from "../leaderboard/index.js";
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
  tagIds,
  tagMatch,
  route,
}: {
  statuses: ProjectStatusDto[];
  hideOverdue: boolean;
  tagIds: string[];
  tagMatch: TagMatch;
  route: string;
}): JSX.Element {
  // Org-scope tags available to filter by. Tags are org-scoped, so a
  // multi-org portfolio surfaces the union of every visible org's
  // tags; the org login disambiguates same-named tags as a hint.
  const tagsQuery = useQuery({
    queryKey: ["tags"],
    queryFn: () => api.listTags(),
    staleTime: 30_000,
  });
  const tagOptions = useMemo(
    () =>
      (tagsQuery.data ?? [])
        .filter((t) => t.scope_kind === "org" && !t.archived_at)
        .map((t) => ({ value: t.id, label: t.name })),
    [tagsQuery.data],
  );

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

  const setTags = (next: string[]) => {
    const params = currentParams(route);
    params.delete("page");
    if (next.length === 0) {
      params.delete("tags");
      params.delete("tag_match");
    } else {
      params.set("tags", next.join(","));
      // Drop a now-meaningless match mode when only one tag remains.
      if (next.length < 2) params.delete("tag_match");
    }
    navigate(buildRoute(params));
  };

  const setTagMatch = (mode: TagMatch) => {
    const params = currentParams(route);
    params.delete("page");
    if (mode === "all") params.set("tag_match", "all");
    else params.delete("tag_match");
    navigate(buildRoute(params));
  };

  const clearAll = () => {
    const params = currentParams(route);
    params.delete("status");
    params.delete("hide_overdue");
    params.delete("tags");
    params.delete("tag_match");
    params.delete("page");
    navigate(buildRoute(params));
  };

  const active = statuses.length === 1 ? statuses[0]! : "";
  const dirty =
    statuses.length > 0 || hideOverdue || tagIds.length > 0;

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

        <div className="flex items-center gap-2" data-testid="portfolio-tag-filter">
          <span className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
            Tags
          </span>
          <MultiSelect
            options={tagOptions}
            value={tagIds}
            onChange={setTags}
            placeholder={tagsQuery.isLoading ? "Loading…" : "Any tag"}
            searchable
            searchPlaceholder="Search tags…"
            summary={(sel) =>
              sel.length === 1 ? sel[0]!.label : `${sel.length} tags`
            }
            // The trigger defaults to `w-full` for the leaderboard's
            // stacked columns; in this inline filter bar that stretches
            // it and crowds the "Hide overdue" toggle. Size to content
            // with a sensible floor instead.
            className="w-auto min-w-[9rem]"
            data-testid="portfolio-tag-multiselect"
          />
          {tagIds.length >= 2 ? (
            <ToggleGroup
              type="single"
              size="sm"
              variant="outline"
              value={tagMatch}
              onValueChange={(v) => {
                if (v === "any" || v === "all") setTagMatch(v);
              }}
              data-testid="portfolio-tag-match"
            >
              <ToggleGroupItem value="any" className="px-2 text-xs">
                Any
              </ToggleGroupItem>
              <ToggleGroupItem value="all" className="px-2 text-xs">
                All
              </ToggleGroupItem>
            </ToggleGroup>
          ) : null}
        </div>

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
