/**
 * Stage-7 mock fixtures for the directory pages.
 *
 * Mirrors the pattern from `reports/freshness-page.tsx` and
 * `reports/user-report-page.tsx`: when `VITE_USE_MOCK_REPORTS=1` is
 * set (the smoke harness flag, reused here for parity), every
 * directory query short-circuits to deterministic data so the pages
 * render fully without `dp-server` running.
 *
 * Three orgs, six users with varied memberships, two of them
 * pre-stamped with a `home_org`, plus a handful of teams so the
 * team list (which is filtered by org) has something to show.
 */

import type { OrgDto, TeamDto, UserDto } from "../api/client.js";

export const USE_MOCK = import.meta.env.VITE_USE_MOCK_REPORTS === "1";

export const MOCK_ORGS: ReadonlyArray<OrgDto> = [
  { id: "00000000-0000-0000-0000-0000000000a1", github_id: 101, login: "acme",       name: "Acme Corp" },
  { id: "00000000-0000-0000-0000-0000000000a2", github_id: 102, login: "globex",     name: "Globex Inc" },
  { id: "00000000-0000-0000-0000-0000000000a3", github_id: 103, login: "initech",    name: "Initech" },
];

/** Per-org user lists. A user appearing in multiple orgs is a
 *  cross-org member; the home_org map below pins which one (if any)
 *  is canonical. */
export const MOCK_USERS_BY_ORG: ReadonlyMap<string, ReadonlyArray<UserDto>> = new Map([
  ["00000000-0000-0000-0000-0000000000a1", [
    { id: "00000000-0000-0000-0000-0000000000u1", github_id: 1, login: "alice",   name: "Alice Example",   email: "alice@example.com" },
    { id: "00000000-0000-0000-0000-0000000000u2", github_id: 2, login: "bob",     name: "Bob Example",     email: "bob@example.com" },
    { id: "00000000-0000-0000-0000-0000000000u3", github_id: 3, login: "carol",   name: "Carol Example",   email: "carol@example.com" },
  ]],
  ["00000000-0000-0000-0000-0000000000a2", [
    { id: "00000000-0000-0000-0000-0000000000u2", github_id: 2, login: "bob",     name: "Bob Example",     email: "bob@example.com" },
    { id: "00000000-0000-0000-0000-0000000000u4", github_id: 4, login: "dave",    name: "Dave Example",    email: "dave@example.com" },
  ]],
  ["00000000-0000-0000-0000-0000000000a3", [
    { id: "00000000-0000-0000-0000-0000000000u3", github_id: 3, login: "carol",   name: "Carol Example",   email: "carol@example.com" },
    { id: "00000000-0000-0000-0000-0000000000u5", github_id: 5, login: "eve",     name: "Eve Example",     email: "eve@example.com" },
    { id: "00000000-0000-0000-0000-0000000000u6", github_id: 6, login: "frank",   name: "Frank Example",   email: "frank@example.com" },
  ]],
]);

/** Initial home-org seeds. The directory UI maintains an optimistic
 *  map on top of this so newly set assignments show up immediately. */
export const MOCK_HOME_ORG: ReadonlyMap<string, string> = new Map([
  ["00000000-0000-0000-0000-0000000000u1", "00000000-0000-0000-0000-0000000000a1"],
  ["00000000-0000-0000-0000-0000000000u2", "00000000-0000-0000-0000-0000000000a1"],
]);

export const MOCK_TEAMS_BY_ORG: ReadonlyMap<string, ReadonlyArray<TeamDto>> = new Map([
  ["00000000-0000-0000-0000-0000000000a1", [
    { id: "00000000-0000-0000-0000-0000000000t1", org_id: "00000000-0000-0000-0000-0000000000a1", github_id: 201, slug: "platform",  name: "Platform" },
    { id: "00000000-0000-0000-0000-0000000000t2", org_id: "00000000-0000-0000-0000-0000000000a1", github_id: 202, slug: "frontend",  name: "Frontend" },
  ]],
  ["00000000-0000-0000-0000-0000000000a2", [
    { id: "00000000-0000-0000-0000-0000000000t3", org_id: "00000000-0000-0000-0000-0000000000a2", github_id: 203, slug: "infra",     name: "Infra" },
  ]],
  ["00000000-0000-0000-0000-0000000000a3", []],
]);

/** Union of every mock user (deduped by id). Used by the
 *  home-org assignment dialog so the user dropdown can name people
 *  even if no org filter is applied. */
export function mockAllUsers(): UserDto[] {
  const seen = new Map<string, UserDto>();
  for (const list of MOCK_USERS_BY_ORG.values()) {
    for (const u of list) if (!seen.has(u.id)) seen.set(u.id, u);
  }
  return [...seen.values()].sort((a, b) => a.login.localeCompare(b.login));
}
