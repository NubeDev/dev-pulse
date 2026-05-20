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

export const CreateCommentRequestSchema = z.object({
  expected_version: z.number().int(),
  body: z.string().min(1),
});
export type CreateCommentRequest = z.infer<typeof CreateCommentRequestSchema>;

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

  /** `GET /repos/{repo_id}/issues/{number}`. */
  async getIssue(repoId: string, number: number): Promise<IssueDto> {
    return this.getJson(
      `/repos/${encodeURIComponent(repoId)}/issues/${number}`,
      IssueDtoSchema,
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

  /** `POST /issues/{id}/comments` — same CAS contract. */
  async commentOnIssue(id: string, req: CreateCommentRequest): Promise<IssueDto> {
    return this.sendJson(
      "POST",
      `/issues/${encodeURIComponent(id)}/comments`,
      req,
      IssueDtoSchema,
    );
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
