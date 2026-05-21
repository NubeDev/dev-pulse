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

import { useQuery } from "@tanstack/react-query";

import {
  api,
  type ListProjectsQuery,
  type ProjectListResponse,
  type ProjectStatusDto,
} from "../api/client.js";

/** Stable cache keys — `useQuery` invalidation surface for the
 *  later mutation hooks. */
export const projectsKeys = {
  count: (status: ProjectStatusDto) =>
    ["projects", "count", status] as const,
  list: (q: ListProjectsQuery) => ["projects", "list", q] as const,
};

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
