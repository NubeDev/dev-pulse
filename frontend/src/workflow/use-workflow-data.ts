/**
 * React-Query bindings + mock short-circuit for the workflow surface.
 *
 * The four hook families mirror the four backend fragments §6 / §7 /
 * §8 / §13.6 lock in:
 *
 * - `usePins` / `useAddPin` / `useRemovePin` / `useReorderPins`
 * - `useTags` / `useTagDetail` / `useCreateTag` / `useUpdateTag` /
 *   `useLinkTagTargets` / `useUnlinkTagTargets`
 * - `useAppInstallBanner` — drives the §8.4 / §13.6 banners.
 * - `useUpdateIssue` / `useCreateIssue` / `useCommentOnIssue` — the
 *   CAS-on-version write path. The mutation hooks rethrow
 *   `DpRestError` verbatim so the §8.3 stale-version reload UX can
 *   read `error.code === "stale_local_version"` and pull
 *   `error.body.current_version`.
 *
 * Under `VITE_USE_MOCK_REPORTS=1` every hook resolves against the
 * in-memory fixtures in `./mocks` instead of calling the network.
 * The mocks deliberately preserve the §8.2 invariants — `updateIssue`
 * bumps `version`, a second PATCH with the stale `expected_version`
 * throws `DpRestError("stale_local_version")` — so smoke tests can
 * exercise the reload UX without a backend.
 */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  api,
  DpRestError,
  type AddPinRequest,
  type AppInstallBannerResponse,
  type CreateCommentRequest,
  type CreateIssueRequest,
  type CreateTagRequest,
  type IssueDto,
  type IssueListResponse,
  type LinkBatchRequest,
  type LinkBatchResponse,
  type ListIssuesQuery,
  type ListReposQuery,
  type PinDto,
  type PinKeyDto,
  type PinKind,
  type RepoListResponse,
  type ReorderRequest,
  type TagDetailResponse,
  type TagDto,
  type UpdateIssueRequest,
  type UpdateTagRequest,
} from "../api/client.js";
import {
  USE_MOCK,
  mockAppInstallBanner,
  mockIssue,
  mockListIssues,
  mockListRepos,
  mockPinsState,
  mockTagDetail,
  mockTagsState,
} from "./mocks.js";

// ---------------------------------------------------------------------------
// Query keys — all reads are scoped under `["workflow", ...]` so the
// section's cache can be invalidated wholesale on logout.
// ---------------------------------------------------------------------------

export const workflowKeys = {
  pins: () => ["workflow", "pins"] as const,
  tags: () => ["workflow", "tags"] as const,
  myTags: () => ["workflow", "my-tags"] as const,
  tag: (id: string) => ["workflow", "tag", id] as const,
  issue: (id: string) => ["workflow", "issue", id] as const,
  issues: (q: ListIssuesQuery) => ["workflow", "issues", q] as const,
  repos: (q: ListReposQuery) => ["workflow", "repos", q] as const,
  banner: () => ["workflow", "app-install-banner"] as const,
};

// ---------------------------------------------------------------------------
// Pins
// ---------------------------------------------------------------------------

export function usePins() {
  return useQuery<PinDto[]>({
    queryKey: workflowKeys.pins(),
    queryFn: () => (USE_MOCK ? Promise.resolve([...mockPinsState]) : api.listPins()),
  });
}

export function useAddPin() {
  const qc = useQueryClient();
  return useMutation<PinDto, Error, AddPinRequest>({
    mutationFn: async (req) => {
      if (USE_MOCK) {
        if (mockPinsState.some((p) => p.kind === req.kind && p.target_id === req.target_id)) {
          throw new DpRestError(409, "pin_exists", "pin already exists for this caller");
        }
        const next: PinDto = {
          kind: req.kind,
          target_id: req.target_id,
          position: mockPinsState.length,
          pinned_at: new Date().toISOString(),
        };
        mockPinsState.push(next);
        return next;
      }
      return api.addPin(req);
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: workflowKeys.pins() }),
  });
}

export function useRemovePin() {
  const qc = useQueryClient();
  return useMutation<void, Error, { kind: PinKind; target_id: string }>({
    mutationFn: async ({ kind, target_id }) => {
      if (USE_MOCK) {
        const idx = mockPinsState.findIndex((p) => p.kind === kind && p.target_id === target_id);
        if (idx >= 0) mockPinsState.splice(idx, 1);
        return;
      }
      await api.removePin(kind, target_id);
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: workflowKeys.pins() }),
  });
}

export function useReorderPins() {
  const qc = useQueryClient();
  return useMutation<void, Error, ReorderRequest>({
    mutationFn: async (req) => {
      if (USE_MOCK) {
        const next = req.order
          .map((k, i) => {
            const existing = mockPinsState.find(
              (p) => p.kind === k.kind && p.target_id === k.target_id,
            );
            return existing ? { ...existing, position: i } : null;
          })
          .filter((p): p is PinDto => p !== null);
        if (next.length !== mockPinsState.length) {
          throw new DpRestError(400, "reorder_set_mismatch", "reorder must cover every pin");
        }
        mockPinsState.splice(0, mockPinsState.length, ...next);
        return;
      }
      await api.reorderPins(req);
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: workflowKeys.pins() }),
  });
}

// ---------------------------------------------------------------------------
// Tags
// ---------------------------------------------------------------------------

export function useTags() {
  return useQuery<TagDto[]>({
    queryKey: workflowKeys.tags(),
    queryFn: () => (USE_MOCK ? Promise.resolve([...mockTagsState]) : api.listTags()),
  });
}

export function useMyTags() {
  return useQuery<TagDto[]>({
    queryKey: workflowKeys.myTags(),
    queryFn: () => (USE_MOCK ? Promise.resolve([...mockTagsState]) : api.listMyTags()),
  });
}

export function useTagDetail(id: string | undefined) {
  return useQuery<TagDetailResponse>({
    queryKey: id ? workflowKeys.tag(id) : ["workflow", "tag", "<none>"],
    enabled: !!id,
    queryFn: () => {
      if (!id) throw new Error("tag id required");
      return USE_MOCK ? Promise.resolve(mockTagDetail(id)) : api.getTag(id);
    },
  });
}

export function useCreateTag() {
  const qc = useQueryClient();
  return useMutation<TagDto, Error, CreateTagRequest>({
    mutationFn: async (req) => {
      if (USE_MOCK) {
        const id = crypto.randomUUID();
        const tag: TagDto = {
          id,
          scope_kind: req.scope_kind,
          scope_id: req.scope_id,
          name: req.name,
          color: req.color,
          description: req.description ?? null,
          created_by: "00000000-0000-0000-0000-000000040001",
          created_at: new Date().toISOString(),
          archived_at: null,
          visible_link_count: 0,
        };
        mockTagsState.push(tag);
        return tag;
      }
      return api.createTag(req);
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: ["workflow"] }),
  });
}

export function useUpdateTag(id: string) {
  const qc = useQueryClient();
  return useMutation<TagDto, Error, UpdateTagRequest>({
    mutationFn: (req) => (USE_MOCK
      ? Promise.resolve(mockUpdateTag(id, req))
      : api.updateTag(id, req)),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["workflow"] }),
  });
}

function mockUpdateTag(id: string, req: UpdateTagRequest): TagDto {
  const t = mockTagsState.find((x) => x.id === id);
  if (!t) throw new DpRestError(404, "tag_not_found", "tag not found");
  if (req.name !== undefined) t.name = req.name;
  if (req.color !== undefined) t.color = req.color;
  if (req.description !== undefined) t.description = req.description;
  if (req.archived !== undefined) {
    t.archived_at = req.archived ? new Date().toISOString() : null;
  }
  return { ...t };
}

export function useLinkTagTargets(id: string) {
  const qc = useQueryClient();
  return useMutation<LinkBatchResponse, Error, LinkBatchRequest>({
    mutationFn: async (req) => {
      if (USE_MOCK) {
        return { linked: [], warning: undefined };
      }
      return api.linkTagTargets(id, req);
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: workflowKeys.tag(id) }),
  });
}

export function useUnlinkTagTargets(id: string) {
  const qc = useQueryClient();
  return useMutation<void, Error, LinkBatchRequest>({
    mutationFn: async (req) => {
      if (USE_MOCK) return;
      await api.unlinkTagTargets(id, req);
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: workflowKeys.tag(id) }),
  });
}

// ---------------------------------------------------------------------------
// App-install banner (§8.4 / §13.6)
// ---------------------------------------------------------------------------

export function useAppInstallBanner() {
  return useQuery<AppInstallBannerResponse>({
    queryKey: workflowKeys.banner(),
    queryFn: () =>
      USE_MOCK
        ? Promise.resolve(mockAppInstallBanner)
        : api.getAppInstallBanner(),
    // The banner gates write affordances across every issue surface;
    // refetch on focus so a re-consent in another tab clears the
    // banner without a manual reload.
    refetchOnWindowFocus: true,
    staleTime: 30_000,
  });
}

// ---------------------------------------------------------------------------
// Issues (SCOPE-PROJECTS §8.2)
// ---------------------------------------------------------------------------

/** Workflow drill-down list pane. Returns the paginated
 *  `{rows, total, limit, offset}` envelope so the table can
 *  render `Showing X–Y of Z` and page through 1000s of issues. */
export function useIssueList(q: ListIssuesQuery) {
  return useQuery<IssueListResponse>({
    queryKey: workflowKeys.issues(q),
    queryFn: () =>
      USE_MOCK ? Promise.resolve(mockListIssues(q)) : api.listIssues(q),
  });
}

/** Workflow master list pane — paginated repo list with
 *  open-issue counts + last-activity timestamps. */
export function useRepoList(q: ListReposQuery) {
  return useQuery<RepoListResponse>({
    queryKey: workflowKeys.repos(q),
    queryFn: () =>
      USE_MOCK ? Promise.resolve(mockListRepos(q)) : api.listRepos(q),
  });
}

export function useIssue(id: string | undefined) {
  return useQuery<IssueDto>({
    queryKey: id ? workflowKeys.issue(id) : ["workflow", "issue", "<none>"],
    enabled: !!id,
    queryFn: () => {
      if (!id) throw new Error("issue id required");
      if (USE_MOCK) return Promise.resolve({ ...mockIssue });
      return api.getIssueById(id);
    },
  });
}

/**
 * §8.2 update mutation. The caller hands us the `expected_version`
 * captured at form load; on `409 stale_local_version` the mutation
 * **rethrows** the `DpRestError` so the form can pull
 * `body.current_version` and drive the reload prompt (§8.3).
 *
 * Under `USE_MOCK`, the in-memory `mockIssue.version` is bumped on
 * every successful PATCH and a stale `expected_version` raises the
 * same `DpRestError` shape — so the §8.3 UX is exercisable from the
 * smoke harness with no backend.
 */
export function useUpdateIssue(id: string) {
  const qc = useQueryClient();
  return useMutation<IssueDto, Error, UpdateIssueRequest>({
    mutationFn: async (req) => {
      if (USE_MOCK) {
        if (req.expected_version !== mockIssue.version) {
          throw new DpRestError(
            409,
            "stale_local_version",
            "local row is stale; reload and re-apply",
            { current_version: mockIssue.version },
          );
        }
        if (req.title !== undefined) mockIssue.title = req.title;
        if (req.body !== undefined) mockIssue.body = req.body;
        if (req.labels !== undefined) mockIssue.labels = req.labels;
        if (req.assignees !== undefined) mockIssue.assignees = req.assignees;
        if (req.milestone !== undefined) mockIssue.milestone = req.milestone;
        if (req.state !== undefined) mockIssue.state = req.state;
        mockIssue.version += 1;
        mockIssue.updated_at = new Date().toISOString();
        return { ...mockIssue };
      }
      return api.updateIssue(id, req);
    },
    onSuccess: (updated) => {
      qc.setQueryData(workflowKeys.issue(updated.id), updated);
    },
  });
}

export function useCreateIssue() {
  const qc = useQueryClient();
  return useMutation<IssueDto, Error, CreateIssueRequest>({
    mutationFn: async (req) => {
      if (USE_MOCK) {
        const id = crypto.randomUUID();
        return {
          ...mockIssue,
          id,
          number: mockIssue.number + 1,
          title: req.title,
          body: req.body ?? null,
          labels: req.labels ?? [],
          assignees: req.assignees ?? [],
          milestone: req.milestone ?? null,
          version: 1,
        };
      }
      return api.createIssue(req);
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: ["workflow"] }),
  });
}

export function useCommentOnIssue(id: string) {
  const qc = useQueryClient();
  return useMutation<IssueDto, Error, CreateCommentRequest>({
    mutationFn: async (req) => {
      if (USE_MOCK) {
        if (req.expected_version !== mockIssue.version) {
          throw new DpRestError(
            409,
            "stale_local_version",
            "local row is stale; reload and re-apply",
            { current_version: mockIssue.version },
          );
        }
        mockIssue.version += 1;
        mockIssue.updated_at = new Date().toISOString();
        return { ...mockIssue };
      }
      return api.commentOnIssue(id, req);
    },
    onSuccess: (updated) => {
      qc.setQueryData(workflowKeys.issue(updated.id), updated);
    },
  });
}

// ---------------------------------------------------------------------------
// Helpers shared by the form components.
// ---------------------------------------------------------------------------

/**
 * Pull `current_version` out of a §8.3 `stale_local_version` error.
 * The §8.2 step 5 contract guarantees the field exists on a CAS
 * miss; we treat its absence as the same "reload from scratch"
 * outcome.
 */
export function staleVersionFromError(e: unknown): number | undefined {
  if (e instanceof DpRestError && e.code === "stale_local_version") {
    const v = e.body?.["current_version"];
    return typeof v === "number" ? v : undefined;
  }
  return undefined;
}

/** Surface the `writes_not_available_for_org` org_login (§8.4). */
export function writesUnavailableOrg(e: unknown): string | undefined {
  if (e instanceof DpRestError && e.code === "writes_not_available_for_org") {
    const l = e.body?.["org_login"];
    return typeof l === "string" ? l : undefined;
  }
  return undefined;
}

// Re-export the `DpRestError` instance check so the workflow pages
// can narrow without importing from two places.
export { DpRestError, type PinKeyDto };
