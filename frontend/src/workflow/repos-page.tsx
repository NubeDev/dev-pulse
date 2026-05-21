/**
 * Repos page — workflow drill-down master.
 *
 * Renders a paginated, searchable repo list across every org dev-pulse
 * knows about. Each row carries the open-issue count and the most
 * recent issue activity so the operator can pick a target repo
 * without a per-row roundtrip. Clicking a row navigates to
 * `#/workflow/issues?repo_id=<uuid>` — the issues page reads the
 * filter from the URL and renders the matching issues.
 *
 * This is intentionally sized for production scale (100s of repos,
 * 1000s of issues, 50+ users across multiple orgs): server-side
 * pagination, server-side `q` search, optional `org_id` filter.
 * No client-side sort/filter shortcut that would only work for
 * "tiny dataset" mocks.
 */

import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { IconExternalLink } from "@tabler/icons-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

import { api, type ListReposQuery } from "../api/client.js";
import { PageHeading } from "../components/page-heading.jsx";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "../components/table.jsx";
import { navigate, useRoute, workflowIssuesRoute } from "../routes.js";

import { MOCK_ORG_NUBE, USE_MOCK } from "./mocks.js";
import { useRepoList } from "./use-workflow-data.js";

const PAGE_SIZE = 50;
const ALL_ORGS = "__all__";

interface ReposPageQuery {
  orgId: string | null;
  q: string;
  offset: number;
}

function parseQuery(route: string): ReposPageQuery {
  const qIdx = route.indexOf("?");
  const params = qIdx >= 0 ? new URLSearchParams(route.slice(qIdx + 1)) : new URLSearchParams();
  const offsetRaw = Number.parseInt(params.get("offset") ?? "0", 10);
  return {
    orgId: params.get("org_id"),
    q: params.get("q") ?? "",
    offset: Number.isFinite(offsetRaw) && offsetRaw > 0 ? offsetRaw : 0,
  };
}

function buildRoute(q: ReposPageQuery): string {
  const params = new URLSearchParams();
  if (q.orgId) params.set("org_id", q.orgId);
  if (q.q) params.set("q", q.q);
  if (q.offset > 0) params.set("offset", String(q.offset));
  const qs = params.toString();
  return qs ? `#/workflow/repos?${qs}` : "#/workflow/repos";
}

/** Lightweight org list used to populate the filter dropdown. We
 *  call the directory endpoint directly so the repos page doesn't
 *  pull in the full directory hook graph for a single dropdown. */
function useOrgChoices() {
  return useQuery({
    queryKey: ["workflow", "orgs-for-filter"],
    staleTime: 5 * 60_000,
    queryFn: () =>
      USE_MOCK
        ? Promise.resolve([
            { id: MOCK_ORG_NUBE, login: "nube" },
          ])
        : api.listOrgs().then((orgs) => orgs.map((o) => ({ id: o.id, login: o.login }))),
  });
}

export function ReposPage(): JSX.Element {
  const route = useRoute();
  const parsed = useMemo(() => parseQuery(route), [route]);
  const [searchDraft, setSearchDraft] = useState(parsed.q);
  useEffect(() => setSearchDraft(parsed.q), [parsed.q]);

  const orgs = useOrgChoices();

  const query: ListReposQuery = useMemo(
    () => ({
      org_id: parsed.orgId ?? undefined,
      q: parsed.q || undefined,
      limit: PAGE_SIZE,
      offset: parsed.offset,
    }),
    [parsed],
  );
  const repos = useRepoList(query);

  const goTo = (next: Partial<ReposPageQuery>): void => {
    navigate(buildRoute({ ...parsed, ...next, offset: 0 }));
  };
  const goToOffset = (offset: number): void => {
    navigate(buildRoute({ ...parsed, offset }));
  };

  const rows = repos.data?.rows ?? [];
  const total = repos.data?.total ?? 0;
  const firstShown = total === 0 ? 0 : parsed.offset + 1;
  const lastShown = Math.min(parsed.offset + rows.length, total);

  return (
    <div className="flex flex-col gap-6 px-4 lg:px-6" data-testid="repos-page">
      <PageHeading
        title="Repos"
        description="Every repo dev-pulse tracks, with open-issue counts and last-activity timestamps. Click through to the issue list filtered by repo."
      />

      <Card>
        <CardContent className="flex flex-wrap items-end gap-3 pt-6">
          <div className="flex flex-col gap-1">
            <Label htmlFor="repos-org">Org</Label>
            <Select
              value={parsed.orgId ?? ALL_ORGS}
              onValueChange={(v) => goTo({ orgId: v === ALL_ORGS ? null : v })}
            >
              <SelectTrigger id="repos-org" className="w-56" data-testid="repos-org-select">
                <SelectValue placeholder="All orgs" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ALL_ORGS}>All orgs</SelectItem>
                {(orgs.data ?? []).map((o) => (
                  <SelectItem key={o.id} value={o.id}>
                    {o.login}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="flex flex-1 flex-col gap-1 min-w-64">
            <Label htmlFor="repos-q">Search</Label>
            <Input
              id="repos-q"
              placeholder="org or repo name…"
              value={searchDraft}
              onChange={(e) => setSearchDraft(e.target.value)}
              onBlur={() => searchDraft !== parsed.q && goTo({ q: searchDraft })}
              onKeyDown={(e) => {
                if (e.key === "Enter") goTo({ q: searchDraft });
              }}
              data-testid="repos-search"
            />
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardContent className="p-0">
          <Table data-testid="repos-table">
            <TableHeader className="bg-muted/50">
              <TableRow>
                <TableHead>Repo</TableHead>
                <TableHead className="w-32 text-right">Open issues</TableHead>
                <TableHead className="w-48">Last activity</TableHead>
                <TableHead className="w-24" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {repos.isLoading && (
                <TableRow>
                  <TableCell colSpan={4} className="text-center text-muted-foreground py-8">
                    Loading repos…
                  </TableCell>
                </TableRow>
              )}
              {repos.isError && !repos.isLoading && (
                <TableRow>
                  <TableCell colSpan={4} className="text-center text-destructive py-8">
                    Could not load repos: {repos.error instanceof Error ? repos.error.message : "unknown"}
                  </TableCell>
                </TableRow>
              )}
              {!repos.isLoading && !repos.isError && rows.length === 0 && (
                <TableRow>
                  <TableCell colSpan={4} className="text-center text-muted-foreground py-8">
                    No repos match these filters.
                  </TableCell>
                </TableRow>
              )}
              {rows.map((r) => (
                <TableRow
                  key={r.id}
                  className="cursor-pointer"
                  data-testid="repos-row"
                  onClick={() => navigate(workflowIssuesRoute({ repoId: r.id }))}
                >
                  <TableCell>
                    <div className="flex flex-col">
                      <span className="font-medium">{r.slug}</span>
                      <span className="text-xs text-muted-foreground">{r.org_login}</span>
                    </div>
                  </TableCell>
                  <TableCell className="text-right">
                    <Badge variant={r.open_issue_count > 0 ? "default" : "secondary"}>
                      {r.open_issue_count}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {r.last_activity_at ? new Date(r.last_activity_at).toLocaleString("en-AU") : "—"}
                  </TableCell>
                  <TableCell className="text-right">
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={(e) => {
                        e.stopPropagation();
                        navigate(workflowIssuesRoute({ repoId: r.id }));
                      }}
                    >
                      <IconExternalLink className="mr-1 size-4" />
                      Issues
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </CardContent>
        <div className="flex items-center justify-between border-t border-border px-4 py-3 text-sm text-muted-foreground">
          <span data-testid="repos-pagination-counter">
            {total === 0 ? "No repos" : `Showing ${firstShown}–${lastShown} of ${total}`}
          </span>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={parsed.offset === 0}
              onClick={() => goToOffset(Math.max(0, parsed.offset - PAGE_SIZE))}
            >
              Prev
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={parsed.offset + rows.length >= total}
              onClick={() => goToOffset(parsed.offset + PAGE_SIZE)}
            >
              Next
            </Button>
          </div>
        </div>
      </Card>
    </div>
  );
}
