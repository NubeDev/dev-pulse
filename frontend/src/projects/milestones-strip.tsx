/**
 * `<MilestonesStrip>` — PROJECT-VIEW.md §5.5 horizontal-roadmap
 * strip of milestone nodes rendered above the workbench on the
 * project detail page.
 *
 * Renders as a connected timeline: closed milestones first
 * (COMPLETED, solid track segment behind them), then open ones
 * by due-date soonest first. The first open milestone is the
 * IN-PROGRESS node — pulsing dot + `Today` ticker below.
 *
 * Mutations live in the overflow `⋯` menu on each node:
 *
 *   * `Adopt as primary` / `Clear primary` (Slice 5 wiring) when
 *     `onAdoptPrimary` is provided.
 *   * `Filter to milestone` (Slice 3↔1 bridge) when
 *     `onFilterToMilestone` is provided — appends a
 *     `milestone:<id>` chip to the workbench filter, replacing any
 *     existing milestone chip (one milestone filter max).
 *   * `Edit`, `Close` / `Reopen`, `Delete` (Slice 1 GitHub
 *     writes).
 *
 * The `+ New milestone` ghost button sits outside the track
 * (below it, separated by `mt-3`) so it reads as a separate
 * action rather than another step on the roadmap.
 *
 * A persistent "Milestones" toggle button (collapse / expand)
 * sits in the section header; the pref is stored in
 * `localStorage` under `dp:projects:milestones-collapsed`.
 */
import { useState } from "react";
import { ChevronDown, ChevronRight, PlusIcon } from "lucide-react";

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Progress } from "@/components/ui/progress";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { HelpHint } from "@/components/help-hint";

import type { MilestoneDto } from "../api/client.js";

const COLLAPSE_KEY = "dp:projects:milestones-collapsed";

function readCollapsedPref(): boolean {
  if (typeof window === "undefined") return false;
  try {
    return window.localStorage.getItem(COLLAPSE_KEY) === "1";
  } catch {
    return false;
  }
}

function writeCollapsedPref(value: boolean): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(COLLAPSE_KEY, value ? "1" : "0");
  } catch {
    /* quota / disabled — pref stays in-memory this session */
  }
}

export interface MilestonesStripProps {
  milestones: MilestoneDto[];
  primaryMilestoneId?: string | null;
  onAdoptPrimary?: (milestoneId: string | null) => void;
  onFilterToMilestone?: (milestoneId: string) => void;
  adoptBusy?: boolean;
  isPending?: boolean;
  onCreateMilestone?: () => void;
  onEditMilestone?: (milestone: MilestoneDto) => void;
  onToggleMilestoneState?: (milestone: MilestoneDto) => void;
  onDeleteMilestone?: (milestone: MilestoneDto) => void;
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
  const [collapsed, setCollapsedState] = useState<boolean>(() =>
    readCollapsedPref(),
  );
  const setCollapsed = (next: boolean): void => {
    setCollapsedState(next);
    writeCollapsedPref(next);
  };

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
  if (milestones.length === 0 && !onCreateMilestone) return null;

  // "Completed" for the purposes of timeline layout = GitHub closed,
  // OR every attached issue is closed (open_issues === 0 with at
  // least one closed). Both kinds sort to the left so the
  // in-progress dot lands on the first node that's still actually
  // accruing work.
  const isCompleted = (m: MilestoneDto): boolean =>
    m.state === "closed" || (m.open_issues === 0 && m.closed_issues > 0);
  const closed = milestones.filter(isCompleted);
  const open = milestones.filter((m) => !isCompleted(m));
  const ordered = [...closed, ...open];
  const inProgressIndex = closed.length;
  const total = milestones.length;

  return (
    <div
      className="flex flex-col gap-2"
      data-testid="project-milestones-strip"
    >
      <div className="flex items-center gap-1.5 text-xs font-medium uppercase tracking-wide text-muted-foreground">
        <button
          type="button"
          onClick={() => setCollapsed(!collapsed)}
          className="-mx-1 flex items-center gap-1 rounded px-1 py-0.5 transition-colors hover:bg-muted hover:text-foreground"
          aria-expanded={!collapsed}
          aria-controls="project-milestones-body"
          data-testid="project-milestones-toggle"
          title={collapsed ? "Show milestones" : "Hide milestones"}
        >
          {collapsed ? (
            <ChevronRight className="h-3.5 w-3.5" />
          ) : (
            <ChevronDown className="h-3.5 w-3.5" />
          )}
          <span>Milestones</span>
          {total > 0 && (
            <span
              className="ml-1 rounded bg-muted px-1.5 py-0.5 text-[10px] normal-case tracking-normal text-muted-foreground"
              data-testid="project-milestones-count"
            >
              {closed.length}/{total} closed
            </span>
          )}
        </button>
        <HelpHint
          title="Milestones"
          body={[
            "Each node on the timeline is a GitHub milestone on one of this project's linked repos. Closed milestones come first, then open ones sorted by due-date.",
            "+ New milestone creates one on GitHub and mirrors it back to dev-pulse instantly. Pick the repo (auto-selected when only one is linked), set a title, and optionally a description and due date.",
            "Use the ⋯ menu on any node to Adopt as primary (★ chip + headline KPI), Filter to milestone (scopes the issue list), Edit, Close / Reopen, or Delete. Edit and Delete write through to GitHub.",
            "Click Milestones to collapse / expand the strip — the choice persists across projects.",
          ]}
        />
        {onCreateMilestone && !collapsed && (
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                onClick={onCreateMilestone}
                className="ml-1 flex size-6 items-center justify-center rounded-md border border-dashed border-muted-foreground/40 text-muted-foreground transition-colors hover:border-foreground hover:text-foreground"
                data-testid="project-milestone-create"
                aria-label="New milestone"
              >
                <PlusIcon className="size-3.5" />
              </button>
            </TooltipTrigger>
            <TooltipContent>New milestone</TooltipContent>
          </Tooltip>
        )}
      </div>
      {!collapsed && (
        <div id="project-milestones-body">
          <MilestoneTimeline
            ordered={ordered}
            inProgressIndex={inProgressIndex}
            primaryMilestoneId={primaryMilestoneId ?? null}
            onAdoptPrimary={onAdoptPrimary}
            onFilterToMilestone={onFilterToMilestone}
            onEditMilestone={onEditMilestone}
            onToggleMilestoneState={onToggleMilestoneState}
            onDeleteMilestone={onDeleteMilestone}
            adoptBusy={adoptBusy}
            writeBusy={writeBusy}
          />
        </div>
      )}
    </div>
  );
}

type TimelineStatus = "completed" | "in-progress" | "upcoming";

interface MilestoneTimelineProps {
  ordered: MilestoneDto[];
  inProgressIndex: number;
  primaryMilestoneId: string | null;
  onAdoptPrimary?: (milestoneId: string | null) => void;
  onFilterToMilestone?: (milestoneId: string) => void;
  onEditMilestone?: (milestone: MilestoneDto) => void;
  onToggleMilestoneState?: (milestone: MilestoneDto) => void;
  onDeleteMilestone?: (milestone: MilestoneDto) => void;
  adoptBusy?: boolean;
  writeBusy?: boolean;
}

/** Horizontal roadmap. Track line runs across the row of dots:
 *  primary-colored & solid up to (and including) the in-progress
 *  node, dashed muted-border after it. When there's only one
 *  milestone the track collapses to a tiny stub on either side
 *  of the lone dot so it still reads as a "point on a line"
 *  rather than a floating bullet. */
function MilestoneTimeline({
  ordered,
  inProgressIndex,
  primaryMilestoneId,
  onAdoptPrimary,
  onFilterToMilestone,
  onEditMilestone,
  onToggleMilestoneState,
  onDeleteMilestone,
  adoptBusy,
  writeBusy,
}: MilestoneTimelineProps): JSX.Element {
  const nodeCount = ordered.length;
  // Solid track fraction. With 1 node it's "before/after" so we
  // show 50% solid when in-progress, full when completed.
  const solidFraction =
    nodeCount === 0
      ? 0
      : nodeCount === 1
        ? inProgressIndex >= 1
          ? 1
          : ordered[0]!.state === "closed"
            ? 1
            : 0.5
        : inProgressIndex >= nodeCount
          ? 1
          : Math.min(1, inProgressIndex / (nodeCount - 1));

  return (
    <div className="relative w-full overflow-x-auto">
      <div
        className="relative grid items-stretch gap-6 px-4 py-6"
        style={{
          gridTemplateColumns: `repeat(${Math.max(nodeCount, 1)}, minmax(140px, 1fr))`,
          minWidth: `${Math.max(nodeCount * 160, 320)}px`,
        }}
      >
        {/* Background track — sits at the vertical centre of the row. */}
        <div
          className="pointer-events-none absolute top-1/2 left-4 right-4 flex h-[2px] -translate-y-1/2"
          aria-hidden="true"
        >
          <div
            className="h-full bg-primary"
            style={{ width: `${solidFraction * 100}%` }}
          />
          <div
            className="h-full flex-1"
            style={{
              backgroundImage:
                "repeating-linear-gradient(to right, hsl(var(--border)) 0 4px, transparent 4px 8px)",
            }}
          />
        </div>

        {ordered.map((m, i) => {
          const completed =
            m.state === "closed" ||
            (m.open_issues === 0 && m.closed_issues > 0);
          const status: TimelineStatus = completed
            ? "completed"
            : i === inProgressIndex
              ? "in-progress"
              : "upcoming";
          return (
            <MilestoneTimelineNode
              key={m.id}
              milestone={m}
              status={status}
              isPrimary={primaryMilestoneId === m.id}
              onAdoptPrimary={onAdoptPrimary}
              onFilterToMilestone={onFilterToMilestone}
              onEditMilestone={onEditMilestone}
              onToggleMilestoneState={onToggleMilestoneState}
              onDeleteMilestone={onDeleteMilestone}
              adoptBusy={adoptBusy}
              writeBusy={writeBusy}
            />
          );
        })}
      </div>
    </div>
  );
}

interface MilestoneTimelineNodeProps {
  milestone: MilestoneDto;
  status: TimelineStatus;
  isPrimary: boolean;
  onAdoptPrimary?: (milestoneId: string | null) => void;
  onFilterToMilestone?: (milestoneId: string) => void;
  onEditMilestone?: (milestone: MilestoneDto) => void;
  onToggleMilestoneState?: (milestone: MilestoneDto) => void;
  onDeleteMilestone?: (milestone: MilestoneDto) => void;
  adoptBusy?: boolean;
  writeBusy?: boolean;
}

function MilestoneTimelineNode({
  milestone,
  status,
  isPrimary,
  onAdoptPrimary,
  onFilterToMilestone,
  onEditMilestone,
  onToggleMilestoneState,
  onDeleteMilestone,
  adoptBusy,
  writeBusy,
}: MilestoneTimelineNodeProps): JSX.Element {
  const due = relativeDueLabel(milestone.due_on);
  const total = milestone.open_issues + milestone.closed_issues;
  const pct = total === 0 ? 0 : (milestone.closed_issues / total) * 100;
  const hasMenu =
    !!onAdoptPrimary ||
    !!onFilterToMilestone ||
    !!onEditMilestone ||
    !!onToggleMilestoneState ||
    !!onDeleteMilestone;

  const chip =
    status === "completed"
      ? {
          label: "COMPLETED",
          className: "bg-primary/15 text-primary border border-primary/20",
        }
      : status === "in-progress"
        ? {
            label: "IN PROGRESS",
            className:
              "bg-amber-100 text-amber-900 border border-amber-300 dark:bg-amber-900/40 dark:text-amber-100 dark:border-amber-700",
          }
        : {
            label: "UPCOMING",
            className: "bg-muted text-muted-foreground border border-border",
          };

  const dot =
    status === "completed"
      ? "h-4 w-4 rounded-full bg-primary border-[3px] border-background shadow"
      : status === "in-progress"
        ? "h-5 w-5 rounded-full bg-amber-500 border-[3px] border-background shadow ring-4 ring-amber-500/20 animate-pulse"
        : "h-4 w-4 rounded-full bg-background border-2 border-muted-foreground/40";

  const opacity =
    status === "upcoming" ? "opacity-70 hover:opacity-100" : "";

  return (
    <div
      className={`group relative z-10 flex min-w-0 flex-col items-center text-center transition-opacity ${opacity}`}
      data-testid={`project-milestone-card-${milestone.id}`}
    >
      <div className="mb-3 flex w-full flex-col items-center gap-1 px-1">
        <div className="flex w-full items-center justify-center gap-1">
          <span
            className={`inline-block rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wider ${chip.className}`}
          >
            {chip.label}
          </span>
          {isPrimary && (
            <span
              className="rounded bg-amber-100 px-1.5 py-0.5 text-[10px] font-medium text-amber-900 dark:bg-amber-900/40 dark:text-amber-100"
              data-testid={`project-milestone-primary-${milestone.id}`}
              title="Project's primary milestone (PROJECT-VIEW.md §5.5)"
            >
              ★
            </span>
          )}
          {hasMenu && (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  className="ml-0.5 text-muted-foreground hover:text-foreground"
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
        <div
          className="truncate text-sm font-semibold text-foreground"
          title={milestone.title}
        >
          {milestone.title}
        </div>
        <div
          className={
            due.overdue
              ? "text-[11px] font-medium text-destructive"
              : status === "in-progress"
                ? "text-[11px] font-medium text-amber-700 dark:text-amber-300"
                : "text-[11px] text-muted-foreground"
          }
          data-testid={`project-milestone-due-${milestone.id}`}
        >
          {due.label}
        </div>
      </div>

      <div className={dot} />

      <div className="mt-3 flex w-full flex-col items-center gap-1 px-2">
        <Progress value={pct} className="h-1 w-full" />
        <div className="text-[10px] text-muted-foreground">
          {milestone.closed_issues} / {total} closed
        </div>
        {status === "in-progress" && (
          <div className="mt-1 flex flex-col items-center">
            <div className="h-3 w-px bg-amber-500/60" />
            <span className="text-[9px] font-semibold uppercase tracking-widest text-amber-700 dark:text-amber-300">
              Today
            </span>
          </div>
        )}
      </div>
    </div>
  );
}

interface RelativeDueLabel {
  label: string;
  overdue: boolean;
}

/** Render the `due_on` field as a human-readable relative phrase.
 *  Days are computed off the *date* (no timestamps) to match the
 *  tz-agnostic `DATE` column on the server. */
export function relativeDueLabel(
  dueOn: string | null,
  now: Date = new Date(),
): RelativeDueLabel {
  if (!dueOn) return { label: "no due date", overdue: false };
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
