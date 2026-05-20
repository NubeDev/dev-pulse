/**
 * `#/reports/repo-activity` — multi-org × multi-repo dashboard that
 * answers "what has each contributor been working on?".
 *
 * Built on the same `/reports/org/:org_id` count endpoint as the
 * leaderboard, with the `repos=` server-side filter applied. Reuses
 * the leaderboard's chart + table components so the look and feel
 * matches `#/reports/leaderboard`.
 */

import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { Card, CardContent } from "@/components/ui/card";

import { api } from "../api/client.js";
import type { OrgDto, RepoSummaryDto, UserDto } from "../api/client.js";
import { PageHeading } from "../components/page-heading.jsx";

import { DataAsOfBanner } from "./data-as-of.jsx";
import {
  defaultWindowState,
  windowStateToParams,
  type WindowState,
} from "./window-picker.jsx";
import {
  ActivityMixChart,
  ContributorBarChart,
  LeaderboardKpis,
  LeaderboardTable,
  LeaderboardTrendChart,
  type DirectoryMaps,
  type LeaderboardData,
} from "./leaderboard/index.js";
import {
  RepoActivityFilters,
  RepoBreakdownTable,
  RepoFocusPanel,
  useRepoActivityData,
  type RepoActivityDirectory,
} from "./repo-activity/index.js";

export function RepoActivityPage(): JSX.Element {
  const orgsQuery = useQuery({
    queryKey: ["orgs"],
    queryFn: () => api.listOrgs(),
  });
  const orgs: ReadonlyArray<OrgDto> = orgsQuery.data ?? [];

  const usersQuery = useQuery({
    queryKey: ["users"],
    queryFn: () => api.listUsers(),
  });
  const users: ReadonlyArray<UserDto> = usersQuery.data ?? [];

  // 500 matches the cap used by the user-report page; well above the
  // realistic repo count for the deployments this UI targets.
  const reposQuery = useQuery({
    queryKey: ["repos", "directory"],
    queryFn: () => api.listRepos({ limit: 500 }),
  });
  const repos: ReadonlyArray<RepoSummaryDto> = reposQuery.data?.rows ?? [];

  const directory = useMemo<RepoActivityDirectory>(
    () => ({
      orgsById: new Map(orgs.map((o) => [o.id, o])),
      usersById: new Map(users.map((u) => [u.id, u])),
      reposById: new Map(repos.map((r) => [r.id, r])),
    }),
    [orgs, users, repos],
  );

  // The leaderboard sub-components type `directory` as `DirectoryMaps`
  // (orgs + users). Project ours down before handing it over.
  const leaderboardDirectory = useMemo<DirectoryMaps>(
    () => ({
      orgsById: directory.orgsById,
      usersById: directory.usersById,
    }),
    [directory.orgsById, directory.usersById],
  );

  const [selectedOrgIds, setSelectedOrgIds] = useState<ReadonlyArray<string>>([]);
  const [selectedRepoIds, setSelectedRepoIds] = useState<ReadonlyArray<string>>([]);
  const [selectedKinds, setSelectedKinds] = useState<ReadonlyArray<string>>([
    "commit",
    "pull_request_merged",
    "review",
  ]);
  const [windowState, setWindowState] = useState<WindowState>(
    defaultWindowState(),
  );
  const [focusedRepoId, setFocusedRepoId] = useState<string | null>(null);

  // Drop the focused repo if it disappears from the visible repo set
  // (e.g. the user narrowed orgs).
  useEffect(() => {
    if (!focusedRepoId) return;
    const r = directory.reposById.get(focusedRepoId);
    if (!r) {
      setFocusedRepoId(null);
      return;
    }
    if (
      selectedOrgIds.length > 0 &&
      !selectedOrgIds.includes(r.org_id)
    ) {
      setFocusedRepoId(null);
    }
  }, [focusedRepoId, directory.reposById, selectedOrgIds]);

  // Default to the first org once the directory call resolves.
  useEffect(() => {
    if (selectedOrgIds.length === 0 && orgs.length > 0) {
      setSelectedOrgIds([orgs[0]!.id]);
    }
  }, [orgs, selectedOrgIds.length]);

  // Drop repo selections that no longer belong to a selected org —
  // e.g. the user narrowed the org filter and stale repo ids are in
  // state.
  useEffect(() => {
    if (selectedRepoIds.length === 0) return;
    const orgFilter =
      selectedOrgIds.length > 0 ? new Set(selectedOrgIds) : null;
    if (!orgFilter) return;
    const reposById = directory.reposById;
    const filtered = selectedRepoIds.filter((id) => {
      const r = reposById.get(id);
      return !!r && orgFilter.has(r.org_id);
    });
    if (filtered.length !== selectedRepoIds.length) {
      setSelectedRepoIds(filtered);
    }
  }, [selectedOrgIds, selectedRepoIds, directory.reposById]);

  const windowParams = useMemo(
    () => windowStateToParams(windowState),
    [windowState],
  );

  const { data, loading, error, dataAsOf } = useRepoActivityData({
    selection: {
      orgIds: selectedOrgIds,
      repoIds: selectedRepoIds,
      kinds: selectedKinds,
    },
    windowParams,
    directory,
  });

  // Shim RepoActivityData → LeaderboardData for the shared KPI / chart
  // components. Same dimensions, just renamed `userRows → rows`.
  const leaderboardData = useMemo<LeaderboardData>(
    () => ({
      rows: data.userRows,
      trend: data.trend,
      mix: data.mix,
      grandTotal: data.grandTotal,
      activeContributors: data.activeContributors,
    }),
    [data],
  );

  const ready = selectedOrgIds.length > 0;

  return (
    <div
      data-testid="report-shell"
      className="flex flex-col gap-4 md:gap-6"
    >
      <div className="px-4 lg:px-6">
        <PageHeading
          title="Repo activity"
          description={
            <>
              <code className="font-mono text-xs">
                GET /reports/org/:org_id?repos=…&amp;group_by=user
              </code>
              {" "}· multi-org · multi-repo · who&apos;s been working on what.
            </>
          }
        />
      </div>

      <div className="px-4 lg:px-6">
        <Card>
          <CardContent className="pt-6">
            <RepoActivityFilters
              orgs={orgs}
              repos={repos}
              selectedOrgIds={selectedOrgIds}
              selectedRepoIds={selectedRepoIds}
              selectedKinds={selectedKinds}
              onOrgsChange={setSelectedOrgIds}
              onReposChange={setSelectedRepoIds}
              onKindsChange={setSelectedKinds}
              windowState={windowState}
              onWindowChange={setWindowState}
              orgsLoading={orgsQuery.isPending}
              reposLoading={reposQuery.isPending}
            />
          </CardContent>
        </Card>
      </div>

      <div className="px-4 lg:px-6">
        <DataAsOfBanner data={dataAsOf} loading={loading && !dataAsOf} />
      </div>

      {!ready ? (
        <div className="px-4 lg:px-6">
          <Card>
            <CardContent className="py-12 text-center text-sm text-muted-foreground">
              Pick at least one org to load the dashboard.
            </CardContent>
          </Card>
        </div>
      ) : error ? (
        <div className="px-4 lg:px-6">
          <Card>
            <CardContent className="py-6 text-sm text-destructive">
              Failed to load dashboard: {error.message}
            </CardContent>
          </Card>
        </div>
      ) : (
        <>
          <LeaderboardKpis
            data={leaderboardData}
            orgCount={selectedOrgIds.length}
          />

          <div className="grid gap-4 px-4 @5xl/main:grid-cols-3 lg:px-6">
            <div className="@5xl/main:col-span-2">
              <ContributorBarChart rows={data.userRows} />
            </div>
            <ActivityMixChart mix={data.mix} grandTotal={data.grandTotal} />
          </div>

          <div className="px-4 lg:px-6">
            <LeaderboardTrendChart trend={data.trend} />
          </div>

          <div className="px-4 lg:px-6">
            <RepoBreakdownTable
              rows={data.repoRows}
              grandTotal={data.grandTotal}
              onSelectRepo={(id) =>
                setFocusedRepoId((cur) => (cur === id ? null : id))
              }
              selectedRepoId={focusedRepoId}
            />
          </div>

          {focusedRepoId && directory.reposById.get(focusedRepoId) ? (
            <div className="px-4 lg:px-6">
              <RepoFocusPanel
                repo={directory.reposById.get(focusedRepoId)!}
                kinds={selectedKinds}
                windowParams={windowParams}
                directory={leaderboardDirectory}
                onClose={() => setFocusedRepoId(null)}
              />
            </div>
          ) : null}

          <div className="px-4 lg:px-6">
            <LeaderboardTable
              rows={data.userRows}
              grandTotal={data.grandTotal}
              directory={leaderboardDirectory}
            />
          </div>
        </>
      )}
    </div>
  );
}
