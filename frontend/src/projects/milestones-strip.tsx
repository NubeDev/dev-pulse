/**
 * `<MilestonesStrip>` — PROJECT-VIEW.md §5.5 (Slice 1) horizontal
 * strip of milestone cards rendered above the workbench on the
 * project detail page.
 *
 * One card per active milestone (`state = "open"`) on any of the
 * project's linked repos, sorted by due date soonest first. Each
 * card shows:
 *
 *   * title (plus repo disambiguation when ambiguous — TODO Slice 1.x;
 *     v1 ships the title alone since the server already sorted them);
 *   * due-relative (`due in 6d`, `overdue 3d`, `no due date`);
 *   * `closed / total` progress bar.
 *
 * Slice 1 is **read-only** for the cards themselves; mutation
 * actions live in the overflow `⋯` menu:
 *
 *   * `Adopt as primary` / `Clear primary` (Slice 5 wiring) when
 *     `onAdoptPrimary` is provided.
 *   * `Filter to milestone` (Slice 3↔1 bridge) when
 *     `onFilterToMilestone` is provided — appends a
 *     `milestone:<id>` chip to the workbench filter, replacing any
 *     existing milestone chip (one milestone filter max).
 *
 * The strip hides itself when there are no active milestones so the
 * detail page stays compact for projects without milestone planning.
 */
import { Card } from "@/components/ui/card";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Progress } from "@/components/ui/progress";

import type { MilestoneDto } from "../api/client.js";

export interface MilestonesStripProps {
  milestones: MilestoneDto[];
  /** Currently adopted primary milestone id (from
   *  `ProjectDto.primary_milestone_id`). Used to render the
   *  `★ primary` chip. `null` when no primary is set. */
  primaryMilestoneId?: string | null;
  /** Optional adopt-handler (PROJECT-VIEW.md §9.5). When omitted
   *  the overflow `⋯` menu still renders if any other action is
   *  available. Passing `null` clears the pointer. */
  onAdoptPrimary?: (milestoneId: string | null) => void;
  /** Optional "Filter to milestone" handler. Receives the
   *  milestone id; caller is responsible for appending /
   *  replacing the `milestone:<id>` chip on the workbench URL
   *  filter. When omitted the menu item is hidden. */
  onFilterToMilestone?: (milestoneId: string) => void;
  /** Disable the overflow item while an adopt request is in
   *  flight to prevent overlapping mutations. */
  adoptBusy?: boolean;
  /** Show a single-line skeleton while the first fetch is in
   *  flight so the strip's vertical real estate doesn't pop in. */
  isPending?: boolean;
}

export function MilestonesStrip({
  milestones,
  primaryMilestoneId,
  onAdoptPrimary,
  onFilterToMilestone,
  adoptBusy,
  isPending,
}: MilestonesStripProps): JSX.Element | null {
  if (isPending) {
    return (
      <div
        className="flex h-16 items-center text-sm text-muted-foreground"
        data-testid="project-milestones-strip-loading"
      >
        Loading milestones…
      </div>
    );
  }
  if (milestones.length === 0) return null;
  return (
    <div
      className="flex flex-wrap gap-2"
      data-testid="project-milestones-strip"
    >
      {milestones.map((m) => (
        <MilestoneCard
          key={m.id}
          milestone={m}
          isPrimary={primaryMilestoneId === m.id}
          onAdoptPrimary={onAdoptPrimary}
          onFilterToMilestone={onFilterToMilestone}
          adoptBusy={adoptBusy}
        />
      ))}
    </div>
  );
}

interface MilestoneCardProps {
  milestone: MilestoneDto;
  isPrimary: boolean;
  onAdoptPrimary?: (milestoneId: string | null) => void;
  onFilterToMilestone?: (milestoneId: string) => void;
  adoptBusy?: boolean;
}

function MilestoneCard({
  milestone,
  isPrimary,
  onAdoptPrimary,
  onFilterToMilestone,
  adoptBusy,
}: MilestoneCardProps): JSX.Element {
  const total = milestone.open_issues + milestone.closed_issues;
  const pct = total === 0 ? 0 : (milestone.closed_issues / total) * 100;
  const due = relativeDueLabel(milestone.due_on);
  return (
    <Card
      className="flex min-w-56 flex-col gap-1 p-3"
      data-testid={`project-milestone-card-${milestone.id}`}
    >
      <div className="flex items-center justify-between gap-1">
        <div
          className="truncate text-sm font-medium"
          title={milestone.title}
        >
          {milestone.title}
        </div>
        <div className="flex items-center gap-1">
          {isPrimary && (
            <span
              className="rounded bg-amber-100 px-1.5 py-0.5 text-xs font-medium text-amber-900 dark:bg-amber-900/40 dark:text-amber-100"
              data-testid={`project-milestone-primary-${milestone.id}`}
              title="Project's primary milestone (PROJECT-VIEW.md §5.5)"
            >
              ★ primary
            </span>
          )}
          {onAdoptPrimary && (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  className="text-muted-foreground hover:text-foreground"
                  aria-label={`Actions for milestone ${milestone.title}`}
                  data-testid={`project-milestone-menu-${milestone.id}`}
                >
                  ⋯
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                {isPrimary ? (
                  <DropdownMenuItem
                    disabled={adoptBusy}
                    onSelect={() => onAdoptPrimary(null)}
                    data-testid={`project-milestone-unadopt-${milestone.id}`}
                  >
                    Clear primary
                  </DropdownMenuItem>
                ) : (
                  <DropdownMenuItem
                    disabled={adoptBusy}
                    onSelect={() => onAdoptPrimary(milestone.id)}
                    data-testid={`project-milestone-adopt-${milestone.id}`}
                  >
                    Adopt as primary
                  </DropdownMenuItem>
                )}
                {onFilterToMilestone && (
                  <DropdownMenuItem
                    onSelect={() => onFilterToMilestone(milestone.id)}
                    data-testid={`project-milestone-filter-${milestone.id}`}
                  >
                    Filter to milestone
                  </DropdownMenuItem>
                )}
              </DropdownMenuContent>
            </DropdownMenu>
          )}
        </div>
      </div>
      <div
        className={
          due.overdue
            ? "text-xs text-destructive"
            : "text-xs text-muted-foreground"
        }
        data-testid={`project-milestone-due-${milestone.id}`}
      >
        {due.label}
      </div>
      <Progress value={pct} className="h-1.5" />
      <div className="text-xs text-muted-foreground">
        {milestone.closed_issues} / {total} closed
      </div>
    </Card>
  );
}

interface RelativeDueLabel {
  label: string;
  overdue: boolean;
}

/** Render the `due_on` field as a human-readable relative phrase.
 *  Days are computed off the *date* (no timestamps) to match the
 *  tz-agnostic `DATE` column on the server. We compare against the
 *  caller's local date so the strip reads "due today" on the user's
 *  calendar rather than UTC's. */
export function relativeDueLabel(
  dueOn: string | null,
  now: Date = new Date(),
): RelativeDueLabel {
  if (!dueOn) return { label: "no due date", overdue: false };
  // Compare on calendar dates only — strip the time-of-day so a
  // morning render of a "due today" milestone doesn't trip into
  // "overdue 1d" before midnight.
  const due = new Date(`${dueOn}T00:00:00`);
  const today = new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate(),
  );
  const diffDays = Math.round(
    (due.getTime() - today.getTime()) / (1000 * 60 * 60 * 24),
  );
  if (diffDays === 0) return { label: "due today", overdue: false };
  if (diffDays > 0) {
    return { label: `due in ${diffDays}d`, overdue: false };
  }
  return { label: `overdue ${-diffDays}d`, overdue: true };
}
