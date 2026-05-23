import { z } from "zod";
import { isoDateTime, uuid } from "./common.js";
import { TagScopeKindSchema } from "./workflow.js";

// ---------------------------------------------------------------------------
// Issue tag chip (embedded on IssueDto)
// ---------------------------------------------------------------------------

export const IssueTagDtoSchema = z.object({
  id: uuid,
  name: z.string(),
  color: z.string(),
  scope_kind: TagScopeKindSchema,
});
export type IssueTagDto = z.infer<typeof IssueTagDtoSchema>;

// ---------------------------------------------------------------------------
// Issues (SCOPE-PROJECTS §8)
// ---------------------------------------------------------------------------

export const IssueDtoSchema = z.object({
  id: uuid,
  repo_id: uuid,
  org_id: uuid,
  number: z.number().int(),
  title: z.string(),
  body: z.string().nullable().optional(),
  state: z.enum(["open", "closed"]),
  labels: z.array(z.string()),
  assignees: z.array(z.string()),
  milestone: z.string().nullable().optional(),
  version: z.number().int(),
  updated_at: isoDateTime,
  unread: z.boolean().optional(),
  tags: z.array(IssueTagDtoSchema).optional(),
  is_local: z.boolean().optional(),
});
export type IssueDto = z.infer<typeof IssueDtoSchema>;

export const CreateIssueRequestSchema = z.object({
  repo_id: uuid,
  title: z.string().min(1),
  body: z.string().optional(),
  labels: z.array(z.string()).optional(),
  assignees: z.array(z.string()).optional(),
  milestone: z.string().optional(),
  project_id: uuid.optional(),
  view_id: uuid.optional(),
  expected_version: z.number().int().optional(),
  local: z.boolean().optional(),
});
export type CreateIssueRequest = z.infer<typeof CreateIssueRequestSchema>;

export const CreateIssueResponseSchema = z.object({
  repo_id: uuid,
  number: z.number().int(),
  issue_id: uuid.nullable().optional(),
});
export type CreateIssueResponse = z.infer<typeof CreateIssueResponseSchema>;

export const UpdateIssueRequestSchema = z.object({
  expected_version: z.number().int(),
  title: z.string().optional(),
  body: z.string().nullable().optional(),
  labels: z.array(z.string()).optional(),
  assignees: z.array(z.string()).optional(),
  milestone: z.string().nullable().optional(),
  state: z.enum(["open", "closed"]).optional(),
});
export type UpdateIssueRequest = z.infer<typeof UpdateIssueRequestSchema>;

// ---------------------------------------------------------------------------
// Issue dates (§3.10)
// ---------------------------------------------------------------------------

export const IssueDatesDtoSchema = z.object({
  issue_id: uuid,
  start_at: isoDateTime.nullable().optional(),
  due_at: isoDateTime.nullable().optional(),
  mirror_node_id: z.string().nullable().optional(),
  mirror_synced_at: isoDateTime.nullable().optional(),
  mirror_error: z.string().nullable().optional(),
  updated_at: isoDateTime,
});
export type IssueDatesDto = z.infer<typeof IssueDatesDtoSchema>;

export const PatchIssueDatesRequestSchema = z.object({
  start_at: isoDateTime.nullable().optional(),
  due_at: isoDateTime.nullable().optional(),
});
export type PatchIssueDatesRequest = z.infer<typeof PatchIssueDatesRequestSchema>;

export const CreateCommentRequestSchema = z.object({
  expected_version: z.number().int(),
  body: z.string().min(1),
});
export type CreateCommentRequest = z.infer<typeof CreateCommentRequestSchema>;

// ---------------------------------------------------------------------------
// Issues list (§14.3 / §14.9)
// ---------------------------------------------------------------------------

export const IssueListItemSchema = z.object({
  id: uuid,
  repo_id: uuid,
  org_id: uuid,
  repo_slug: z.string().nullable().optional(),
  number: z.number().int(),
  title: z.string(),
  body: z.string().nullable().optional(),
  milestone: z.string().nullable().optional(),
  version: z.number().int(),
  state: z.enum(["open", "closed"]),
  labels: z.array(z.string()),
  assignees: z.array(z.string()),
  updated_at: isoDateTime,
  unread: z.boolean().optional(),
  bucket_keys: z.array(z.string().nullable()).optional(),
  tags: z.array(IssueTagDtoSchema).optional(),
  is_local: z.boolean().optional(),
});
export type IssueListItem = z.infer<typeof IssueListItemSchema>;

export const IssueListResponseSchema = z.object({
  rows: z.array(IssueListItemSchema),
  total: z.number().int(),
  limit: z.number().int(),
  offset: z.number().int(),
  buckets: z
    .array(
      z.object({
        key: z.string().nullable(),
        label: z.string(),
        open: z.number().int(),
        closed: z.number().int(),
      }),
    )
    .optional(),
});
export type IssueListResponse = z.infer<typeof IssueListResponseSchema>;
export type IssueBucket = NonNullable<IssueListResponse["buckets"]>[number];

export const GroupByOptionsResponseSchema = z.object({
  dims: z.array(
    z.object({
      id: z.string(),
      label: z.string(),
    }),
  ),
});
export type GroupByOptionsResponse = z.infer<typeof GroupByOptionsResponseSchema>;
export type GroupByOption = GroupByOptionsResponse["dims"][number];

// ---------------------------------------------------------------------------
// Issue query & inbox types
// ---------------------------------------------------------------------------

export interface ListIssuesQuery {
  repo_id?: string;
  repo_ids?: string[];
  org_id?: string;
  org_ids?: string[];
  state?: "open" | "closed" | "all";
  assignee?: string;
  assignees?: string[];
  labels?: string[];
  author?: string;
  state_reason?: string;
  updated_since?: string;
  untriaged?: boolean;
  q?: string;
  limit?: number;
  offset?: number;
}

export const InboxStatusSchema = z.enum(["inbox", "snoozed", "done"]);
export type InboxStatus = z.infer<typeof InboxStatusSchema>;

export const UserIssueStateDtoSchema = z.object({
  issue_id: uuid,
  last_seen_version: z.number().int(),
  status: InboxStatusSchema,
  snoozed_until: isoDateTime.nullable().optional(),
  updated_at: isoDateTime,
});
export type UserIssueStateDto = z.infer<typeof UserIssueStateDtoSchema>;

export interface MarkSeenRequest {
  issue_ids: string[];
}

export interface SetInboxStateRequest {
  status?: InboxStatus;
  snoozed_until?: string | null;
}

export const BulkInboxOpSchema = z.enum([
  "mark_all_seen",
  "snooze_all",
  "done_all",
  "inbox_all",
]);
export type BulkInboxOp = z.infer<typeof BulkInboxOpSchema>;

export interface BulkInboxRequest {
  issue_ids: string[];
  op: BulkInboxOp;
  snoozed_until?: string | null;
}

export const BulkInboxResponseSchema = z.object({
  touched: z.number().int().nonnegative(),
});
export type BulkInboxResponse = z.infer<typeof BulkInboxResponseSchema>;

export function buildIssueListQs(q: ListIssuesQuery): string {
  const params = new URLSearchParams();
  if (q.repo_id) params.set("repo_id", q.repo_id);
  if (q.repo_ids?.length) params.set("repo_ids", q.repo_ids.join(","));
  if (q.org_id) params.set("org_id", q.org_id);
  if (q.org_ids?.length) params.set("org_ids", q.org_ids.join(","));
  if (q.state) params.set("state", q.state);
  if (q.assignee) params.set("assignee", q.assignee);
  if (q.assignees?.length) params.set("assignees", q.assignees.join(","));
  if (q.labels?.length) params.set("labels", q.labels.join(","));
  if (q.author) params.set("author", q.author);
  if (q.state_reason) params.set("state_reason", q.state_reason);
  if (q.updated_since) params.set("updated_since", q.updated_since);
  if (q.untriaged) params.set("untriaged", "true");
  if (q.q) params.set("q", q.q);
  if (q.limit !== undefined) params.set("limit", String(q.limit));
  if (q.offset !== undefined) params.set("offset", String(q.offset));
  const qs = params.toString();
  return qs ? `?${qs}` : "";
}
