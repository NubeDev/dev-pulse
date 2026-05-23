import { z } from "zod";
import { isoDateTime, uuid, ResolvedWindowSchema } from "./common.js";
import { ProjectStatusDtoSchema } from "./projects.js";

// ---------------------------------------------------------------------------
// Report query types
// ---------------------------------------------------------------------------

export type WindowLabel =
  | "today"
  | "yesterday"
  | "this_week"
  | "last_week"
  | "this_month"
  | "last_month"
  | "last_7_days"
  | "last_30_days"
  | "last_90_days"
  | "custom";

export type WindowAnchor = "viewer" | "org" | "utc";

export type ScopeMode = "single_org" | "all_orgs_combined" | "per_org_split";

export type GroupBy = "user" | "team" | "repo" | "org" | "day" | "week" | "month";

export interface ReportParams {
  window_label?: WindowLabel;
  tz?: string;
  anchor?: WindowAnchor;
  custom_start?: string;
  custom_end?: string;
  scope_mode?: ScopeMode;
  group_by?: GroupBy;
  orgs?: string[];
  users?: string[];
  teams?: string[];
  repos?: string[];
  activity_types?: string[];
  actor_roles?: string[];
}

export function reportParamsToQuery(params: ReportParams | undefined): string {
  if (!params) return "";
  const usp = new URLSearchParams();
  const csv = (k: string, v: string[] | undefined) => {
    if (v && v.length > 0) usp.set(k, v.join(","));
  };
  if (params.window_label) usp.set("window_label", params.window_label);
  if (params.tz) usp.set("tz", params.tz);
  if (params.anchor) usp.set("anchor", params.anchor);
  if (params.custom_start) usp.set("custom_start", params.custom_start);
  if (params.custom_end) usp.set("custom_end", params.custom_end);
  if (params.scope_mode) usp.set("scope_mode", params.scope_mode);
  if (params.group_by) usp.set("group_by", params.group_by);
  csv("orgs", params.orgs);
  csv("users", params.users);
  csv("teams", params.teams);
  csv("repos", params.repos);
  csv("activity_types", params.activity_types);
  csv("actor_roles", params.actor_roles);
  const s = usp.toString();
  return s ? `?${s}` : "";
}

// ---------------------------------------------------------------------------
// Portfolio report (SCOPE-PROJECT-REPORTS.md)
// ---------------------------------------------------------------------------

export const PortfolioSortSchema = z.enum([
  "due_asc_nulls_last",
  "due_desc_nulls_last",
  "slip_days_desc",
  "progress_asc",
  "name_asc",
  "updated_desc",
]);
export type PortfolioSort = z.infer<typeof PortfolioSortSchema>;

export const WindowSpecSchema = z
  .object({
    label: z.string(),
    tz: z.string(),
    anchor: z.enum(["viewer", "org", "utc"]),
    custom_start: isoDateTime.nullable().optional(),
    custom_end: isoDateTime.nullable().optional(),
  })
  .partial({ custom_start: true, custom_end: true });
export type WindowSpec = z.infer<typeof WindowSpecSchema>;

export const ProjectPortfolioRequestSchema = z.object({
  orgs: z.array(uuid).default([]),
  statuses: z.array(ProjectStatusDtoSchema).default([]),
  window: WindowSpecSchema.nullable().optional(),
  hide_overdue: z.boolean().default(false),
  sort: PortfolioSortSchema.default("due_asc_nulls_last"),
  limit: z.number().int().min(1).max(200).default(50),
  offset: z.number().int().min(0).default(0),
});
export type ProjectPortfolioRequest = z.input<typeof ProjectPortfolioRequestSchema>;

export const UserChipSchema = z.object({
  id: uuid,
  login: z.string(),
});
export type UserChip = z.infer<typeof UserChipSchema>;

export const ProjectPortfolioRowSchema = z.object({
  id: uuid,
  org_id: uuid,
  org_login: z.string(),
  name: z.string(),
  status: ProjectStatusDtoSchema,
  start_at: isoDateTime.nullable().optional(),
  due_at: isoDateTime.nullable().optional(),
  issue_count: z.number().int(),
  closed_issue_count: z.number().int(),
  progress_pct: z.number().int(),
  slip_days: z.number().int().nullable().optional(),
  issue_overdue_count: z.number().int(),
  lead: UserChipSchema.nullable().optional(),
  mirrored_to_github: z.boolean(),
  version: z.number().int(),
});
export type ProjectPortfolioRow = z.infer<typeof ProjectPortfolioRowSchema>;

export const PortfolioKpisSchema = z.object({
  total_projects: z.number().int(),
  on_track: z.number().int(),
  overdue: z.number().int(),
  completed: z.number().int(),
  avg_progress_pct: z.number().int(),
  total_issues_open: z.number().int(),
  total_issues_overdue: z.number().int(),
});
export type PortfolioKpis = z.infer<typeof PortfolioKpisSchema>;

export const ProjectPortfolioResponseSchema = z.object({
  rows: z.array(ProjectPortfolioRowSchema),
  resolved_window: ResolvedWindowSchema.nullable().optional(),
  now: isoDateTime,
  total: z.number().int(),
  limit: z.number().int(),
  offset: z.number().int(),
  kpis: PortfolioKpisSchema,
});
export type ProjectPortfolioResponse = z.infer<typeof ProjectPortfolioResponseSchema>;
