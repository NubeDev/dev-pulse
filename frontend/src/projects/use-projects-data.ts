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
  type CreateProjectRequest,
  type IssueListResponse,
  type ListProjectsQuery,
  type OrgProjectPickerDto,
  type PatchProjectRequest,
  type ProjectDto,
  type ProjectListResponse,
  type ProjectRepoDto,
  type ProjectStatusDto,
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
  forIssue: (issueId: string) =>
    ["projects", "for-issue", issueId] as const,
  repos: (projectId: string) =>
    ["projects", "repos", projectId] as const,
};

/** Query shape for [`useProjectIssues`]. Mirrors the wire params. */
export interface ListProjectIssuesQuery {
  state?: "open" | "closed" | "all";
  q?: string;
  limit?: number;
  offset?: number;
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

/** `DELETE /projects/{id}/issues/{issue_id}` — single detach.
 *  Takes the project's `expected_version` plus the issue id; the
 *  store advances `version` on success and the hook invalidates
 *  the detail / membership / for-issue caches. */
export function useRemoveIssueFromProject(projectId: string) {
  const qc = useQueryClient();
  return useMutation<
    void,
    Error,
    { issueId: string; expectedVersion: number }
  >({
    mutationFn: ({ issueId, expectedVersion }) =>
      api.removeIssueFromProject(projectId, issueId, expectedVersion),
    onSuccess: (_, { issueId }) => {
      qc.invalidateQueries({ queryKey: ["projects", "issues", projectId] });
      qc.invalidateQueries({ queryKey: projectsKeys.detail(projectId) });
      qc.invalidateQueries({ queryKey: projectsKeys.forIssue(issueId) });
      invalidateProjectsRoot(qc);
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
