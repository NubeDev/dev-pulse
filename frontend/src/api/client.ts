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
// API wrapper
// ---------------------------------------------------------------------------

const CSRF_COOKIE = "starter_csrf";

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
    if (!res.ok) throw await StarterError.fromResponse(res);
    return schema.parse(await res.json());
  }

  private async postJson<TBody, TRes>(
    path: string,
    body: TBody | undefined,
    schema: z.ZodType<TRes>,
  ): Promise<TRes> {
    const csrf = readCookie(CSRF_COOKIE);
    const headers: Record<string, string> = { ...this.client.headers };
    if (body !== undefined) headers["content-type"] = "application/json";
    if (csrf) headers["X-CSRF-Token"] = csrf;
    const res = await this.client.fetch(`${this.client.baseUrl}${path}`, {
      method: "POST",
      credentials: "include",
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (!res.ok) throw await StarterError.fromResponse(res);
    return schema.parse(await res.json());
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
