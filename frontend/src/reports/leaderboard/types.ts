/**
 * Internal types for the leaderboard dashboard.
 *
 * The leaderboard composes several `/reports/org/:org_id` calls (one
 * per `(org × kind)` for the stacked composition + one per org with
 * `group_by=day` for the trend) and folds them into a few derived
 * shapes the chart / table components consume directly.
 */

import type { OrgDto, UserDto } from "../../api/client.js";

/** A single user row in the merged leaderboard, with per-activity
 *  breakdown so the table and stacked bar share the same source. */
export interface LeaderUserRow {
  /** User UUID — the bucket key the server returned. */
  userId: string;
  /** Resolved display label (falls back to `userId` slice when the
   *  directory hasn't returned a matching `UserDto`). */
  label: string;
  /** GitHub login, when known — rendered as the muted subtitle. */
  login?: string;
  /** Per-activity-kind counts, keyed by snake_case `EventKind`. */
  perKind: Record<string, number>;
  /** Sum of `perKind` — the column the leaderboard ranks on. */
  total: number;
  /** Per-org breakdown (org UUID → count). Used to show which orgs
   *  the user contributes to and as a tooltip in the bar chart. */
  perOrg: Record<string, number>;
}

/** One bucket on the daily trend chart — `events` is the sum across
 *  every selected org / kind / user for that day. */
export interface TrendBucket {
  /** ISO date (start-of-day UTC) the server returned for this row. */
  date: string;
  /** Total events. */
  events: number;
}

/** One slice on the activity-mix donut. */
export interface MixSlice {
  /** snake_case `EventKind`. */
  kind: string;
  /** Human label from `ACTIVITY_KINDS`. */
  label: string;
  /** Sum across every selected org + user for this kind. */
  count: number;
}

/** The derived dataset every leaderboard child component consumes. */
export interface LeaderboardData {
  rows: ReadonlyArray<LeaderUserRow>;
  trend: ReadonlyArray<TrendBucket>;
  mix: ReadonlyArray<MixSlice>;
  /** Total events across the whole selection — the headline KPI. */
  grandTotal: number;
  /** Distinct contributors (rows with `total > 0`). */
  activeContributors: number;
}

export interface LeaderboardSelection {
  /** Selected org UUIDs. Empty == "no org" (we render an empty state
   *  rather than a cross-tenant default). */
  orgIds: ReadonlyArray<string>;
  /** Selected user UUIDs. Empty == "all users in those orgs". */
  userIds: ReadonlyArray<string>;
  /** Selected `EventKind` strings. Empty == "all activity kinds". */
  kinds: ReadonlyArray<string>;
}

export interface DirectoryMaps {
  orgsById: ReadonlyMap<string, OrgDto>;
  usersById: ReadonlyMap<string, UserDto>;
}
