/**
 * dev-pulse typed REST client.
 *
 * Stage 2 of the frontend job: a thin wrapper over `@nube/starter-client-ts`'s
 * `StarterClient` (which already owns the `/auth/*` cookie surface) that adds
 * one typed method per dp-rest endpoint in the OpenAPI snapshot
 * (`crates/dp-rest/tests/openapi.snapshot.json`).
 *
 * Why hand-rolled and not full codegen: dp-rest's `ReportResponse.rows` is
 * deliberately `serde_json::Value` on the server (the same envelope carries
 * five different row shapes), so codegen would emit `unknown` for the
 * interesting parts. The zod schemas here capture the discriminated row
 * shapes (`CountRow`, `HomeOrgSplitRow`) the §11.5 pages render, so the
 * frontend gets runtime validation and `z.infer` types in one go.
 *
 * Wire conventions mirrored from `crates/dp-rest/src/reports.rs`:
 * - Query strings: comma-separated UUIDs / enums (the server splits on `,`).
 * - All requests use `credentials: "include"` so the session cookie set by
 *   `POST /auth/login` reaches dp-rest on the same origin / proxy.
 * - Mutating endpoints echo the `starter_csrf` cookie back as `X-CSRF-Token`.
 *
 * Smoke (`pnpm --filter dev-pulse-frontend typecheck`):
 *   const r = await api.getReportUser(uid, { window_label: "last_7_days" });
 *   r.rows; // CountRow[] | null, fully typed.
 */

import { StarterClient, StarterError } from "@nube/starter-client-ts";
import { z } from "zod";

// ---------------------------------------------------------------------------
// Zod schemas — one per OpenAPI component. Types derived via `z.infer<...>`
// so the runtime validator and the static type can never drift.
// ---------------------------------------------------------------------------

/** RFC3339 instant string. The server emits UTC; we keep it as a string so
 *  the UI can pick its own `Date` / `Temporal` convention later. */
const isoDateTime = z.string().datetime({ offset: true });

/** UUID v4 string. dp-rest validates these server-side; we mirror the check
 *  so we throw `ZodError` close to the call site if a test passes garbage. */
const uuid = z.string().uuid();

export const AckSchema = z.object({
  ok: z.boolean(),
});
export type Ack = z.infer<typeof AckSchema>;

export const CountRowSchema = z.object({
  /** Stringified bucket key. UUID for `User|Team|Repo|Org`, RFC3339 for
   *  `Day|Week|Month` group-bys. */
  key: z.string(),
  /** Event count attributed to this bucket (i64 on the wire). */
  count: z.number().int(),
});
export type CountRow = z.infer<typeof CountRowSchema>;

export const HomeOrgSplitRowSchema = z.object({
  user_id: uuid,
  org_id: uuid,
  count: z.number().int(),
});
export type HomeOrgSplitRow = z.infer<typeof HomeOrgSplitRowSchema>;

export const DataAsOfSchema = z.object({
  /** Lens-aware headline timestamp (SCOPE §11.7). `null` for `per_org_split`
   *  and for `single_org` / `all_orgs_combined` when the requested orgs have
   *  no freshness entry yet. */
  headline: isoDateTime.nullable().optional(),
  /** Per-org reconciler freshness. Absent orgs are "pending", not "stale". */
  per_org: z.record(uuid, isoDateTime),
  reconciler_latest: isoDateTime.nullable().optional(),
  webhook_latest: isoDateTime.nullable().optional(),
});
export type DataAsOf = z.infer<typeof DataAsOfSchema>;

/** Echoed resolved window. dp-rest serialises `dp_domain::window::Window`
 *  verbatim; the OpenAPI snapshot leaves the shape open so we keep a
 *  passthrough record here and let `Window.label` etc. surface as `unknown`. */
export const ResolvedWindowSchema = z.object({
  start: isoDateTime,
  end: isoDateTime,
  label: z.string(),
  tz: z.string().optional(),
}).passthrough();
export type ResolvedWindow = z.infer<typeof ResolvedWindowSchema>;

/**
 * The envelope every `/reports/*` endpoint returns. The `rows` field is
 * generic so each method can narrow it to the actual row shape (see
 * `ReportResponseOf` below).
 */
export const ReportResponseSchema = z.object({
  resolved_window: ResolvedWindowSchema,
  data_as_of: DataAsOfSchema,
  /** `null` for `/reports/freshness`; per-route shape otherwise. */
  rows: z.unknown(),
});
export type ReportResponse<TRow = unknown> = {
  resolved_window: ResolvedWindow;
  data_as_of: DataAsOf;
  rows: TRow;
};

function reportResponseOf<TRow>(rowsSchema: z.ZodType<TRow>) {
  return z.object({
    resolved_window: ResolvedWindowSchema,
    data_as_of: DataAsOfSchema,
    rows: rowsSchema,
  });
}

export const OrgDtoSchema = z.object({
  id: uuid,
  github_id: z.number().int(),
  login: z.string(),
  name: z.string().nullable().optional(),
});
export type OrgDto = z.infer<typeof OrgDtoSchema>;

export const TeamDtoSchema = z.object({
  id: uuid,
  org_id: uuid,
  github_id: z.number().int(),
  slug: z.string(),
  name: z.string(),
});
export type TeamDto = z.infer<typeof TeamDtoSchema>;

export const UserDtoSchema = z.object({
  id: uuid,
  github_id: z.number().int(),
  login: z.string(),
  name: z.string().nullable().optional(),
  email: z.string().nullable().optional(),
});
export type UserDto = z.infer<typeof UserDtoSchema>;

export const MembershipDtoSchema = z.object({
  user_id: uuid,
  org_id: uuid,
  role: z.string(),
  joined_at: isoDateTime,
  home_org: uuid.nullable().optional(),
});
export type MembershipDto = z.infer<typeof MembershipDtoSchema>;

export const FetchRunDtoSchema = z.object({
  id: uuid,
  kind: z.string(),
  started: isoDateTime,
  finished: isoDateTime.nullable().optional(),
  items: z.number().int(),
  errors: z.number().int(),
  partial: z.boolean(),
});
export type FetchRunDto = z.infer<typeof FetchRunDtoSchema>;

export const ExportEventSchema = z.object({
  event_id: uuid,
  org_id: uuid,
  repo_id: uuid,
  kind: z.string(),
  ts: isoDateTime,
  roles: z.array(z.string()),
});
export type ExportEvent = z.infer<typeof ExportEventSchema>;

export const UserExportSchema = z.object({
  user: UserDtoSchema,
  memberships: z.array(MembershipDtoSchema),
  events: z.array(ExportEventSchema),
});
export type UserExport = z.infer<typeof UserExportSchema>;

/** `POST /admin/refresh` is a discriminated oneOf on `ran`. */
export const RefreshResponseSchema = z.discriminatedUnion("ran", [
  z.object({
    ran: z.literal(true),
    items: z.number().int(),
    errors: z.number().int(),
    partial: z.boolean(),
  }),
  z.object({
    ran: z.literal(false),
  }),
]);
export type RefreshResponse = z.infer<typeof RefreshResponseSchema>;

export const SetHomeOrgRequestSchema = z.object({
  user_id: uuid,
  org_id: uuid,
});
export type SetHomeOrgRequest = z.infer<typeof SetHomeOrgRequestSchema>;

// ---------------------------------------------------------------------------
// Report query shape. Mirrors `dp_rest::reports::ReportQuery` — the server
// expects comma-separated lists in the query string, so we accept ergonomic
// arrays here and join them on send.
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

/**
 * Query params for the report endpoints. All fields are optional; the
 * server fills defaults (`window_label=last_7_days`, `tz=UTC`,
 * `anchor=utc`, `scope_mode=single_org`).
 *
 * `custom_start` / `custom_end` are required iff `window_label === "custom"`;
 * this is enforced server-side, not here, so a partial caller still
 * gets a real `400` from dp-rest.
 */
export interface ReportParams {
  window_label?: WindowLabel;
  tz?: string;
  anchor?: WindowAnchor;
  /** RFC3339, UTC. Required when `window_label === "custom"`. */
  custom_start?: string;
  /** RFC3339, UTC. Required when `window_label === "custom"`. */
  custom_end?: string;
  scope_mode?: ScopeMode;
  group_by?: GroupBy;
  orgs?: string[];
  users?: string[];
  teams?: string[];
  /** Repo UUIDs — server narrows the event stream to these repos
   *  (CSV-encoded on the wire). */
  repos?: string[];
  /** snake_case `EventKind` names. */
  activity_types?: string[];
  /** snake_case `ActorRole` names. */
  actor_roles?: string[];
}

function reportParamsToQuery(params: ReportParams | undefined): string {
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
// Workflow surface (SCOPE-PROJECTS §6 / §7 / §8 / §13.6) DTOs.
// ---------------------------------------------------------------------------

/** §6.1 / §13.5 working assumption — data-model cap. Mirrored from
 *  `dp_domain::PIN_CAP`. */
export const PIN_CAP = 20;
/** §6.1 / §13.5 working assumption — sidebar render cap *after* tag
 *  expansion. Above this the overflow collapses into "…and N more". */
export const PIN_RENDER_CAP = 50;
/** §13.5 — group-by-tag cap. */
export const TAGS_GROUP_BY_CAP = 50;
/** §13.5 — soft warning threshold for a single tag's link count. */
export const TAG_LINK_WARN_THRESHOLD = 500;

export const PinKindSchema = z.enum(["repo", "tag"]);
export type PinKind = z.infer<typeof PinKindSchema>;

export const PinDtoSchema = z.object({
  kind: PinKindSchema,
  target_id: uuid,
  position: z.number().int(),
  pinned_at: isoDateTime,
});
export type PinDto = z.infer<typeof PinDtoSchema>;

export const AddPinRequestSchema = z.object({
  kind: PinKindSchema,
  target_id: uuid,
});
export type AddPinRequest = z.infer<typeof AddPinRequestSchema>;

export const PinKeyDtoSchema = z.object({
  kind: PinKindSchema,
  target_id: uuid,
});
export type PinKeyDto = z.infer<typeof PinKeyDtoSchema>;

export const ReorderRequestSchema = z.object({
  order: z.array(PinKeyDtoSchema),
});
export type ReorderRequest = z.infer<typeof ReorderRequestSchema>;

export const TagScopeKindSchema = z.enum(["user", "team", "org"]);
export type TagScopeKind = z.infer<typeof TagScopeKindSchema>;

export const TagLinkKindSchema = z.enum(["repo", "issue", "user", "team"]);
export type TagLinkKind = z.infer<typeof TagLinkKindSchema>;

export const TagDtoSchema = z.object({
  id: uuid,
  scope_kind: TagScopeKindSchema,
  scope_id: uuid,
  name: z.string(),
  color: z.string(),
  description: z.string().nullable().optional(),
  created_by: uuid,
  created_at: isoDateTime,
  archived_at: isoDateTime.nullable().optional(),
  /** Viewer-filtered count (§7.4). Never the true total. */
  visible_link_count: z.number().int(),
});
export type TagDto = z.infer<typeof TagDtoSchema>;

export const TagLinkDtoSchema = z.object({
  id: uuid,
  tag_id: uuid,
  kind: TagLinkKindSchema,
  target_id: uuid,
  added_by: uuid,
  added_at: isoDateTime,
});
export type TagLinkDto = z.infer<typeof TagLinkDtoSchema>;

export const TagDetailResponseSchema = z.object({
  tag: TagDtoSchema,
  links: z.array(TagLinkDtoSchema),
  links_page: z.number().int(),
  links_page_size: z.number().int(),
});
export type TagDetailResponse = z.infer<typeof TagDetailResponseSchema>;

export const CreateTagRequestSchema = z.object({
  scope_kind: TagScopeKindSchema,
  scope_id: uuid,
  name: z.string(),
  color: z.string(),
  description: z.string().nullable().optional(),
});
export type CreateTagRequest = z.infer<typeof CreateTagRequestSchema>;

export const UpdateTagRequestSchema = z.object({
  name: z.string().optional(),
  color: z.string().optional(),
  description: z.string().nullable().optional(),
  archived: z.boolean().optional(),
});
export type UpdateTagRequest = z.infer<typeof UpdateTagRequestSchema>;

export const LinkRequestItemSchema = z.object({
  kind: TagLinkKindSchema,
  target_id: uuid,
});
export type LinkRequestItem = z.infer<typeof LinkRequestItemSchema>;

export const LinkBatchRequestSchema = z.object({
  items: z.array(LinkRequestItemSchema),
});
export type LinkBatchRequest = z.infer<typeof LinkBatchRequestSchema>;

export const LinkBatchResponseSchema = z.object({
  linked: z.array(TagLinkDtoSchema),
  warning: z.string().optional(),
});
export type LinkBatchResponse = z.infer<typeof LinkBatchResponseSchema>;

/** §13.6 banner row. */
export const AppInstallBannerOrgDtoSchema = z.object({
  org_id: uuid,
  login: z.string(),
  name: z.string().nullable().optional(),
  writes_available: z.boolean(),
  manage_url: z.string().optional(),
  admin_copy_text: z.string(),
});
export type AppInstallBannerOrgDto = z.infer<typeof AppInstallBannerOrgDtoSchema>;

export const AppInstallBannerResponseSchema = z.object({
  request_issues_write: z.boolean(),
  orgs: z.array(AppInstallBannerOrgDtoSchema),
});
export type AppInstallBannerResponse = z.infer<typeof AppInstallBannerResponseSchema>;

// --- Issues write path (SCOPE-PROJECTS §8) --------------------------------
//
// The CAS-on-`version` write path. UI captures `version` at form load,
// submits as `expected_version`; the server returns the new row on
// success and `409 { code: "stale_local_version", current_version }`
// when the CAS misses (§8.3) — the frontend then reloads and reprompts.

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
  /** §8.2 — monotonically bumped on every fetched update *and* every
   *  optimistic local write. CAS token for the §8 write path. */
  version: z.number().int(),
  updated_at: isoDateTime,
  /** `true` when the caller has not yet marked this issue's current
   *  `version` as seen (`linear-projects-idea.md` §3.8). Only set on
   *  inbox-aware responses (`GET /me/queue`); absent or `false`
   *  elsewhere. Treat `undefined` as `false` at the call site. */
  unread: z.boolean().optional(),
});
export type IssueDto = z.infer<typeof IssueDtoSchema>;

export const CreateIssueRequestSchema = z.object({
  repo_id: uuid,
  title: z.string().min(1),
  body: z.string().optional(),
  labels: z.array(z.string()).optional(),
  assignees: z.array(z.string()).optional(),
  milestone: z.string().optional(),
});
export type CreateIssueRequest = z.infer<typeof CreateIssueRequestSchema>;

export const UpdateIssueRequestSchema = z.object({
  /** CAS token from form load (§8.2 step 1). */
  expected_version: z.number().int(),
  title: z.string().optional(),
  body: z.string().nullable().optional(),
  labels: z.array(z.string()).optional(),
  assignees: z.array(z.string()).optional(),
  milestone: z.string().nullable().optional(),
  state: z.enum(["open", "closed"]).optional(),
});
export type UpdateIssueRequest = z.infer<typeof UpdateIssueRequestSchema>;

// --- Issue dates (linear-projects-idea.md §3.10) -------------------------
//
// Local-first start / due dates with a best-effort Projects v2 mirror.
// `start_at` / `due_at` are nullable on the wire and uniformly serialised
// — clearing a side is just `{ start_at: null }`. The server returns the
// canonical row on GET (zero-filled when no row exists) and PATCH.

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

// --- §3.10 repo → Projects v2 link (admin pane) -----------------------------
//
// One row per (NubeIO repo, GitHub Projects v2 board). All four
// `*_node_id` fields carry GraphQL global ids — the picker shells
// them out of the live `projectsV2` envelope so the operator never
// has to handcraft them. `created_by` / `*_at` are server-stamped.

export const RepoProjectLinkDtoSchema = z.object({
  repo_id: uuid,
  project_node_id: z.string(),
  start_field_node_id: z.string().nullable().optional(),
  due_field_node_id: z.string().nullable().optional(),
  created_by: uuid.nullable().optional(),
  created_at: isoDateTime,
  updated_at: isoDateTime,
});
export type RepoProjectLinkDto = z.infer<typeof RepoProjectLinkDtoSchema>;

export const PutRepoProjectLinkRequestSchema = z.object({
  project_node_id: z.string().min(1),
  start_field_node_id: z.string().nullable().optional(),
  due_field_node_id: z.string().nullable().optional(),
});
export type PutRepoProjectLinkRequest = z.infer<
  typeof PutRepoProjectLinkRequestSchema
>;

// --- Projects (linear-projects-v2.md §6 / §7.1) ----------------------------
//
// First-class `dp_projects` surface — cross-repo issue membership,
// CAS via `version`, denormalised counts for the §6.1 sidebar and
// §6.2 progress bar. Slice A ships read + write CRUD; the
// board-link / mirror columns land in slice B (the DTO carries
// `board_link_count` today as `0` so the wire shape is stable).

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
});
export type ProjectDto = z.infer<typeof ProjectDtoSchema>;

export const ProjectListResponseSchema = z.object({
  rows: z.array(ProjectDtoSchema),
  total: z.number().int(),
  limit: z.number().int(),
  offset: z.number().int(),
});
export type ProjectListResponse = z.infer<typeof ProjectListResponseSchema>;

/** Query params for `GET /projects`. `count_only=1` collapses the
 *  envelope to `{ rows: [], total, limit: 0, offset }` — used by the
 *  §6.1 sidebar so the per-status badges never drag full row
 *  payloads over the wire. */
export interface ListProjectsQuery {
  org_id?: string;
  status?: ProjectStatusDto;
  q?: string;
  limit?: number;
  offset?: number;
  count_only?: boolean;
}

export const CreateCommentRequestSchema = z.object({
  expected_version: z.number().int(),
  body: z.string().min(1),
});
export type CreateCommentRequest = z.infer<typeof CreateCommentRequestSchema>;

// --- Issues list (SCOPE-PROJECTS §14.3 / §14.9) ----------------------------
//
// Dense row shape rendered by the workbench list pane. Backed by a
// minimal `GET /issues` taking `repos`, `tags`, `state`, `assignee`
// per §14.9 (group_by / sort axes are deferred to a follow-up slice).

export const IssueListItemSchema = z.object({
  id: uuid,
  repo_id: uuid,
  org_id: uuid,
  /** Short `owner/repo` label rendered in the row. The server can
   *  cheaply join through `repos`; we keep it nullable so the row
   *  still renders if the join is unavailable. */
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
  /** Inbox-only: `dp_issues.version > last_seen_version` for the
   *  caller (`linear-projects-idea.md` §3.8). Absent / `false`
   *  outside the `GET /me/queue` envelope. Treat `undefined` as
   *  `false` at the call site. */
  unread: z.boolean().optional(),
});
export type IssueListItem = z.infer<typeof IssueListItemSchema>;

/** Paginated envelope returned by `GET /issues` and `GET /repos`. */
export const IssueListResponseSchema = z.object({
  rows: z.array(IssueListItemSchema),
  total: z.number().int(),
  limit: z.number().int(),
  offset: z.number().int(),
});
export type IssueListResponse = z.infer<typeof IssueListResponseSchema>;

/** Repo summary row returned by `GET /repos`. */
export const RepoSummaryDtoSchema = z.object({
  id: uuid,
  org_id: uuid,
  org_login: z.string(),
  name: z.string(),
  slug: z.string(),
  open_issue_count: z.number().int(),
  last_activity_at: isoDateTime.nullable().optional(),
});
export type RepoSummaryDto = z.infer<typeof RepoSummaryDtoSchema>;

export const RepoListResponseSchema = z.object({
  rows: z.array(RepoSummaryDtoSchema),
  total: z.number().int(),
  limit: z.number().int(),
  offset: z.number().int(),
});
export type RepoListResponse = z.infer<typeof RepoListResponseSchema>;

/** Query params for `GET /issues` and `GET /me/queue`. All optional;
 *  the server clamps `limit` into `1..=200` (default 50) and treats
 *  negative `offset` as zero. `state` defaults to `"open"` server-side.
 *
 *  Array fields are sent as comma-separated strings (`repo_ids=a,b,c`)
 *  per `linear-projects-idea.md` §5.4 / §5.8. Each is AND-combined
 *  with the rest of the filter; within a single array, multi-value
 *  semantics match Linear (assignees / labels are AND, ids are OR). */
export interface ListIssuesQuery {
  /** Single-repo back-compat. Prefer `repo_ids` for multi-select. */
  repo_id?: string;
  /** Multi-repo OR; comma-joined wire-side. Empty array is omitted. */
  repo_ids?: string[];
  /** Single-org back-compat. */
  org_id?: string;
  /** Multi-org OR; comma-joined wire-side. */
  org_ids?: string[];
  state?: "open" | "closed" | "all";
  /** Single-assignee back-compat. Prefer `assignees`. */
  assignee?: string;
  /** Multi-assignee AND (Linear semantics — "issues assigned to all
   *  of these people"). */
  assignees?: string[];
  /** Multi-label AND. */
  labels?: string[];
  /** Author login (exact match). */
  author?: string;
  /** GitHub `state_reason` (`completed` / `not_planned` / `reopened`). */
  state_reason?: string;
  /** RFC3339 lower bound on `updated_at`. */
  updated_since?: string;
  /** Filter to rows with no assignee and no label — the Linear-style
   *  "Untriaged" smart view (`linear-projects-idea.md` §3.5). */
  untriaged?: boolean;
  /** Substring search on title. */
  q?: string;
  limit?: number;
  offset?: number;
}

/** Query params for `GET /repos`. */
export interface ListReposQuery {
  org_id?: string;
  q?: string;
  limit?: number;
  offset?: number;
}

// --- Inbox (linear-projects-idea.md §3.8 / §5.8) --------------------------

/** Tri-state inbox status, lower-case to match the SQL form in
 *  `dp_user_issue_state.status`. */
export const InboxStatusSchema = z.enum(["inbox", "snoozed", "done"]);
export type InboxStatus = z.infer<typeof InboxStatusSchema>;

/** Echo of one `dp_user_issue_state` row — returned by
 *  `PATCH /me/inbox/{issue_id}` so the UI can confirm the write
 *  without a follow-up GET. */
export const UserIssueStateDtoSchema = z.object({
  issue_id: uuid,
  last_seen_version: z.number().int(),
  status: InboxStatusSchema,
  snoozed_until: isoDateTime.nullable().optional(),
  updated_at: isoDateTime,
});
export type UserIssueStateDto = z.infer<typeof UserIssueStateDtoSchema>;

/** Body for `POST /me/inbox/seen`. Capped at 200 ids per request
 *  (server-enforced, `SEEN_BATCH_CAP`). */
export interface MarkSeenRequest {
  issue_ids: string[];
}

/** Body for `PATCH /me/inbox/{issue_id}`. Either field may be
 *  absent; omitting both leaves the row at defaults. Pass
 *  `snoozed_until: null` to clear the snooze deadline. */
export interface SetInboxStateRequest {
  status?: InboxStatus;
  snoozed_until?: string | null;
}

/** Operation kind for [`BulkInboxRequest`] — one of the four §3.8
 *  list-header bulk actions (mark-all-seen / snooze-all / done-all
 *  / inbox-all). Names match the snake_case wire form. */
export const BulkInboxOpSchema = z.enum([
  "mark_all_seen",
  "snooze_all",
  "done_all",
  "inbox_all",
]);
export type BulkInboxOp = z.infer<typeof BulkInboxOpSchema>;

/** Body for `POST /me/inbox/bulk`. `snoozed_until` is required for
 *  `snooze_all` and ignored otherwise. Capped at 200 ids per
 *  request (server-enforced). */
export interface BulkInboxRequest {
  issue_ids: string[];
  op: BulkInboxOp;
  snoozed_until?: string | null;
}

/** Response from `POST /me/inbox/bulk`. `touched` is the number of
 *  `dp_user_issue_state` rows the server upserted. */
export const BulkInboxResponseSchema = z.object({
  touched: z.number().int().nonnegative(),
});
export type BulkInboxResponse = z.infer<typeof BulkInboxResponseSchema>;

/**
 * Serialise a [`ListIssuesQuery`] into a `?key=value` query string
 * for `GET /issues` and `GET /me/queue`. Array fields are joined
 * with commas (the server splits on the same separator,
 * `csv_uuids` / `csv_strings` in `issues_read.rs`). Empty / undefined
 * fields are omitted; a leading `?` is included when at least one
 * param survives.
 */
function buildIssueListQs(q: ListIssuesQuery): string {
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

// ---------------------------------------------------------------------------
// API wrapper
// ---------------------------------------------------------------------------

const CSRF_COOKIE = "starter_csrf";

/**
 * dp-rest's structured error envelope. Captures the `code` field
 * surfaced by `crates/dp-rest/src/error.rs` so callers can switch on
 * stable codes (`"stale_local_version"`, `"writes_not_available_for_org"`,
 * `"batch_rejected"`, `"pin_cap_exceeded"`, …) without parsing
 * human-readable strings.
 *
 * The §8.3 stale-version reload UX hangs off this — the issue form
 * catches `DpRestError` with `code === "stale_local_version"` and
 * pulls `current_version` from `body` to drive the reload prompt.
 */
export class DpRestError extends Error {
  readonly status: number;
  readonly code: string;
  /** Full decoded JSON body, when the server sent one. The shape is
   *  per-code; see `error.rs` `WritesNotAvailableBody`,
   *  `BatchErrorBody`, `StaleLocalVersionBody`. */
  readonly body: Record<string, unknown> | undefined;

  constructor(status: number, code: string, message: string, body?: Record<string, unknown>) {
    super(message);
    this.name = "DpRestError";
    this.status = status;
    this.code = code;
    this.body = body;
  }

  static async fromResponse(res: Response): Promise<DpRestError> {
    let body: Record<string, unknown> | undefined;
    try {
      const j = (await res.clone().json()) as Record<string, unknown>;
      if (j && typeof j === "object") body = j;
    } catch {
      // not JSON — fall through.
    }
    const code = (body?.["code"] as string | undefined) ?? `http_${res.status}`;
    const message = (body?.["error"] as string | undefined)
      ?? (body?.["message"] as string | undefined)
      ?? `HTTP ${res.status}`;
    return new DpRestError(res.status, code, message, body);
  }
}

/** Type guard — narrows an unknown thrown value to a `DpRestError`. */
export function isDpRestError(e: unknown): e is DpRestError {
  return e instanceof DpRestError;
}

function readCookie(name: string): string | undefined {
  if (typeof document === "undefined") return undefined;
  for (const part of document.cookie.split(";")) {
    const [k, v] = part.trim().split("=");
    if (k === name) return v;
  }
  return undefined;
}

/**
 * Typed dp-rest client. Composes `StarterClient` (auth surface) and adds
 * one method per OpenAPI operation in the snapshot. The `client` field is
 * exposed so the auth provider in stage 4 can call `login` / `logout` /
 * `me` without going through this wrapper.
 */
export class DevPulseApi {
  readonly client: StarterClient;

  constructor(client: StarterClient) {
    this.client = client;
  }

  // -- internals ------------------------------------------------------------

  private async getJson<T>(
    path: string,
    schema: z.ZodType<T>,
  ): Promise<T> {
    const res = await this.client.fetch(`${this.client.baseUrl}${path}`, {
      credentials: "include",
      headers: this.client.headers,
    });
    if (!res.ok) throw await DpRestError.fromResponse(res);
    return schema.parse(await res.json());
  }

  private async sendJson<TBody, TRes>(
    method: "POST" | "PUT" | "PATCH" | "DELETE",
    path: string,
    body: TBody | undefined,
    schema: z.ZodType<TRes>,
  ): Promise<TRes> {
    const csrf = readCookie(CSRF_COOKIE);
    const headers: Record<string, string> = { ...this.client.headers };
    if (body !== undefined) headers["content-type"] = "application/json";
    if (csrf) headers["X-CSRF-Token"] = csrf;
    const res = await this.client.fetch(`${this.client.baseUrl}${path}`, {
      method,
      credentials: "include",
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (!res.ok) throw await DpRestError.fromResponse(res);
    return schema.parse(await res.json());
  }

  /** Body-less sibling of [`sendJson`] for `204 No Content` endpoints
   *  (`POST /me/inbox/seen`). Drains and discards the response body
   *  so the keep-alive socket can be reused. */
  private async sendNoContent<TBody>(
    method: "POST" | "PUT" | "PATCH" | "DELETE",
    path: string,
    body: TBody | undefined,
  ): Promise<void> {
    const csrf = readCookie(CSRF_COOKIE);
    const headers: Record<string, string> = { ...this.client.headers };
    if (body !== undefined) headers["content-type"] = "application/json";
    if (csrf) headers["X-CSRF-Token"] = csrf;
    const res = await this.client.fetch(`${this.client.baseUrl}${path}`, {
      method,
      credentials: "include",
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (!res.ok) throw await DpRestError.fromResponse(res);
    // Drain any body so the connection can be reused. `204` may
    // still carry an empty body that needs consuming.
    await res.text().catch(() => "");
  }

  private postJson<TBody, TRes>(
    path: string,
    body: TBody | undefined,
    schema: z.ZodType<TRes>,
  ): Promise<TRes> {
    return this.sendJson("POST", path, body, schema);
  }

  // -- reports --------------------------------------------------------------

  /** `GET /reports/freshness` — `rows` is always `null`. */
  async getReportFreshness(): Promise<ReportResponse<null>> {
    return this.getJson(
      "/reports/freshness",
      reportResponseOf(z.null()),
    );
  }

  /** `GET /reports/user/{user_id}`. */
  async getReportUser(
    userId: string,
    params?: ReportParams,
  ): Promise<ReportResponse<CountRow[]>> {
    return this.getJson(
      `/reports/user/${encodeURIComponent(userId)}${reportParamsToQuery(params)}`,
      reportResponseOf(z.array(CountRowSchema)),
    );
  }

  /** `GET /reports/team/{team_id}`. */
  async getReportTeam(
    teamId: string,
    params?: ReportParams,
  ): Promise<ReportResponse<CountRow[]>> {
    return this.getJson(
      `/reports/team/${encodeURIComponent(teamId)}${reportParamsToQuery(params)}`,
      reportResponseOf(z.array(CountRowSchema)),
    );
  }

  /** `GET /reports/org/{org_id}`. */
  async getReportOrg(
    orgId: string,
    params?: ReportParams,
  ): Promise<ReportResponse<CountRow[]>> {
    return this.getJson(
      `/reports/org/${encodeURIComponent(orgId)}${reportParamsToQuery(params)}`,
      reportResponseOf(z.array(CountRowSchema)),
    );
  }

  /** `GET /reports/home-org-split` — always `per_org_split` lens. */
  async getReportHomeOrgSplit(
    params?: ReportParams,
  ): Promise<ReportResponse<HomeOrgSplitRow[]>> {
    return this.getJson(
      `/reports/home-org-split${reportParamsToQuery(params)}`,
      reportResponseOf(z.array(HomeOrgSplitRowSchema)),
    );
  }

  // -- directory -----------------------------------------------------------

  /** `GET /orgs`. */
  async listOrgs(): Promise<OrgDto[]> {
    return this.getJson("/orgs", z.array(OrgDtoSchema));
  }

  /** `GET /teams?org_id=…`. */
  async listTeams(orgId: string): Promise<TeamDto[]> {
    const q = new URLSearchParams({ org_id: orgId }).toString();
    return this.getJson(`/teams?${q}`, z.array(TeamDtoSchema));
  }

  /** `GET /users?org_id=…` — `orgId` optional. */
  async listUsers(orgId?: string): Promise<UserDto[]> {
    const q = orgId ? `?${new URLSearchParams({ org_id: orgId }).toString()}` : "";
    return this.getJson(`/users${q}`, z.array(UserDtoSchema));
  }

  /** `POST /home-org` — atomic home-org flip. CSRF-protected. */
  async setHomeOrg(req: SetHomeOrgRequest): Promise<Ack> {
    return this.postJson("/home-org", SetHomeOrgRequestSchema.parse(req), AckSchema);
  }

  // -- admin ---------------------------------------------------------------

  /**
   * `POST /admin/refresh` — operator-triggered reconciler tick.
   * Pass `{ org_id }` to narrow to one org, `{ org_id, repo_id }` to narrow
   * to one repo. `repo_id` without `org_id` is rejected server-side.
   */
  async adminRefresh(opts: {
    org_id?: string;
    repo_id?: string;
  } = {}): Promise<RefreshResponse> {
    const usp = new URLSearchParams();
    if (opts.org_id) usp.set("org_id", opts.org_id);
    if (opts.repo_id) usp.set("repo_id", opts.repo_id);
    const q = usp.toString();
    return this.postJson(
      `/admin/refresh${q ? `?${q}` : ""}`,
      undefined,
      RefreshResponseSchema,
    );
  }

  /** `GET /admin/runs?limit=&offset=` — paginated run log. */
  async listRuns(opts: { limit?: number; offset?: number } = {}): Promise<FetchRunDto[]> {
    const usp = new URLSearchParams();
    if (opts.limit !== undefined) usp.set("limit", String(opts.limit));
    if (opts.offset !== undefined) usp.set("offset", String(opts.offset));
    const q = usp.toString();
    return this.getJson(`/admin/runs${q ? `?${q}` : ""}`, z.array(FetchRunDtoSchema));
  }

  /** `POST /admin/users/{id}/anonymise` — irreversible GDPR cascade. */
  async anonymiseUser(userId: string): Promise<Ack> {
    return this.postJson(
      `/admin/users/${encodeURIComponent(userId)}/anonymise`,
      undefined,
      AckSchema,
    );
  }

  /**
   * `GET /admin/users/{id}/export` — full GDPR dump. The server streams
   * chunked JSON; we collect once and validate. For large exports, callers
   * should switch to a streaming consumer instead.
   */
  async exportUser(userId: string): Promise<UserExport> {
    return this.getJson(
      `/admin/users/${encodeURIComponent(userId)}/export`,
      UserExportSchema,
    );
  }

  // -- pins (SCOPE-PROJECTS §6.4) -------------------------------------------

  /** `GET /me/pins` — ordered list of caller's pins. */
  async listPins(): Promise<PinDto[]> {
    return this.getJson("/me/pins", z.array(PinDtoSchema));
  }

  /** `POST /me/pins` — append. Throws `DpRestError` with
   *  `code === "pin_cap_exceeded"` past §13.5 cap. */
  async addPin(req: AddPinRequest): Promise<PinDto> {
    return this.sendJson("POST", "/me/pins", req, PinDtoSchema);
  }

  /** `DELETE /me/pins/{kind}/{target_id}`. */
  async removePin(kind: PinKind, targetId: string): Promise<Ack> {
    return this.sendJson(
      "DELETE",
      `/me/pins/${kind}/${encodeURIComponent(targetId)}`,
      undefined,
      AckSchema,
    );
  }

  /** `PUT /me/pins/order` — atomic full-set reorder (§6.4). */
  async reorderPins(req: ReorderRequest): Promise<Ack> {
    return this.sendJson("PUT", "/me/pins/order", req, AckSchema);
  }

  // -- tags (SCOPE-PROJECTS §7.5) -------------------------------------------

  /** `GET /tags` — visible tags + viewer-filtered link counts (§7.4). */
  async listTags(): Promise<TagDto[]> {
    return this.getJson("/tags", z.array(TagDtoSchema));
  }

  /** `GET /me/tags` — tags the caller owns or is a scope member of. */
  async listMyTags(): Promise<TagDto[]> {
    return this.getJson("/me/tags", z.array(TagDtoSchema));
  }

  /** `GET /tags/{id}?links_page=n`. */
  async getTag(id: string, linksPage?: number): Promise<TagDetailResponse> {
    const q = linksPage !== undefined ? `?links_page=${linksPage}` : "";
    return this.getJson(
      `/tags/${encodeURIComponent(id)}${q}`,
      TagDetailResponseSchema,
    );
  }

  /** `POST /tags` — create. */
  async createTag(req: CreateTagRequest): Promise<TagDto> {
    return this.sendJson("POST", "/tags", req, TagDtoSchema);
  }

  /** `PATCH /tags/{id}` — rename / recolour / archive. */
  async updateTag(id: string, req: UpdateTagRequest): Promise<TagDto> {
    return this.sendJson(
      "PATCH",
      `/tags/${encodeURIComponent(id)}`,
      req,
      TagDtoSchema,
    );
  }

  /** `POST /tags/{id}/links` — transactional all-or-nothing batch (§7.5). */
  async linkTagTargets(id: string, req: LinkBatchRequest): Promise<LinkBatchResponse> {
    return this.sendJson(
      "POST",
      `/tags/${encodeURIComponent(id)}/links`,
      req,
      LinkBatchResponseSchema,
    );
  }

  /** `DELETE /tags/{id}/links` — transactional all-or-nothing unlink. */
  async unlinkTagTargets(id: string, req: LinkBatchRequest): Promise<Ack> {
    return this.sendJson(
      "DELETE",
      `/tags/${encodeURIComponent(id)}/links`,
      req,
      AckSchema,
    );
  }

  // -- GitHub App permission banner (§8.4 / §13.6) --------------------------

  /** `GET /me/app-install-banner`. */
  async getAppInstallBanner(): Promise<AppInstallBannerResponse> {
    return this.getJson("/me/app-install-banner", AppInstallBannerResponseSchema);
  }

  // -- issue writes (SCOPE-PROJECTS §8.2) -----------------------------------
  //
  // The frontend captures `version` from the GET shape, submits it
  // back as `expected_version` on PATCH/comment. A `409 stale_local_version`
  // surfaces as `DpRestError` with `body.current_version` so the UI
  // can reload and re-prompt (§8.3).

  /** `GET /repos/{repo_id}/issues/{number}` — deep-link form. */
  async getIssue(repoId: string, number: number): Promise<IssueDto> {
    return this.getJson(
      `/repos/${encodeURIComponent(repoId)}/issues/${number}`,
      IssueDtoSchema,
    );
  }

  /** `GET /issues/{id}` — id-form, used by the drill-down detail drawer. */
  async getIssueById(id: string): Promise<IssueDto> {
    return this.getJson(`/issues/${encodeURIComponent(id)}`, IssueDtoSchema);
  }

  /** `GET /issues` — paginated drill-down list. The server returns a
   *  `{rows, total, limit, offset}` envelope so the UI can render
   *  `Showing X–Y of Z` without a second round-trip. */
  async listIssues(q: ListIssuesQuery = {}): Promise<IssueListResponse> {
    return this.getJson(
      `/issues${buildIssueListQs(q)}`,
      IssueListResponseSchema,
    );
  }

  /** `GET /me/queue` — caller's inbox. Accepts the same filter set
   *  as [`listIssues`]; rows include `unread` for the dot indicator
   *  (`linear-projects-idea.md` §3.8 / §5.4). Default landing view
   *  for the triage page. */
  async listMyQueue(q: ListIssuesQuery = {}): Promise<IssueListResponse> {
    return this.getJson(
      `/me/queue${buildIssueListQs(q)}`,
      IssueListResponseSchema,
    );
  }

  /** `POST /me/inbox/seen` — bulk-mark issues read. Idempotent;
   *  empty list is a no-op. Capped at 200 ids per request
   *  server-side. Returns `204`. */
  async markInboxSeen(issueIds: string[]): Promise<void> {
    if (issueIds.length === 0) return;
    return this.sendNoContent("POST", "/me/inbox/seen", {
      issue_ids: issueIds,
    } satisfies MarkSeenRequest);
  }

  /** `PATCH /me/inbox/{issue_id}` — set inbox status / snooze.
   *  Returns the resulting row so the UI can update the cache
   *  without a follow-up GET. */
  async setInboxState(
    issueId: string,
    req: SetInboxStateRequest,
  ): Promise<UserIssueStateDto> {
    return this.sendJson(
      "PATCH",
      `/me/inbox/${encodeURIComponent(issueId)}`,
      req,
      UserIssueStateDtoSchema,
    );
  }

  /** `POST /me/inbox/bulk` — bulk inbox transitions
   *  (mark-all-seen / snooze-all / done-all / inbox-all) per §3.8.
   *  Skips the round-trip when `issue_ids` is empty. */
  async bulkInbox(req: BulkInboxRequest): Promise<BulkInboxResponse> {
    if (req.issue_ids.length === 0) return { touched: 0 };
    return this.sendJson(
      "POST",
      "/me/inbox/bulk",
      req,
      BulkInboxResponseSchema,
    );
  }

  /** `GET /repos` — paginated repo list with open-issue counts. */
  async listRepos(q: ListReposQuery = {}): Promise<RepoListResponse> {
    const params = new URLSearchParams();
    if (q.org_id) params.set("org_id", q.org_id);
    if (q.q) params.set("q", q.q);
    if (q.limit !== undefined) params.set("limit", String(q.limit));
    if (q.offset !== undefined) params.set("offset", String(q.offset));
    const qs = params.toString();
    return this.getJson(`/repos${qs ? `?${qs}` : ""}`, RepoListResponseSchema);
  }

  // --- projects (linear-projects-v2.md §7.1) ---------------------------

  /** `GET /projects` — paginated project list. Pass
   *  `{ count_only: true }` for the §6.1 sidebar's per-status
   *  badge counts (server returns `{ rows: [], total, limit: 0,
   *  offset }`, the wire-cheap shape the spec calls for). */
  async listProjects(q: ListProjectsQuery = {}): Promise<ProjectListResponse> {
    const params = new URLSearchParams();
    if (q.org_id) params.set("org_id", q.org_id);
    if (q.status) params.set("status", q.status);
    if (q.q) params.set("q", q.q);
    if (q.limit !== undefined) params.set("limit", String(q.limit));
    if (q.offset !== undefined) params.set("offset", String(q.offset));
    if (q.count_only) params.set("count_only", "1");
    const qs = params.toString();
    return this.getJson(
      `/projects${qs ? `?${qs}` : ""}`,
      ProjectListResponseSchema,
    );
  }

  /** `POST /issues` — create. May throw `DpRestError` with
   *  `code === "writes_not_available_for_org"` per §8.4. */
  async createIssue(req: CreateIssueRequest): Promise<IssueDto> {
    return this.sendJson("POST", "/issues", req, IssueDtoSchema);
  }

  /** `PATCH /issues/{id}` — partial update. CAS on `expected_version`.
   *  Throws `DpRestError` with `code === "stale_local_version"` (with
   *  `body.current_version`) per §8.3, or `"writes_not_available_for_org"`
   *  per §8.4. */
  async updateIssue(id: string, req: UpdateIssueRequest): Promise<IssueDto> {
    return this.sendJson(
      "PATCH",
      `/issues/${encodeURIComponent(id)}`,
      req,
      IssueDtoSchema,
    );
  }

  /** `GET /issues/{id}/dates` — caller-readable §3.10 dates row.
   *  Returns zero-filled fields when no row exists yet so the UI
   *  picker never has to special-case "missing row". */
  async getIssueDates(id: string): Promise<IssueDatesDto> {
    return this.getJson(
      `/issues/${encodeURIComponent(id)}/dates`,
      IssueDatesDtoSchema,
    );
  }

  /** `PATCH /issues/{id}/dates` — local upsert with best-effort
   *  Projects v2 mirror. Pass `{ start_at: null }` to clear a side. */
  async patchIssueDates(
    id: string,
    req: PatchIssueDatesRequest,
  ): Promise<IssueDatesDto> {
    return this.sendJson(
      "PATCH",
      `/issues/${encodeURIComponent(id)}/dates`,
      req,
      IssueDatesDtoSchema,
    );
  }

  /** `POST /issues/{id}/comments` — same CAS contract. Returns
   *  the fresh `IssueDto` post-comment so the UI's `comment_count`
   *  / `updated_at` / `version` advance without a follow-up GET
   *  (server does a best-effort GitHub re-fetch on our behalf). */
  async commentOnIssue(id: string, req: CreateCommentRequest): Promise<IssueDto> {
    return this.sendJson(
      "POST",
      `/issues/${encodeURIComponent(id)}/comments`,
      req,
      IssueDtoSchema,
    );
  }

  /** `POST /issues/{id}/refresh` — fire-and-forget lazy resync.
   *  The server re-fetches the issue from GitHub, projects the
   *  payload through the regular ingest path, and returns the
   *  post-upsert row. Safe to call unconditionally on issue open:
   *  the route falls back to the stored row when no GitHub backend
   *  is wired, so it never 5xx's the UI. */
  async refreshIssue(id: string): Promise<IssueDto> {
    return this.sendJson(
      "POST",
      `/issues/${encodeURIComponent(id)}/refresh`,
      undefined,
      IssueDtoSchema,
    );
  }

  // --- §3.10 admin: repo → Projects v2 board link --------------------
  //
  // The picker GET is best-effort — the server proxies a GraphQL
  // query against the linked GitHub installation and returns the
  // raw `projectsV2` envelope. We surface it as `unknown` so the
  // page can render whatever fields GitHub returns without us
  // having to re-validate every node id schema here.

  /** `GET /repos/{id}/project-link` — 404 means no link configured. */
  async getRepoProjectLink(repoId: string): Promise<RepoProjectLinkDto | null> {
    try {
      return await this.getJson(
        `/repos/${encodeURIComponent(repoId)}/project-link`,
        RepoProjectLinkDtoSchema,
      );
    } catch (e) {
      if (e instanceof DpRestError && e.status === 404) return null;
      throw e;
    }
  }

  /** `PUT /repos/{id}/project-link` — upsert. Empty field strings
   *  are normalised to `null` on the server. */
  async putRepoProjectLink(
    repoId: string,
    body: PutRepoProjectLinkRequest,
  ): Promise<RepoProjectLinkDto> {
    return this.sendJson(
      "PUT",
      `/repos/${encodeURIComponent(repoId)}/project-link`,
      body,
      RepoProjectLinkDtoSchema,
    );
  }

  /** `DELETE /repos/{id}/project-link` — 204 on success (also when
   *  the row didn't exist; the route is idempotent). */
  async deleteRepoProjectLink(repoId: string): Promise<void> {
    try {
      await this.sendNoContent(
        "DELETE",
        `/repos/${encodeURIComponent(repoId)}/project-link`,
        undefined,
      );
    } catch (e) {
      if (e instanceof DpRestError && e.status === 404) return;
      throw e;
    }
  }

  /** `GET /repos/{id}/projects` — picker payload. Returns the raw
   *  `projectsV2` envelope GitHub sent so the admin pane can list
   *  boards + their `dateFields`. Returns `null` on 503 (means
   *  the deployment has no GraphQL transport wired and the operator
   *  must paste node ids by hand). */
  async getRepoProjects(repoId: string): Promise<unknown | null> {
    const res = await this.client.fetch(
      `${this.client.baseUrl}/repos/${encodeURIComponent(repoId)}/projects`,
      { credentials: "include", headers: this.client.headers },
    );
    if (res.status === 503) return null;
    if (!res.ok) throw await DpRestError.fromResponse(res);
    return (await res.json()) as unknown;
  }
}

// ---------------------------------------------------------------------------
// Singleton
// ---------------------------------------------------------------------------

/**
 * Base URL for the dp-server REST surface.
 *
 * In dev the Vite proxy (see `vite.config.ts`) forwards `/reports`,
 * `/directory`, `/admin`, `/auth`, `/health`, `/openapi.json` to
 * `http://localhost:3000`, so the default base URL is the empty string —
 * `fetch("/reports/...")` hits the proxy and arrives at dp-server with the
 * session cookie attached (same-origin).
 *
 * In prod dp-server serves the static bundle from the same origin, so the
 * empty base URL still works. Override with `VITE_API_BASE_URL` if the UI
 * is hosted on a different origin (then dp-server needs CORS configured).
 */
const baseUrl = (import.meta.env.VITE_API_BASE_URL ?? "").replace(/\/$/, "");

/** Shared singleton — used by react-query hooks and the auth provider. */
export const api: DevPulseApi = new DevPulseApi(
  new StarterClient({ baseUrl }),
);

export { StarterClient, StarterError };
