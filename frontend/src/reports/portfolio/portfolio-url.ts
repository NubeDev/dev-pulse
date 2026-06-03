import type {
  PortfolioSort,
  ProjectPortfolioRequest,
  ProjectStatusDto,
  TagMatch,
} from "../../api/client.js";
import { PAGE_SIZE, VALID_SORTS, VALID_STATUSES } from "./portfolio-constants.js";

export function buildRoute(params: URLSearchParams): string {
  const qs = params.toString();
  return qs ? `/reports/projects?${qs}` : `/reports/projects`;
}

export function currentParams(route: string): URLSearchParams {
  const idx = route.indexOf("?");
  return new URLSearchParams(idx >= 0 ? route.slice(idx + 1) : "");
}

export function pageFromParams(params: URLSearchParams): number {
  const raw = params.get("page");
  return raw ? Math.max(1, Number.parseInt(raw, 10) || 1) : 1;
}

export function parseQuery(route: string): ProjectPortfolioRequest {
  const hashIdx = route.indexOf("?");
  const search = hashIdx >= 0 ? route.slice(hashIdx + 1) : "";
  const params = new URLSearchParams(search);

  const statusCsv = params.get("status");
  const statuses: ProjectStatusDto[] = statusCsv
    ? statusCsv
        .split(",")
        .map((s) => s.trim())
        .filter((s): s is ProjectStatusDto =>
          VALID_STATUSES.has(s as ProjectStatusDto),
        )
    : [];

  const sortRaw = params.get("sort");
  const sort: PortfolioSort =
    sortRaw && VALID_SORTS.has(sortRaw as PortfolioSort)
      ? (sortRaw as PortfolioSort)
      : "due_asc_nulls_last";

  const hide_overdue = params.get("hide_overdue") === "1";

  const tagsCsv = params.get("tags");
  const tag_ids: string[] = tagsCsv
    ? tagsCsv
        .split(",")
        .map((s) => s.trim())
        .filter((s) => s.length > 0)
    : [];
  const tag_match: TagMatch = params.get("tag_match") === "all" ? "all" : "any";

  const pageRaw = params.get("page");
  const page = pageRaw ? Math.max(1, Number.parseInt(pageRaw, 10) || 1) : 1;
  const offset = (page - 1) * PAGE_SIZE;

  return {
    orgs: [],
    statuses,
    hide_overdue,
    tag_ids,
    tag_match,
    sort,
    limit: PAGE_SIZE,
    offset,
  };
}
