# Triage: a Linear-style workflow for dev-pulse

> A re-pitch of the workflow surface. Replaces the rejected
> "Repos → Issues drill-down" with a single cross-org triage view
> sized for 100s of repos, 1000s of issues, 50+ users across
> multiple orgs.

---

## 0. Progress log (2026-05-20)

Slice 1 of the §8 plan landed, plus the first half of slice 2.
The triage shell is now reachable at `#/workflow/triage` (also
the default `#/workflow` redirect) and the old `Repos` / `Issues`
pages are still mounted as siblings under the Workflow group in
the sidebar.

### Done

**Backend (cargo build + tests green):**

- Migration `crates/dp-store-pg/migrations/dp/0011_triage_spine.sql`
  — adds `dp_issues.author` / `state_reason`, GIN indexes on
  `labels` / `assignees`, and the new `dp_user_issue_state` table
  (`user_id, issue_id, last_seen_version BIGINT, status TEXT
  CHECK ('inbox','snoozed','done'), snoozed_until, updated_at`)
  with partial indexes.
- `dp-domain` — new `inbox.rs` (`InboxStatus`, `UserIssueState`,
  `InboxIssueRow`); `IssueListFilter` extended with array
  filters (`repo_ids`, `org_ids`, `assignees`, `labels`,
  `author`, `state_reason`, `updated_since`, `untriaged_only`)
  plus the four new `Store` methods (`list_inbox_issues`,
  `count_inbox_issues`, `mark_issues_seen`, `set_inbox_state`).
- `dp-store-pg` — implementations for all new methods; 15-bind
  guarded SQL (`(cardinality($N::uuid[]) = 0 OR ... = ANY($N))`
  + `($N::jsonb IS NULL OR labels @> $N)`); `row_to_inbox_issue_row`
  decoder computes `unread = version > last_seen_version`.
- `dp-rest` — `IssueDto.unread` (optional), `ListIssuesQuery`
  with CSV array deserializers, `filter_from_query` helper,
  `GET /me/queue` (`issues_read::me_queue`), new `inbox.rs`
  exposing `POST /me/inbox/seen` (cap 200) and
  `PATCH /me/inbox/{issue_id}`, both gated on `(issues, read)`.
- `dp-server` — `inbox_router` merged into protected; also
  registered the missing `issues` + `tags` resources in
  `register_dev_pulse_resources` so the policy engine stops
  returning `unknown_resource` for the new routes.

**Frontend (`pnpm typecheck` + `make build` green):**

- `frontend/src/api/client.ts` — extended `IssueDto` /
  `IssueListItem` with optional `unread`, new
  `InboxStatusSchema` / `UserIssueStateDtoSchema` /
  `MarkSeenRequest` / `SetInboxStateRequest`,
  `buildIssueListQs` helper, `sendNoContent` private,
  `listMyQueue` / `markInboxSeen` / `setInboxState` methods.
- `frontend/src/workflow/mocks.ts` — `mockInboxState` map +
  `mockListMyQueue` / `mockMarkInboxSeen` / `mockSetInboxState`
  fixtures.
- `frontend/src/workflow/use-workflow-data.ts` — `useMyQueue`,
  `useMarkInboxSeen` (silent failure), `useSetInboxState`,
  plus `workflowKeys.myQueue`.
- `frontend/src/workflow/issues-page.tsx` — added `view`
  (`list` / `inbox` / `untriaged`) URL state, view toggle rail,
  unread-dot column, inline Snooze 1d / Done buttons,
  mark-seen-on-open, keyboard shortcuts (`j` `k` `Enter` `Esc`
  `e` `h` `?`) + help dialog.
- `frontend/src/workflow/triage-page.tsx` (new, ~570 lines) —
  Linear-style 3-pane shell: left rail VIEWS
  (`mine` / `untriaged` / `snoozed` / `all`) + Pinned repos,
  middle dense `<ol>/<li>` with unread dots / state pills /
  hover-revealed inbox actions, right peek panel (xl only)
  embedding `IssueEditCard` inline (no Sheet flash), keyboard
  parity with the issues page, help dialog.
- `frontend/src/routes.ts` — `WorkflowTab` extended with
  `triage` (now the default), `TriageView` +
  `workflowTriageRoute(...)` helper.
- `frontend/src/app.tsx` — `WorkflowPane` switch routes
  `triage` → `<TriagePage />`.
- `frontend/src/layout/app-shell.tsx` — `NAV_MAIN` exposes
  Triage above Repos / Issues (IconInbox); the live inbox badge
  follows `#/workflow/triage` (testid
  `workflow-triage-inbox-badge`); `WORKFLOW_TITLE` carries
  `triage: "Triage"`.
- `frontend/src/components/nav-main.tsx` — `NavMainSubItem`
  gains an optional `badge?: ReactNode` slot (testid
  `nav-sub-badge`), right-aligned.
- `frontend/vite.config.ts` — proxy table now forwards
  `/issues`, `/repos`, `/me`, `/pins`, `/tags` to dp-server on
  `:8731`. Without this the dev server returned the SPA
  `index.html` for `/me/queue` and the client failed with
  `Unexpected token '<', "<!doctype "... is not valid JSON`.

### Verified

- `cargo test -p dp-domain -p dp-rest -p dp-store-pg -p dp-server --lib` — all green.
- `cd frontend && pnpm typecheck` — clean.
- `make build` — clean.

### TODO — pick up here

**Backend (slice 1.5 — P0 bugs found in peer review):**

- [ ] **Fix `list_inbox_issues` to actually return mine.** Today's
      query returns "all open everywhere" rather than the
      caller's identity-set. Until this lands, every screenshot
      lies. Block on §3.0 identity-set semantics so we don't
      rewrite the predicate twice.
- [ ] **Unread must not count my own writes.** §3.8: split
      `dp_issues.version` so unread compares against an
      `external_version` that only bumps on reconciler-applied
      changes — *not* on the caller's own CAS commits. Today
      alice edits a row, walks away, comes back, and sees her
      own edit as unread. Migration in §6.
- [ ] **`POST /me/inbox/seen` body must carry the version the
      client actually saw** (`[{ issue_id, version }]`), and the
      server upserts `last_seen_version = LEAST(version_seen,
      current_external_version)`. The current `[Uuid]` shape
      races any write that lands between row-open and bump.

**Backend (slice 2):**

- [ ] **Multi-identity model (NEW — see §3.0).** Add
      `dp_user_identities(user_id, github_user_id, github_login,
      linked_at, verified_via)` migration; backfill from the
      existing `dp_users.github_id` primary identity. Extend
      `GithubOrgsStamper` to stamp
      `Principal.extra.github.{logins,user_ids,orgs}` (sets, not
      scalars). Add `identities` resource to
      `register_dev_pulse_resources`. Add endpoints:
      `GET /me/identities`, `POST /me/identities/link/{start,
      callback}`, `DELETE /me/identities/{github_user_id}` (refuse
      last-identity removal), `POST /admin/users/{user_id}/identities`.
      Add `dev-pulse link-identity` CLI for admin link.
- [ ] **Tighten "My queue" to identity-set semantics.**
      `list_inbox_issues` currently returns every open issue with
      no `done`/`snoozed` row — i.e. it's "all open everywhere"
      not "mine". Rewrite to: open AND
      (`assignees ?| caller.github_logins` OR
       `author = ANY(caller.github_logins)` OR
       repo is in `dp_user_pins(kind='repo')` OR
       repo is linked to a `dp_tags` row the caller owns/follows)
      AND inbox status filter. The §5.4 SQL sketch is the target
      shape.
- [ ] **Start / due dates on issues (NEW — see §3.10).** Add
      `dp_issue_dates` (per-issue, `start_date DATE`, `due_date
      DATE`, both optional + check `start <= due`) and the
      optional `dp_repo_project_link` table (Projects v2 board
      node id + the two Date field node ids the operator picks
      as Start / Due). Implement `PATCH /issues/{id}/dates`
      (local-only write; fail-soft mirror to Projects v2 via
      GraphQL `addProjectV2ItemById` +
      `updateProjectV2ItemFieldValue` when the repo is linked).
      Extend `IssueDto` with optional `start_date` / `due_date`.
      Add `Due this week` + `Overdue` smart views and the list
      `Due` column. **Defer the pull-back path** (Projects v2 →
      `dp_issue_dates`) to slice 3 — slice 2 is push-only.
      *Native-GitHub story*: plain issues have **no** date fields;
      Milestones share one `due_on` across many issues; only
      Projects v2 has per-issue dates and only via GraphQL. The
      hybrid above keeps dates optional everywhere and lets
      operators opt into mirror per repo.
- [ ] Timeline endpoint: `GET /issues/{id}/timeline` backed by
      `dp_activity_events` + `dp_event_actors` so the peek panel
      can stop pretending it has comments.
- [ ] Sync visibility endpoints: per-repo last-synced-at +
      reconciler-run summaries so the list can render the
      "synced 4m ago" affordances from the §14.3 mock.
- [ ] Tag-link surfaces in the issue list (the §6 saved-views
      story leans on `dp_tag_links` joins; not yet exposed in
      `IssueDto`).
- [ ] OpenAPI: `me_queue` / `inbox` handlers are still absent
      from `DevPulseApi`. Matches the existing `list_issues`
      omission but should be closed before slice 3.

**Frontend (slice 2 finish + slice 3):**

- [ ] **Identity manager page** (`#/account/identities`, slice 2)
      — list linked GitHub accounts with login + linked-at, "Link
      another GitHub account" button (kicks off the OAuth
      round-trip from §3.0.2), unlink confirm dialog. Surface the
      live identity set in the user menu so operators can see "you
      are alice-acme + alice + alice-oncall" without leaving the
      page.
- [ ] Command palette `⌘K` (§14.5) — jump-to repo / issue /
      saved view; currently only `?` exists.
- [ ] Snoozed view backing: `TriagePage::filterFor("snoozed")`
      currently returns an empty synthetic page; needs a real
      `GET /me/inbox?status=snoozed` (or a `status` filter on
      `/me/queue`) and a wake-up affordance.
- [ ] Group-by + sort dropdowns on the middle pane (the §14.3
      "phoenix/api · 12 open · synced 4m ago" grouped rows from
      the mock; right now the list is flat).
- [ ] Saved views = pins/tags rendered as first-class entries in
      the left rail with their own counts (§14.6). The Pinned
      repos section exists; tags + count badges do not.
- [ ] Resizable pane splitters (§14.1). Current shell uses a
      fixed `grid-cols-[14rem_minmax(28rem,1fr)_minmax(28rem,32rem)]`.
- [ ] Bulk actions on selection (`x` to toggle, then `e` / `h`
      on a multi-select). Single-row actions only today.
- [ ] Reports surface that finally pays back `dp_activity_events`
      (the §1 promise — deferred to slice 3 in the PR plan).
- [ ] Polish: dark-mode pass on the new TriagePage; the inline
      peek panel uses tokens but hasn't been eyeballed against
      the theme tokens used in `IssueEditCard`.

**Tech debt found during the slice:**

- The `dp-fetcher` test failures observed at the start of the
  session were pre-existing local mods to
  `crates/dp-fetcher/src/{client/mod.rs, reconciler/mod.rs,
  reconciler/synth.rs}` — unrelated to triage but flagged as
  "fixed" by the operator. Worth a follow-up to confirm CI is
  green on `main`.
- `PinDto` has no human-readable `label` field, so the left-rail
  Pinned repos entries currently show `target_id.slice(0,8)` as
  a placeholder. Either denormalize the repo slug onto the pin
  row or do a client-side join against the repo list.

---

## 1. TL;DR

The drill-down I built ([frontend/src/workflow/repos-page.tsx](frontend/src/workflow/repos-page.tsx) →
[frontend/src/workflow/issues-page.tsx](frontend/src/workflow/issues-page.tsx))
is a worse GitHub. It makes the operator click *through* the org
and the repo before they can do work. Linear's lesson: **never
make the user navigate to their work — start them on it.**

The replacement is a **single Triage page**, landing route
`#/workflow`, three resizable panes:

```
┌──────────────┬──────────────────────────────────────────────┬────────────────┐
│ Views        │ filter pills  • group ▾  • sort ▾   • ⌘K     │  Peek panel    │
│              │──────────────────────────────────────────────│                │
│ ★ My queue 7 │ ▾ phoenix/api · 12 open · synced 4m ago      │  body          │
│   Assigned   │   #482  Flaky retry on 503  ● open  @alice 2h│  + timeline    │
│   Untriaged  │ • #481  Webhook backfill    ● open  @bob   4h│  + comment box │
│   Mentioned  │ ▾ phoenix/web · 5 open · synced 11m ago      │                │
│   Created    │   #99   401 on logout       ● open  —      6h│                │
│ Pins         │                                              │                │
│   nube/api   │ 1–25 of 187   [Prev] [Next]                  │                │
│ Tags         │                                              │                │
│   on-call    │                                              │                │
│ Orgs (3)     │                                              │                │
└──────────────┴──────────────────────────────────────────────┴────────────────┘
```

(Bold dot = unread since the user's `last_seen_version`; the
`7` next to `My queue` is the inbox badge.)

Everything dev-pulse already has (cross-org reach, the §8.2 CAS
write path, pins, tags, `dp_activity_events`) finally has a
single surface that uses it — plus three small additions that
turn it from "a list page" into Linear: a per-user **inbox**
(unread / snoozed / done), per-repo **sync visibility**, and a
**Reports** surface that finally pays back `dp_activity_events`.

---

## 2. Reference material

This proposal **realizes** the workbench already specified, it
does not redesign it. The spec exists; the implementation has
been wrong.

| Doc / code | What it locks down |
|---|---|
| [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md) §14.1 "Three-pane workbench (the shell)" | Navigator / list / detail layout, single `#/workflow` route |
| [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md) §14.2 Navigator pane | Smart views (`My queue`, `Assigned`, `Mentioned`, `Created`) + pins + tags + orgs |
| [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md) §14.3 Issue list pane | Grouping, sort, pagination, density |
| [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md) §14.4 Issue detail pane | Peek panel hosting the existing `IssueEditCard` |
| [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md) §14.5 Command palette `⌘K` | Jump-to / global actions |
| [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md) §14.6 Saved views | **Pins / tags ARE the saved views** |
| [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md) §14.7 Keyboard model | `j/k` `Enter` `Esc` `c` `e` `a` `l` `s` `g i` |
| [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md) §14.9 Minimum first slice | What lands in one PR |
| [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md) §8.2 / §8.3 | CAS write path + stale-version reload UX (already implemented) |
| [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md) §13.5 | Pin cap, sidebar render cap, tag scope cap |
| [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md) §13.6 | `issues: write` permission gate |
| [SCOPE.md](SCOPE.md) §6 "Activity signals tracked" | What goes on the timeline |
| [crates/dp-store-pg/migrations/dp/0001_init.sql](crates/dp-store-pg/migrations/dp/0001_init.sql) | `dp_issues`, `dp_activity_events`, `dp_event_actors` schema |
| [crates/dp-store-pg/migrations/dp/0005_user_pins_tags_tag_links.sql](crates/dp-store-pg/migrations/dp/0005_user_pins_tags_tag_links.sql) | `dp_user_pins`, `dp_tags`, `dp_tag_links` schema |
| [crates/dp-rest/src/issues_read.rs](crates/dp-rest/src/issues_read.rs) | `GET /issues` (just landed, paginated) |
| [crates/dp-rest/src/repos.rs](crates/dp-rest/src/repos.rs) | `GET /repos` (just landed, paginated) |
| [crates/dp-rest/src/issues.rs](crates/dp-rest/src/issues.rs) | §8 write-path helpers (acquire/commit/rollback) — **handlers not yet wired** |

---

## 3. In scope

Everything below is what the "Triage" surface owns. Anything not
listed stays out for the first three slices.

### 3.0 User ↔ GitHub identity model (NEW)

dev-pulse is a **multi-user operator console** (50+ devs across
4 orgs in the reference deployment). The §3.2 smart views and
the per-user inbox in §3.8 only make sense if "me" is well-defined
— and "me" is **not** a single GitHub login.

A real operator commonly has:

- a **work** GitHub account (`alice-acme`) — member of `acme-co`,
  `acme-platform`.
- a **personal** GitHub account (`alice`) — member of `acme-oss`
  and assigned to drive-by issues there.
- a **bot / on-call** account (`alice-oncall`) used during
  rotations.

All three map to **one** dp-pulse user. Their My queue / Assigned
/ Mentioned / Created views must union across the set, otherwise
the inbox is a lie.

#### 3.0.1 Schema (new tables, slice 2)

```sql
-- One dp-user can claim many GitHub identities.
CREATE TABLE dp_user_identities (
    user_id        UUID    NOT NULL REFERENCES dp_users(id) ON DELETE CASCADE,
    github_user_id BIGINT  NOT NULL,           -- stable across renames
    github_login   TEXT    NOT NULL,           -- denormalized for joins
    is_primary     BOOLEAN NOT NULL DEFAULT FALSE,
    linked_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    verified_via   TEXT    NOT NULL
                     CHECK (verified_via IN ('oauth','admin_link','rotation')),
    PRIMARY KEY (user_id, github_user_id),
    UNIQUE (github_user_id)                    -- one GH account → one dp-user
);
CREATE INDEX dp_user_identities_login_idx ON dp_user_identities (github_login);
-- Exactly one primary identity per dp-user.
CREATE UNIQUE INDEX dp_user_identities_primary_idx
  ON dp_user_identities (user_id) WHERE is_primary;

-- §3.0.1.a Per-identity provenance on memberships so unlink can
-- subtract *only* orgs no remaining identity still covers.
CREATE TABLE dp_membership_identities (
    user_id        UUID   NOT NULL,
    org_id         UUID   NOT NULL REFERENCES dp_orgs(id) ON DELETE CASCADE,
    github_user_id BIGINT NOT NULL,
    observed_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, org_id, github_user_id),
    FOREIGN KEY (user_id, github_user_id)
      REFERENCES dp_user_identities (user_id, github_user_id) ON DELETE CASCADE
);
CREATE INDEX dp_membership_identities_org_idx
  ON dp_membership_identities (org_id, user_id);
```

`dp_orgs` ↔ `dp_users` already exists via `dp_memberships`
(`user_id, org_id, role, home_org, joined_at`). One dp-user can
already belong to many orgs — that part is **not** new. What's
new is the **multi-identity** layer above it, *plus* the
`dp_membership_identities` provenance join so we can answer
"does alice still reach `acme-co` after unlinking
`alice-acme`?" without re-querying GitHub.

**Single source of truth.** `dp_users.github_id` (the UNIQUE
single-identity column on `0001_init.sql`) is **deprecated** by
this change. Migration 0013 backfills `dp_user_identities` from
it (one row per existing user, `is_primary = TRUE`,
`verified_via = 'oauth'`) and migration 0014 (one release
later, after every read site has migrated) drops the column.
In the interim every read goes through
`dp_user_identities WHERE is_primary` — no dual reads. The
primary identity is mutable via
`PATCH /me/identities/{github_user_id}/primary`.

#### 3.0.2 Linking flow

- **First login** → OAuth callback creates `dp_users` row +
  primary `dp_user_identities` row (`is_primary = TRUE`). No
  change from today's UX.
- **Add identity** → operator clicks "Link another GitHub
  account" in `#/account/identities`. Server inserts a row in
  `dp_identity_link_pending(nonce UUID, session_id, dp_user_id,
  expires_at)` and redirects to GitHub OAuth with `state =
  nonce` (opaque; **never** the session id directly — see
  §3.0.2.a). Callback consumes the nonce, verifies the session
  still binds to the same dp-user, and inserts the new
  `dp_user_identities` row. Reject if `github_user_id` already
  claimed by a different dp-user (return 409 + audit
  `IDENTITY_CLAIM_CONFLICT`).
- **Admin link** (break-glass) → `dev-pulse link-identity
  --user <uuid> --github-login <login>` for the CLI-seeded admin
  to fix mis-attributions. Writes `verified_via = 'admin_link'`
  *and* emits a high-visibility audit row that surfaces in the
  target user's own audit log (so alice sees "your account was
  linked to `alice-oncall` by admin bob at T"). The user can
  one-click unlink an admin-linked identity without re-OAuth.
- **Unlink** → `DELETE /me/identities/{github_user_id}`; refuse
  if `is_primary` (operator must `PATCH .../primary` to another
  identity first) or if it would leave the user with zero
  identities. Drives §3.0.2.b membership subtraction.
- **Transfer** (admin-only) →
  `POST /admin/identities/{github_user_id}/transfer { to_user }`.
  Moves the identity to another dp-user (covers "alice quit, bob
  inherits `alice-oncall`" without unlink-then-link racing).

##### 3.0.2.a OAuth `state` is a server-side nonce

The link round-trip never puts the session id on the wire. The
`state` parameter is an opaque UUID looked up server-side
against `dp_identity_link_pending`; the row is consumed on
callback and bound to the session via the cookie the browser
still presents. This is the standard CSRF-safe OAuth pattern.

##### 3.0.2.b Membership reconciliation (the load-bearing part)

`dp_memberships` rows are **derived** from
`dp_membership_identities` rows. The invariant:

> A `dp_memberships(user_id, org_id)` row exists iff at least
> one `dp_membership_identities` row exists for the same
> `(user_id, org_id)`.

Three transitions touch it:

1. **Link / re-stamp identity `i`** — for each org `i` can
   reach, INSERT `dp_membership_identities(user_id, org_id, i)`
   (idempotent on PK). Then UPSERT `dp_memberships(user_id,
   org_id)`.
2. **Unlink identity `i`** — DELETE
   `dp_membership_identities WHERE github_user_id = i` (ON
   DELETE CASCADE from `dp_user_identities` already does this).
   For each affected `(user_id, org_id)`, DELETE
   `dp_memberships` *only if* no provenance rows remain.
3. **Token expiry for `i`** — treated like a stamp returning
   the empty org list for that identity: drop `i`'s provenance
   rows, then collapse `dp_memberships` for any
   now-unprovenanced `(user_id, org_id)`.

With this, deleting an identity *cannot* silently revoke a
user's access to orgs her other identities still cover, and the
stamper's worst-case behaviour is "membership absent until next
stamp" rather than "membership ambient and wrong."

##### 3.0.2.c Re-stamping on identity change

Linking, unlinking, transferring, or `PATCH .../primary` all
invalidate the in-flight `Principal`. The link/unlink/transfer
handlers append the affected `dp_user_id` to a `principal_dirty`
table; the existing principal cache key includes the row's
`updated_at` so the next request re-stamps before serving. No
forced re-login.

If `caller.github_logins = []` (a race during first OAuth or
all identities revoked mid-session), the principal stamper
**refuses to mint a principal** and the request returns 401
`identity_set_empty`. The frontend handles this with a re-login
prompt.

#### 3.0.3 Principal stamping

`Principal.extra.github` is extended at session-mint by the
existing `GithubOrgsStamper` (see
[crates/dp-server/src/auth/github_orgs.rs](crates/dp-server/src/auth/github_orgs.rs)):

```jsonc
{
  "github": {
    "logins":   ["alice-acme", "alice", "alice-oncall"], // identities
    "user_ids": [12345678, 8765, 99887766],
    "orgs":     ["acme-co", "acme-platform", "acme-oss"],
    "in_allowed_org": true
  }
}
```

The org-gate policy rule still keys on `in_allowed_org`; nothing
in the policy needs to know about the identity set.

#### 3.0.4 Implications for the §3.2 smart views

Every "me" predicate becomes a **set** match across identities:

| View | Old (single login) | New (identity set) |
|---|---|---|
| `Assigned to me` | `assignees @> [caller.login]` | `assignees ?\| caller.github_logins` |
| `Mentioned` | mention table `mentioned_login = caller.login` | `mentioned_login = ANY(caller.github_logins)` |
| `Created by me` | `author = caller.login` | `author = ANY(caller.github_logins)` |
| `My queue` | join on `(user_id, issue_id)` | unchanged — keyed on the **dp-user** uuid, not a GH login |

The inbox table (`dp_user_issue_state`) is already keyed by
`user_id UUID` so the union just falls out — `My queue` never
needed a login. The other three views are the ones that get a
plural rewrite.

#### 3.0.5 Endpoint surface (slice 2)

- `GET    /me/identities` — list linked GH identities.
- `POST   /me/identities/link/start` → OAuth round-trip start
  with `link_to_session = <session_id>`.
- `POST   /me/identities/link/callback` → finishes link.
- `DELETE /me/identities/{github_user_id}` — unlink (refuse
  last-identity removal).
- `POST   /admin/users/{user_id}/identities` (admin link),
  gated on `admin.write`.

All gated on a new `identities` resource (`read` / `write`),
registered in
[crates/dp-server/src/auth/policy.rs](crates/dp-server/src/auth/policy.rs#L61)
the same way `issues` / `tags` just were.

### 3.1 Single landing page

- Route: `#/workflow` (the legacy `#/workflow/repos` and
  `#/workflow/issues` redirect here).
- Three resizable panes: Views (left), List (center), Peek (right).
- Peek pane can be collapsed; list expands to full width.
- No per-page sub-navigation, no tabs across the top.

### 3.2 Views rail (left)

Each entry is one click → repopulates the list. The active view
is reflected in the URL so links are shareable.

- **Smart views** (server-resolved; "me" = the union of every
  GitHub identity linked to the dp-user, per §3.0):
  - `★ My queue` — issues in the caller's **inbox** (see §3.8):
    open + version newer than `last_seen_version` + not snoozed
    + not marked done. Default landing. Carries an unread badge.
    Keyed on `dp_user_issue_state.user_id` — identity-set
    agnostic.
  - `Assigned to me` — open issues where `assignees` overlaps
    `caller.github_logins` (any linked identity).
  - `Untriaged` — open + no assignee + no labels + age ≥ 24h,
    scoped to `caller.org_ids` from `dp_memberships` (the
    authoritative source — survives token churn, unlike the
    GitHub stamper's `orgs` list). The actual "triage" view that
    names the page. The 24h floor drops just-opened issues whose
    author is still typing.
  - `Mentioned` — issues whose body or whose comments mention
    **any** of the caller's linked logins. Backed by the §6
    projection table; slice 3.
  - `Created by me` — issues whose `author` is any of
    `caller.github_logins`.
- **Pins** (per-user, ordered, capped at 20 per §13.5):
  - Repo pin → issues in that repo.
  - Tag pin → issues whose repo is linked to that tag.
- **Tags** (org-scoped or team-scoped per §7.4):
  - Click a tag → issues whose repo is linked.
- **Teams** (NEW — slice 2). `dp_memberships` already carries
  team data. With 50 devs in 4 orgs the team is the actionable
  unit, not the org:
  - `My team's untriaged`
  - `Assigned to anyone on my team`
  - `Team @platform's WIP` (manager peek)

  Rendered between Tags and Orgs. Each team entry shows open
  count + stale count badges.
- **People** (NEW — slice 2). Flat list of users in the
  caller's orgs, each row showing `open / stale / last activity`.
  Click → list filtered to that assignee. Sortable by stale
  count. This is the "who's drowning, who has bandwidth" view
  that lets a lead actually rebalance load without leaving the
  page; without it, that question lives only in
  `/reports/issues?metric=wip` and never gets asked.
- **Orgs** — flat list of orgs the caller is a member of; click
  → org filter.

Sidebar render cap of 50 (§13.5) applies after expanding pins +
tags + teams + people into rows; People is collapsed by default
with a "show all" reveal to keep the rail under the cap at
50-dev scale.

### 3.3 List pane (center)

- **Filter pills** above the list: `org`, `repo`, `state`,
  `state_reason`, `assignee`, `label`, `milestone`,
  `updated_since`. Pills are composable (every pill is an
  AND across fields). Repeatable pills (`label`, `assignee`,
  `repo`, `org`) are AND within the field too — matches Linear.
  Each pill is removable with `x`.
- **Group by** dropdown: `none | repo | assignee | label |
  milestone | state`. Sticky group headers; each header carries
  the count for the visible group.
- **Sort** dropdown: `updated_at desc` (default) | `created_at
  desc` | `number desc` | `assignee` | `repo`.
- **Server-paginated** (default page size 50, max 200), with
  `Showing X–Y of Z` counter.
- **Density**: single-line row — `#number`, title, state pill,
  assignees, age. `repo_slug` shown when group is not `repo`.
- **Keyboard**: `j`/`k` move selection, `Enter` opens peek,
  `x` toggles row in bulk-selection, `Shift+click` range-select.
- **Virtualised** — needs `@tanstack/react-virtual` for 1000+
  row pages.
- **Empty state** distinguishes "no matches for these filters"
  from "no data yet for this org" (the latter links to the
  Admin → Refresh page).

### 3.4 Peek pane (right)

- Mounts the existing **`IssueEditCard`** verbatim — the §8.2
  CAS write path and the §8.3 stale-version reload UX survive
  with zero changes ([frontend/src/workflow/issues-page.tsx](frontend/src/workflow/issues-page.tsx) ).
- Adds three new sections under the form:
  1. **Activity timeline** — chronological view of every
     `dp_activity_events` row tagged with this issue
     (`IssueOpened`, `IssueComment`, `IssueClosed`, plus PR
     events that reference the issue number). One row per
     event: actor avatar + verb + relative-time.
  2. **Linked tags** — chips of `dp_tags` whose `dp_tag_links`
     point at this `issue_id` or its `repo_id`. Click to
     filter the list to that tag.
  3. **Open in new tab** — deep-link back to
     `#/workflow?issue=<uuid>&view=<active_view>`.

### 3.5 Command palette (`⌘K`)

Slice-2 scope per §14.5. Three action classes:

- **Jump-to** — `#482 webhook` → matches issue number, title,
  repo slug, org login, tag name, user login.
- **Switch view** — `view: my queue`, `view: phoenix tag`.
- **Apply action to selection** — `assign @alice`, `label
  oncall`, `close`. Each action is one §8.2 CAS round per
  selected row, with a progress toast.

### 3.6 Inline edits

All inline edits are **optimistic**: the row updates locally
on click, the §8.2 CAS round-trip runs in the background, and a
409 reverts the cell with a toast (`"Out of date — refresh to
see the latest version"`). This is the single biggest piece of
"Linear feel" — without it every click waits for the network.
The §8.3 stale-version reload UX is the rollback path.

- **Slice 1**: state cell only (`open ↔ closed`). Clicking the
  pill opens a 2-item menu; the mutation goes through
  `PATCH /issues/{id}` with `expected_version` from the row.
- **Slice 2**: assignee cell + label cell — same pattern,
  popover pickers.

### 3.7 URL state

Every selection lives in the hash route so links are shareable:

```
#/workflow?view=my_queue&org=phoenix&state=open&group=repo&issue=<uuid>
```

The `Router` in [frontend/src/app.tsx](frontend/src/app.tsx) already
mounts the workflow section; the existing
`workflowSelectedIssue()` / `workflowSelectedRepoId()` helpers in
[frontend/src/routes.ts](frontend/src/routes.ts) grow `workflowViewId()`,
`workflowGroupBy()`, `workflowFilters()`.

### 3.8 Inbox (per-user unread / snooze / done)

The missing Linear feature that the original draft skipped.
Without it, `My queue` is just "Assigned to me" plus noise — no
badge count, no `e` to mark done, no "new since I last looked".

State lives in a new `dp_user_issue_state` table (§6) keyed on
`(user_id, issue_id)`:

- `last_seen_version BIGINT` — compared against
  `dp_issues.external_version` (see *Own-write hazard* below).
  Row is **unread** when `external_version > last_seen_version`.
- `status TEXT` — `inbox` (default) | `snoozed` | `done`.
- `snoozed_until TIMESTAMPTZ NULL` — wakes back into `inbox` when past.

**Own-write hazard.** `dp_issues.version` bumps on *every* CAS,
including the caller's own writes. If unread compared against
`version` directly, alice would mark her own edit as unread the
moment she walked away from the row. Fix: add
`dp_issues.external_version BIGINT NOT NULL DEFAULT 0` (§6)
that the **reconciler** bumps on remote-applied changes but the
§8.2 commit path does **not**. Unread compares against
`external_version`; the §8.2 CAS still uses `version` for
concurrency control. Two counters, two different jobs.

UX surfaces:

- Bold dot on unread rows; sidebar badge is the inbox count.
- `e` marks the selected row(s) **done** (removes from inbox).
- `h` snoozes selected row(s) (popover: "until tomorrow / next
  week / custom"). Snoozed rows are hidden from `My queue`.
- Opening the peek auto-bumps `last_seen_version` to the
  `external_version` the **client actually saw** (not
  server-current). Server upserts with `last_seen_version =
  LEAST(version_seen, current_external_version)` so it never
  advances past what the user observed — any write landing
  between row-open and the seen-request stays unread.
- **Bulk inbox actions** (slice 2): `mark-all-visible read`,
  `snooze-all-visible 1d`, `done-all-visible`. Required for the
  PTO-returner who comes back to inbox-2000; single-row UX is
  unusable at that scale. Backed by `POST /me/inbox/seen`
  (already bulk) and a new bulk variant of
  `PATCH /me/inbox/{issue_id}` →
  `POST /me/inbox/bulk { issue_ids: [Uuid], status, snoozed_until }`.

New endpoints (slice 1):

- `POST /me/inbox/seen` `{ entries: [{ issue_id, version }] }`
  → bulk-mark read using the version the client actually saw.
- `PATCH /me/inbox/{issue_id}` `{ status?, snoozed_until? }`.
- `POST /me/inbox/bulk` (slice 2) — bulk status / snooze.

Auth pair: `("issues", "read")` is enough; this is per-user UI
state, not an issue mutation.

### 3.9 Sync visibility (local DB ↔ GitHub)

dev-pulse is a local mirror; the #1 trust question is "is what
I'm looking at fresh?" The current refetch-on-focus answers it
implicitly. We surface it explicitly:

- **Per-repo badge** in the list group header and views rail:
  `synced 4m ago`, derived from the fetcher's existing
  `last_synced_at` column on `dp_repos`. Goes amber > 30 min,
  red > 6 h.
- **"Sync now" action** in the views-rail header — enqueues
  the fetcher for the active filter's repo scope. No
  long-poll; the user refreshes the page when ready (or the
  badge ticks down).
- **Per-issue "out of date"** banner in the peek panel if the
  reconciler has noted a remote `updated_at` newer than the
  local row's `updated_at` since it was opened.

New endpoints (slice 2):

- `GET /repos/{id}/sync-status` → `{ last_synced_at,
  last_attempt_at, last_error, queued }`.
- `POST /repos/{id}/sync` → enqueues; idempotent if already queued.

### 3.10 Dates on an issue (start / due) — NEW

#### What GitHub gives us natively

| Surface | Per-issue? | Read/write? | Notes |
|---|---|---|---|
| **Issue body / fields** | — | — | Plain issues have **no native start/due date fields**. The only dates on an issue itself are `created_at`, `updated_at`, `closed_at`. |
| **Milestones** | shared | r/w via REST | `due_on TIMESTAMPTZ`, but per-milestone — every issue in the milestone shares the same date. Useful for sprint-level "ship by", useless for per-issue planning. |
| **Projects (classic, v1)** | yes | r/w | Deprecated since Aug 2024 — do not build on it. |
| **Projects v2** | yes | r/w via **GraphQL only** | Per-project **custom fields** including `Date` type. The Linear-equivalent of "due date on an issue" is *"a Projects v2 board has a `Due date` Date field, and the issue is added as an item to that project"*. This is what GitHub's own UI shows when you see a date column on an issue list — it is always a project field, never an issue field. |

So: there is **no `issue.due_on` to round-trip**. To use
"native GitHub" we have to (a) require a Projects v2 board per
repo or per org and (b) talk GraphQL to read/write the date
fields on each project item.

#### Design — hybrid, dates always optional

dev-pulse treats start/due as **first-class but optional**
metadata that lives in **local state** by default, with an
**opt-in mirror** to Projects v2 when a board is configured.
Neither side blocks the other; the operator picks per repo
whether to mirror, and the inbox still works with zero dates
anywhere.

##### Storage

```sql
-- Per-issue dates; either column may be NULL.
-- One row per issue (NOT per user) — these are the issue's
-- planning dates, not the caller's personal reminders.
CREATE TABLE dp_issue_dates (
    issue_id      UUID         PRIMARY KEY REFERENCES dp_issues(id) ON DELETE CASCADE,
    start_date    DATE         NULL,
    due_date      DATE         NULL,
    set_by        UUID         NULL REFERENCES dp_users(id),
    set_at        TIMESTAMPTZ  NOT NULL DEFAULT now(),
    -- Mirror bookkeeping (NULL when not mirrored)
    gh_project_id      TEXT    NULL,   -- node id of the Projects v2 board
    gh_item_id         TEXT    NULL,   -- node id of the issue's project-item
    gh_start_field_id  TEXT    NULL,   -- node id of the Date field used as Start
    gh_due_field_id    TEXT    NULL,   -- node id of the Date field used as Due
    last_mirrored_at   TIMESTAMPTZ NULL,
    mirror_error       TEXT    NULL,
    CHECK (start_date IS NULL OR due_date IS NULL OR start_date <= due_date)
);
CREATE INDEX dp_issue_dates_due_idx   ON dp_issue_dates (due_date)
    WHERE due_date IS NOT NULL;
CREATE INDEX dp_issue_dates_start_idx ON dp_issue_dates (start_date)
    WHERE start_date IS NOT NULL;
```

DATE (not TIMESTAMPTZ) because "due Tuesday" is a calendar
concept, not an instant — matches how Projects v2 stores its
`Date` field (no time, no tz).

##### Per-repo mirror config (optional)

```sql
CREATE TABLE dp_repo_project_link (
    repo_id            UUID  PRIMARY KEY REFERENCES dp_repos(id) ON DELETE CASCADE,
    gh_project_id      TEXT  NOT NULL,   -- Projects v2 board node id
    gh_start_field_id  TEXT  NULL,
    gh_due_field_id    TEXT  NULL,
    auto_add_items     BOOL  NOT NULL DEFAULT true,  -- add issue as project item on first date set
    configured_by      UUID  NOT NULL REFERENCES dp_users(id),
    configured_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

If a row exists in `dp_repo_project_link` for the issue's repo,
writes to `dp_issue_dates` enqueue a background mirror task
that:

1. Resolves / adds the issue as a Projects v2 item
   (`addProjectV2ItemById` mutation).
2. Updates the configured Date fields
   (`updateProjectV2ItemFieldValue` mutation, one per field).
3. Writes back `gh_item_id` + `last_mirrored_at` on success, or
   `mirror_error` on failure (the local date stays — mirror is
   best-effort, never blocking).

If no row exists, dates are **local-only**. That's the default,
and it's a complete experience — the operator just doesn't get
the dates surfaced on github.com.

##### Pull direction (Projects v2 → dp-pulse)

The fetcher (slice 3, not slice 2) periodically reads project
items for repos with a `dp_repo_project_link` and syncs the
configured Date fields **into** `dp_issue_dates`. Last-writer
wins, keyed on `set_at` vs `last_mirrored_at`. This lets the
operator edit dates on github.com (e.g. on mobile) and have
them flow back.

##### Write semantics

`PATCH /issues/{id}/dates` `{ start_date?, due_date? }` —
either field may be `null` to clear. Auth `("issues", "write")`.
Synchronous local upsert; enqueues mirror if linked. Returns
`{ start_date, due_date, mirror: "local" | "queued" | "synced" | "error" }`.

##### UI surfaces

- **Peek panel**: two compact date pickers ("Start", "Due") under
  the title row. Empty pill reads "Add date". Past-due rows get
  a red badge on the row in the list pane.
- **List pane**: a `Due` column (hidden by default, toggle via
  `g d`); rows sortable by due_date asc with NULLs last.
- **Smart views**: add `Due this week` and `Overdue` to the §3.2
  rail (server-resolved on the new index).
- **Inbox bump**: an issue assigned/created/mentioned to the
  caller that goes past-due re-appears in `My queue` with
  `status='inbox'` cleared from `done` — the snooze never
  outlives the due date. (Done in the §3.8 inbox-state-set
  trigger; see the `dp_user_issue_state` upsert path.)

##### Auth & scope notes

- Projects v2 mirror requires the GitHub App or PAT to hold
  `projects: write` on the org. Surface the `app-install-banner`
  treatment from §13.6 if missing — the local date still saves,
  the mirror just fails-soft.
- `dp_repo_project_link` is an admin-write surface (gated on the
  same `admin.write` as the rest of §7); operators set it once
  per repo and forget it.

---

## 4. Out of scope (deferred to a later slice)

| Area | Why deferred |
|---|---|
| Drag-to-reorder pins in the rail | `PUT /me/pins/order` exists; ship the UI in slice 2 |
| Saved-views CRUD beyond pins/tags | §14.6 leaves "saved view as a third object" open; v1 reuses pins/tags |
| Kanban / Gantt / timeline boards | Explicit §13.9 non-goal |
| Cross-issue dependencies | Explicit §4 non-goal |
| New write verbs (milestone create, label create) | §4 non-goal — labels/milestones are read-only enumerations from GitHub |
| Real-time push (SSE/WebSocket) | §10 says scheduled ingestion only; refetch-on-focus is enough for v1 |
| Mobile layout | §13.8 calls workbench desktop-only |

---

## 5. Endpoint inventory

`✅` = exists today, no changes needed.
`🟡` = exists but needs a small extension.
`🆕` = needs to be added.

### 5.1 Reads driving the list

| Endpoint | Status | Notes |
|---|---|---|
| `GET /issues` | ✅ | Paginated, filterable on `repo_id`/`org_id`/`state`/`assignee`/`q`/`limit`/`offset`. See [crates/dp-rest/src/issues_read.rs](crates/dp-rest/src/issues_read.rs#L161) |
| `GET /issues/{id}` | ✅ | Single issue by id, used by Peek. See [crates/dp-rest/src/issues_read.rs](crates/dp-rest/src/issues_read.rs#L240) |
| `GET /repos/{repo_id}/issues/{number}` | ✅ | Deep-link form. See [crates/dp-rest/src/issues_read.rs](crates/dp-rest/src/issues_read.rs#L269) |
| `GET /repos` | ✅ | Paginated; populates the repo filter pill autocomplete. See [crates/dp-rest/src/repos.rs](crates/dp-rest/src/repos.rs#L127) |
| `GET /orgs` | ✅ | Populates the org filter pill + Orgs rail. See [crates/dp-rest/src/directory.rs](crates/dp-rest/src/directory.rs#L301) |
| `GET /users` | ✅ | Populates the assignee picker. See [crates/dp-rest/src/directory.rs](crates/dp-rest/src/directory.rs#L296) |
| `GET /me/queue` | 🆕 | Server-resolved "my queue" smart view (inbox-aware). See §5.4. |
| `GET /issues` extended filters | 🟡 | Add `label`, `milestone`, `author`, `state_reason`, `updated_since`, `mentions`, `repo_ids`, `org_ids`. See §5.5. |
| `GET /issues/{id}/timeline` | 🆕 | Projects `dp_activity_events` for the issue. See §5.6. |
| `POST /me/inbox/seen` | 🆕 | Bulk-mark issues read. See §3.8 / §5.8. |
| `PATCH /me/inbox/{issue_id}` | 🆕 | Set inbox status / snooze. See §3.8 / §5.8. |
| `GET /repos/{id}/sync-status` | 🆕 | Sync freshness for the badge. See §3.9 / §5.9. |
| `POST /repos/{id}/sync` | 🆕 | Enqueue a fetcher run. See §3.9 / §5.9. |
| `GET /reports/issues` | 🆕 | Aggregated metrics from `dp_issues` + `dp_activity_events`. See §5.10. |

### 5.2 Reads driving the rail

| Endpoint | Status | Notes |
|---|---|---|
| `GET /me/pins` | ✅ | Per-caller pin list. See [crates/dp-rest/src/pins.rs](crates/dp-rest/src/pins.rs#L415) |
| `GET /me/tags` | ✅ | Tags visible to caller (own + scope-visible). See [crates/dp-rest/src/tags.rs](crates/dp-rest/src/tags.rs#L1185) |
| `GET /tags` | ✅ | Full tag list (admin / palette). See [crates/dp-rest/src/tags.rs](crates/dp-rest/src/tags.rs#L1183) |
| `GET /tags/{id}` | ✅ | Tag detail with linked repos/issues. See [crates/dp-rest/src/tags.rs](crates/dp-rest/src/tags.rs#L1184) |
| `GET /me/app-install-banner` | ✅ | §13.6 write-gate banner. See [crates/dp-rest/src/app_permissions.rs](crates/dp-rest/src/app_permissions.rs#L404) |

### 5.3 Writes driving the peek + bulk actions

| Endpoint | Status | Notes |
|---|---|---|
| `POST /issues` | 🟡 | Helper exists (`commit_issue_mutation`) but **handler not mounted** — see [crates/dp-rest/src/issues.rs](crates/dp-rest/src/issues.rs) (4 `pub async fn` helpers, no `Router`). Need an `issues_write_router` that wires create/patch/comment through the acquire/commit/rollback dance. |
| `PATCH /issues/{id}` | 🟡 | Same — helper exists, handler not mounted. CAS on `expected_version` per §8.2. |
| `POST /issues/{id}/comments` | 🟡 | Same — helper exists, handler not mounted. |
| `POST /me/pins` | ✅ | Add pin (slice 2 rail editing). |
| `DELETE /me/pins/{kind}/{target_id}` | ✅ | Remove pin. |
| `PUT /me/pins/order` | ✅ | Atomic reorder. |
| `POST /tags/{id}/links` | ✅ | Link a repo / issue / user / team to a tag. |
| `DELETE /tags/{id}/links` | ✅ | Unlink. |

### 5.4 🆕 `GET /me/queue`

Composite default landing view. Server resolves the union of
"things the caller probably wants to see next" in one round-trip
so the list pane has data on first paint without N hooks.

```
GET /me/queue?limit=50&offset=0&state=open
→ 200 IssueListResponse  // same envelope as GET /issues, with
                         //   `unread: bool` added per row
```

Server semantics — UNION over four arms, deduped on `id`,
ordered by `(updated_at DESC, id DESC)`. "Me" expands to the
full identity set per §3.0.4.

1. Open issues where `assignees ?| caller.github_logins`
   (any linked login is assigned)
2. Open issues whose `repo_id` is in the caller's pinned repos
3. Open issues whose `repo_id` is in `dp_tag_links` for any tag
   the caller has **pinned** (named explicitly to settle §10 Q1)
4. Issues in the caller's **inbox**: rows where
   `dp_user_issue_state.status = 'inbox'` AND
   `dp_issues.external_version > coalesce(last_seen_version, 0)`
   AND `(snoozed_until IS NULL OR snoozed_until < now())`.

**Pagination & cost.** Each arm pushes `ORDER BY updated_at
DESC, id DESC LIMIT $cap` before the UNION so a user with 100
pinned repos doesn't scan 5k arm-2 rows just to slice 50. The
outer query then re-orders + dedupes + LIMITs. Pagination is
**keyset on `(updated_at, id)`**, not OFFSET — inbox UX never
needs deep pagination and OFFSET at depth 950 is wasteful.

LEFT JOIN `dp_user_issue_state` on `(caller.id, issue.id)` to
project `unread = (external_version > coalesce(last_seen_version, 0))`.

Always AND with `org_id = ANY(caller.org_ids)` (from
`dp_memberships`, not the stamper's transient `orgs` list) — the
policy layer enforces per-row authz, but the SQL stays tight too.

Auth pair: `("issues", "read")`. Needs a new covering index
`dp_issues (updated_at DESC, id)` for the outer sort (the
existing `_org_state_idx` and `_repo_updated_idx` cover the
per-arm filters but not the cross-repo merge); added in §6.

Add to [crates/dp-rest/src/issues_read.rs](crates/dp-rest/src/issues_read.rs)
as `me_queue` handler; mount alongside the existing
`issues_read_router` registrations.

### 5.5 🟡 `GET /issues` filter extensions

Today's `ListIssuesQuery` ([crates/dp-rest/src/issues_read.rs](crates/dp-rest/src/issues_read.rs#L110))
accepts `repo_id`, `org_id`, `state`, `assignee`, `q`, `limit`,
`offset`. The triage pills add:

| Field | Wire | Store predicate |
|---|---|---|
| `label` | repeatable, AND | `labels @> to_jsonb($labels::text[])` (single containment, one row in the WHERE) |
| `assignee` | repeatable, AND | `assignees @> to_jsonb($assignees::text[])` |
| `milestone` | string | `milestone = $milestone` |
| `author` | string | New column on `dp_issues` — **schema change**, see §6 |
| `state_reason` | string | `state_reason = $state_reason` (`completed`/`not_planned`/`reopened`) — needed by reporting too, see §5.10 |
| `updated_since` | RFC3339 | `updated_at >= $since` |
| `mentions` | string | `EXISTS (SELECT 1 FROM dp_issue_mentions m WHERE m.issue_id = i.id AND m.login = $login)` — slice 3, projection table per §6 |
| `repo_id` | repeatable | `repo_id = ANY($repo_ids)` |
| `org_id` | repeatable | `org_id = ANY($org_ids)` |

**Wire compatibility**: the current `repo_id` / `assignee` /
`org_id` params are scalar. The new shape accepts both — a
scalar value is treated as a one-element array on the server.
No breaking change to existing callers.

The `repo_id`/`org_id`/`assignee`/`label` changes need both the
wire shape and the store `IssueListFilter` extended to
`Vec<…>`. The existing `dp_issues_org_state_idx` covers the
common `(org_id, state)` access pattern; `(repo_id,
updated_at)` is covered. `label` and `assignees` containment
searches over JSONB need GIN indexes — added in §6.

### 5.6 🆕 `GET /issues/{id}/timeline`

Powers the peek-pane timeline.

```
GET /issues/{id}/timeline?limit=50&offset=0
→ 200 { rows: TimelineEntry[], total, limit, offset }

TimelineEntry := {
  id: Uuid,
  kind: EventKind,           // IssueOpened | IssueComment | IssueClosed | …
  ts: DateTime<Utc>,
  actors: [{ user_id, login, role }],
  payload_summary: string    // e.g. "added label 'oncall'"
}
```

Implementation:

- `dp_activity_events` has `repo_id` + `payload JSONB` but **no
  `issue_id` column**. The payload from the fetcher includes
  `payload.number` for issue-kind events
  ([crates/dp-fetcher/src/worker/handlers.rs](crates/dp-fetcher/src/worker/handlers.rs#L509)).
- The lookup uses a guarded expression: the §6 expression index
  is partial on `kind IN (…) AND payload ? 'number' AND
  payload->>'number' ~ '^[0-9]+$'`, so the cast in the WHERE
  clause can never raise on malformed rows.
- Filter: `repo_id = $repo_id AND kind = ANY($kinds) AND
  payload ? 'number' AND payload->>'number' ~ '^[0-9]+$' AND
  (payload->>'number')::int = $number`.
- This is the **only place** dev-pulse beats GitHub — nobody
  else has the cross-source merged timeline. Worth doing right.
- Auth: `("issues", "read")`.
- Add as `get_issue_timeline` in
  [crates/dp-rest/src/issues_read.rs](crates/dp-rest/src/issues_read.rs);
  store method `list_events_for_issue(repo_id, number, limit,
  offset)` in `dp_domain::store::Store`, PG impl in
  [crates/dp-store-pg/src/store.rs](crates/dp-store-pg/src/store.rs).

### 5.7 Already-shipped infra that just works

- `with_principal` + `require_permission` middleware
  ([crates/dp-server/src/lib.rs](crates/dp-server/src/lib.rs#L242)).
- `starter-authz` wildcard `oauth.in_allowed_org == true` policy
  in [crates/dp-server/policy/dev-pulse.toml](crates/dp-server/policy/dev-pulse.toml) — new
  read endpoints just need the `("issues", "read")` /
  `("repos", "read")` pair already declared.
- §8.2 acquire / commit / rollback helpers
  ([crates/dp-rest/src/issues.rs](crates/dp-rest/src/issues.rs#L78)) — wire-up only.
- Audit vocabulary ([crates/dp-rest/src/audit.rs](crates/dp-rest/src/audit.rs)) — issue
  writes already have verbs.

### 5.8 🆕 Inbox endpoints

Backing the §3.8 inbox UX. All per-caller; no new auth pair
(reuses `("issues", "read")` since this is per-user UI state).

```
POST /me/inbox/seen
body: { issue_ids: [Uuid] }
→ 204                    // upserts last_seen_version = current version

PATCH /me/inbox/{issue_id}
body: { status?: "inbox"|"snoozed"|"done", snoozed_until?: RFC3339 | null }
→ 200 UserIssueState
```

Store methods (`dp_domain::store::Store`):
`mark_issues_seen(user_id, [issue_id])`,
`set_inbox_state(user_id, issue_id, status, snoozed_until)`.

### 5.9 🆕 Sync-status endpoints

Backing §3.9. Reads `dp_repos.last_synced_at` /
`last_sync_error` (already written by the fetcher reconciler).

```
GET /repos/{id}/sync-status
→ 200 { last_synced_at, last_attempt_at, last_error: string | null, queued: bool }

POST /repos/{id}/sync
→ 202 { queued: true }     // idempotent: no-op if already queued
```

Auth pair: `("repos", "read")` for the GET; the POST needs
`("repos", "sync")` — **new auth pair**, gate via the existing
`require_permission` middleware. (The one new pair in the
proposal; the inbox endpoints reuse existing pairs.)

### 5.10 🆕 `GET /reports/issues`

The surface the original draft skipped. Backed entirely by
existing tables — no new ingestion, no new fetcher work.

```
GET /reports/issues
  ?metric=throughput|lead_time|wip|stale|untriaged
  &group_by=repo|org|assignee|week|day
  &since=RFC3339  &until=RFC3339
  &org_id=…  &repo_id=…  (repeatable, scope filters)
→ 200 { rows: [{ bucket, value, count }], total }
```

Metric definitions (v1 — all expressible against `dp_issues` +
`dp_activity_events`, no new columns):

| Metric | Source | SQL shape |
|---|---|---|
| `throughput` | closed issues per bucket | `COUNT(*) FILTER (WHERE state='closed' AND closed_at BETWEEN $since AND $until) GROUP BY bucket` |
| `lead_time` | open → close duration (seconds) | `percentile_cont(0.5) WITHIN GROUP (ORDER BY EXTRACT(EPOCH FROM (closed_at - created_at)))` per bucket |
| `wip` | currently-open assigned, per-login | `SELECT a.login, COUNT(*) FROM dp_issues i CROSS JOIN LATERAL jsonb_array_elements_text(i.assignees) a(login) WHERE i.state='open' GROUP BY a.login`. *An issue with 2 assignees counts in both WIP buckets — intentional, surfaces shared load.* |
| `stale` | open + idle ≥30d | `COUNT(*) FILTER (WHERE state='open' AND updated_at < now() - interval '30 days')` |
| `untriaged` | open + no assignee + no label + age ≥24h | `COUNT(*) FILTER (WHERE state='open' AND jsonb_array_length(assignees) = 0 AND jsonb_array_length(labels) = 0 AND created_at < now() - interval '24 hours')` |

Auth pair: `("issues", "read")`. Filter scope is always
intersected with `caller.org_ids`.

New file [crates/dp-rest/src/reports.rs](crates/dp-rest/src/reports.rs)
mounting a `reports_router`; store trait grows
`issue_metrics(filter, metric, group_by)` returning a tagged
result enum keyed by metric kind.

---

## 6. Schema changes

Three forward-only migrations in
[crates/dp-store-pg/migrations/dp/](crates/dp-store-pg/migrations/dp/):

```sql
-- 0010_triage_indexes.sql  (slice 1)

-- §5.5 label filter (JSONB containment).
CREATE INDEX dp_issues_labels_gin ON dp_issues USING GIN (labels);

-- §5.5 assignee filter (already issued in PG queries, but uncovered).
CREATE INDEX dp_issues_assignees_gin ON dp_issues USING GIN (assignees);

-- §5.5 author filter. New column.
ALTER TABLE dp_issues ADD COLUMN author TEXT NULL;
CREATE INDEX dp_issues_author_idx ON dp_issues (author);

-- §5.5 state_reason filter (and §5.10 reporting). May already
-- exist as JSONB-buried; promote to a column for indexing.
-- Partial: state_reason is null on most rows and low-cardinality
-- (completed / not_planned / reopened) so a partial index is
-- much smaller without losing coverage.
ALTER TABLE dp_issues ADD COLUMN state_reason TEXT NULL;
CREATE INDEX dp_issues_state_reason_idx
  ON dp_issues (state_reason) WHERE state_reason IS NOT NULL;

-- §3.8 own-write hazard: split the version counter. `version`
-- bumps on every CAS (reconciler + caller). `external_version`
-- bumps only on reconciler-applied remote changes, so unread
-- never flags the caller's own edits.
ALTER TABLE dp_issues ADD COLUMN external_version BIGINT NOT NULL DEFAULT 0;
UPDATE dp_issues SET external_version = version;  -- backfill

-- §5.4 /me/queue cross-repo outer sort.
CREATE INDEX dp_issues_updated_at_idx ON dp_issues (updated_at DESC, id);

-- One-shot backfill for `author` and `state_reason` from the
-- payload the fetcher already stored on the last sync. Cheap;
-- avoids the "empty until next reconciler tick" window.
UPDATE dp_issues
   SET author       = COALESCE(author,       payload->>'user_login'),
       state_reason = COALESCE(state_reason, payload->>'state_reason')
 WHERE author IS NULL OR state_reason IS NULL;
```

```sql
-- 0011_user_issue_state.sql  (slice 1, §3.8)
CREATE TABLE dp_user_issue_state (
  user_id            UUID        NOT NULL REFERENCES dp_users(id)  ON DELETE CASCADE,
  issue_id           UUID        NOT NULL REFERENCES dp_issues(id) ON DELETE CASCADE,
  last_seen_version  BIGINT      NOT NULL DEFAULT 0,
  status             TEXT        NOT NULL DEFAULT 'inbox'
                       CHECK (status IN ('inbox','snoozed','done')),
  snoozed_until      TIMESTAMPTZ NULL,
  updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (user_id, issue_id)
);

-- Inbox query: "my inbox rows that are unread or due-from-snooze".
CREATE INDEX dp_user_issue_state_inbox_idx
  ON dp_user_issue_state (user_id, status)
  WHERE status <> 'done';

-- updated_at trigger (the column has DEFAULT now() at INSERT
-- only; the app must not be trusted to set it on UPDATE).
CREATE OR REPLACE FUNCTION dp_user_issue_state_touch()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN NEW.updated_at = now(); RETURN NEW; END;
$$;
CREATE TRIGGER dp_user_issue_state_touch_trg
  BEFORE UPDATE ON dp_user_issue_state
  FOR EACH ROW EXECUTE FUNCTION dp_user_issue_state_touch();
```

```sql
-- 0012_triage_timeline_and_mentions.sql  (slice 2/3)

-- §5.6 issue timeline filter. Guarded so the cast in the
-- expression index can never raise on malformed payloads.
CREATE INDEX dp_activity_events_issue_idx
  ON dp_activity_events
     (repo_id, ((payload->>'number')::int), ts DESC)
  WHERE kind IN ('issue_opened','issue_closed','issue_comment')
    AND payload ? 'number'
    AND payload->>'number' ~ '^[0-9]+$';

-- §5.5 `mentions` filter projection (slice 3). Populated by the
-- fetcher when it ingests IssueComment events; backfill by
-- scanning dp_activity_events of kind=issue_comment.
CREATE TABLE dp_issue_mentions (
  issue_id  UUID NOT NULL REFERENCES dp_issues(id) ON DELETE CASCADE,
  login     TEXT NOT NULL,
  source    TEXT NOT NULL CHECK (source IN ('body','comment')),
  PRIMARY KEY (issue_id, login, source)
);
CREATE INDEX dp_issue_mentions_login_idx ON dp_issue_mentions (login);
```

The `author` / `state_reason` columns are the only `dp_issues`
touch and both are populated by the in-migration backfill from
payload already on disk. `dp_user_issue_state` and
`dp_issue_mentions` are additive.

---

## 7. Frontend deliverables

### 7.1 Files to add

| Path | What |
|---|---|
| `frontend/src/workflow/triage-page.tsx` | The single landing page; mounts the three panes. |
| `frontend/src/workflow/views-rail.tsx` | Left pane: smart views + pins + tags + orgs + inbox badge. |
| `frontend/src/workflow/issue-list.tsx` | Center pane: pills, group/sort, virtualised table, pagination, unread dots, group-header sync badge. |
| `frontend/src/workflow/peek-panel.tsx` | Right pane: `IssueEditCard` + timeline + linked tags + out-of-date banner. |
| `frontend/src/workflow/timeline.tsx` | Renders `GET /issues/{id}/timeline` rows. |
| `frontend/src/workflow/use-triage-state.ts` | URL <-> filter object reducer (everything is hash-routed). |
| `frontend/src/workflow/sync-badge.tsx` | Per-repo "synced Nm ago" pill (§3.9). |
| `frontend/src/workflow/shortcuts-overlay.tsx` | `?` keyboard cheatsheet. |
| `frontend/src/reports/issues-report.tsx` | Reports page (slice 3) — picks metric/group/range, renders chart + table. |

### 7.2 Files to delete

| Path | Why |
|---|---|
| `frontend/src/workflow/repos-page.tsx` | Replaced by the views rail (org filter) + list. |
| Current contents of `frontend/src/workflow/issues-page.tsx` | Replaced by `issue-list.tsx` + `peek-panel.tsx`. `IssueEditCard` extracted to its own file. |

### 7.3 Files to extract / extend

| Path | Change |
|---|---|
| `frontend/src/workflow/issue-edit-card.tsx` (new) | Hoist `IssueEditCard` from `issues-page.tsx` so the peek panel can import it without pulling the page chrome. |
| [frontend/src/routes.ts](frontend/src/routes.ts) | Drop `WorkflowTab`; add `workflowViewId()`, `workflowGroupBy()`, `workflowFilters()` URL helpers. Workflow becomes a single route. |
| [frontend/src/layout/app-shell.tsx](frontend/src/layout/app-shell.tsx) | Remove the Repos/Issues sidebar sub-entries — Workflow is one nav row again, like Phase 7's original design. |
| [frontend/src/api/client.ts](frontend/src/api/client.ts) | Add `listMyQueue()`, `listIssueTimeline()`, `markInboxSeen()`, `setInboxState()`, `getRepoSyncStatus()`, `enqueueRepoSync()`, `getIssueReport()`; extend `ListIssuesQuery` with the §5.5 fields. |
| [frontend/src/workflow/use-workflow-data.ts](frontend/src/workflow/use-workflow-data.ts) | Add `useMyQueue()`, `useIssueTimeline()`, `useInboxMutations()`, `useRepoSyncStatus()`. |

### 7.4 New deps

- `@tanstack/react-virtual@^3` — list pane virtualisation. ~3KB
  gzipped. Use **fixed row height** (single-line rows by §3.3);
  dynamic measurement is heavier and unnecessary. No
  alternative in `@nube/starter-ui-kit`.
- A small charting lib for the Reports page (slice 3) — pick
  `recharts` (already in the broader nube monorepo) if
  available, else `@tremor/react`. Decide at slice-3 kickoff.
- Everything else (Select, Sheet, Table, dropdown menu) already
  ships in `frontend/src/components/ui/`.

---

## 8. Slice plan (three PRs)

### Slice 1 — Spine + feel (must merge first)

- Migrations: `0010_triage_indexes.sql` (full, including
  `author` + `state_reason` with in-SQL backfill) and
  `0011_user_issue_state.sql`.
- 🆕 `GET /me/queue` handler + store method (inbox-aware).
- 🆕 Inbox endpoints (`POST /me/inbox/seen`,
  `PATCH /me/inbox/{id}`) + store methods.
- 🟡 Wire `POST /issues` / `PATCH /issues/{id}` /
  `POST /issues/{id}/comments` handlers (helpers exist; just
  needs an `issues_write_router`).
- Frontend: `triage-page.tsx` with three panes. Views rail
  shows Smart Views (`My queue` default with unread badge,
  `Assigned`, `Untriaged`, `Created`). List pane: filter pills,
  no grouping yet, paginated, virtualised, unread dots. Peek
  pane: `IssueEditCard` only, no timeline yet; opening it
  auto-marks the row read.
- Keyboard: `j/k/Enter/Esc`, `e` mark done, `h` snooze, `?`
  cheatsheet overlay.
- Inline edit: state cell, **optimistic** with 409 rollback.

**Merge gate**: open `#/workflow`, see your inbox count, click
an unread row, watch the dot disappear, edit the title, save —
round-trips through §8.2 and the page reflects the new version.
Flip a state pill and watch it revert if the row was stale
(§8.3 reload UX). The `?` overlay lists every shortcut.

### Slice 2 — Power + sync visibility

- Migration: `0012_triage_timeline_and_mentions.sql` (timeline
  expression index + mentions projection table; mentions stay
  unpopulated until slice 3).
- 🆕 `GET /issues/{id}/timeline` handler + store method.
- 🆕 Sync endpoints (`GET /repos/{id}/sync-status`,
  `POST /repos/{id}/sync`) + new `("repos", "sync")` auth pair.
- Frontend: timeline section in peek; out-of-date banner;
  per-repo sync badge in list group headers and views rail;
  "Sync now" action; pins + tags expansion in rail (with §13.5
  caps); pill autocomplete for repo/assignee; group-by; sort.
- Keyboard: `x` for bulk select, `c` to compose, `g i` for
  inbox jump.
- `⌘K` palette — jump-to only (issue number, repo slug,
  user login).

### Slice 3 — Bulk + mentions + reports

- Fetcher: on every `IssueComment` ingest, write
  `dp_issue_mentions` rows; one-shot backfill scans existing
  `dp_activity_events` of kind=`issue_comment`. Comment
  ingestion already exists
  ([crates/dp-fetcher/src/worker/handlers.rs](crates/dp-fetcher/src/worker/handlers.rs#L584)).
- 🟡 Extend `GET /issues` with `mentions` filter (predicate
  joins the projection table — see §5.5).
- 🆕 `GET /reports/issues` handler + store method + Reports
  page (`frontend/src/reports/issues-report.tsx`).
- Bulk actions: `A` assign / `L` label / `S` state / `C` close.
  Client fans out §8.2 CAS calls with a concurrency limit of
  4–6; 409s requeue with refreshed `expected_version`. Toast
  tracks progress. No new backend route.
- Inline edits: assignee popover, label popover.
- Saved-view chips in the rail (built on top of pins for now).

---

## 9. Cut lines that don't move

- **One new auth pair only.** `("repos", "sync")` for the
  "Sync now" action. Everything else reuses
  `("issues", "read")`, `("repos", "read")`,
  `("issues", "write")`, `("pins", *)`, `("tags", *)`. Inbox
  endpoints are per-user UI state and ride `("issues", "read")`.
- **No new state machine.** §8.2 CAS handles every issue write;
  bulk actions are client-side fanouts over single-issue CAS
  calls, not a new transaction.
- **SPI growth is bounded.** `Store` grows:
  `list_issues_for_me`, `list_events_for_issue`,
  `mark_issues_seen`, `set_inbox_state`, `issue_metrics`,
  `repo_sync_status`, `enqueue_repo_sync`. Domain types add
  `UserIssueState` and a tagged `IssueMetricsResult`.
- **The fetcher changes once.** Slice 3 only: write
  `dp_issue_mentions` on comment ingest. `author` /
  `state_reason` are backfilled by the migration itself.
- **No real-time.** §10 mandates scheduled ingestion; the UI
  refetches on focus + on every successful mutation. Sync
  freshness is surfaced explicitly via the badge (§3.9) instead.
- **Frontend stays in one route for workflow.** `#/workflow` is
  the only workflow URL; the Reports page lives at `#/reports`
  and is a separate nav row, not a workflow tab.

---

## 10. Open questions

1. **Smart-view definitions are server-policy.** Should `My
   queue` include closed-recently issues, or strictly open?
   Linear includes "completed in last 7 days" — proposal:
   match that for the `Assigned` view (closed_at within 7d
   counts), keep `My queue` strictly inbox-status (§3.8) since
   inbox already handles "recently changed". Resolve in slice 1.
2. ~~**`mentions` over comment bodies needs the fetcher to land
   comment ingestion.**~~ **Resolved**: comment ingestion exists
   today ([crates/dp-fetcher/src/worker/handlers.rs](crates/dp-fetcher/src/worker/handlers.rs#L584));
   slice 3 only needs to write the projection table.
3. **Bulk action atomicity.** Slice 3's "assign 25 rows" — one
   §8.2 acquire per row is correct but chatty. Plan: client
   fanout with concurrency 4–6, requeue on 409. Promote to a
   `POST /issues:bulk` envelope only if slice 3 shows real
   latency (p95 > 2s for 25 rows).
4. **`@tanstack/react-virtual` vs Tailwind v4 styling.** Check
   that virtualised row heights play with the existing
   `components/table.tsx` shadcn primitives; if not, the list
   pane uses a hand-rolled `div` grid instead of `Table`. Use
   fixed row height (single-line per §3.3) either way.
5. **Saved-views as a third object** (§14.6 open). Pins + tags
   cover v1; promote to a real `dp_saved_views` table only if
   slice 3 surfaces a real need (e.g. filter combinations that
   can't be expressed as "pin a tag").
6. **Snooze defaults.** Linear offers "tomorrow / next week /
   custom". Confirm exact buckets in slice 1 design review; the
   schema (§3.8) is bucket-agnostic.
7. **Reporting chart lib.** `recharts` vs `@tremor/react` — pick
   at slice 3 kickoff based on what's already vendored in the
   broader nube monorepo.

---

## 11. Why this is better than the drill-down

| Drill-down (rejected) | Triage (proposed) |
|---|---|
| 2 page loads + a modal to read an issue body | 1 page load, peek opens with `Enter` |
| One filter at a time (repo via URL) | Composable pills + smart views |
| Pins and tags are dead sidebar widgets | Pins and tags ARE the saved views |
| `dp_activity_events` unused | Powers peek timeline + Reports (the moat) |
| Each org is a separate workflow | Cross-org by default; org filter is opt-in |
| Read-only rows | Optimistic inline edits + bulk over §8.2 |
| No notion of "new since I looked" | Per-user inbox: unread / snooze / done |
| Sync freshness invisible | Per-repo "synced Nm ago" + Sync-now |
| No reporting | `/reports/issues`: throughput, lead time, WIP, stale, untriaged |
| Sized for "one repo, a few issues" | Sized for 100s of repos, 1000s of issues |
| Re-invents GitHub Issues | Uses what dev-pulse has that GitHub doesn't |
