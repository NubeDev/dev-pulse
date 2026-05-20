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
  PinDto,
  TagDto,
  TagDetailResponse,
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
