/**
 * `RepoSnapshotPanel` — small read-only badge of the GitHub-side
 * snapshot for the currently-focused repo. Renders inline with the
 * `RepoFocusPanel` (activity drilldown), giving a quick "what is
 * this repo?" answer alongside the "who works on it?" view.
 *
 * Source: `GET /repos/{id}/metadata`, populated by the fetcher off
 * every webhook delivery's `repository` block. Returns `null` when
 * no snapshot has been recorded yet (fresh install) — the panel
 * shows a "Snapshot pending" placeholder rather than rendering
 * zeros that would look like a real reading.
 *
 * SCOPE §4 fit: every field describes the repo, not a user. No
 * per-author attribution, no LOC ranking — safe by construction
 * for any surface that lands a repo id.
 */

import { useQuery } from "@tanstack/react-query";
import {
  Archive,
  ExternalLink,
  Eye,
  GitFork,
  Lock,
  Star,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

import { api } from "../../api/client.js";
import type { RepoSummaryDto } from "../../api/client.js";

export interface RepoSnapshotPanelProps {
  /** The focused repo (already loaded into the directory). */
  repo: RepoSummaryDto;
}

export function RepoSnapshotPanel({
  repo,
}: RepoSnapshotPanelProps): JSX.Element {
  const q = useQuery({
    queryKey: ["repo-metadata", repo.id],
    queryFn: () => api.getRepoMetadata(repo.id),
    // Snapshot moves on webhook deliveries, not on user action —
    // a 60s cache is plenty and avoids hammering the endpoint as
    // the user clicks around different repos.
    staleTime: 60_000,
  });

  return (
    <Card>
      <CardHeader className="pb-3">
        <CardTitle className="text-base">
          <a
            href={`https://github.com/${repo.slug}`}
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-1 hover:underline"
          >
            {repo.slug}
            <ExternalLink className="size-3.5" aria-hidden />
          </a>
        </CardTitle>
        <CardDescription>
          GitHub snapshot — refreshed off every webhook delivery.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {q.isPending ? (
          <p className="text-sm text-muted-foreground">Loading snapshot…</p>
        ) : q.isError ? (
          <p className="text-sm text-destructive">
            Failed to load snapshot:{" "}
            {q.error instanceof Error ? q.error.message : "unknown error"}
          </p>
        ) : q.data === null ? (
          <p className="text-sm text-muted-foreground">
            Snapshot pending — no webhook delivery has carried the repo
            object yet. Trigger a manual sync from the workflow page to
            populate it now.
          </p>
        ) : (
          <SnapshotBody data={q.data} />
        )}
      </CardContent>
    </Card>
  );
}

function SnapshotBody({
  data,
}: {
  data: NonNullable<Awaited<ReturnType<typeof api.getRepoMetadata>>>;
}): JSX.Element {
  const updated = new Date(data.metadata_updated_at);
  const pushed = data.pushed_at ? new Date(data.pushed_at) : null;
  return (
    <div className="flex flex-col gap-4">
      {data.description ? (
        <p className="text-sm leading-relaxed">{data.description}</p>
      ) : null}

      <div className="flex flex-wrap items-center gap-2">
        {data.primary_language ? (
          <Badge variant="secondary">{data.primary_language}</Badge>
        ) : null}
        {data.default_branch ? (
          <Badge variant="outline" className="font-mono text-xs">
            {data.default_branch}
          </Badge>
        ) : null}
        {data.is_archived ? (
          <Badge variant="destructive" className="gap-1">
            <Archive className="size-3" aria-hidden /> archived
          </Badge>
        ) : null}
        {data.is_fork ? (
          <Badge variant="outline" className="gap-1">
            <GitFork className="size-3" aria-hidden /> fork
          </Badge>
        ) : null}
        {data.is_private ? (
          <Badge variant="outline" className="gap-1">
            <Lock className="size-3" aria-hidden /> private
          </Badge>
        ) : null}
      </div>

      <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm sm:grid-cols-4">
        <Metric
          icon={<Star className="size-4" aria-hidden />}
          label="Stars"
          value={formatCount(data.stars)}
        />
        <Metric
          icon={<GitFork className="size-4" aria-hidden />}
          label="Forks"
          value={formatCount(data.forks)}
        />
        <Metric
          icon={<Eye className="size-4" aria-hidden />}
          label="Watchers"
          value={formatCount(data.watchers)}
        />
        <Metric
          label="Open (GH)"
          // Tooltip-worthy footnote: differs from dev-pulse's own
          // open_issue_count because GitHub includes PRs.
          value={formatCount(data.open_issues_remote)}
          hint="GitHub counts open issues + PRs"
        />
      </dl>

      {data.homepage ? (
        <div className="text-sm">
          <span className="text-muted-foreground">Homepage:</span>{" "}
          <a
            href={data.homepage}
            target="_blank"
            rel="noreferrer"
            className="hover:underline"
          >
            {data.homepage}
          </a>
        </div>
      ) : null}

      <div className="text-xs text-muted-foreground">
        {pushed ? (
          <>Last push to GitHub: {pushed.toLocaleString("en-AU")} · </>
        ) : null}
        Snapshot recorded {updated.toLocaleString("en-AU")}
      </div>
    </div>
  );
}

function Metric({
  icon,
  label,
  value,
  hint,
}: {
  icon?: React.ReactNode;
  label: string;
  value: string;
  hint?: string;
}): JSX.Element {
  return (
    <div className="flex flex-col gap-0.5">
      <dt
        className="flex items-center gap-1 text-xs text-muted-foreground"
        title={hint}
      >
        {icon}
        {label}
      </dt>
      <dd className="text-base font-medium tabular-nums">{value}</dd>
    </div>
  );
}

function formatCount(n: number): string {
  if (n < 1000) return String(n);
  if (n < 10_000) return `${(n / 1000).toFixed(1)}k`;
  if (n < 1_000_000) return `${Math.round(n / 1000)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}
