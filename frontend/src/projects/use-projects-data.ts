/**
 * React-Query bindings for the §6 Projects surface
 * (`linear-projects-v2.md`). Slice A only ships the read side the
 * sidebar needs — `useProjectCount(status)` for the §6.1 badges
 * and `useProjectList(query)` for the §6.2 list page that lands
 * in stage 6. The mutation hooks (create / patch / archive) land
 * with the dialogs in later stages.
 *
 * The sidebar counts are cheap: each hook fires
 * `GET /projects?status={s}&count_only=1`, which the server
 * answers with an empty `rows` array and a `total` int.
 * Three statuses (active / backlog / done) ⇒ three small probes
 * — same cost model as the workflow inbox badge (`useMyQueue`
 * with `limit: 1`).
 *
 * Cache keys live under `["projects", …]` so a logout flush
 * clears them alongside the other section caches.
 */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  api,
  type ArchiveProjectRequest,
  type BoardLinkDto,
  type BulkAddIssuesRequest,
  type BulkAddResult,
  type CreateBoardLinkRequest,
  type CreateIssueRequest,
  type CreateIssueResponse,
  type CreateProjectRequest,
  type GroupByOptionsResponse,
  type IssueListResponse,
  type ListProjectsQuery,
  type OrgProjectPickerDto,
  type PatchProjectRequest,
  type ProjectDto,
  type ProjectListResponse,
  type ProjectRepoDto,
  type ProjectStatusDto,
  type ProjectViewDto,
  type ProjectViewWriteBody,
  type CreateMilestoneRequest,
  type PatchMilestoneRequest,
  type MilestoneDto,
} from "../api/client.js";

/** Stable cache keys — `useQuery` invalidation surface for the
 *  later mutation hooks. */
export const projectsKeys = {
  count: (status: ProjectStatusDto) =>
    ["projects", "count", status] as const,
  list: (q: ListProjectsQuery) => ["projects", "list", q] as const,
  detail: (id: string) => ["projects", "detail", id] as const,
  boardLinks: (projectId: string) =>
    ["projects", "board-links", projectId] as const,
  orgPicker: (orgId: string) =>
    ["projects", "org-picker", orgId] as const,
  issues: (projectId: string, q: ListProjectIssuesQuery) =>
    ["projects", "issues", projectId, q] as const,
  groupByOptions: (projectId: string) =>
    ["projects", "group-by-options", projectId] as const,
  forIssue: (issueId: string) =>
    ["projects", "for-issue", issueId] as const,
  repos: (projectId: string) =>
    ["projects", "repos", projectId] as const,
  views: (projectId: string) =>
    ["projects", "views", projectId] as const,
  milestones: (projectId: string, includeClosed: boolean) =>
    ["projects", "milestones", projectId, includeClosed] as const,
};

/** Query shape for [`useProjectIssues`]. Mirrors the wire params. */
export interface ListProjectIssuesQuery {
  state?: "open" | "closed" | "all";
  q?: string;
  limit?: number;
  offset?: number;
  /** PROJECT-VIEW.md §5.1 — `status` or `tag:<key>`. When set, the
   *  response carries a `buckets` sidecar the workbench uses to
   *  render collapsible sections. */
  group_by?: string;
  /** PROJECT-VIEW.md §5.2/§5.4 — AND-combined chip string. */
  filter?: string;
  /** PROJECT-VIEW.md §5.3 — sort order. */
  sort?: string;
  /** PROJECT-VIEW.md §5.4 amendment — scope to a saved view's
   *  manual membership table. Omitted = "All" tab. */
  view?: string;
}

/** Per-status count probe, backed by `GET /projects?count_only=1`.
 *  Returns `0` while loading / on error so the sidebar never
 *  flashes a partial number — the spec deliberately hides the
 *  `Archived` count, so the worst case here is "live count not
 *  yet known" which a quiet zero handles cleanly. */
export function useProjectCount(status: ProjectStatusDto) {
  const query = useQuery<ProjectListResponse>({
    queryKey: projectsKeys.count(status),
    queryFn: () => api.listProjects({ status, count_only: true }),
    staleTime: 30_000,
    refetchOnWindowFocus: true,
  });
  return {
    ...query,
    count: query.data?.total ?? 0,
  };
}

/** Full row fetch for the §6.2 list page. Stage 5 ships the hook
 *  but not the consumer — the list page itself lands in stage 6. */
export function useProjectList(q: ListProjectsQuery) {
  return useQuery<ProjectListResponse>({
    queryKey: projectsKeys.list(q),
    queryFn: () => api.listProjects(q),
    staleTime: 30_000,
  });
}

/** Single-project read for the §6.3 detail page. Returns `null`
 *  when the project does not exist (or was archived and not in the
 *  caller's visible set) so the page can render a clean
 *  "not found" placeholder. */
export function useProject(id: string | null) {
  return useQuery<ProjectDto | null>({
    queryKey: id ? projectsKeys.detail(id) : ["projects", "detail", "(none)"],
    queryFn: () => (id ? api.getProject(id) : Promise.resolve(null)),
    enabled: !!id,
    staleTime: 15_000,
  });
}

/** `GET /projects/{id}/board-links` — list a project's linked
 *  GitHub Projects v2 boards. Surfaces per-link `last_mirror_at` /
 *  `last_mirror_error` so the §6.3 row can render mirror status
 *  inline (no second round-trip). */
export function useBoardLinks(projectId: string | null) {
  return useQuery<BoardLinkDto[]>({
    queryKey: projectId
      ? projectsKeys.boardLinks(projectId)
      : ["projects", "board-links", "(none)"],
    queryFn: () =>
      projectId ? api.listBoardLinks(projectId) : Promise.resolve([]),
    enabled: !!projectId,
    // Mirror status refreshes when the user re-opens the page —
    // background refetches add Prometheus noise without buying
    // much UX on a low-rate surface.
    staleTime: 30_000,
  });
}

/** `GET /orgs/{org_id}/projects-v2` — normalized org-wide picker
 *  for the §6.4 Link-a-board dialog. `null` payload ⇒ picker
 *  backend unconfigured / GitHub transport error — the dialog
 *  then renders the `[Open GitHub project settings]` hint
 *  instead of a node-id paste field (no node-id paste field on
 *  the primary path, per §6.4). */
export function useOrgBoardPicker(orgId: string | null, enabled: boolean) {
  return useQuery<OrgProjectPickerDto | null>({
    queryKey: orgId
      ? projectsKeys.orgPicker(orgId)
      : ["projects", "org-picker", "(none)"],
    queryFn: () => (orgId ? api.getOrgProjectsV2(orgId) : Promise.resolve(null)),
    enabled: !!orgId && enabled,
    // The picker is cache-on-the-server-but-rebuilds-on-every-call;
    // we keep the client cache hot for the 60s the dialog is
    // typically open so re-rendering the dropdown doesn't refire.
    staleTime: 60_000,
    retry: false,
  });
}

/** `POST /projects/{id}/board-links` — link a board to the
 *  project. Invalidates both the link list and the project detail
 *  query (the `board_link_count` denormal on `ProjectDto` advances). */
export function useCreateBoardLink(projectId: string) {
  const qc = useQueryClient();
  return useMutation<BoardLinkDto, Error, CreateBoardLinkRequest>({
    mutationFn: (body) => api.createBoardLink(projectId, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: projectsKeys.boardLinks(projectId) });
      qc.invalidateQueries({ queryKey: projectsKeys.detail(projectId) });
    },
  });
}

/** `POST /orgs/{org_id}/projects-v2/date-fields` — create a Date
 *  field on a Projects v2 board. Invalidates the org picker so
 *  the new field appears in the dialog dropdowns immediately. */
export function useCreateOrgProjectV2DateField(orgId: string) {
  const qc = useQueryClient();
  return useMutation<
    { node_id: string; name: string },
    Error,
    { project_node_id: string; name: string }
  >({
    mutationFn: (body) => api.createOrgProjectV2DateField(orgId, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: projectsKeys.orgPicker(orgId) });
    },
  });
}

/** `DELETE /projects/{id}/board-links/{link_id}` — unlink a board.
 *  §9.2 elevation is enforced server-side; the UI hides the
 *  control when the viewer is not the project's creator / lead,
 *  but the server is the source of truth. */
export function useDeleteBoardLink(projectId: string) {
  const qc = useQueryClient();
  return useMutation<void, Error, string>({
    mutationFn: (linkId) => api.deleteBoardLink(projectId, linkId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: projectsKeys.boardLinks(projectId) });
      qc.invalidateQueries({ queryKey: projectsKeys.detail(projectId) });
    },
  });
}

// ---------------------------------------------------------------------------
// Write-side mutations — §6.2 [+ New project] modal, §6.3 detail
// header edits / archive, §6.6 bulk-add from triage.
// ---------------------------------------------------------------------------

/** Invalidate every list / count probe after a write so the sidebar
 *  badges and the §6.2 list page redraw without a manual refresh. */
function invalidateProjectsRoot(qc: ReturnType<typeof useQueryClient>): void {
  qc.invalidateQueries({ queryKey: ["projects"] });
}

/** `POST /projects` — create a project. The modal lives on the
 *  §6.2 list page; success closes the dialog and the surrounding
 *  page picks up the new row via the invalidation. */
export function useCreateProject() {
  const qc = useQueryClient();
  return useMutation<ProjectDto, Error, CreateProjectRequest>({
    mutationFn: (body) => api.createProject(body),
    onSuccess: () => invalidateProjectsRoot(qc),
  });
}

/** `PATCH /projects/{id}` — partial update under §8.2 CAS. */
export function usePatchProject(projectId: string) {
  const qc = useQueryClient();
  return useMutation<ProjectDto, Error, PatchProjectRequest>({
    mutationFn: (body) => api.patchProject(projectId, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: projectsKeys.detail(projectId) });
      invalidateProjectsRoot(qc);
    },
  });
}

/** `POST /projects/{id}/archive` — idempotent archive. */
export function useArchiveProject(projectId: string) {
  const qc = useQueryClient();
  return useMutation<ProjectDto, Error, ArchiveProjectRequest>({
    mutationFn: (body) => api.archiveProject(projectId, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: projectsKeys.detail(projectId) });
      invalidateProjectsRoot(qc);
    },
  });
}

/** `GET /projects/{id}/issues` — paginated issue membership for
 *  the §6.3 issue list. */
export function useProjectIssues(
  projectId: string | null,
  q: ListProjectIssuesQuery = {},
) {
  return useQuery<IssueListResponse>({
    queryKey: projectId
      ? projectsKeys.issues(projectId, q)
      : ["projects", "issues", "(none)"],
    queryFn: () =>
      projectId
        ? api.listProjectIssues(projectId, q)
        : Promise.resolve({ rows: [], total: 0, limit: 0, offset: 0 }),
    enabled: !!projectId,
    staleTime: 15_000,
  });
}

/** `GET /projects/{id}/group-by-options` — dimensions the
 *  workbench Group-by dropdown should offer for this project
 *  (PROJECT-VIEW.md §7.3). Stays cached for the page lifetime so
 *  re-opening the dropdown is instant; invalidated by any tag
 *  link mutation against this project's issues. */
export function useProjectGroupByOptions(projectId: string | null) {
  return useQuery<GroupByOptionsResponse>({
    queryKey: projectId
      ? projectsKeys.groupByOptions(projectId)
      : ["projects", "group-by-options", "(none)"],
    queryFn: () =>
      projectId
        ? api.getProjectGroupByOptions(projectId)
        : Promise.resolve({ dims: [] }),
    enabled: !!projectId,
    staleTime: 60_000,
  });
}

// -- saved views (PROJECT-VIEW.md §5.4 / §7.1) --------------------------

/** `GET /projects/{id}/views` — caller's saved views, position ASC.
 *  Backs the ViewsTabStrip; cached for the page lifetime so tab
 *  reopens are instant. Mutations below invalidate the same key. */
export function useProjectViews(projectId: string | null) {
  return useQuery<ProjectViewDto[]>({
    queryKey: projectId
      ? projectsKeys.views(projectId)
      : ["projects", "views", "(none)"],
    queryFn: () =>
      projectId ? api.listProjectViews(projectId) : Promise.resolve([]),
    enabled: !!projectId,
    staleTime: 60_000,
  });
}

/** `POST /projects/{id}/views` — create + append. */
export function useCreateProjectView(projectId: string) {
  const qc = useQueryClient();
  return useMutation<ProjectViewDto, Error, ProjectViewWriteBody>({
    mutationFn: (body) => api.createProjectView(projectId, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: projectsKeys.views(projectId) });
    },
  });
}

/** `PATCH /projects/{id}/views/{view_id}` — edit a saved view in
 *  place (used by the dirty "Save changes" affordance). */
export function useUpdateProjectView(projectId: string) {
  const qc = useQueryClient();
  return useMutation<
    ProjectViewDto,
    Error,
    { viewId: string; body: ProjectViewWriteBody }
  >({
    mutationFn: ({ viewId, body }) =>
      api.updateProjectView(projectId, viewId, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: projectsKeys.views(projectId) });
    },
  });
}

/** `DELETE /projects/{id}/views/{view_id}` — idempotent. */
export function useDeleteProjectView(projectId: string) {
  const qc = useQueryClient();
  return useMutation<void, Error, string>({
    mutationFn: (viewId) => api.deleteProjectView(projectId, viewId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: projectsKeys.views(projectId) });
    },
  });
}

/** `POST /projects/{id}/views/reorder` — atomic position rewrite. */
export function useReorderProjectViews(projectId: string) {
  const qc = useQueryClient();
  return useMutation<ProjectViewDto[], Error, string[]>({
    mutationFn: (orderedIds) =>
      api.reorderProjectViews(projectId, orderedIds),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: projectsKeys.views(projectId) });
    },
  });
}

// -- milestones (PROJECT-VIEW.md §5.5, Slice 1) ------------------------

/** `GET /projects/{id}/milestones` — milestones across every linked
 *  repo, sorted soonest-due first. `includeClosed = false` returns
 *  only `state = "open"` (the strip's primary case); `true` appends
 *  closed rows so the `▸ Show closed` toggle can render. */
export function useProjectMilestones(
  projectId: string | null,
  includeClosed = false,
) {
  return useQuery<MilestoneDto[]>({
    queryKey: projectId
      ? projectsKeys.milestones(projectId, includeClosed)
      : ["projects", "milestones", "(none)", includeClosed],
    queryFn: () =>
      projectId
        ? api.listProjectMilestones(projectId, includeClosed)
        : Promise.resolve([]),
    enabled: !!projectId,
    staleTime: 60_000,
  });
}

/** `POST /projects/{id}/adopt-milestone` — set or clear the
 *  project's primary milestone. Invalidates the project detail
 *  query (the `★ primary` chip lives on `ProjectDto`) and the
 *  milestones list so the strip re-renders with the new badge. */
export function useAdoptProjectMilestone(projectId: string) {
  const qc = useQueryClient();
  return useMutation<ProjectDto, Error, string | null>({
    mutationFn: (milestoneId) =>
      api.adoptProjectMilestone(projectId, milestoneId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: projectsKeys.detail(projectId) });
      qc.invalidateQueries({
        queryKey: ["projects", "milestones", projectId],
      });
    },
  });
}

/** `POST /projects/{id}/milestones` — create a milestone on a
 *  linked repo and mirror it into `dp_milestones` in the same
 *  request. Invalidates the project's milestones list so the
 *  strip re-renders with the new card. */
export function useCreateProjectMilestone(projectId: string) {
  const qc = useQueryClient();
  return useMutation<MilestoneDto, Error, CreateMilestoneRequest>({
    mutationFn: (body) => api.createProjectMilestone(projectId, body),
    onSuccess: () => {
      qc.invalidateQueries({
        queryKey: ["projects", "milestones", projectId],
      });
    },
  });
}

/** `PATCH /projects/{id}/milestones/{milestone_id}` — edit /
 *  close / reopen a mirrored milestone. Invalidates the
 *  project's milestones list and the project detail (in case
 *  the strip pivots a chip's state class). */
export function useUpdateProjectMilestone(projectId: string) {
  const qc = useQueryClient();
  return useMutation<
    MilestoneDto,
    Error,
    { milestoneId: string; body: PatchMilestoneRequest }
  >({
    mutationFn: ({ milestoneId, body }) =>
      api.patchProjectMilestone(projectId, milestoneId, body),
    onSuccess: () => {
      qc.invalidateQueries({
        queryKey: ["projects", "milestones", projectId],
      });
      qc.invalidateQueries({ queryKey: projectsKeys.detail(projectId) });
    },
  });
}

/** `DELETE /projects/{id}/milestones/{milestone_id}` — delete a
 *  milestone on GitHub and locally. Invalidates the milestone
 *  list and the project detail (the `primary_milestone_id` may
 *  cascade-clear). */
export function useDeleteProjectMilestone(projectId: string) {
  const qc = useQueryClient();
  return useMutation<void, Error, string>({
    mutationFn: (milestoneId) =>
      api.deleteProjectMilestone(projectId, milestoneId),
    onSuccess: () => {
      qc.invalidateQueries({
        queryKey: ["projects", "milestones", projectId],
      });
      qc.invalidateQueries({ queryKey: projectsKeys.detail(projectId) });
    },
  });
}

/** `POST /projects/{id}/issues` — bulk add (capped at 100 per
 *  request, per `BULK_ADD_ISSUE_CAP`). Invalidates membership +
 *  the issue→project cache for every accepted row so the §6.5
 *  detail-pane chip and the §6.3 list refresh in lockstep. */
export function useAddIssuesToProject(projectId: string) {
  const qc = useQueryClient();
  return useMutation<BulkAddResult, Error, BulkAddIssuesRequest>({
    mutationFn: (body) => api.addIssuesToProject(projectId, body),
    onSuccess: (result) => {
      qc.invalidateQueries({ queryKey: ["projects", "issues", projectId] });
      qc.invalidateQueries({ queryKey: projectsKeys.detail(projectId) });
      invalidateProjectsRoot(qc);
      for (const issueId of result.added) {
        qc.invalidateQueries({ queryKey: projectsKeys.forIssue(issueId) });
      }
    },
  });
}

/** `POST /issues` — create a fresh GitHub issue. The local
 *  `dp_issues` row materialises on the next fetcher / webhook
 *  pass; we invalidate the project's issue list so the row
 *  shows up as soon as it lands. */
export function useCreateIssue(projectId?: string) {
  const qc = useQueryClient();
  return useMutation<CreateIssueResponse, Error, CreateIssueRequest>({
    mutationFn: (body) => api.createIssue(body),
    onSuccess: () => {
      if (projectId) {
        qc.invalidateQueries({ queryKey: ["projects", "issues", projectId] });
        qc.invalidateQueries({ queryKey: projectsKeys.detail(projectId) });
      }
      qc.invalidateQueries({ queryKey: ["issues"] });
    },
  });
}

/** `DELETE /projects/{id}/issues/{issue_id}` — single detach.
 *  When `viewId` is omitted (the "All" tab), the detach is
 *  project-level and requires the project's `expectedVersion`.
 *  When `viewId` is set (any named saved-view tab), the detach is
 *  scoped to that view's membership only; `expectedVersion` is
 *  ignored. */
export function useRemoveIssueFromProject(projectId: string) {
  const qc = useQueryClient();
  return useMutation<
    void,
    Error,
    {
      issueId: string;
      /** Required when `viewId` is null/undefined. */
      expectedVersion: number | null;
      /** When set, scopes the detach to this saved view's tab
       *  membership table and leaves the project link alone. */
      viewId?: string | null;
    }
  >({
    mutationFn: ({ issueId, expectedVersion, viewId }) =>
      api.removeIssueFromProject(projectId, issueId, expectedVersion, viewId),
    onSuccess: (_, { issueId, viewId }) => {
      qc.invalidateQueries({ queryKey: ["projects", "issues", projectId] });
      // Project-level state (counts, version) only changes when the
      // detach is project-level; skip the noisy invalidations for a
      // view-scoped detach.
      if (!viewId) {
        qc.invalidateQueries({ queryKey: projectsKeys.detail(projectId) });
        qc.invalidateQueries({ queryKey: projectsKeys.forIssue(issueId) });
        invalidateProjectsRoot(qc);
      }
    },
  });
}

/** `GET /issues/{id}/project` — resolve the project for an issue,
 *  or `null` when the issue is not currently in any project. Backs
 *  the §6.5 detail-pane Project chip on the workflow surface. */
export function useProjectForIssue(issueId: string | null) {
  return useQuery<ProjectDto | null>({
    queryKey: issueId
      ? projectsKeys.forIssue(issueId)
      : ["projects", "for-issue", "(none)"],
    queryFn: () =>
      issueId ? api.getProjectForIssue(issueId) : Promise.resolve(null),
    enabled: !!issueId,
    staleTime: 30_000,
  });
}

/** `GET /projects/{id}/repos` — list the repos associated with a
 *  project (soft scoping for the §6.3 issue picker). */
export function useProjectRepos(projectId: string | null) {
  return useQuery<ProjectRepoDto[]>({
    queryKey: projectId
      ? projectsKeys.repos(projectId)
      : ["projects", "repos", "(none)"],
    queryFn: () =>
      projectId ? api.listProjectRepos(projectId) : Promise.resolve([]),
    enabled: !!projectId,
    staleTime: 30_000,
  });
}

/** `PUT /projects/{id}/repos/{repo_id}` — idempotently associate
 *  a repo with the project. */
export function useAddProjectRepo(projectId: string) {
  const qc = useQueryClient();
  return useMutation<ProjectRepoDto, Error, string>({
    mutationFn: (repoId) => api.addProjectRepo(projectId, repoId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: projectsKeys.repos(projectId) });
    },
  });
}

/** `DELETE /projects/{id}/repos/{repo_id}` — remove the
 *  association. Idempotent. */
export function useRemoveProjectRepo(projectId: string) {
  const qc = useQueryClient();
  return useMutation<void, Error, string>({
    mutationFn: (repoId) => api.removeProjectRepo(projectId, repoId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: projectsKeys.repos(projectId) });
    },
  });
}
