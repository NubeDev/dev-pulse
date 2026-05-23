import { z } from "zod";
import { isoDateTime, uuid } from "./common.js";

// ---------------------------------------------------------------------------
// Projects (linear-projects-v2.md §6 / §7.1)
// ---------------------------------------------------------------------------

export const ProjectStatusDtoSchema = z.enum([
  "active",
  "backlog",
  "done",
  "archived",
]);
export type ProjectStatusDto = z.infer<typeof ProjectStatusDtoSchema>;

export const ProjectDtoSchema = z.object({
  id: uuid,
  org_id: uuid,
  name: z.string(),
  description: z.string().nullable().optional(),
  lead_user_id: uuid.nullable().optional(),
  status: ProjectStatusDtoSchema,
  start_at: isoDateTime.nullable().optional(),
  due_at: isoDateTime.nullable().optional(),
  issue_count: z.number().int(),
  closed_issue_count: z.number().int(),
  board_link_count: z.number().int(),
  version: z.number().int(),
  created_by: uuid.nullable().optional(),
  created_at: isoDateTime,
  updated_at: isoDateTime,
  primary_milestone_id: uuid.nullable().optional(),
});
export type ProjectDto = z.infer<typeof ProjectDtoSchema>;

export const ProjectListResponseSchema = z.object({
  rows: z.array(ProjectDtoSchema),
  total: z.number().int(),
  limit: z.number().int(),
  offset: z.number().int(),
});
export type ProjectListResponse = z.infer<typeof ProjectListResponseSchema>;

export interface ListProjectsQuery {
  org_id?: string;
  status?: ProjectStatusDto;
  q?: string;
  limit?: number;
  offset?: number;
  count_only?: boolean;
}

// --- Project write surface ---

export const CreateProjectRequestSchema = z.object({
  org_id: uuid,
  name: z.string().min(1).max(200),
  description: z.string().nullable().optional(),
  lead_user_id: uuid.nullable().optional(),
  status: ProjectStatusDtoSchema.optional(),
  start_at: isoDateTime.nullable().optional(),
  due_at: isoDateTime.nullable().optional(),
});
export type CreateProjectRequest = z.infer<typeof CreateProjectRequestSchema>;

export interface PatchProjectRequest {
  expected_version: number;
  name?: string;
  description?: string | null;
  lead_user_id?: string | null;
  status?: ProjectStatusDto;
  start_at?: string | null;
  due_at?: string | null;
}

export interface ArchiveProjectRequest {
  expected_version: number;
}

export interface BulkAddIssuesRequest {
  expected_version: number;
  issue_ids: string[];
  view_id?: string | null;
}

export const BulkAddSkipDtoSchema = z.object({
  issue_id: uuid,
  reason: z.string(),
  existing_project_id: uuid.nullable().optional(),
});
export type BulkAddSkipDto = z.infer<typeof BulkAddSkipDtoSchema>;

export const BulkAddResultSchema = z.object({
  added: z.array(uuid),
  skipped: z.array(BulkAddSkipDtoSchema),
});
export type BulkAddResult = z.infer<typeof BulkAddResultSchema>;

export const BULK_ADD_ISSUE_CAP = 100;

export const ProjectRepoDtoSchema = z.object({
  project_id: uuid,
  repo_id: uuid,
  repo_org_id: uuid,
  repo_org_login: z.string(),
  repo_name: z.string(),
  added_by: uuid.nullable().optional(),
  added_at: isoDateTime,
});
export type ProjectRepoDto = z.infer<typeof ProjectRepoDtoSchema>;

// ---------------------------------------------------------------------------
// Project saved views — PROJECT-VIEW.md §6.1 / §7.1
// ---------------------------------------------------------------------------

export const ProjectViewFilterClauseSchema = z.discriminatedUnion("dim", [
  z.object({ dim: z.literal("status"), value: z.string() }),
  z.object({ dim: z.literal("assignee"), value: z.string() }),
  z.object({ dim: z.literal("label"), value: z.string() }),
  z.object({ dim: z.literal("tag"), key: z.string(), value: z.string() }),
  z.object({ dim: z.literal("milestone"), value: z.string() }),
]);
export type ProjectViewFilterClause = z.infer<typeof ProjectViewFilterClauseSchema>;

export const ProjectViewDtoSchema = z.object({
  id: uuid,
  project_id: uuid,
  owner_user_id: uuid,
  name: z.string(),
  group_by: z.string().nullable(),
  filter_clauses: z.array(ProjectViewFilterClauseSchema),
  sort: z.string(),
  position: z.number().int(),
  visibility: z.string(),
  start_date: z
    .string()
    .regex(/^\d{4}-\d{2}-\d{2}$/, "expected YYYY-MM-DD")
    .nullable()
    .optional(),
  due_date: z
    .string()
    .regex(/^\d{4}-\d{2}-\d{2}$/, "expected YYYY-MM-DD")
    .nullable()
    .optional(),
  categories: z.array(z.string()),
  created_at: isoDateTime,
  updated_at: isoDateTime,
  open_issue_count: z.number().int().optional(),
  total_issue_count: z.number().int().optional(),
});
export type ProjectViewDto = z.infer<typeof ProjectViewDtoSchema>;

export interface ProjectViewWriteBody {
  name: string;
  group_by: string | null;
  filter_clauses: ProjectViewFilterClause[];
  sort: string;
  start_date?: string | null;
  due_date?: string | null;
  categories?: string[];
}

// ---------------------------------------------------------------------------
// Milestones — PROJECT-VIEW.md §5.5
// ---------------------------------------------------------------------------

const dateOnly = z
  .string()
  .regex(/^\d{4}-\d{2}-\d{2}$/, "expected YYYY-MM-DD");

export const MilestoneDtoSchema = z.object({
  id: uuid,
  repo_id: uuid,
  github_number: z.number().int(),
  title: z.string(),
  description: z.string().nullable(),
  state: z.enum(["open", "closed"]),
  due_on: dateOnly.nullable(),
  open_issues: z.number().int(),
  closed_issues: z.number().int(),
  created_at: isoDateTime,
  updated_at: isoDateTime,
  closed_at: isoDateTime.nullable(),
});
export type MilestoneDto = z.infer<typeof MilestoneDtoSchema>;

export interface CreateMilestoneRequest {
  repo_id: string;
  title: string;
  description?: string | null;
  due_on?: string | null;
}

export interface PatchMilestoneRequest {
  title?: string;
  state?: "open" | "closed";
  description?: string | null;
  due_on?: string | null;
}

// ---------------------------------------------------------------------------
// Board links (linear-projects-v2.md §7.3)
// ---------------------------------------------------------------------------

export const DateFieldDtoSchema = z.object({
  node_id: z.string(),
  name: z.string(),
});
export type DateFieldDto = z.infer<typeof DateFieldDtoSchema>;

export const BoardPickerDtoSchema = z.object({
  node_id: z.string(),
  title: z.string(),
  url: z.string().nullable().optional(),
  number: z.number().int().nullable().optional(),
  date_fields: z.array(DateFieldDtoSchema),
});
export type BoardPickerDto = z.infer<typeof BoardPickerDtoSchema>;

export const OrgProjectPickerDtoSchema = z.object({
  boards: z.array(BoardPickerDtoSchema),
  fetched_at: isoDateTime,
});
export type OrgProjectPickerDto = z.infer<typeof OrgProjectPickerDtoSchema>;

export const BoardLinkDtoSchema = z.object({
  id: uuid,
  project_id: uuid,
  github_board_node_id: z.string(),
  github_board_title: z.string().nullable().optional(),
  github_board_url: z.string().nullable().optional(),
  github_board_cached_at: isoDateTime.nullable().optional(),
  start_field_node_id: z.string().nullable().optional(),
  due_field_node_id: z.string().nullable().optional(),
  last_mirror_at: isoDateTime.nullable().optional(),
  last_mirror_error: z.string().nullable().optional(),
  created_at: isoDateTime,
  updated_at: isoDateTime,
});
export type BoardLinkDto = z.infer<typeof BoardLinkDtoSchema>;

export const CreateBoardLinkRequestSchema = z.object({
  github_board_node_id: z.string().min(1),
  github_board_title: z.string().nullable().optional(),
  github_board_url: z.string().nullable().optional(),
  start_field_node_id: z.string().nullable().optional(),
  due_field_node_id: z.string().nullable().optional(),
});
export type CreateBoardLinkRequest = z.infer<typeof CreateBoardLinkRequestSchema>;
