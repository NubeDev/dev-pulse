/**
 * Shared data hook for the directory section.
 *
 * The dp-rest surface gives us three relevant endpoints:
 *
 *   - `GET /orgs`               — all orgs.
 *   - `GET /users?org_id=…`     — users in one org. `org_id` is
 *                                 optional so a bare call lists every
 *                                 user dev-pulse has observed.
 *   - `GET /teams?org_id=…`     — teams in one org (`org_id` required).
 *
 * Memberships and `home_org` are NOT exposed read-only by Phase 4
 * (only the GDPR export carries them — and that endpoint writes an
 * audit row per call, so we deliberately don't use it for ordinary
 * directory browsing). To still render "memberships + home_org" per
 * the stage description, we:
 *
 *   1. Fan out `GET /users?org_id=…` across every org and invert the
 *      result into a `user_id -> Set<org_id>` map. That gives us
 *      genuine, server-derived membership data.
 *   2. Maintain a client-side optimistic `home_org` map keyed by
 *      `user_id`, seeded empty (or by the mock fixture in smoke
 *      mode) and updated by the home-org assignment mutation. Until
 *      a read-only endpoint lands, this map is the UI's source of
 *      truth for the badge column for the session.
 *
 * `useDirectory()` returns everything in one shape so the four
 * directory pages can share the same react-query cache + the same
 * optimistic home-org store.
 */

import { useCallback, useMemo, useState } from "react";
import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";

import { api } from "../api/client.js";
import type { OrgDto, TeamDto, UserDto } from "../api/client.js";

import {
  MOCK_HOME_ORG,
  MOCK_ORGS,
  MOCK_TEAMS_BY_ORG,
  MOCK_USERS_BY_ORG,
  USE_MOCK,
  mockAllUsers,
} from "./mocks.js";

export interface DirectoryUser {
  user: UserDto;
  /** Org ids this user is a member of. Derived from the per-org
   *  `listUsers` fanout, sorted by org login for stability. */
  org_ids: string[];
  /** Current home-org id, or `null` if unset. Maintained client-side
   *  on top of the seed map (see module doc). */
  home_org: string | null;
}

export interface DirectoryData {
  orgs: ReadonlyArray<OrgDto>;
  /** Flat user list, deduped across orgs, with derived memberships. */
  users: ReadonlyArray<DirectoryUser>;
  /** `org_id -> member count`, derived from the same fanout. */
  memberCount: ReadonlyMap<string, number>;
  loading: boolean;
  error: string | null;
}

/**
 * Optimistic home-org store, scoped to one `useDirectory()` consumer
 * tree. The home-org assignment dialog calls `setHomeOrg` which
 * updates this map before the network round-trip lands, then
 * rolls back on failure. The Users page reads it for the badge
 * column.
 */
interface HomeOrgStore {
  byUser: ReadonlyMap<string, string>;
  set(userId: string, orgId: string): void;
  rollback(userId: string, prev: string | null): void;
}

function useHomeOrgStore(): HomeOrgStore {
  const [byUser, setByUser] = useState<Map<string, string>>(() =>
    USE_MOCK ? new Map(MOCK_HOME_ORG) : new Map(),
  );
  const set = useCallback((userId: string, orgId: string) => {
    setByUser((prev) => {
      const next = new Map(prev);
      next.set(userId, orgId);
      return next;
    });
  }, []);
  const rollback = useCallback((userId: string, prev: string | null) => {
    setByUser((cur) => {
      const next = new Map(cur);
      if (prev === null) next.delete(userId);
      else next.set(userId, prev);
      return next;
    });
  }, []);
  return { byUser, set, rollback };
}

export interface UseDirectoryResult extends DirectoryData {
  /** Look up + mutate the optimistic home-org store. */
  homeOrg: HomeOrgStore;
  /** Refetch every directory query after a mutation lands. */
  invalidate(): void;
}

export function useDirectory(): UseDirectoryResult {
  const qc = useQueryClient();
  const homeOrg = useHomeOrgStore();

  const orgsQuery = useQuery({
    queryKey: ["orgs"],
    queryFn: () => (USE_MOCK ? Promise.resolve([...MOCK_ORGS]) : api.listOrgs()),
  });
  const orgs = orgsQuery.data ?? [];

  // Fan out `listUsers(orgId)` across every org. Each query is
  // keyed by `["users", orgId]` so the cache is shared with any
  // other consumer (e.g. the team page also wants the per-org user
  // count) and individual orgs can be invalidated independently.
  const userQueries = useQueries({
    queries: orgs.map((o) => ({
      queryKey: ["users", o.id],
      queryFn: () =>
        USE_MOCK
          ? Promise.resolve([...(MOCK_USERS_BY_ORG.get(o.id) ?? [])])
          : api.listUsers(o.id),
      // Trade a little staleness for a snappier directory.
      staleTime: 60_000,
    })),
  });

  const { users, memberCount } = useMemo(() => {
    const byUser = new Map<string, DirectoryUser>();
    const counts = new Map<string, number>();
    orgs.forEach((org, idx) => {
      const list = userQueries[idx]?.data ?? [];
      counts.set(org.id, list.length);
      for (const u of list) {
        const cur = byUser.get(u.id);
        if (cur) {
          if (!cur.org_ids.includes(org.id)) cur.org_ids.push(org.id);
        } else {
          byUser.set(u.id, {
            user: u,
            org_ids: [org.id],
            home_org: null,
          });
        }
      }
    });
    // Stamp the optimistic home_org and sort memberships by the
    // canonical org login so the table is deterministic.
    const orgLogin = new Map(orgs.map((o) => [o.id, o.login]));
    const out: DirectoryUser[] = [];
    for (const entry of byUser.values()) {
      entry.org_ids.sort((a, b) =>
        (orgLogin.get(a) ?? "").localeCompare(orgLogin.get(b) ?? ""),
      );
      entry.home_org = homeOrg.byUser.get(entry.user.id) ?? null;
      out.push(entry);
    }
    out.sort((a, b) => a.user.login.localeCompare(b.user.login));
    return { users: out, memberCount: counts };
  }, [orgs, userQueries, homeOrg.byUser]);

  // The home-org assignment dialog has a "select user" dropdown that
  // wants to include users we may not have fetched yet (the smoke
  // harness pre-seeds them via the mock helper). In mock mode we
  // graft those rows in so the dialog isn't empty before the org
  // fanout lands.
  const fallbackUsers = useMemo<DirectoryUser[]>(() => {
    if (!USE_MOCK || users.length > 0) return [];
    return mockAllUsers().map((u) => ({
      user: u,
      org_ids: [],
      home_org: homeOrg.byUser.get(u.id) ?? null,
    }));
  }, [users.length, homeOrg.byUser]);

  const loading =
    orgsQuery.isPending || userQueries.some((q) => q.isPending);
  const error =
    orgsQuery.error?.message ??
    userQueries.find((q) => q.error)?.error?.message ??
    null;

  const invalidate = useCallback(() => {
    void qc.invalidateQueries({ queryKey: ["users"] });
    void qc.invalidateQueries({ queryKey: ["orgs"] });
    void qc.invalidateQueries({ queryKey: ["teams"] });
  }, [qc]);

  return {
    orgs,
    users: users.length > 0 ? users : fallbackUsers,
    memberCount,
    loading,
    error,
    homeOrg,
    invalidate,
  };
}

/**
 * Helper hook for the teams page — `listTeams(org_id)` is mandatory
 * per-org, so we only fire the query once an org is selected.
 */
export function useTeamsForOrg(orgId: string | null): {
  teams: TeamDto[];
  loading: boolean;
  error: string | null;
} {
  const q = useQuery({
    queryKey: ["teams", orgId ?? "__none__"],
    enabled: orgId !== null,
    queryFn: () => {
      if (!orgId) return Promise.resolve([] as TeamDto[]);
      return USE_MOCK
        ? Promise.resolve([...(MOCK_TEAMS_BY_ORG.get(orgId) ?? [])])
        : api.listTeams(orgId);
    },
    staleTime: 60_000,
  });
  return {
    teams: q.data ?? [],
    loading: q.isPending && orgId !== null,
    error: q.error?.message ?? null,
  };
}
