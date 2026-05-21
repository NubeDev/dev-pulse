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
import { HelpHint } from "@/components/help-hint";

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
  /** Optional "+ New milestone" affordance. When provided, a
   *  ghost card renders at the end of the strip (and the strip
   *  itself stays visible even when `milestones` is empty so
   *  there's always somewhere to click). Caller owns the dialog
   *  / mutation. */
  onCreateMilestone?: () => void;
  /** Optional edit handler. When provided, the overflow menu
   *  surfaces an "Edit" entry that hands the milestone back to
   *  the caller (which typically opens an edit dialog). */
  onEditMilestone?: (milestone: MilestoneDto) => void;
  /** Optional close/reopen toggle. Verb is picked by the caller
   *  from `milestone.state`. */
  onToggleMilestoneState?: (milestone: MilestoneDto) => void;
  /** Optional delete handler. The caller is expected to confirm
   *  via `AlertDialog` (the strip itself stays free of
   *  destructive prompts). */
  onDeleteMilestone?: (milestone: MilestoneDto) => void;
  /** Mirrors `adoptBusy` for the edit/close/delete flow. */
  writeBusy?: boolean;
}

export function MilestonesStrip({
  milestones,
  primaryMilestoneId,
  onAdoptPrimary,
  onFilterToMilestone,
  adoptBusy,
  isPending,
  onCreateMilestone,
  onEditMilestone,
  onToggleMilestoneState,
  onDeleteMilestone,
  writeBusy,
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
  // Hide entirely only when there are no milestones AND no way
  // to create one — otherwise we'd strand the user without an
  // entry point on a fresh project.
  if (milestones.length === 0 && !onCreateMilestone) return null;
  return (
    <div
      className="flex flex-col gap-2"
      data-testid="project-milestones-strip"
    >
      <div className="flex items-center gap-1.5 text-xs font-medium uppercase tracking-wide text-muted-foreground">
        <span>Milestones</span>
        <HelpHint
          title="Milestones"
          body={[
            "Each card is a GitHub milestone on one of this project's linked repos. Cards are sorted by due-date, soonest first.",
            "+ New milestone creates one on GitHub and mirrors it back to dev-pulse instantly. Pick the repo (auto-selected when only one is linked), set a title, and optionally a description and due date.",
            "Use the ⋯ menu on any card to Adopt as primary (★ chip + headline KPI), Filter to milestone (scopes the issue list), Edit, Close / Reopen, or Delete. Edit and Delete write through to GitHub.",
            "Closed milestones are hidden by default — they're still available behind GitHub's milestones tab.",
          ]}
        />
      </div>
      <div className="flex flex-wrap gap-2">
        {milestones.map((m) => (
          <MilestoneCard
            key={m.id}
            milestone={m}
            isPrimary={primaryMilestoneId === m.id}
            onAdoptPrimary={onAdoptPrimary}
            onFilterToMilestone={onFilterToMilestone}
            onEditMilestone={onEditMilestone}
            onToggleMilestoneState={onToggleMilestoneState}
            onDeleteMilestone={onDeleteMilestone}
            adoptBusy={adoptBusy}
            writeBusy={writeBusy}
          />
        ))}
        {onCreateMilestone && (
          <button
            type="button"
            onClick={onCreateMilestone}
            className="flex min-w-40 items-center justify-center rounded-md border border-dashed border-muted-foreground/40 px-3 py-3 text-sm text-muted-foreground transition-colors hover:border-foreground hover:text-foreground"
            data-testid="project-milestone-create"
          >
            + New milestone
          </button>
        )}
      </div>
    </div>
  );
}

interface MilestoneCardProps {
  milestone: MilestoneDto;
  isPrimary: boolean;
  onAdoptPrimary?: (milestoneId: string | null) => void;
  onFilterToMilestone?: (milestoneId: string) => void;
  onEditMilestone?: (milestone: MilestoneDto) => void;
  onToggleMilestoneState?: (milestone: MilestoneDto) => void;
  onDeleteMilestone?: (milestone: MilestoneDto) => void;
  adoptBusy?: boolean;
  writeBusy?: boolean;
}

function MilestoneCard({
  milestone,
  isPrimary,
  onAdoptPrimary,
  onFilterToMilestone,
  onEditMilestone,
  onToggleMilestoneState,
  onDeleteMilestone,
  adoptBusy,
  writeBusy,
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
          {(onAdoptPrimary ||
            onFilterToMilestone ||
            onEditMilestone ||
            onToggleMilestoneState ||
            onDeleteMilestone) && (
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
                {onAdoptPrimary &&
                  (isPrimary ? (
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
                  ))}
                {onFilterToMilestone && (
                  <DropdownMenuItem
                    onSelect={() => onFilterToMilestone(milestone.id)}
                    data-testid={`project-milestone-filter-${milestone.id}`}
                  >
                    Filter to milestone
                  </DropdownMenuItem>
                )}
                {onEditMilestone && (
                  <DropdownMenuItem
                    disabled={writeBusy}
                    onSelect={() => onEditMilestone(milestone)}
                    data-testid={`project-milestone-edit-${milestone.id}`}
                  >
                    Edit
                  </DropdownMenuItem>
                )}
                {onToggleMilestoneState && (
                  <DropdownMenuItem
                    disabled={writeBusy}
                    onSelect={() => onToggleMilestoneState(milestone)}
                    data-testid={`project-milestone-toggle-${milestone.id}`}
                  >
                    {milestone.state === "closed" ? "Reopen" : "Close"}
                  </DropdownMenuItem>
                )}
                {onDeleteMilestone && (
                  <DropdownMenuItem
                    disabled={writeBusy}
                    onSelect={() => onDeleteMilestone(milestone)}
                    className="text-destructive focus:text-destructive"
                    data-testid={`project-milestone-delete-${milestone.id}`}
                  >
                    Delete…
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
