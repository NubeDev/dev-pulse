/**
 * Stage 11 mock fixtures for the workflow surface (Pins / Tags /
 * Issues). Same `VITE_USE_MOCK_REPORTS=1` flag the other sections
 * use — short-circuits `/me/pins`, `/tags`, `/me/app-install-banner`,
 * and the §8 issue CRUD endpoints so the surfaces render and the
 * forms exercise the §8.3 stale-version reload UX without
 * `dp-server`.
 */

import type {
  AppInstallBannerResponse,
  IssueDto,
  IssueListItem,
  IssueListResponse,
  ListIssuesQuery,
  ListReposQuery,
  PinDto,
  RepoListResponse,
  RepoSummaryDto,
  TagDto,
  TagDetailResponse,
  UserIssueStateDto,
} from "../api/client.js";

export const USE_MOCK = import.meta.env.VITE_USE_MOCK_REPORTS === "1";

const MOCK_NOW = Date.UTC(2026, 4, 20, 12, 0, 0);

const u = (n: number): string =>
  `00000000-0000-0000-0000-${n.toString(16).padStart(12, "0")}`;

export const MOCK_ORG_NUBE = u(0x10001);
export const MOCK_ORG_PHX = u(0x10002);
export const MOCK_REPO_API = u(0x20001);
export const MOCK_REPO_WEB = u(0x20002);
export const MOCK_REPO_INFRA = u(0x20003);
export const MOCK_TAG_PHOENIX = u(0x30001);
export const MOCK_TAG_ONCALL = u(0x30002);
export const MOCK_USER_VIEWER = u(0x40001);
export const MOCK_ISSUE_1 = u(0x50001);

/** Mutable so the form mock can flip `version` after a successful
 *  PATCH and exercise the §8.3 stale-version reload. */
export const mockPinsState: PinDto[] = [
  { kind: "repo", target_id: MOCK_REPO_API, position: 0, pinned_at: new Date(MOCK_NOW).toISOString() },
  { kind: "tag", target_id: MOCK_TAG_PHOENIX, position: 1, pinned_at: new Date(MOCK_NOW).toISOString() },
  { kind: "repo", target_id: MOCK_REPO_WEB, position: 2, pinned_at: new Date(MOCK_NOW).toISOString() },
];

export const mockTagsState: TagDto[] = [
  {
    id: MOCK_TAG_PHOENIX,
    scope_kind: "org",
    scope_id: MOCK_ORG_NUBE,
    name: "Phoenix",
    color: "indigo",
    description: "Cross-org migration to the new pipeline.",
    created_by: MOCK_USER_VIEWER,
    created_at: new Date(MOCK_NOW - 5 * 86400 * 1000).toISOString(),
    archived_at: null,
    visible_link_count: 7,
  },
  {
    id: MOCK_TAG_ONCALL,
    scope_kind: "user",
    scope_id: MOCK_USER_VIEWER,
    name: "On-call follow-ups",
    color: "red",
    description: null,
    created_by: MOCK_USER_VIEWER,
    created_at: new Date(MOCK_NOW - 2 * 86400 * 1000).toISOString(),
    archived_at: null,
    visible_link_count: 3,
  },
];

export const mockTagDetail = (id: string): TagDetailResponse => {
  const tag = mockTagsState.find((t) => t.id === id);
  if (!tag) throw new Error("mock tag_not_found");
  const links =
    tag.id === MOCK_TAG_PHOENIX
      ? [
          { id: u(0x60001), tag_id: tag.id, kind: "repo" as const, target_id: MOCK_REPO_API,
            added_by: MOCK_USER_VIEWER, added_at: tag.created_at },
          { id: u(0x60002), tag_id: tag.id, kind: "repo" as const, target_id: MOCK_REPO_WEB,
            added_by: MOCK_USER_VIEWER, added_at: tag.created_at },
          { id: u(0x60003), tag_id: tag.id, kind: "repo" as const, target_id: MOCK_REPO_INFRA,
            added_by: MOCK_USER_VIEWER, added_at: tag.created_at },
          { id: u(0x60004), tag_id: tag.id, kind: "issue" as const, target_id: MOCK_ISSUE_1,
            added_by: MOCK_USER_VIEWER, added_at: tag.created_at },
        ]
      : [];
  return { tag, links, links_page: 0, links_page_size: 100 };
};

/** Mutable so `updateIssue` mock can bump version and exercise the
 *  CAS roundtrip the form runs on submit. */
export const mockIssue: IssueDto = {
  id: MOCK_ISSUE_1,
  repo_id: MOCK_REPO_API,
  org_id: MOCK_ORG_NUBE,
  number: 1234,
  title: "Pipeline reaper hangs on shutdown",
  body: "Reaper holds a connection-pool slot through SIGTERM; rolling restarts stall.",
  state: "open",
  labels: ["bug", "infra"],
  assignees: ["operator"],
  milestone: null,
  version: 3,
  updated_at: new Date(MOCK_NOW - 60_000).toISOString(),
};

/** One read-only org (so the writes-not-available banner shows) and
 *  one writable. */
export const mockAppInstallBanner: AppInstallBannerResponse = {
  request_issues_write: true,
  orgs: [
    {
      org_id: MOCK_ORG_NUBE,
      login: "NubeIO",
      name: "Nube",
      writes_available: true,
      manage_url: "https://github.com/organizations/NubeIO/settings/installations/111/permissions",
      admin_copy_text:
        "Hi — please re-consent to the dev-pulse GitHub App so it can file follow-up issues.",
    },
    {
      org_id: MOCK_ORG_PHX,
      login: "phoenix-dev",
      name: "Phoenix Dev",
      writes_available: false,
      manage_url: "https://github.com/organizations/phoenix-dev/settings/installations/222/permissions",
      admin_copy_text:
        "Hi — please re-consent to the dev-pulse GitHub App so it can file follow-up issues in phoenix-dev.",
    },
  ],
};

// ---------------------------------------------------------------------------
// Workbench (§14.9) — issue list fixtures.
//
// One row only: the existing `mockIssue`, projected into the list shape.
// Keeps the §8.3 stale-version UX exercisable without fabricating
// product data the smoke harness does not need.
// ---------------------------------------------------------------------------

export const mockIssueList: IssueListItem[] = [
  {
    id: mockIssue.id,
    repo_id: mockIssue.repo_id,
    org_id: mockIssue.org_id,
    repo_slug: null,
    number: mockIssue.number,
    title: mockIssue.title,
    body: mockIssue.body,
    milestone: mockIssue.milestone,
    version: mockIssue.version,
    state: mockIssue.state,
    labels: mockIssue.labels,
    assignees: mockIssue.assignees,
    updated_at: mockIssue.updated_at,
  },
];

/** Mock-side per-user inbox state, keyed by issue id. Mirrors the
 *  `dp_user_issue_state` row the real backend stores. Mutable so the
 *  smoke harness can exercise the §3.8 mark-seen / snooze / done UX
 *  without a backend. */
export const mockInboxState = new Map<string, UserIssueStateDto>();

/** Mock-side `GET /me/queue`. Hides rows with `status = "done"` and
 *  rows snoozed past `now`; projects `unread` from the mock state. */
export function mockListMyQueue(q: ListIssuesQuery): IssueListResponse {
  const base = mockListIssues({ ...q });
  const now = Date.now();
  const rows = base.rows
    .map((row) => {
      const st = mockInboxState.get(row.id);
      const unread = !st || row.version > st.last_seen_version;
      return { row, st, unread };
    })
    .filter(({ st }) => {
      if (!st) return true;
      if (st.status === "done") return false;
      if (
        st.status === "snoozed" &&
        st.snoozed_until &&
        new Date(st.snoozed_until).getTime() > now
      ) {
        return false;
      }
      return true;
    })
    .map(({ row, unread }) => ({ ...row, unread }));
  return { ...base, rows, total: rows.length };
}

/** Mock-side `POST /me/inbox/seen`. Bumps `last_seen_version` to the
 *  current row version. */
export function mockMarkInboxSeen(issueIds: string[]): void {
  for (const id of issueIds) {
    const row = mockIssueList.find((r) => r.id === id);
    if (!row) continue;
    const prev = mockInboxState.get(id);
    mockInboxState.set(id, {
      issue_id: id,
      last_seen_version: Math.max(prev?.last_seen_version ?? 0, row.version),
      status: prev?.status ?? "inbox",
      snoozed_until: prev?.snoozed_until ?? null,
      updated_at: new Date().toISOString(),
    });
  }
}

/** Mock-side `PATCH /me/inbox/{issue_id}`. */
export function mockSetInboxState(
  issueId: string,
  status: "inbox" | "snoozed" | "done",
  snoozed_until: string | null | undefined,
): UserIssueStateDto {
  const row = mockIssueList.find((r) => r.id === issueId);
  const prev = mockInboxState.get(issueId);
  const next: UserIssueStateDto = {
    issue_id: issueId,
    last_seen_version: prev?.last_seen_version ?? row?.version ?? 0,
    status,
    snoozed_until: snoozed_until ?? null,
    updated_at: new Date().toISOString(),
  };
  mockInboxState.set(issueId, next);
  return next;
}

/** Mock-side filter mirroring the real `GET /issues` axes. Returns
 *  the same paginated envelope shape the server emits. */
export function mockListIssues(q: ListIssuesQuery): IssueListResponse {
  const state = q.state ?? "open";
  const filtered = mockIssueList.filter((row) => {
    if (state !== "all" && row.state !== state) return false;
    if (q.repo_id && row.repo_id !== q.repo_id) return false;
    if (q.org_id && row.org_id !== q.org_id) return false;
    if (q.assignee && !row.assignees.includes(q.assignee)) return false;
    if (q.q && !row.title.toLowerCase().includes(q.q.toLowerCase())) return false;
    return true;
  });
  const offset = Math.max(0, q.offset ?? 0);
  const limit = Math.min(Math.max(1, q.limit ?? 50), 200);
  return {
    rows: filtered.slice(offset, offset + limit),
    total: filtered.length,
    limit,
    offset,
  };
}

// ---------------------------------------------------------------------------
// Repos mock list (workflow drill-down master).
// ---------------------------------------------------------------------------

export const mockRepoList: RepoSummaryDto[] = [
  {
    id: MOCK_REPO_API,
    org_id: MOCK_ORG_NUBE,
    org_login: "nube",
    name: "api",
    slug: "nube/api",
    open_issue_count: 1,
    last_activity_at: mockIssue.updated_at,
  },
];

export function mockListRepos(q: ListReposQuery): RepoListResponse {
  const filtered = mockRepoList.filter((r) => {
    if (q.org_id && r.org_id !== q.org_id) return false;
    if (q.q) {
      const needle = q.q.toLowerCase();
      if (!r.slug.toLowerCase().includes(needle)) return false;
    }
    return true;
  });
  const offset = Math.max(0, q.offset ?? 0);
  const limit = Math.min(Math.max(1, q.limit ?? 50), 200);
  return {
    rows: filtered.slice(offset, offset + limit),
    total: filtered.length,
    limit,
    offset,
  };
}
