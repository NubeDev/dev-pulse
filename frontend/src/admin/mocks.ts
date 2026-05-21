/**
 * Stage-8 mock fixtures for the admin pages.
 *
 * Same `VITE_USE_MOCK_REPORTS=1` flag the other sections use (reused
 * here for parity) — short-circuits `GET /admin/runs`,
 * `POST /admin/refresh`, `GET /admin/users/:id/export`, and
 * `POST /admin/users/:id/anonymise` to deterministic data so the
 * pages render and the buttons resolve without `dp-server`.
 */

import type {
  FetchRunDto,
  OrgDto,
  RefreshResponse,
  UserDto,
  UserExport,
} from "../api/client.js";

export const USE_MOCK = import.meta.env.VITE_USE_MOCK_REPORTS === "1";

/** Deterministic fixed-instant clock used by the run log mock so the
 *  rendered "started/finished" timestamps don't drift between renders. */
const MOCK_NOW = Date.UTC(2026, 4, 20, 12, 0, 0); // 2026-05-20T12:00:00Z
const MIN = 60 * 1000;

/** Big enough that pagination shows two pages at default `limit=25`,
 *  small enough that the smoke harness doesn't spend visible time
 *  scrolling.  One row per kind/state combination so the rendering
 *  branches (running / partial / failed / clean) are all exercised. */
export const MOCK_RUNS: ReadonlyArray<FetchRunDto> = [
  // running (no `finished`)
  { id: "00000000-0000-0000-0000-0000000000r1", kind: "reconciler",
    started: new Date(MOCK_NOW - 2 * MIN).toISOString(), finished: null,
    items: 0, errors: 0, partial: false },
  // clean
  { id: "00000000-0000-0000-0000-0000000000r2", kind: "reconciler",
    started: new Date(MOCK_NOW - 10 * MIN).toISOString(),
    finished: new Date(MOCK_NOW - 9 * MIN).toISOString(),
    items: 142, errors: 0, partial: false },
  // partial
  { id: "00000000-0000-0000-0000-0000000000r3", kind: "reconciler",
    started: new Date(MOCK_NOW - 70 * MIN).toISOString(),
    finished: new Date(MOCK_NOW - 69 * MIN).toISOString(),
    items: 88, errors: 3, partial: true,
    error_sample: [
      { org: "acme",   repo: "acme/api",        kind: "Issues",       error: "GitHub 502 Bad Gateway" },
      { org: "globex", repo: "globex/web",      kind: "PullRequests", error: "rate limit: remaining=0, reset in 412s" },
      { org: "acme",   repo: "acme/scheduler",  kind: "Commits",      error: "timeout reading branch list (deadline 10s)" },
    ] },
  // failed (errors > 0, not partial — every item failed)
  { id: "00000000-0000-0000-0000-0000000000r4", kind: "backfill",
    started: new Date(MOCK_NOW - 4 * 60 * MIN).toISOString(),
    finished: new Date(MOCK_NOW - 3 * 60 * MIN).toISOString(),
    items: 0, errors: 12, partial: false,
    error_sample: [
      { org: "initech", repo: "initech/legacy", kind: "Issues",       error: "GitHub 401 Unauthorized — installation token expired" },
      { org: "initech", repo: "initech/legacy", kind: "PullRequests", error: "GitHub 401 Unauthorized — installation token expired" },
      { org: "initech", repo: "initech/legacy", kind: "Commits",      error: "GitHub 401 Unauthorized — installation token expired" },
    ] },
  ...Array.from({ length: 28 }, (_, i): FetchRunDto => ({
    id: `00000000-0000-0000-0000-${(0xc0ffee + i).toString(16).padStart(12, "0")}`,
    kind: i % 4 === 0 ? "backfill" : "reconciler",
    started: new Date(MOCK_NOW - (i + 5) * 60 * MIN).toISOString(),
    finished: new Date(MOCK_NOW - (i + 5) * 60 * MIN + 45_000).toISOString(),
    items: 50 + i * 7,
    errors: 0,
    partial: false,
  })),
];

export function paginateMockRuns(
  limit: number,
  offset: number,
): FetchRunDto[] {
  return MOCK_RUNS.slice(offset, offset + limit);
}

export const MOCK_ORGS: ReadonlyArray<OrgDto> = [
  { id: "00000000-0000-0000-0000-0000000000a1", github_id: 101, login: "acme",    name: "Acme Corp" },
  { id: "00000000-0000-0000-0000-0000000000a2", github_id: 102, login: "globex",  name: "Globex Inc" },
  { id: "00000000-0000-0000-0000-0000000000a3", github_id: 103, login: "initech", name: "Initech" },
];

export const MOCK_USERS: ReadonlyArray<UserDto> = [
  { id: "00000000-0000-0000-0000-0000000000u1", github_id: 1, login: "alice", name: "Alice Example", email: "alice@example.com" },
  { id: "00000000-0000-0000-0000-0000000000u2", github_id: 2, login: "bob",   name: "Bob Example",   email: "bob@example.com" },
  { id: "00000000-0000-0000-0000-0000000000u3", github_id: 3, login: "carol", name: "Carol Example", email: "carol@example.com" },
];

export function mockRefresh(): RefreshResponse {
  return { ran: true, items: 17, errors: 0, partial: false };
}

export function mockUserExport(userId: string): UserExport {
  const user = MOCK_USERS.find((u) => u.id === userId) ?? MOCK_USERS[0]!;
  return {
    user,
    memberships: [
      { user_id: user.id, org_id: MOCK_ORGS[0]!.id, role: "member",
        joined_at: new Date(MOCK_NOW - 365 * 24 * 60 * MIN).toISOString(),
        home_org: MOCK_ORGS[0]!.id },
    ],
    events: [
      { event_id: "00000000-0000-0000-0000-0000000000e1",
        org_id: MOCK_ORGS[0]!.id,
        repo_id: "00000000-0000-0000-0000-0000000000re",
        kind: "pull_request_opened",
        ts: new Date(MOCK_NOW - 60 * MIN).toISOString(),
        roles: ["author"] },
    ],
  };
}
