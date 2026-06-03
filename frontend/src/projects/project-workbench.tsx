/**
 * `<ProjectWorkbench>` — the §6.3 issues card wrapped with a
 * Group-by dropdown and a server-side sectioned list
 * (PROJECT-VIEW.md §5.1 / §5.2 / §7.2).
 *
 * Slice 2 (PROJECT-VIEW.md §8 — "Slice 2"): ships Group-by only.
 * Filter and Sort are intentionally stubbed as disabled controls
 * so the visual shape lands first; Slice 3 wires up the chip bar.
 *
 * Wire contract:
 *
 *   * When `group_by` is unset, the response is the existing flat
 *     `IssueListResponse`; this component renders a single flat
 *     list (same UX as the pre-PROJECT-VIEW issues card).
 *   * When `group_by` is set, the server returns the same flat
 *     `rows` plus a `buckets` sidecar with **post-filter** counts
 *     and each row carries `bucket_keys` (PROJECT-VIEW.md §7.2).
 *     The client never re-buckets — it groups in-place using
 *     `bucket_keys` so the section counts always agree with the
 *     dropdown's authoritative numbers.
 *
 * URL hash: `?group=<dim>` (§5.4 — clause C "ad-hoc"). Saved-view
 * (§5.4 clauses A/B) lands in Slice 4 — for now the `view` param
 * is ignored.
 */

import { useMemo, useState } from "react";
import {
  ChevronDownIcon,
  ChevronRightIcon,
  PlusIcon,
  SettingsIcon,
} from "lucide-react";

import type { IssueBucket, IssueListItem, ProjectDto } from "../api/client.js";

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
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { HelpHint } from "@/components/help-hint";
import {
  navigate,
  projectDetailRouteWithParams,
  projectFilter,
  projectGroupBy,
  projectSort,
  projectViewId,
  useRoute,
} from "../routes.js";

import { AddIssuesDialog } from "./add-issues-dialog.js";
import { CategoriesManagerDialog } from "./categories-manager-dialog.js";
import { orderGateViews } from "./icon-for-name.js";
import {
  FilterChipBar,
  parseFilterString,
  serializeFilterChips,
  type FilterChip,
} from "./project-filter-chips.js";
import {
  useCreateProjectView,
  useDeleteProjectView,
  useProjectGroupByOptions,
  useProjectIssues,
  useProjectMilestones,
  useProjectViews,
  useRemoveIssueFromProject,
  useReorderProjectViews,
  useUpdateProjectView,
} from "./use-projects-data.js";
import { ViewsTabStrip } from "./views-tab-strip.js";
import {
  CATEGORISED_GROUP_BY,
  CATEGORY_TAG_KEY,
  findCategoryTag,
  writeDateDisplayMode,
  type DateDisplayMode,
} from "./view-wizard/index.js";
import { useTags } from "../workflow/use-workflow-data.js";

export interface ProjectWorkbenchProps {
  project: ProjectDto;
  selectedIssueId: string | null;
  onSelectIssue: (id: string | null) => void;
  /** Issue row renderer. Provided by the detail page so the
   *  row's chrome stays in one place — the workbench only owns
   *  layout and section grouping. */
  renderRow: (row: IssueListItem, selected: boolean) => JSX.Element;
}

export function ProjectWorkbench({
  project,
  renderRow,
  selectedIssueId,
}: ProjectWorkbenchProps): JSX.Element {
  const route = useRoute();
  const urlGroupBy = projectGroupBy(route);
  const urlFilterRaw = projectFilter(route);
  const urlSort = projectSort(route);
  const urlViewId = projectViewId(route);
  const [dialogOpen, setDialogOpen] = useState(false);
  // Inverted from the previous `collapsed` set so the default
  // empty set = every section starts COLLAPSED. Cuts the visual
  // weight on first paint for categorised views with eight-plus
  // buckets; the user opens what they care about.
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const [categoriesManagerOpen, setCategoriesManagerOpen] = useState(false);

  // Saved views (PROJECT-VIEW.md §5.4 / §7.1). The strip + dirty
  // detection both depend on this list, so resolve `activeView`
  // *before* deriving the effective workbench shape below.
  const viewsQuery = useProjectViews(project.id);
  // Gate-progression tabs (`G1` … `G8`) are fanned out by the create
  // wizard as eight back-to-back POSTs, so the server can persist
  // them out of order (G1, G7, G8, G2 …). Re-impose canonical G1→G8
  // order here; non-gate tabs keep their stored / dragged position.
  const views = useMemo(
    () => orderGateViews(viewsQuery.data ?? []),
    [viewsQuery.data],
  );
  const activeView =
    urlViewId !== null ? views.find((v) => v.id === urlViewId) ?? null : null;

  // §5.4 precedence — per-field:
  //   1. If `?<param>=...` is present, the URL wins.
  //   2. Else, if a saved view is active, the view's stored value wins.
  //   3. Else, fall back to the legacy ad-hoc default (null / default sort).
  const viewFilterSerialised = useMemo(
    () =>
      activeView
        ? serializeFilterChips(
            activeView.filter_clauses.map((c) =>
              c.dim === "tag"
                ? { dim: "tag", key: c.key, value: c.value }
                : { dim: c.dim, value: c.value },
            ),
          )
        : "",
    [activeView],
  );

  // Categorised views (PROJECT-VIEW.md — categories amendment):
  // when the active view has a non-empty `categories` list it
  // *always* groups by `tag:category` and renders one section per
  // category in saved order, including empty ones. URL group
  // overrides win because the user is in ad-hoc mode.
  const viewIsCategorised =
    activeView !== null && activeView.categories.length > 0;
  const groupBy =
    urlGroupBy ??
    (viewIsCategorised
      ? CATEGORISED_GROUP_BY
      : activeView
        ? activeView.group_by
        : null);
  // `urlFilterRaw === ""` is the explicit-empty override sentinel
  // (see [`FILTER_EMPTY_OVERRIDE`] in routes.ts) — when set we must
  // *not* fall back to the view's filter; the user is saying "no
  // filter on this dirty tab".
  const filterRaw =
    urlFilterRaw !== null
      ? urlFilterRaw
      : activeView
        ? viewFilterSerialised || null
        : null;
  const sort = urlSort ?? (activeView ? activeView.sort : null);

  const chips = useMemo(() => parseFilterString(filterRaw), [filterRaw]);

  // §5.4 dirty marker — any URL override while a view is active.
  // We deliberately use "any override present" rather than a deep
  // value-equality check; the user's intent in writing the URL was
  // already an explicit ad-hoc edit, even if it happens to equal
  // the saved value.
  const isDirty =
    activeView !== null &&
    (urlGroupBy !== null || urlFilterRaw !== null || urlSort !== null);

  const issues = useProjectIssues(project.id, {
    state: "all",
    limit: 100,
    group_by: groupBy ?? undefined,
    filter: filterRaw ?? undefined,
    sort: sort ?? undefined,
    view: activeView ? activeView.id : undefined,
  });
  const groupOptions = useProjectGroupByOptions(project.id);
  const milestonesForFilter = useProjectMilestones(project.id);
  const milestoneFilterOptions = useMemo(
    () =>
      (milestonesForFilter.data ?? []).map((m) => ({
        id: m.id,
        title: m.title,
      })),
    [milestonesForFilter.data],
  );
  const remove = useRemoveIssueFromProject(project.id);

  const createView = useCreateProjectView(project.id);
  const updateView = useUpdateProjectView(project.id);
  const deleteView = useDeleteProjectView(project.id);
  const reorderViews = useReorderProjectViews(project.id);
  const viewMutationsBusy =
    createView.isPending ||
    updateView.isPending ||
    deleteView.isPending ||
    reorderViews.isPending;

  // Tag context — used to auto-tag new issues created from a
  // category section. The tag id is resolved lazily per click
  // because the tags query is shared with the workflow surface
  // and may not be populated by the time the workbench mounts.
  const tagsQuery = useTags();

  // Section-scoped "Add issue" context. `null` = the global
  // toolbar `+ Add issue` button (no category bias); a string =
  // a per-section `+` was clicked inside a categorised view.
  const [sectionAddCategory, setSectionAddCategory] = useState<
    string | null
  >(null);
  const sectionAddTagId =
    sectionAddCategory !== null
      ? findCategoryTag(
          tagsQuery.data ?? [],
          project.org_id,
          sectionAddCategory,
        )?.id ?? null
      : null;

  /** Replace the entire ad-hoc URL state in one navigation so the
   *  history stack stays clean (PROJECT-VIEW.md §5.4 — ad-hoc edits
   *  always overwrite, never push). Pass `view: undefined` to keep
   *  the current view selection; pass `null` to clear it. */
  const patchUrl = (patch: {
    group?: string | null;
    filter?: string | null;
    sort?: string | null;
    view?: string | null;
  }): void => {
    navigate(
      projectDetailRouteWithParams(project.id, {
        issueId: selectedIssueId,
        view: patch.view !== undefined ? patch.view : urlViewId,
        group: patch.group !== undefined ? patch.group : urlGroupBy,
        filter: patch.filter !== undefined ? patch.filter : urlFilterRaw,
        sort: patch.sort !== undefined ? patch.sort : urlSort,
      }),
    );
  };

  const setGroupBy = (next: string | null): void => {
    patchUrl({ group: next });
  };

  const setFilterChips = (next: FilterChip[]): void => {
    const serialised = serializeFilterChips(next);
    // When a view is active and the user clears all chips, we must
    // write an *explicit-empty* override (empty string) so the
    // workbench knows the user intended to drop the view's stored
    // filter — `null` would just remove the override and let the
    // view's filter resurface. Without a view active, `null` is the
    // right choice: it strips the `?filter=` param entirely.
    if (serialised.length > 0) {
      patchUrl({ filter: serialised });
    } else {
      patchUrl({ filter: activeView ? "" : null });
    }
  };

  const setSort = (next: string | null): void => {
    patchUrl({ sort: next });
  };

  /** Activate (or clear) a saved view. Clears every ad-hoc URL
   *  override in the same navigation so the saved view's shape
   *  takes over cleanly (state A in §5.4). */
  const selectView = (viewId: string | null): void => {
    patchUrl({
      view: viewId,
      group: null,
      filter: null,
      sort: null,
    });
  };

  /** "Discard" on a dirty active view — keep the view, drop the
   *  per-field overrides. */
  const discardDirty = (): void => {
    patchUrl({ group: null, filter: null, sort: null });
  };

  /** "Save changes" on a dirty active view: PATCH the view to the
   *  current effective shape, then clear the URL overrides so the
   *  strip immediately drops the `*` marker. */
  const handleUpdateView = (
    viewId: string,
    body: Parameters<typeof updateView.mutate>[0]["body"],
  ): void => {
    updateView.mutate(
      { viewId, body },
      {
        onSuccess: () => {
          patchUrl({ group: null, filter: null, sort: null });
        },
      },
    );
  };

  /** Create-from-current — write the new view server-side and
   *  pivot the URL onto it. The strip is fed by the same query so
   *  the new tab pops into place on invalidation. */
  const handleCreateView = (
    body: Parameters<typeof createView.mutate>[0],
    dateDisplay: DateDisplayMode,
  ): void => {
    createView.mutate(body, {
      onSuccess: (v) => {
        // The tab badge preference is machine-local (keyed by view
        // id), so it can only be persisted now that the POST has
        // handed us a real id. `writeDateDisplayMode` no-ops for
        // the "week" default, so the common case writes nothing.
        writeDateDisplayMode(v.id, dateDisplay);
        patchUrl({ view: v.id, group: null, filter: null, sort: null });
      },
    });
  };

  const handleDeleteView = (viewId: string): void => {
    deleteView.mutate(viewId, {
      onSuccess: () => {
        if (urlViewId === viewId) {
          patchUrl({ view: null, group: null, filter: null, sort: null });
        }
      },
    });
  };

  // §5.4 — stale `?view=` recovery: the id was valid on the wire
  // but isn't in the loaded list (deleted in another tab, etc.).
  // We silently strip it once the load has resolved.
  //
  // Must wait for `!isFetching` (not just `!isPending`): after a
  // create-view mutation we call `patchUrl({ view: v.id })` from
  // the mutation's call-site `onSuccess` and the hook's
  // `onSuccess` invalidates the views query in the same tick. The
  // cached `data` is still the OLD list while the refetch is in
  // flight (`isPending=false`, `isFetching=true`) — without the
  // `!isFetching` guard this branch fires, bounces the URL back
  // to "All", and the user perceives the just-saved view as
  // missing even though the tab actually arrives a beat later.
  if (
    urlViewId !== null &&
    !viewsQuery.isPending &&
    !viewsQuery.isFetching &&
    !viewsQuery.isError &&
    activeView === null
  ) {
    // eslint-disable-next-line no-console
    console.warn(
      `[projects] view ${urlViewId} no longer exists; falling back to ad-hoc.`,
    );
    // Defer the navigation to the next tick to avoid setState-in-render.
    queueMicrotask(() => patchUrl({ view: null }));
  }

  const toggleSection = (key: string): void => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
  };

  const rows = issues.data?.rows ?? [];
  const buckets = issues.data?.buckets;

  // Pre-bucket the rows once per response so re-renders during
  // section collapse / expand don't recompute. We honour the
  // server's `buckets` ordering verbatim for non-categorised
  // views (PROJECT-VIEW.md §5.1). Categorised views post-process
  // the bucket list to enforce the saved category order, surface
  // empty sections, and append a trailing "Uncategorised" group
  // for tag values not on the view's list.
  const sectioned = useMemo<SectionedRows | null>(() => {
    if (!groupBy || !buckets) return null;
    if (viewIsCategorised && activeView) {
      return groupRowsByCategorisedView(rows, buckets, activeView.categories);
    }
    return groupRowsByBuckets(rows, buckets);
  }, [groupBy, buckets, rows, viewIsCategorised, activeView]);

  const expandAll = (): void => {
    if (!sectioned) return;
    const next = new Set<string>();
    for (const s of sectioned.sections) {
      next.add(bucketKeyForState(s.bucket.key));
    }
    setExpanded(next);
  };

  const collapseAll = (): void => {
    setExpanded(new Set());
  };

  const allExpanded =
    sectioned !== null &&
    sectioned.sections.length > 0 &&
    sectioned.sections.every((s) =>
      expanded.has(bucketKeyForState(s.bucket.key)),
    );

  return (
    <Card data-testid="project-issues">
      <CardHeader className="flex flex-row items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                size="icon"
                onClick={() => setDialogOpen(true)}
                data-testid="project-add-issue-button"
                aria-label="Add issue"
                className="size-8"
              >
                <PlusIcon className="size-4" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Add issue</TooltipContent>
          </Tooltip>
          <CardTitle className="text-base">
            Issues{" "}
            <span className="ml-2 text-sm font-normal text-muted-foreground">
              ({project.closed_issue_count}/{project.issue_count} closed)
            </span>
          </CardTitle>
        </div>
      </CardHeader>

      <CardContent className="flex flex-col gap-3">
        <ViewsTabStrip
          views={views}
          activeViewId={activeView ? activeView.id : null}
          isDirty={isDirty}
          current={{
            groupBy,
            filterClauses: chips.map((c) =>
              c.dim === "tag"
                ? { dim: "tag", key: c.key ?? "", value: c.value }
                : { dim: c.dim, value: c.value },
            ),
            sort: sort ?? "updated_desc",
          }}
          orgId={project.org_id}
          existingTags={tagsQuery.data ?? null}
          onSelectView={selectView}
          onCreateView={handleCreateView}
          onUpdateView={handleUpdateView}
          onDeleteView={handleDeleteView}
          onReorderViews={(ids) => reorderViews.mutate(ids)}
          onDiscardDirty={discardDirty}
          busy={viewMutationsBusy}
        />
        <Toolbar
          groupBy={groupBy}
          options={groupOptions.data?.dims ?? []}
          onChange={setGroupBy}
          chips={chips}
          onChipsChange={setFilterChips}
          milestoneOptions={milestoneFilterOptions}
          sort={sort}
          onSortChange={setSort}
          onManageCategories={
            // Show the gear on any saved view so the manager stays
            // reachable after a `Delete all` empties `categories`
            // (which would otherwise flip `viewIsCategorised` to
            // false and hide the entry point — leaving the user
            // unable to add categories back without a page change).
            activeView !== null
              ? () => setCategoriesManagerOpen(true)
              : undefined
          }
        />

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

        {!issues.isPending &&
          !issues.isError &&
          rows.length === 0 &&
          !viewIsCategorised && (
            <p
              className="py-6 text-center text-sm text-muted-foreground"
              data-testid="project-issues-empty"
            >
              No issues in this project yet. Click [+ Add issue] to attach work from the workflow surface.
            </p>
          )}

        {/* Flat list — no grouping active. */}
        {!sectioned && rows.length > 0 && (
          <div className="flex flex-col gap-2" data-testid="project-issues-flat">
            {rows.map((row) => renderRow(row, row.id === selectedIssueId))}
          </div>
        )}

        {/* Sectioned list — one collapsible block per bucket. */}
        {sectioned && (
          <div
            className="flex flex-col gap-3"
            data-testid="project-issues-grouped"
          >
            {sectioned.sections.length > 0 && (
              <div className="flex items-center justify-end">
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={allExpanded ? collapseAll : expandAll}
                  data-testid="project-issues-toggle-all"
                  className="h-7 text-xs text-muted-foreground hover:text-foreground"
                >
                  {allExpanded ? "Collapse all" : "Expand all"}
                </Button>
              </div>
            )}
            {sectioned.sections.map((section) => {
              const sectionKey = bucketKeyForState(section.bucket.key);
              const isCollapsed = !expanded.has(sectionKey);
              // Categorised sections expose a `+` button that
              // pre-scopes the add dialog to the category. Non-
              // category buckets (status, milestone, gate, the
              // trailing "Uncategorised" pseudo-section) don't.
              const sectionCategoryKey = section.categoryKey;
              return (
                <BucketSection
                  key={sectionKey}
                  bucket={section.bucket}
                  rows={section.rows}
                  collapsed={isCollapsed}
                  onToggle={() => toggleSection(sectionKey)}
                  renderRow={renderRow}
                  selectedIssueId={selectedIssueId}
                  onAddIssue={
                    viewIsCategorised && sectionCategoryKey !== null
                      ? () => {
                          setSectionAddCategory(sectionCategoryKey);
                          setDialogOpen(true);
                        }
                      : undefined
                  }
                />
              );
            })}
            {sectioned.empty && !viewIsCategorised && (
              <p className="py-6 text-center text-sm text-muted-foreground">
                No issues match this grouping.
              </p>
            )}
          </div>
        )}

        {remove.error && (
          <Alert variant="destructive">
            <AlertTitle>Remove failed</AlertTitle>
            <AlertDescription>{remove.error.message}</AlertDescription>
          </Alert>
        )}
      </CardContent>

      <CategoriesManagerDialog
        open={categoriesManagerOpen}
        view={activeView}
        orgId={project.org_id}
        existingTags={tagsQuery.data ?? null}
        busy={updateView.isPending}
        onClose={() => setCategoriesManagerOpen(false)}
        onSubmit={(viewId, body) =>
          updateView.mutate({ viewId, body })
        }
      />

      <AddIssuesDialog
        open={dialogOpen}
        onOpenChange={(o) => {
          setDialogOpen(o);
          // Drop the section context when the dialog closes so
          // the next global `+ Add issue` click doesn't carry
          // a stale category.
          if (!o) setSectionAddCategory(null);
        }}
        project={project}
        activeViewId={activeView ? activeView.id : null}
        activeViewName={activeView ? activeView.name : null}
        activeCategoryKey={sectionAddCategory}
        activeCategoryTagId={sectionAddTagId}
        categoryOptions={
          viewIsCategorised && activeView ? activeView.categories : undefined
        }
      />
    </Card>
  );
}

// ---------------------------------------------------------------------------
// Toolbar — Group-by dropdown + disabled Filter/Sort stubs.
// ---------------------------------------------------------------------------

interface ToolbarProps {
  groupBy: string | null;
  options: { id: string; label: string }[];
  onChange: (next: string | null) => void;
  chips: FilterChip[];
  onChipsChange: (next: FilterChip[]) => void;
  milestoneOptions: { id: string; title: string }[];
  sort: string | null;
  onSortChange: (next: string | null) => void;
  /** When provided, renders a trailing settings icon on the right
   *  edge of the toolbar that opens the categories manager popup.
   *  Set by the workbench only when the active view is
   *  categorised. */
  onManageCategories?: () => void;
}

const GROUP_NONE = "__none__";
const SORT_DEFAULT = "updated_desc";

/** Sort options exposed by the dropdown — values match
 *  `parse_sort` in `crates/dp-rest/src/project_issues.rs`. */
const SORT_OPTIONS: { value: string; label: string }[] = [
  { value: "updated_desc", label: "Updated ↓" },
  { value: "updated_asc", label: "Updated ↑" },
  { value: "title_asc", label: "Title A→Z" },
];

function Toolbar({
  groupBy,
  options,
  onChange,
  chips,
  onChipsChange,
  milestoneOptions,
  sort,
  onSortChange,
  onManageCategories,
}: ToolbarProps): JSX.Element {
  // Sentinel value so the Select can represent "no grouping" — the
  // routing layer keeps the URL as the source of truth, but Radix
  // <Select> requires non-empty option values.
  const value = groupBy ?? GROUP_NONE;
  const sortValue = sort ?? SORT_DEFAULT;
  return (
    <div
      className="flex flex-wrap items-center gap-x-4 gap-y-2 rounded-md border border-dashed border-border bg-muted/20 px-3 py-2 text-sm"
      data-testid="project-workbench-toolbar"
    >
      <HelpHint
        title="Workbench toolbar"
        body={[
          "Group by: bucket the issue list by status, milestone, or any tag-key your repos use. Pick None to flatten back to a single list.",
          "Filter: + Add chips like status:open, tag:gate:g3, milestone:<…>. Chips are AND-combined; multiple values on the same dim are OR'd within (Linear semantics).",
          "Sort: choose between most-recently updated, oldest first, or alphabetical title.",
          "Save the current Group / Filter / Sort as a tab via the Views strip above — tabs become containers, not just saved searches.",
        ]}
      />
      <label className="flex items-center gap-2">
        <span className="text-muted-foreground">Group by:</span>
        <Select
          value={value}
          onValueChange={(v) => onChange(v === GROUP_NONE ? null : v)}
        >
          <SelectTrigger className="h-7 w-40" data-testid="project-group-by-select">
            <SelectValue placeholder="None" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={GROUP_NONE}>None</SelectItem>
            {options.map((opt) => (
              <SelectItem key={opt.id} value={opt.id}>
                {opt.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </label>

      <FilterChipBar
        chips={chips}
        groupOptions={options}
        milestoneOptions={milestoneOptions}
        onChange={onChipsChange}
      />

      <label className="flex items-center gap-2">
        <span className="text-muted-foreground">Sort:</span>
        <Select
          value={sortValue}
          onValueChange={(v) =>
            onSortChange(v === SORT_DEFAULT ? null : v)
          }
        >
          <SelectTrigger className="h-7 w-36" data-testid="project-sort-select">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {SORT_OPTIONS.map((o) => (
              <SelectItem key={o.value} value={o.value}>
                {o.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </label>

      {onManageCategories && (
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              size="icon"
              variant="ghost"
              onClick={onManageCategories}
              aria-label="Manage categories"
              data-testid="project-manage-categories"
              className="ml-auto size-7"
            >
              <SettingsIcon className="size-4" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Manage categories</TooltipContent>
        </Tooltip>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Section — one collapsible bucket.
// ---------------------------------------------------------------------------

interface BucketSectionProps {
  bucket: IssueBucket;
  rows: IssueListItem[];
  collapsed: boolean;
  onToggle: () => void;
  renderRow: (row: IssueListItem, selected: boolean) => JSX.Element;
  selectedIssueId: string | null;
  /** When set, renders a `+` button in the section header that
   *  calls this handler. Used by categorised views to open the
   *  add-issue dialog pre-scoped to the section's category. */
  onAddIssue?: () => void;
}

function BucketSection({
  bucket,
  rows,
  collapsed,
  onToggle,
  renderRow,
  selectedIssueId,
  onAddIssue,
}: BucketSectionProps): JSX.Element {
  return (
    <section className="flex flex-col" data-testid="project-issues-section">
      <div className="flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm font-medium hover:bg-accent/30">
        <button
          type="button"
          onClick={onToggle}
          className="flex items-center gap-2 bg-transparent text-left"
          aria-expanded={!collapsed}
          data-testid="project-issues-section-header"
          data-bucket-key={bucket.key ?? ""}
        >
          <span className="flex items-center gap-1.5">
            {collapsed ? (
              <ChevronRightIcon className="h-4 w-4 text-muted-foreground" />
            ) : (
              <ChevronDownIcon className="h-4 w-4 text-muted-foreground" />
            )}
            <span>{bucket.label}</span>
            {bucket.key === null && (
              <Badge variant="outline" className="ml-1 text-[10px]">
                none
              </Badge>
            )}
          </span>
          <span className="text-xs font-normal tabular-nums text-muted-foreground">
            {bucket.open} open
            {bucket.closed > 0 ? ` · ${bucket.closed} ✓` : ""}
          </span>
        </button>
        {onAddIssue && (
          <Button
            type="button"
            size="icon"
            variant="ghost"
            onClick={onAddIssue}
            aria-label={`Add issue to ${bucket.label}`}
            data-testid="project-issues-section-add"
            className="size-7 shrink-0"
          >
            <PlusIcon className="size-4" />
          </Button>
        )}
      </div>
      {!collapsed && (
        <div className="ml-1 mt-1 flex flex-col gap-2 border-l border-border/60 pl-3">
          {rows.length === 0 ? (
            <p className="py-3 text-xs text-muted-foreground">
              Nothing in this bucket.
            </p>
          ) : (
            rows.map((row) => renderRow(row, row.id === selectedIssueId))
          )}
        </div>
      )}
    </section>
  );
}

// ---------------------------------------------------------------------------
// Bucketing helpers — exported for unit tests in Slice 4 onward.
// ---------------------------------------------------------------------------

interface SectionedRows {
  sections: SectionedRow[];
  empty: boolean;
}

interface SectionedRow {
  bucket: IssueBucket;
  rows: IssueListItem[];
  /** Category slug when the bucket represents one of the view's
   *  curated categories — used to scope the per-section `+`
   *  button to that category. `null` for non-category buckets
   *  (status, milestone, etc.) and the "Uncategorised" trailing
   *  group inside a categorised view. */
  categoryKey: string | null;
}

/** Compose `IssueListItem.bucket_keys` against the server's
 *  authoritative `buckets` ordering. An issue with multiple keys
 *  appears in every matching section (PROJECT-VIEW.md §5.1 — kv
 *  tags can be multi-valued, e.g. `category:firmware` +
 *  `category:hardware`). Issues whose `bucket_keys` is missing
 *  (server didn't ship the field) are dropped from the sectioned
 *  view — that branch should never fire because the server only
 *  omits the field when no grouping is active. */
export function groupRowsByBuckets(
  rows: IssueListItem[],
  buckets: IssueBucket[],
): SectionedRows {
  const sections: SectionedRow[] = buckets.map((b) => ({
    bucket: b,
    rows: [] as IssueListItem[],
    categoryKey: null,
  }));
  // Use the bucket key (stringified `null` → "") as map key so the
  // "No <key>" bucket is reachable too. The synthetic bucket is
  // emitted with `key: null` by the server (PROJECT-VIEW.md §7.2).
  const byKey = new Map<string, IssueListItem[]>();
  for (const s of sections) {
    byKey.set(bucketKeyForState(s.bucket.key), s.rows);
  }
  for (const row of rows) {
    const keys = row.bucket_keys;
    if (!keys || keys.length === 0) {
      // Fallback — shouldn't happen, but route through "no key" so
      // the row is still visible.
      byKey.get(bucketKeyForState(null))?.push(row);
      continue;
    }
    for (const k of keys) {
      const slot = byKey.get(bucketKeyForState(k));
      if (slot) slot.push(row);
    }
  }
  return {
    sections,
    empty: rows.length === 0,
  };
}

/** Categorised-view override of [`groupRowsByBuckets`].
 *
 *  The server returns whichever buckets actually have issues for
 *  the `tag:category` dimension; we layer the view's curated
 *  list on top to:
 *
 *    1. Render sections in the saved order, not count-desc.
 *    2. Surface EMPTY sections for categories with zero issues
 *       so the user can drop work into them with the per-section
 *       `+` button.
 *    3. Append a trailing "Uncategorised" pseudo-section that
 *       collects every issue whose `category:<x>` value is
 *       *not* in the curated list (or has no category tag at
 *       all — routed through the server's `key: null` bucket).
 *
 *  Empty sections get a synthesised `IssueBucket` with `open = 0`
 *  / `closed = 0` so the header counts read "0 open" rather than
 *  vanishing the row. Multi-valued issues (an issue tagged both
 *  `category:hardware` and `category:firmware`) appear in BOTH
 *  category sections, mirroring the existing tag-bucket
 *  behaviour in [`groupRowsByBuckets`]. */
export function groupRowsByCategorisedView(
  rows: IssueListItem[],
  buckets: IssueBucket[],
  categories: readonly string[],
): SectionedRows {
  // Index server buckets by their key (category slug for the
  // `tag:category` dim; `null` -> empty string for the synthetic
  // "No <key>" bucket).
  const serverByKey = new Map<string, IssueBucket>();
  for (const b of buckets) {
    serverByKey.set(bucketKeyForState(b.key), b);
  }
  const curatedSet = new Set(categories);

  // Build curated sections in saved order. Use the server bucket
  // when present so counts are server-authoritative; synthesise
  // an empty one otherwise.
  const sections: SectionedRow[] = categories.map((key) => {
    const existing = serverByKey.get(key);
    return {
      bucket: existing ?? emptyCategoryBucket(key),
      rows: [] as IssueListItem[],
      categoryKey: key,
    };
  });

  // Trailing "Uncategorised" bucket: every server bucket that
  // ISN'T on the curated list, plus the server's `key: null`
  // bucket (issues with no `category:<x>` tag at all).
  const uncategorisedKeys = new Set<string>();
  let unOpen = 0;
  let unClosed = 0;
  for (const b of buckets) {
    if (b.key === null || !curatedSet.has(b.key)) {
      uncategorisedKeys.add(bucketKeyForState(b.key));
      unOpen += b.open;
      unClosed += b.closed;
    }
  }
  const includeUncategorised = uncategorisedKeys.size > 0;
  if (includeUncategorised) {
    sections.push({
      bucket: {
        key: null,
        label: "Uncategorised",
        open: unOpen,
        closed: unClosed,
      },
      rows: [],
      categoryKey: null,
    });
  }
  const uncategorisedSlot = includeUncategorised
    ? sections[sections.length - 1]!.rows
    : null;

  // Per-curated-key direct lookup for row distribution.
  const curatedRowSlot = new Map<string, IssueListItem[]>();
  for (const s of sections) {
    if (s.categoryKey !== null) {
      curatedRowSlot.set(s.categoryKey, s.rows);
    }
  }

  for (const row of rows) {
    const keys = row.bucket_keys;
    if (!keys || keys.length === 0) {
      uncategorisedSlot?.push(row);
      continue;
    }
    let landed = false;
    for (const k of keys) {
      if (k === null) {
        // Issues with no category tag — route to Uncategorised.
        uncategorisedSlot?.push(row);
        landed = true;
        continue;
      }
      const slot = curatedRowSlot.get(k);
      if (slot) {
        slot.push(row);
        landed = true;
      }
    }
    if (!landed) {
      // Row carries `category:<x>` values not on the curated
      // list — route to Uncategorised so it stays visible.
      uncategorisedSlot?.push(row);
    }
  }

  return {
    sections,
    empty: rows.length === 0,
  };
}

/** Synthesise an empty `IssueBucket` for a curated category slug
 *  the server didn't return (because no issues currently carry
 *  that tag). The label mirrors the slug — the editor is the
 *  single source of truth for display form, and storing the
 *  display label on a per-view basis would double-write the
 *  same string. */
function emptyCategoryBucket(slug: string): IssueBucket {
  return {
    key: slug,
    label: slug,
    open: 0,
    closed: 0,
  };
}

/** Stable string form of a bucket key for `Map` / `Set` lookups
 *  and React `key` props. `null` (the "No <key>" bucket) maps to
 *  the empty string — kv keys can never be empty under the §3
 *  grammar so the collision is impossible. */
function bucketKeyForState(key: string | null): string {
  return key ?? "";
}
