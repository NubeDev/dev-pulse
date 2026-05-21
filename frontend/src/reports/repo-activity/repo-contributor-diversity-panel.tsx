/**
 * `RepoContributorDiversityPanel` — repo-level "bus factor"
 * view. Answers: if our top contributor goes on leave, how much
 * of this repo's merge volume disappears?
 *
 * Source: `GET /repos/{id}/contributor-diversity`. Backed by the
 * existing `(merged-PR, author)` join — no schema change.
 *
 * SCOPE §4 fit: this is a property of the **repo's** risk
 * profile, not a ranking of contributors. The DTO carries no
 * user identifiers and the panel surfaces only aggregates. Not
 * mounted on the user-report or leaderboard surfaces.
 *
 * Sample-size guard (SCOPE §15.9): top-1 / top-3 shares are
 * `null` when `sample_n < 5`; the panel falls back to a count
 * with a "not enough data" hint.
 */

import { useQuery } from "@tanstack/react-query";

import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

import { api } from "../../api/client.js";
import type {
  RepoContributorDiversityDto,
  RepoSummaryDto,
} from "../../api/client.js";

const WINDOW_DAYS = 90;

export interface RepoContributorDiversityPanelProps {
  /** The focused repo (already loaded into the directory). */
  repo: RepoSummaryDto;
}

export function RepoContributorDiversityPanel({
  repo,
}: RepoContributorDiversityPanelProps): JSX.Element {
  const q = useQuery({
    queryKey: ["repo-contributor-diversity", repo.id],
    queryFn: () => api.getRepoContributorDiversity(repo.id),
    staleTime: 60_000,
  });

  return (
    <Card>
      <CardHeader className="pb-3">
        <CardTitle className="text-base">Contributor diversity</CardTitle>
        <CardDescription>
          Last {WINDOW_DAYS} days · concentration of merged-PR
          authorship across{" "}
          <span className="font-mono">{repo.slug}</span>.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {q.isPending ? (
          <p className="text-sm text-muted-foreground">Computing…</p>
        ) : q.isError ? (
          <p className="text-sm text-destructive">
            Failed to load diversity stats:{" "}
            {q.error instanceof Error ? q.error.message : "unknown error"}
          </p>
        ) : (
          <Body data={q.data} />
        )}
      </CardContent>
    </Card>
  );
}

function Body({
  data,
}: {
  data: RepoContributorDiversityDto;
}): JSX.Element {
  if (data.sample_n === 0) {
    return (
      <p className="text-sm text-muted-foreground">
        No merged PRs in the last {WINDOW_DAYS} days.
      </p>
    );
  }

  // `top*_share` are null when the §15.9 mask fires. Show counts
  // either way — concentration ratios on n=2 always look
  // catastrophic, so we lead with raw counts and overlay shares
  // only once we have enough signal to trust them.
  const masked =
    data.top1_share === null || data.top1_share === undefined;

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-baseline gap-3">
        <span className="text-3xl font-semibold tabular-nums">
          {data.distinct_authors.toLocaleString()}
        </span>
        <span className="text-xs text-muted-foreground">
          distinct{" "}
          {data.distinct_authors === 1 ? "author" : "authors"} ·{" "}
          {data.sample_n.toLocaleString()} merged{" "}
          {data.sample_n === 1 ? "PR" : "PRs"}
        </span>
      </div>

      {masked ? (
        <p className="text-xs text-muted-foreground">
          Sample too small ({data.sample_n}{" "}
          {data.sample_n === 1 ? "merge" : "merges"}). Concentration
          ratios need n ≥ 5 to be meaningful.
        </p>
      ) : (
        <div className="grid grid-cols-2 gap-x-4 gap-y-1.5 text-sm">
          <ShareStat
            label="Top contributor"
            share={data.top1_share!}
            hint="Share of merges from the single most-active author. Higher = more concentration risk."
          />
          <ShareStat
            label="Top 3 combined"
            share={data.top3_share!}
            hint="Share of merges from the three most-active authors combined."
          />
        </div>
      )}
    </div>
  );
}

function ShareStat({
  label,
  share,
  hint,
}: {
  label: string;
  share: number;
  hint: string;
}): JSX.Element {
  return (
    <div className="flex flex-col" title={hint}>
      <span className="text-xs text-muted-foreground">{label}</span>
      <span className="font-mono tabular-nums">
        {(share * 100).toFixed(1)}%
      </span>
    </div>
  );
}
