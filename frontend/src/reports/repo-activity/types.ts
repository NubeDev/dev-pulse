/**
 * Internal types for the repo-activity dashboard
 * (`#/reports/repo-activity`).
 *
 * The page lets the user pick one or many orgs and one or many repos
 * and answers "what has each contributor been working on?". It
 * composes several `/reports/org/:org_id?repos=…` calls and folds
 * them into a single dataset every chart and table consumes.
 *
 * The shape intentionally mirrors `leaderboard/types.ts` so the
 * existing `ContributorBarChart`, `ActivityMixChart`,
 * `LeaderboardTrendChart`, and `LeaderboardKpis` components can be
 * reused unchanged.
 */

import type { OrgDto, RepoSummaryDto, UserDto } from "../../api/client.js";
import type {
  LeaderUserRow,
  MixSlice,
  TrendBucket,
} from "../leaderboard/types.js";

/** One row in the per-repo breakdown table. */
export interface RepoActivityRow {
  /** Repo UUID — the bucket key the server returned. */
  repoId: string;
  /** `org/name` slug, when known. Falls back to a UUID prefix. */
  label: string;
  /** Owning org UUID, when known. */
  orgId?: string;
  /** Owning org login, when known — rendered as the muted subtitle. */
  orgLogin?: string;
  /** Per-activity-kind counts, keyed by snake_case `EventKind`. */
  perKind: Record<string, number>;
  /** Sum of `perKind` — the column the table ranks on. */
  total: number;
  /** Distinct contributor count for this repo across the selection. */
  contributors: number;
}

/** The derived dataset every repo-activity child component consumes. */
export interface RepoActivityData {
  /** Per-user rows (same shape as the leaderboard so the existing
   *  contributor chart + table can be reused). */
  userRows: ReadonlyArray<LeaderUserRow>;
  /** Per-repo rows for the breakdown table below. */
  repoRows: ReadonlyArray<RepoActivityRow>;
  /** Daily trend bucket — sum across every selected org/repo/kind. */
  trend: ReadonlyArray<TrendBucket>;
  /** Activity-mix donut data. */
  mix: ReadonlyArray<MixSlice>;
  /** Total events across the whole selection — the headline KPI. */
  grandTotal: number;
  /** Distinct contributors (rows with `total > 0`). */
  activeContributors: number;
  /** Distinct repos that produced at least one event in the window. */
  activeRepos: number;
}

export interface RepoActivitySelection {
  /** Selected org UUIDs. Empty == "no org" (we render an empty
   *  state rather than a cross-tenant default). */
  orgIds: ReadonlyArray<string>;
  /** Selected repo UUIDs. Empty == "every repo inside the selected
   *  orgs" (the server returns all of them when no filter is set). */
  repoIds: ReadonlyArray<string>;
  /** Selected `EventKind` strings. Empty == "all activity kinds". */
  kinds: ReadonlyArray<string>;
}

export interface RepoActivityDirectory {
  orgsById: ReadonlyMap<string, OrgDto>;
  usersById: ReadonlyMap<string, UserDto>;
  reposById: ReadonlyMap<string, RepoSummaryDto>;
}
