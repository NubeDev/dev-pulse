import { StarterClient } from "@nube/starter-client-ts";
import { z } from "zod";
import { DpRestError } from "./error.js";
import {
  AckSchema,
  CountRowSchema,
  HomeOrgSplitRowSchema,
  reportResponseOf,
  type ReportResponse,
  type CountRow,
  type HomeOrgSplitRow,
  type Ack,
} from "./schemas/common.js";
import {
  OrgDtoSchema,
  TeamDtoSchema,
  UserDtoSchema,
  SetHomeOrgRequestSchema,
  SetUserRoleRequestSchema,
  AdminUserIdentitiesResponseSchema,
  type OrgDto,
  type TeamDto,
  type UserDto,
  type UserRole,
  type SetHomeOrgRequest,
  type SetUserRoleRequest,
  type AdminUserIdentitiesResponse,
} from "./schemas/directory.js";
import {
  FetchRunDtoSchema,
  UserExportSchema,
  RefreshResponseSchema,
  ImportRepoResponseSchema,
  type FetchRunDto,
  type UserExport,
  type RefreshResponse,
  type ImportRepoRequest,
  type ImportRepoResponse,
} from "./schemas/admin.js";
import {
  reportParamsToQuery,
  ProjectPortfolioResponseSchema,
  type ReportParams,
  type ProjectPortfolioRequest,
  type ProjectPortfolioResponse,
} from "./schemas/reports.js";
import {
  PinDtoSchema,
  TagDtoSchema,
  TagDetailResponseSchema,
  LinkBatchResponseSchema,
  AppInstallBannerResponseSchema,
  type PinDto,
  type AddPinRequest,
  type PinKind,
  type ReorderRequest,
  type TagDto,
  type TagDetailResponse,
  type CreateTagRequest,
  type UpdateTagRequest,
  type LinkBatchRequest,
  type LinkBatchResponse,
  type AppInstallBannerResponse,
} from "./schemas/workflow.js";
import {
  IssueDtoSchema,
  CreateIssueResponseSchema,
  IssueListResponseSchema,
  IssueDatesDtoSchema,
  UserIssueStateDtoSchema,
  BulkInboxResponseSchema,
  GroupByOptionsResponseSchema,
  buildIssueListQs,
  type IssueDto,
  type CreateIssueRequest,
  type CreateIssueResponse,
  type UpdateIssueRequest,
  type IssueListResponse,
  type IssueDatesDto,
  type PatchIssueDatesRequest,
  type CreateCommentRequest,
  type ListIssuesQuery,
  type MarkSeenRequest,
  type SetInboxStateRequest,
  type UserIssueStateDto,
  type BulkInboxRequest,
  type BulkInboxResponse,
  type GroupByOptionsResponse,
} from "./schemas/issues.js";
import {
  ProjectDtoSchema,
  ProjectListResponseSchema,
  BulkAddResultSchema,
  ProjectRepoDtoSchema,
  ProjectViewDtoSchema,
  MilestoneDtoSchema,
  BoardLinkDtoSchema,
  OrgProjectPickerDtoSchema,
  type ProjectDto,
  type ProjectListResponse,
  type ListProjectsQuery,
  type CreateProjectRequest,
  type PatchProjectRequest,
  type ArchiveProjectRequest,
  type BulkAddIssuesRequest,
  type BulkAddResult,
  type ProjectRepoDto,
  type ProjectViewDto,
  type ProjectViewWriteBody,
  type MilestoneDto,
  type CreateMilestoneRequest,
  type PatchMilestoneRequest,
  type BoardLinkDto,
  type CreateBoardLinkRequest,
  type OrgProjectPickerDto,
} from "./schemas/projects.js";
import {
  SettingDtoSchema,
  TestGithubPatResponseSchema,
  type SettingDto,
  type PutSettingRequest,
  type TestGithubPatResponse,
} from "./schemas/settings.js";
import {
  RepoListResponseSchema,
  RepoMetadataDtoSchema,
  RepoPrSizeStatsDtoSchema,
  RepoCiStatsDtoSchema,
  RepoActivityHeatmapDtoSchema,
  RepoReviewVelocityDtoSchema,
  RepoContributorDiversityDtoSchema,
  type RepoListResponse,
  type ListReposQuery,
  type RepoMetadataDto,
  type RepoPrSizeStatsDto,
  type RepoCiStatsDto,
  type RepoActivityHeatmapDto,
  type RepoReviewVelocityDto,
  type RepoContributorDiversityDto,
} from "./schemas/repos.js";
import {
  ExecSummaryDtoSchema,
  ExecSummaryImageDtoSchema,
  ExecSummaryDocumentDtoSchema,
  ExecSummaryChangelogEntrySchema,
  type ExecSummaryDto,
  type ExecSummaryImageDto,
  type ExecSummaryDocumentDto,
  type ExecSummaryChangelogEntry,
  type PatchExecSummaryRequest,
  type AddChangelogEntryRequest,
  type ApproveExecSummaryRequest,
} from "./schemas/exec-summary.js";

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
 * Typed dp-rest client.
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

  async getReportFreshness(): Promise<ReportResponse<null>> {
    return this.getJson("/reports/freshness", reportResponseOf(z.null()));
  }

  async getReportUser(
    userId: string,
    params?: ReportParams,
  ): Promise<ReportResponse<CountRow[]>> {
    return this.getJson(
      `/reports/user/${encodeURIComponent(userId)}${reportParamsToQuery(params)}`,
      reportResponseOf(z.array(CountRowSchema)),
    );
  }

  async getReportTeam(
    teamId: string,
    params?: ReportParams,
  ): Promise<ReportResponse<CountRow[]>> {
    return this.getJson(
      `/reports/team/${encodeURIComponent(teamId)}${reportParamsToQuery(params)}`,
      reportResponseOf(z.array(CountRowSchema)),
    );
  }

  async getReportOrg(
    orgId: string,
    params?: ReportParams,
  ): Promise<ReportResponse<CountRow[]>> {
    return this.getJson(
      `/reports/org/${encodeURIComponent(orgId)}${reportParamsToQuery(params)}`,
      reportResponseOf(z.array(CountRowSchema)),
    );
  }

  async getReportHomeOrgSplit(
    params?: ReportParams,
  ): Promise<ReportResponse<HomeOrgSplitRow[]>> {
    return this.getJson(
      `/reports/home-org-split${reportParamsToQuery(params)}`,
      reportResponseOf(z.array(HomeOrgSplitRowSchema)),
    );
  }

  async getReportProjectPortfolio(
    req: ProjectPortfolioRequest,
  ): Promise<ProjectPortfolioResponse> {
    return this.postJson("/reports/project-portfolio", req, ProjectPortfolioResponseSchema);
  }

  // -- directory -----------------------------------------------------------

  async listOrgs(): Promise<OrgDto[]> {
    return this.getJson("/orgs", z.array(OrgDtoSchema));
  }

  async listMyOrgs(): Promise<OrgDto[]> {
    return this.getJson("/me/orgs", z.array(OrgDtoSchema));
  }

  async listTeams(orgId: string): Promise<TeamDto[]> {
    const q = new URLSearchParams({ org_id: orgId }).toString();
    return this.getJson(`/teams?${q}`, z.array(TeamDtoSchema));
  }

  async listUsers(orgId?: string): Promise<UserDto[]> {
    const q = orgId ? `?${new URLSearchParams({ org_id: orgId }).toString()}` : "";
    return this.getJson(`/users${q}`, z.array(UserDtoSchema));
  }

  async setHomeOrg(req: SetHomeOrgRequest): Promise<Ack> {
    return this.postJson("/home-org", SetHomeOrgRequestSchema.parse(req), AckSchema);
  }

  // -- admin ---------------------------------------------------------------

  async adminRefresh(opts: { org_id?: string; repo_id?: string } = {}): Promise<RefreshResponse> {
    const usp = new URLSearchParams();
    if (opts.org_id) usp.set("org_id", opts.org_id);
    if (opts.repo_id) usp.set("repo_id", opts.repo_id);
    const q = usp.toString();
    return this.postJson(`/admin/refresh${q ? `?${q}` : ""}`, undefined, RefreshResponseSchema);
  }

  async adminImportRepo(req: ImportRepoRequest): Promise<ImportRepoResponse> {
    return this.postJson("/admin/repos", req, ImportRepoResponseSchema);
  }

  async listRuns(opts: { limit?: number; offset?: number } = {}): Promise<FetchRunDto[]> {
    const usp = new URLSearchParams();
    if (opts.limit !== undefined) usp.set("limit", String(opts.limit));
    if (opts.offset !== undefined) usp.set("offset", String(opts.offset));
    const q = usp.toString();
    return this.getJson(`/admin/runs${q ? `?${q}` : ""}`, z.array(FetchRunDtoSchema));
  }

  async anonymiseUser(userId: string): Promise<Ack> {
    return this.postJson(`/admin/users/${encodeURIComponent(userId)}/anonymise`, undefined, AckSchema);
  }

  async exportUser(userId: string): Promise<UserExport> {
    return this.getJson(`/admin/users/${encodeURIComponent(userId)}/export`, UserExportSchema);
  }

  // -- operator role management (DOCS/SCOPE-AUTHZ-USERS.md §3) -------------

  async setUserRole(userId: string, role: UserRole): Promise<UserDto> {
    const body: SetUserRoleRequest = { role };
    return this.sendJson(
      "PUT",
      `/admin/users/${encodeURIComponent(userId)}/role`,
      SetUserRoleRequestSchema.parse(body),
      UserDtoSchema,
    );
  }

  async listUserIdentities(
    userId: string,
  ): Promise<AdminUserIdentitiesResponse> {
    return this.getJson(
      `/admin/users/${encodeURIComponent(userId)}/identities`,
      AdminUserIdentitiesResponseSchema,
    );
  }

  // -- pins (SCOPE-PROJECTS §6.4) -------------------------------------------

  async listPins(): Promise<PinDto[]> {
    return this.getJson("/me/pins", z.array(PinDtoSchema));
  }

  async addPin(req: AddPinRequest): Promise<PinDto> {
    return this.sendJson("POST", "/me/pins", req, PinDtoSchema);
  }

  async removePin(kind: PinKind, targetId: string): Promise<Ack> {
    return this.sendJson("DELETE", `/me/pins/${kind}/${encodeURIComponent(targetId)}`, undefined, AckSchema);
  }

  async reorderPins(req: ReorderRequest): Promise<Ack> {
    return this.sendJson("PUT", "/me/pins/order", req, AckSchema);
  }

  // -- tags (SCOPE-PROJECTS §7.5) -------------------------------------------

  async listTags(): Promise<TagDto[]> {
    return this.getJson("/tags", z.array(TagDtoSchema));
  }

  async listMyTags(): Promise<TagDto[]> {
    return this.getJson("/me/tags", z.array(TagDtoSchema));
  }

  async getTag(id: string, linksPage?: number): Promise<TagDetailResponse> {
    const q = linksPage !== undefined ? `?links_page=${linksPage}` : "";
    return this.getJson(`/tags/${encodeURIComponent(id)}${q}`, TagDetailResponseSchema);
  }

  async createTag(req: CreateTagRequest): Promise<TagDto> {
    return this.sendJson("POST", "/tags", req, TagDtoSchema);
  }

  async updateTag(id: string, req: UpdateTagRequest): Promise<TagDto> {
    return this.sendJson("PATCH", `/tags/${encodeURIComponent(id)}`, req, TagDtoSchema);
  }

  async linkTagTargets(id: string, req: LinkBatchRequest): Promise<LinkBatchResponse> {
    return this.sendJson("POST", `/tags/${encodeURIComponent(id)}/links`, req, LinkBatchResponseSchema);
  }

  async unlinkTagTargets(id: string, req: LinkBatchRequest): Promise<Ack> {
    return this.sendJson("DELETE", `/tags/${encodeURIComponent(id)}/links`, req, AckSchema);
  }

  // -- GitHub App permission banner -----------------------------------------

  async getAppInstallBanner(): Promise<AppInstallBannerResponse> {
    return this.getJson("/me/app-install-banner", AppInstallBannerResponseSchema);
  }

  // -- issues ---------------------------------------------------------------

  async getIssue(repoId: string, number: number): Promise<IssueDto> {
    return this.getJson(`/repos/${encodeURIComponent(repoId)}/issues/${number}`, IssueDtoSchema);
  }

  async getIssueById(id: string): Promise<IssueDto> {
    return this.getJson(`/issues/${encodeURIComponent(id)}`, IssueDtoSchema);
  }

  async listIssues(q: ListIssuesQuery = {}): Promise<IssueListResponse> {
    return this.getJson(`/issues${buildIssueListQs(q)}`, IssueListResponseSchema);
  }

  async listMyQueue(q: ListIssuesQuery = {}): Promise<IssueListResponse> {
    return this.getJson(`/me/queue${buildIssueListQs(q)}`, IssueListResponseSchema);
  }

  async markInboxSeen(issueIds: string[]): Promise<void> {
    if (issueIds.length === 0) return;
    return this.sendNoContent("POST", "/me/inbox/seen", { issue_ids: issueIds } satisfies MarkSeenRequest);
  }

  async setInboxState(issueId: string, req: SetInboxStateRequest): Promise<UserIssueStateDto> {
    return this.sendJson("PATCH", `/me/inbox/${encodeURIComponent(issueId)}`, req, UserIssueStateDtoSchema);
  }

  async bulkInbox(req: BulkInboxRequest): Promise<BulkInboxResponse> {
    if (req.issue_ids.length === 0) return { touched: 0 };
    return this.sendJson("POST", "/me/inbox/bulk", req, BulkInboxResponseSchema);
  }

  async createIssue(req: CreateIssueRequest): Promise<CreateIssueResponse> {
    return this.sendJson("POST", "/issues", req, CreateIssueResponseSchema);
  }

  async updateIssue(id: string, req: UpdateIssueRequest): Promise<IssueDto> {
    return this.sendJson("PATCH", `/issues/${encodeURIComponent(id)}`, req, IssueDtoSchema);
  }

  async getIssueDates(id: string): Promise<IssueDatesDto> {
    return this.getJson(`/issues/${encodeURIComponent(id)}/dates`, IssueDatesDtoSchema);
  }

  async patchIssueDates(id: string, req: PatchIssueDatesRequest): Promise<IssueDatesDto> {
    return this.sendJson("PATCH", `/issues/${encodeURIComponent(id)}/dates`, req, IssueDatesDtoSchema);
  }

  async commentOnIssue(id: string, req: CreateCommentRequest): Promise<IssueDto> {
    return this.sendJson("POST", `/issues/${encodeURIComponent(id)}/comments`, req, IssueDtoSchema);
  }

  async refreshIssue(id: string): Promise<IssueDto> {
    return this.sendJson("POST", `/issues/${encodeURIComponent(id)}/refresh`, undefined, IssueDtoSchema);
  }

  // -- repos ----------------------------------------------------------------

  async listRepos(q: ListReposQuery = {}): Promise<RepoListResponse> {
    const params = new URLSearchParams();
    if (q.org_id) params.set("org_id", q.org_id);
    if (q.q) params.set("q", q.q);
    if (q.limit !== undefined) params.set("limit", String(q.limit));
    if (q.offset !== undefined) params.set("offset", String(q.offset));
    const qs = params.toString();
    return this.getJson(`/repos${qs ? `?${qs}` : ""}`, RepoListResponseSchema);
  }

  async getRepoMetadata(repoId: string): Promise<RepoMetadataDto | null> {
    try {
      return await this.getJson(`/repos/${encodeURIComponent(repoId)}/metadata`, RepoMetadataDtoSchema);
    } catch (e) {
      if (e instanceof DpRestError && e.status === 404) return null;
      throw e;
    }
  }

  async getRepoPrSizeStats(
    repoId: string,
    q: { since?: string; until?: string } = {},
  ): Promise<RepoPrSizeStatsDto> {
    const params = new URLSearchParams();
    if (q.since) params.set("since", q.since);
    if (q.until) params.set("until", q.until);
    const qs = params.toString();
    return this.getJson(`/repos/${encodeURIComponent(repoId)}/pr-size-stats${qs ? `?${qs}` : ""}`, RepoPrSizeStatsDtoSchema);
  }

  async getRepoCiStats(
    repoId: string,
    q: { since?: string; until?: string } = {},
  ): Promise<RepoCiStatsDto> {
    const params = new URLSearchParams();
    if (q.since) params.set("since", q.since);
    if (q.until) params.set("until", q.until);
    const qs = params.toString();
    return this.getJson(`/repos/${encodeURIComponent(repoId)}/ci-stats${qs ? `?${qs}` : ""}`, RepoCiStatsDtoSchema);
  }

  async getRepoActivityHeatmap(
    repoId: string,
    q: { since?: string; until?: string; timezone?: string } = {},
  ): Promise<RepoActivityHeatmapDto> {
    const params = new URLSearchParams();
    if (q.since) params.set("since", q.since);
    if (q.until) params.set("until", q.until);
    if (q.timezone) params.set("timezone", q.timezone);
    const qs = params.toString();
    return this.getJson(`/repos/${encodeURIComponent(repoId)}/activity-heatmap${qs ? `?${qs}` : ""}`, RepoActivityHeatmapDtoSchema);
  }

  async getRepoReviewVelocity(
    repoId: string,
    q: { since?: string; until?: string } = {},
  ): Promise<RepoReviewVelocityDto> {
    const params = new URLSearchParams();
    if (q.since) params.set("since", q.since);
    if (q.until) params.set("until", q.until);
    const qs = params.toString();
    return this.getJson(`/repos/${encodeURIComponent(repoId)}/review-velocity${qs ? `?${qs}` : ""}`, RepoReviewVelocityDtoSchema);
  }

  async getRepoContributorDiversity(
    repoId: string,
    q: { since?: string; until?: string } = {},
  ): Promise<RepoContributorDiversityDto> {
    const params = new URLSearchParams();
    if (q.since) params.set("since", q.since);
    if (q.until) params.set("until", q.until);
    const qs = params.toString();
    return this.getJson(`/repos/${encodeURIComponent(repoId)}/contributor-diversity${qs ? `?${qs}` : ""}`, RepoContributorDiversityDtoSchema);
  }

  // -- projects -------------------------------------------------------------

  async listProjects(q: ListProjectsQuery = {}): Promise<ProjectListResponse> {
    const params = new URLSearchParams();
    if (q.org_id) params.set("org_id", q.org_id);
    if (q.status) params.set("status", q.status);
    if (q.q) params.set("q", q.q);
    if (q.limit !== undefined) params.set("limit", String(q.limit));
    if (q.offset !== undefined) params.set("offset", String(q.offset));
    if (q.count_only) params.set("count_only", "1");
    const qs = params.toString();
    return this.getJson(`/projects${qs ? `?${qs}` : ""}`, ProjectListResponseSchema);
  }

  async getProject(id: string): Promise<ProjectDto | null> {
    try {
      return await this.getJson(`/projects/${encodeURIComponent(id)}`, ProjectDtoSchema);
    } catch (e) {
      if (e instanceof DpRestError && e.status === 404) return null;
      throw e;
    }
  }

  async createProject(body: CreateProjectRequest): Promise<ProjectDto> {
    return this.sendJson("POST", "/projects", body, ProjectDtoSchema);
  }

  async patchProject(id: string, body: PatchProjectRequest): Promise<ProjectDto> {
    return this.sendJson("PATCH", `/projects/${encodeURIComponent(id)}`, body, ProjectDtoSchema);
  }

  async archiveProject(id: string, body: ArchiveProjectRequest): Promise<ProjectDto> {
    return this.sendJson("POST", `/projects/${encodeURIComponent(id)}/archive`, body, ProjectDtoSchema);
  }

  async listProjectIssues(
    projectId: string,
    q: {
      state?: "open" | "closed" | "all";
      q?: string;
      limit?: number;
      offset?: number;
      group_by?: string;
      filter?: string;
      sort?: string;
      view?: string;
    } = {},
  ): Promise<IssueListResponse> {
    const params = new URLSearchParams();
    if (q.state) params.set("state", q.state);
    if (q.q) params.set("q", q.q);
    if (q.limit !== undefined) params.set("limit", String(q.limit));
    if (q.offset !== undefined) params.set("offset", String(q.offset));
    if (q.group_by) params.set("group_by", q.group_by);
    if (q.filter) params.set("filter", q.filter);
    if (q.sort) params.set("sort", q.sort);
    if (q.view) params.set("view", q.view);
    const qs = params.toString();
    return this.getJson(
      `/projects/${encodeURIComponent(projectId)}/issues${qs ? `?${qs}` : ""}`,
      IssueListResponseSchema,
    );
  }

  async getProjectGroupByOptions(projectId: string): Promise<GroupByOptionsResponse> {
    return this.getJson(`/projects/${encodeURIComponent(projectId)}/group-by-options`, GroupByOptionsResponseSchema);
  }

  // -- saved views ----------------------------------------------------------

  async listProjectViews(projectId: string): Promise<ProjectViewDto[]> {
    return this.getJson(`/projects/${encodeURIComponent(projectId)}/views`, z.array(ProjectViewDtoSchema));
  }

  async createProjectView(projectId: string, body: ProjectViewWriteBody): Promise<ProjectViewDto> {
    return this.sendJson("POST", `/projects/${encodeURIComponent(projectId)}/views`, body, ProjectViewDtoSchema);
  }

  async updateProjectView(projectId: string, viewId: string, body: ProjectViewWriteBody): Promise<ProjectViewDto> {
    return this.sendJson("PATCH", `/projects/${encodeURIComponent(projectId)}/views/${encodeURIComponent(viewId)}`, body, ProjectViewDtoSchema);
  }

  async deleteProjectView(projectId: string, viewId: string): Promise<void> {
    try {
      await this.sendNoContent("DELETE", `/projects/${encodeURIComponent(projectId)}/views/${encodeURIComponent(viewId)}`, undefined);
    } catch (e) {
      if (e instanceof DpRestError && e.status === 404) return;
      throw e;
    }
  }

  async reorderProjectViews(projectId: string, orderedIds: string[]): Promise<ProjectViewDto[]> {
    return this.sendJson("POST", `/projects/${encodeURIComponent(projectId)}/views/reorder`, { ordered_ids: orderedIds }, z.array(ProjectViewDtoSchema));
  }

  // -- milestones -----------------------------------------------------------

  async listProjectMilestones(projectId: string, includeClosed = false): Promise<MilestoneDto[]> {
    const qs = includeClosed ? "?include_closed=true" : "";
    return this.getJson(`/projects/${encodeURIComponent(projectId)}/milestones${qs}`, z.array(MilestoneDtoSchema));
  }

  async adoptProjectMilestone(projectId: string, milestoneId: string | null): Promise<ProjectDto> {
    return this.sendJson("POST", `/projects/${encodeURIComponent(projectId)}/adopt-milestone`, { milestone_id: milestoneId }, ProjectDtoSchema);
  }

  async createProjectMilestone(projectId: string, body: CreateMilestoneRequest): Promise<MilestoneDto> {
    return this.sendJson("POST", `/projects/${encodeURIComponent(projectId)}/milestones`, body, MilestoneDtoSchema);
  }

  async patchProjectMilestone(projectId: string, milestoneId: string, body: PatchMilestoneRequest): Promise<MilestoneDto> {
    return this.sendJson("PATCH", `/projects/${encodeURIComponent(projectId)}/milestones/${encodeURIComponent(milestoneId)}`, body, MilestoneDtoSchema);
  }

  async deleteProjectMilestone(projectId: string, milestoneId: string): Promise<void> {
    try {
      await this.sendNoContent("DELETE", `/projects/${encodeURIComponent(projectId)}/milestones/${encodeURIComponent(milestoneId)}`, undefined);
    } catch (e) {
      if (e instanceof DpRestError && e.status === 404) return;
      throw e;
    }
  }

  // -- project issues -------------------------------------------------------

  async addIssuesToProject(projectId: string, body: BulkAddIssuesRequest): Promise<BulkAddResult> {
    return this.sendJson("POST", `/projects/${encodeURIComponent(projectId)}/issues`, body, BulkAddResultSchema);
  }

  async removeIssueFromProject(
    projectId: string,
    issueId: string,
    expectedVersion: number | null,
    viewId?: string | null,
  ): Promise<void> {
    const params = new URLSearchParams();
    if (viewId) {
      params.set("view", viewId);
    } else if (expectedVersion !== null) {
      params.set("expected_version", String(expectedVersion));
    }
    const qs = params.toString();
    try {
      await this.sendNoContent("DELETE", `/projects/${encodeURIComponent(projectId)}/issues/${encodeURIComponent(issueId)}${qs ? `?${qs}` : ""}`, undefined);
    } catch (e) {
      if (e instanceof DpRestError && e.status === 404) return;
      throw e;
    }
  }

  async getProjectForIssue(issueId: string): Promise<ProjectDto | null> {
    const schema = z.union([ProjectDtoSchema, z.null()]);
    return this.getJson(`/issues/${encodeURIComponent(issueId)}/project`, schema);
  }

  async listProjectRepos(projectId: string): Promise<ProjectRepoDto[]> {
    return this.getJson(`/projects/${encodeURIComponent(projectId)}/repos`, z.array(ProjectRepoDtoSchema));
  }

  async addProjectRepo(projectId: string, repoId: string): Promise<ProjectRepoDto> {
    return this.sendJson("PUT", `/projects/${encodeURIComponent(projectId)}/repos/${encodeURIComponent(repoId)}`, undefined, ProjectRepoDtoSchema);
  }

  async removeProjectRepo(projectId: string, repoId: string): Promise<void> {
    try {
      await this.sendNoContent("DELETE", `/projects/${encodeURIComponent(projectId)}/repos/${encodeURIComponent(repoId)}`, undefined);
    } catch (e) {
      if (e instanceof DpRestError && e.status === 404) return;
      throw e;
    }
  }

  // -- board links ----------------------------------------------------------

  async getOrgProjectsV2(orgId: string): Promise<OrgProjectPickerDto | null> {
    try {
      return await this.getJson(`/orgs/${encodeURIComponent(orgId)}/projects-v2`, OrgProjectPickerDtoSchema);
    } catch (e) {
      if (e instanceof DpRestError && (e.code === "upstream_unavailable" || e.code === "github_validation_failed")) return null;
      throw e;
    }
  }

  async listBoardLinks(projectId: string): Promise<BoardLinkDto[]> {
    return this.getJson(`/projects/${encodeURIComponent(projectId)}/board-links`, z.array(BoardLinkDtoSchema));
  }

  async createBoardLink(projectId: string, body: CreateBoardLinkRequest): Promise<BoardLinkDto> {
    return this.sendJson("POST", `/projects/${encodeURIComponent(projectId)}/board-links`, body, BoardLinkDtoSchema);
  }

  async createOrgProjectV2DateField(
    orgId: string,
    body: { project_node_id: string; name: string },
  ): Promise<{ node_id: string; name: string }> {
    return this.sendJson("POST", `/orgs/${encodeURIComponent(orgId)}/projects-v2/date-fields`, body, z.object({ node_id: z.string(), name: z.string() }));
  }

  async deleteBoardLink(projectId: string, linkId: string): Promise<void> {
    try {
      await this.sendNoContent("DELETE", `/projects/${encodeURIComponent(projectId)}/board-links/${encodeURIComponent(linkId)}`, undefined);
    } catch (e) {
      if (e instanceof DpRestError && e.status === 404) return;
      throw e;
    }
  }

  // -- settings -------------------------------------------------------------

  async listSettings(): Promise<SettingDto[]> {
    return this.getJson("/me/settings", z.array(SettingDtoSchema));
  }

  async getSetting(key: string): Promise<SettingDto> {
    return this.getJson(`/me/settings/${encodeURIComponent(key)}`, SettingDtoSchema);
  }

  async putSetting(key: string, value: string): Promise<SettingDto> {
    return this.sendJson("PUT", `/me/settings/${encodeURIComponent(key)}`, { value } satisfies PutSettingRequest, SettingDtoSchema);
  }

  async deleteSetting(key: string): Promise<Ack> {
    return this.sendJson("DELETE", `/me/settings/${encodeURIComponent(key)}`, undefined, AckSchema);
  }

  async testGithubPat(): Promise<TestGithubPatResponse> {
    return this.sendJson("POST", "/me/settings/github.pat/test", undefined, TestGithubPatResponseSchema);
  }

  // -- project exec summary (SCOPE-PROJECT-EXECUTIVE-SUMMARY.md §3.2) -------

  async getProjectExecSummary(projectId: string): Promise<ExecSummaryDto> {
    return this.getJson(
      `/projects/${encodeURIComponent(projectId)}/exec-summary`,
      ExecSummaryDtoSchema,
    );
  }

  async patchProjectExecSummary(
    projectId: string,
    body: PatchExecSummaryRequest,
  ): Promise<ExecSummaryDto> {
    return this.sendJson(
      "PATCH",
      `/projects/${encodeURIComponent(projectId)}/exec-summary`,
      body,
      ExecSummaryDtoSchema,
    );
  }

  async submitProjectExecSummary(
    projectId: string,
    opts: { force?: boolean } = {},
  ): Promise<ExecSummaryDto> {
    const qs = opts.force ? "?force=true" : "";
    return this.sendJson(
      "POST",
      `/projects/${encodeURIComponent(projectId)}/exec-summary/submit${qs}`,
      undefined,
      ExecSummaryDtoSchema,
    );
  }

  async approveProjectExecSummary(
    projectId: string,
    body: ApproveExecSummaryRequest = {},
  ): Promise<ExecSummaryDto> {
    return this.sendJson(
      "POST",
      `/projects/${encodeURIComponent(projectId)}/exec-summary/approve`,
      body,
      ExecSummaryDtoSchema,
    );
  }

  async revertProjectExecSummary(projectId: string): Promise<ExecSummaryDto> {
    return this.sendJson(
      "POST",
      `/projects/${encodeURIComponent(projectId)}/exec-summary/revert`,
      undefined,
      ExecSummaryDtoSchema,
    );
  }

  async uploadProjectExecSummaryImage(
    projectId: string,
    file: File,
    caption?: string,
  ): Promise<ExecSummaryImageDto> {
    return this.uploadMultipart(
      `/projects/${encodeURIComponent(projectId)}/exec-summary/images`,
      file,
      ExecSummaryImageDtoSchema,
      caption ? { caption } : undefined,
    );
  }

  async deleteProjectExecSummaryImage(
    projectId: string,
    imageId: string,
  ): Promise<void> {
    await this.sendNoContent(
      "DELETE",
      `/projects/${encodeURIComponent(projectId)}/exec-summary/images/${encodeURIComponent(imageId)}`,
      undefined,
    );
  }

  async uploadProjectExecSummaryDocument(
    projectId: string,
    file: File,
    fields: { title: string; doc_type?: string; notes?: string; required_action?: string },
  ): Promise<ExecSummaryDocumentDto> {
    return this.uploadMultipart(
      `/projects/${encodeURIComponent(projectId)}/exec-summary/documents`,
      file,
      ExecSummaryDocumentDtoSchema,
      fields,
    );
  }

  async patchProjectExecSummaryDocument(
    projectId: string,
    documentId: string,
    body: { title?: string; doc_type?: string | null; notes?: string | null; required_action?: string | null },
  ): Promise<ExecSummaryDocumentDto> {
    return this.sendJson(
      "PATCH",
      `/projects/${encodeURIComponent(projectId)}/exec-summary/documents/${encodeURIComponent(documentId)}`,
      body,
      ExecSummaryDocumentDtoSchema,
    );
  }

  async deleteProjectExecSummaryDocument(
    projectId: string,
    documentId: string,
  ): Promise<void> {
    await this.sendNoContent(
      "DELETE",
      `/projects/${encodeURIComponent(projectId)}/exec-summary/documents/${encodeURIComponent(documentId)}`,
      undefined,
    );
  }

  async addProjectExecSummaryChangelog(
    projectId: string,
    body: AddChangelogEntryRequest,
  ): Promise<ExecSummaryChangelogEntry> {
    return this.sendJson(
      "POST",
      `/projects/${encodeURIComponent(projectId)}/exec-summary/changelog`,
      body,
      ExecSummaryChangelogEntrySchema,
    );
  }

  async deleteProjectExecSummaryChangelog(
    projectId: string,
    entryId: string,
  ): Promise<void> {
    await this.sendNoContent(
      "DELETE",
      `/projects/${encodeURIComponent(projectId)}/exec-summary/changelog/${encodeURIComponent(entryId)}`,
      undefined,
    );
  }

  private async uploadMultipart<T>(
    path: string,
    file: File,
    schema: z.ZodType<T>,
    fields?: Record<string, string>,
  ): Promise<T> {
    const csrf = readCookie(CSRF_COOKIE);
    const headers: Record<string, string> = { ...this.client.headers };
    if (csrf) headers["X-CSRF-Token"] = csrf;
    const form = new FormData();
    form.append("file", file);
    if (fields) {
      for (const [k, v] of Object.entries(fields)) form.append(k, v);
    }
    const res = await this.client.fetch(`${this.client.baseUrl}${path}`, {
      method: "POST",
      credentials: "include",
      headers,
      body: form,
    });
    if (!res.ok) throw await DpRestError.fromResponse(res);
    return schema.parse(await res.json());
  }
}
