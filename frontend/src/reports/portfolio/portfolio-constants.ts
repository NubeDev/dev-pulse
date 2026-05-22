import type {
  PortfolioSort,
  ProjectStatusDto,
} from "../../api/client.js";

export const VALID_SORTS: ReadonlySet<PortfolioSort> = new Set<PortfolioSort>([
  "due_asc_nulls_last",
  "due_desc_nulls_last",
  "slip_days_desc",
  "progress_asc",
  "name_asc",
  "updated_desc",
]);

export const VALID_STATUSES: ReadonlySet<ProjectStatusDto> = new Set<ProjectStatusDto>([
  "active",
  "backlog",
  "done",
  "archived",
]);

export const PAGE_SIZE = 50;

export const DUE_TONE_CLASSES: Record<"ok" | "soon" | "overdue", string> = {
  ok: "border-transparent bg-emerald-100 text-emerald-900 dark:bg-emerald-900/40 dark:text-emerald-100",
  soon: "border-transparent bg-amber-100 text-amber-900 dark:bg-amber-900/40 dark:text-amber-100",
  overdue:
    "border-transparent bg-red-100 text-red-900 dark:bg-red-900/40 dark:text-red-100",
};

export const STATUS_TONE: Record<ProjectStatusDto, string> = {
  active:
    "bg-emerald-100 text-emerald-700 ring-emerald-500/20 " +
    "dark:bg-emerald-500/15 dark:text-emerald-300 dark:ring-emerald-400/20",
  backlog:
    "bg-slate-100 text-slate-700 ring-slate-500/20 " +
    "dark:bg-slate-500/15 dark:text-slate-300 dark:ring-slate-400/20",
  done:
    "bg-blue-100 text-blue-700 ring-blue-500/20 " +
    "dark:bg-blue-500/15 dark:text-blue-300 dark:ring-blue-400/20",
  archived:
    "bg-zinc-200/70 text-zinc-600 ring-zinc-500/20 " +
    "dark:bg-zinc-500/15 dark:text-zinc-400 dark:ring-zinc-400/20",
};

export const FILTER_STATUSES: ProjectStatusDto[] = [
  "active",
  "backlog",
  "done",
  "archived",
];

export const FILTER_STATUS_LABEL: Record<ProjectStatusDto, string> = {
  active: "Active",
  backlog: "Backlog",
  done: "Done",
  archived: "Archived",
};

export const SORT_PAIRS: Partial<
  Record<"name" | "due" | "progress", [PortfolioSort, PortfolioSort]>
> = {
  name: ["name_asc", "name_asc"],
  due: ["due_asc_nulls_last", "due_desc_nulls_last"],
  progress: ["progress_asc", "progress_asc"],
};
