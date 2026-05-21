/**
 * `GET /reports/org/:org_id?group_by=user&activity_types=…` rendered
 * as a dashboard-style leaderboard built on the existing per-org
 * count endpoint. The full `dp-reports::leaderboard` SQL machinery
 * (PR #9) ships the library but doesn't expose a REST route yet;
 * this page fans the count-by-user reducer across multiple
 * `(org × kind)` slices client-side and folds the results in
 * `useLeaderboardData`.
 *
 * Features that lift this above the v1 single-org list:
 *
 *   - Multi-org selection (one user can belong to many orgs).
 *   - Multi-user filter (focus on a custom cohort).
 *   - Multi-activity-type selection (default: all kinds).
 *   - KPI strip · stacked bar chart · activity-mix donut · daily
 *     trend area · ranked table with per-org chips and share bar.
 *
 * The visual + component vocabulary mirrors the existing
 * user/team/org pages (PageHeading + filter Card + DataAsOfBanner +
 * SectionCards + recharts) so the page slots into the app shell
 * without introducing a new look-and-feel.
 */

import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { Card, CardContent } from "@/components/ui/card";

import { api } from "../api/client.js";
import type { OrgDto, UserDto } from "../api/client.js";
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
  LeaderboardFilters,
  LeaderboardKpis,
  LeaderboardTable,
  LeaderboardTrendChart,
  useLeaderboardData,
  type DirectoryMaps,
} from "./leaderboard/index.js";

export function LeaderboardPage(): JSX.Element {
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

  const directory = useMemo<DirectoryMaps>(
    () => ({
      orgsById: new Map(orgs.map((o) => [o.id, o])),
      usersById: new Map(users.map((u) => [u.id, u])),
    }),
    [orgs, users],
  );

  const [selectedOrgIds, setSelectedOrgIds] = useState<ReadonlyArray<string>>([]);
  const [selectedUserIds, setSelectedUserIds] = useState<ReadonlyArray<string>>([]);
  const [selectedKinds, setSelectedKinds] = useState<ReadonlyArray<string>>([
    "commit",
    "pull_request_merged",
    "review",
  ]);
  const [windowState, setWindowState] = useState<WindowState>(
    defaultWindowState(),
  );

  // Default to the first org once the directory call resolves; the
  // user can then add more from the multi-select.
  useEffect(() => {
    if (selectedOrgIds.length === 0 && orgs.length > 0) {
      setSelectedOrgIds([orgs[0]!.id]);
    }
  }, [orgs, selectedOrgIds.length]);

  // Drop user filters that no longer correspond to a known user (e.g.
  // org selection narrowed and a stale id is in state).
  useEffect(() => {
    if (selectedUserIds.length === 0) return;
    const known = new Set(users.map((u) => u.id));
    const filtered = selectedUserIds.filter((id) => known.has(id));
    if (filtered.length !== selectedUserIds.length) {
      setSelectedUserIds(filtered);
    }
  }, [users, selectedUserIds]);

  const windowParams = useMemo(
    () => windowStateToParams(windowState),
    [windowState],
  );

  const { data, loading, error, dataAsOf } = useLeaderboardData({
    selection: {
      orgIds: selectedOrgIds,
      userIds: selectedUserIds,
      kinds: selectedKinds,
    },
    windowParams,
    directory,
  });

  const ready = selectedOrgIds.length > 0;

  return (
    <div
      data-testid="report-shell"
      className="flex flex-col gap-4 md:gap-6"
    >
      <div className="px-4 lg:px-6">
        <PageHeading
          title="Contributor dashboard"
          description={
            <>
              <code className="font-mono text-xs">
                GET /reports/org/:org_id?group_by=user
              </code>
              {" "}· multi-org · multi-user · multi-activity ranked view.
            </>
          }
        />
      </div>

      <div className="px-4 lg:px-6">
        <Card>
          <CardContent className="pt-6">
            <LeaderboardFilters
              orgs={orgs}
              users={users}
              selectedOrgIds={selectedOrgIds}
              selectedUserIds={selectedUserIds}
              selectedKinds={selectedKinds}
              onOrgsChange={setSelectedOrgIds}
              onUsersChange={setSelectedUserIds}
              onKindsChange={setSelectedKinds}
              windowState={windowState}
              onWindowChange={setWindowState}
              orgsLoading={orgsQuery.isPending}
              usersLoading={usersQuery.isPending}
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
          <LeaderboardKpis data={data} orgCount={selectedOrgIds.length} />

          <div className="grid gap-4 px-4 @5xl/main:grid-cols-3 lg:px-6">
            <div className="@5xl/main:col-span-2">
              <ContributorBarChart rows={data.rows} limit={data.rows.length} />
            </div>
            <ActivityMixChart mix={data.mix} grandTotal={data.grandTotal} />
          </div>

          <div className="px-4 lg:px-6">
            <LeaderboardTrendChart trend={data.trend} />
          </div>

          <div className="px-4 lg:px-6">
            <LeaderboardTable
              rows={data.rows}
              grandTotal={data.grandTotal}
              directory={directory}
              limit={data.rows.length}
            />
          </div>
        </>
      )}
    </div>
  );
}
