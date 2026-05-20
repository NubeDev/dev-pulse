# Users & Identities — scope

> Scope doc for the user / GitHub-identity surface. Resolves the
> overlap between `#/account/identities` and `#/directory/users`
> and pins down what ships, in what order, and where each piece
> of the model lives.
>
> Companion to [linear-projects-idea.md §3.0](linear-projects-idea.md)
> (identity data model) and [linear-projects-idea.md §10](linear-projects-idea.md)
> (deferred write handlers).

---

## 0. TL;DR

dev-pulse has **two** kinds of "user":

1. **System user** (`dp_users`) — a stable UUID, one per human
   operator. Owns inbox, pins, tags, audit, memberships.
2. **GitHub identity** (`dp_user_identities`) — a GitHub login
   the system user has proven they own. A system user can have
   **N** identities (`alice-acme`, `alice`, `alice-oncall`).

These are **not** peers and **not** interchangeable. The UI must
make that loud. We ship three pages, each owned by exactly one
actor:

| Page | Route | Required scope | Reads / writes |
|---|---|---|---|
| My GitHub logins | `#/account/identities` | `identities.read` / `identities.write` (scoped `me`) | caller's own identity set |
| Directory · Users | `#/directory/users` | `users.read` | read-only browse of every system user + their linked logins |
| Admin · User detail | `#/admin/users/:id` | `admin.write` | destructive cross-user ops (transfer, admin-link, force-unlink) |

The current "2 tabs in Directory" proposal is **rejected** — see
§5 for the rationale.

---

## 1. Data model (recap, authoritative)

```
dp_users (id UUID PK, …)
  └── dp_user_identities (user_id, github_user_id BIGINT,
                          github_login TEXT, is_primary BOOL,
                          verified_via TEXT, linked_at)
        └── dp_membership_identities (user_id, org_id,         -- DERIVED
                                       github_user_id)
              └── dp_memberships (user_id, org_id, role,       -- DERIVED
                                  home_org, joined_at)
```

Invariants (from [§3.0.2.b](linear-projects-idea.md)):

- One GitHub account maps to **at most one** `dp_users.id`
  (`UNIQUE (github_user_id)`).
- Exactly one identity per user is `is_primary`, enforced by a
  partial unique index:
  `CREATE UNIQUE INDEX dp_user_identities_primary_idx`
  `  ON dp_user_identities (user_id) WHERE is_primary;`
  App-layer checks are belt-and-braces only.
- `dp_memberships(user_id, org_id)` exists **iff** at least one
  `dp_membership_identities` row exists for the same pair.
  Memberships are derived; the UI never edits them directly.
- A user always has ≥ 1 identity. Last-identity unlink is
  refused server-side.
- The legacy `dp_users.github_id` column is **deprecated** by
  migration 0013 and dropped by 0014.

---

## 2. Page contracts

### 2.1 `#/account/identities` — My GitHub logins

**Actor.** The signed-in user, acting on themselves only.

**Why it exists.** OAuth proof-of-possession is per-identity. A
user proves they control `alice-oncall` by doing an OAuth
round-trip from their own session — no admin can grant that on
their behalf without it being a break-glass event.

**Contents.**

- Header: "My GitHub logins" + one-line "Every login you've
  linked to your dev-pulse account."
- **Identity list**, primary first, then linked-at desc. Each
  row:
  - `github_login` (mono).
  - `linked_at` (relative).
  - `verified_via` chip (`oauth` / `admin-link`). The third
    value `rotation` from earlier drafts is dropped — it had
    no defined trigger and no handler. Re-introduce it (with a
    spec) if/when scheduled token rotation lands.
  - Row actions, gated:
    - `Set primary` (disabled if already primary).
    - `Unlink` (disabled if `is_primary` — must set another
      primary first; also disabled if it would be the last).
- **Link another login** — single button → kicks the OAuth
  start endpoint, returns to this page, toasts result. The
  round-trip is the CSRF-critical path; see §2.1.1.
- **Audit hint** — small "Recent identity events" footer
  showing the last 5 `IDENTITY_*` audit rows for this user
  (linked / unlinked / primary changed / admin-linked /
  transferred). Same source the admin page reads from.

**Endpoints (all gated on `("identities","read"|"write")`, scoped
to `me`):**

- `GET /me/identities`
- `POST /me/identities/link/start` → returns OAuth start URL
- `GET  /me/identities/link/callback` → finishes link (see §2.1.1)
- `DELETE /me/identities/{github_user_id}`
- `PATCH  /me/identities/{github_user_id}/primary`

#### 2.1.1 OAuth link callback — CSRF & nonce flow

The link callback is the highest-impact security path on this
surface: GitHub redirects the user-agent to it with
`?code=…&state=…`, and the caller's session cookie rides along.
If `state` is not properly bound and consumed, an attacker can
trick a logged-in admin into linking an attacker-controlled
GitHub login to the admin's dp-user.

Rules — all required, all server-enforced:

- `POST /me/identities/link/start` writes a row to
  `dp_identity_link_pending(nonce UUID PK, dp_user_id,
  session_id, created_at, expires_at)` and returns a GitHub
  OAuth URL with `state = nonce`. The session id is **never**
  on the wire — `state` is an opaque server-side handle.
- `GET /me/identities/link/callback` is gated on
  `("identities","write")` scoped `me` exactly like the other
  link routes — the user must have a live session.
- Callback validation, in order, all-or-nothing in one
  transaction:
  1. Look up `state` in `dp_identity_link_pending`; row must
     exist, be unused, and `expires_at > now()` (TTL: 5 min).
  2. `dp_user_id` on the row **must equal** the caller's
     current `dp_users.id`. Reject if the session changed users
     between start and callback.
  3. `session_id` on the row **must equal** the caller's
     current session id. Reject if the cookie was rotated.
  4. Exchange `code` for a GitHub token, fetch the GitHub
     user, and assert `github_user_id` is not already claimed
     by a different dp-user (return 409 +
     `IDENTITY_CLAIM_CONFLICT` audit).
  5. Insert `dp_user_identities` and delete the pending row in
     the same transaction (single-use nonce).
- Any failure deletes the pending row and audits
  `IDENTITY_LINK_REJECTED { reason }`.

**What it does NOT do.**

- No view of other users' identities.
- No transfer-to-another-user.
- No admin-link.
- No membership editing — memberships re-derive on the next
  stamp.

### 2.2 `#/directory/users` — Directory · Users

**Actor.** Anyone with `("users","read")`. Read-only.

**Why it exists.** "Who is this person, really, and which orgs
do they reach?" is asked dozens of times a day during triage.
Today the page shows memberships + home-org but **hides the
linked GitHub logins**, which is the whole point of the
multi-identity model.

**Contents (additive over what ships today).**

- Existing: search by login/name/email, org filter, table with
  `Login`, `Name / email`, `Memberships`, `Home org`.
- **New column: `GitHub logins`** — chips for every
  `dp_user_identities.github_login` belonging to the row, with
  the primary one badged. Hover reveals `verified_via` and
  `linked_at`.
- **New: row expander** (`>`) — reveals the full identity table
  inline (same shape as `/account/identities` but read-only),
  recent identity audit events, and a `Manage →` button that
  deep-links to `#/admin/users/:id` when the caller has
  `admin.write`; otherwise hidden.
- **Search must match identity logins**, not just the primary
  login. Searching `alice-oncall` finds the dp-user even if
  their primary is `alice-acme`.

**Endpoints.**

- `GET /users` (existing) — extended response shape includes
  `identities: [{ github_login, is_primary, verified_via,
  linked_at }]`. No new endpoint.

**What it does NOT do.** Any write. Period.

### 2.3 `#/admin/users/:id` — Admin · User detail (new)

**Actor.** `admin.write`.

**Why it exists.** Break-glass ops on **other people's**
identity sets. These are dangerous (transfer moves audit/inbox
ownership; admin-link bypasses OAuth proof) and need to live in
a clearly admin-shaped place with louder audit verbs.

**Contents.**

- User summary (login, name, email, home org, memberships).
- Identity table (same shape as §2.1) with the destructive ops
  the user themselves can't do:
  - **Admin-link** new identity (no OAuth round-trip — writes
    `verified_via = 'admin_link'` and emits an
    `IDENTITY_ADMIN_LINK` audit row that **also surfaces in the
    target user's own audit log**). Before write, the handler
    **must** resolve the login against the GitHub API to get a
    canonical `github_user_id` and confirm the account exists;
    typos otherwise create unresolvable phantom identities.
    Reject if the `github_user_id` is already claimed by
    another dp-user (same 409 path as the OAuth flow).
  - **Transfer** identity to another dp-user (see §2.3.1 for
    what moves and what doesn't).
  - **Force-unlink** — skips the "set another primary first"
    guard but still enforces the last-identity rule; see
    §2.3.2 for the compromised-account path.
- Full `IDENTITY_*` audit trail for this user, paginated. The
  query is **target-scoped** (rows where `subject_user_id =
  :id`), not actor-scoped, so admin-authored events land in
  the target user's own footer at §2.1.

#### 2.3.1 Transfer semantics

Transfer moves an identity row from `user_src` to `user_dst`.
It does **not** rewrite history. Specifically:

- `dp_user_identities` row: `user_id` updated to `user_dst`;
  `is_primary` always reset to `FALSE` (dst picks primary
  explicitly via `PATCH …/primary`).
- `dp_membership_identities`: provenance rows for the moved
  `github_user_id` are re-keyed to `user_dst` **in the same
  transaction** as the identity move. `dp_memberships` is
  recomputed for both `user_src` and `user_dst` atomically —
  see §4 Slice A item 4. There is no window where `user_src`
  retains org access whose provenance has moved.
- `dp_audit_events`: **not rewritten**. Past actions taken by
  `user_src` remain attributed to `user_src`. A single
  `IDENTITY_TRANSFER { from: user_src, to: user_dst,
  github_user_id }` row is emitted; it surfaces in both users'
  audit footers.
- `dp_user_issue_state` (inbox), `dp_user_pins`, `dp_tags`:
  **not moved**. These are per-system-user state; the
  identity changing hands doesn't transfer the workbench. If
  `user_src` is being decommissioned the admin handles that
  separately (out of scope here).

#### 2.3.2 Compromised / deleted GitHub account

The last-identity guard refuses zero-identity unlinks. For a
compromised or deleted GitHub account that is a user's **only**
identity, the path is **transfer-then-unlink**, not a
zero-identity tombstone:

1. Admin admin-links a replacement identity to the user (the
   user's new GitHub login, or a placeholder owned by the
   admin if the user is mid-rotation).
2. Admin sets the replacement as primary.
3. Admin force-unlinks the compromised identity.

We explicitly **do not** support zero-identity users. Every
system user must always have at least one identity; this keeps
the principal stamper's invariant (`identity_set_empty` → 401)
universal.

**Endpoints.**

- `POST   /admin/users/{user_id}/identities` (admin-link)
- `POST   /admin/identities/{github_user_id}/transfer`
  `{ to_user }`
- `DELETE /admin/users/{user_id}/identities/{github_user_id}`
  (force-unlink)

**What it does NOT do.** Membership editing — still derived.

---

## 3. Navigation

```
Reports
  User · Team · Org · Leaderboard · Repo activity · Home-org split · Freshness
Workflow
  Triage · Repos · Issues
Directory
  Users · Orgs · Teams · Home-org
Admin
  Runs · Refresh · Users          ← new: links to /admin/users index
Account (top-right menu, not rail)
  My GitHub logins                ← renamed from "Identities"
  Sign out
```

Rationale:

- "My GitHub logins" is **personal config**, not a directory.
  It belongs under the user menu, like "Profile" in every other
  product. The current rail entry under "Account" is fine if
  the group exists; just rename the label.
- "Admin · Users" is a thin index page that lists every dp-user
  with a `Manage →` per row. Each row links to
  `#/admin/users/:id`.
- The "1 identity" chip in the top chrome is **removed** — it
  implies a count the user should act on globally; the
  affordance lives one click away in the user menu.

> **Cross-doc note.** `linear-projects-idea.md` is the source
> of truth for navigation. This nav delta (rename, admin index,
> chip removal) **overrides** the slice-2 entries there and
> must be back-ported to that doc when this scope ships.

---

## 4. Slice plan

### Slice A — Backend writes (unblocks everything)

The current `identities-page.tsx` is a scaffold because the
write handlers were deferred in §10 of `linear-projects-idea.md`.
Land them first. Until they exist, no UI reshuffle helps.

1. Migrations:
   - `dp_user_identities` and `dp_membership_identities` —
     already shipped by slice 2 of `linear-projects-idea.md`
     §3.0.1.
   - `dp_identity_link_pending(nonce UUID PK, dp_user_id UUID,
     session_id TEXT, created_at TIMESTAMPTZ, expires_at
     TIMESTAMPTZ)` with `CREATE INDEX ON
     dp_identity_link_pending (expires_at)` for the GC sweep
     — **new migration this slice**. Required by §2.1.1.
   - Partial unique index from §1 if not already in place:
     `CREATE UNIQUE INDEX dp_user_identities_primary_idx ON
     dp_user_identities (user_id) WHERE is_primary;`
2. Handlers:
   - `GET    /me/identities`
   - `POST   /me/identities/link/start`
   - `GET    /me/identities/link/callback`
   - `DELETE /me/identities/{github_user_id}`
   - `PATCH  /me/identities/{github_user_id}/primary`
3. Resource registration: `identities` resource registered in
   `register_dev_pulse_resources` with `read` / `write`.
4. Membership reconciler hook on link / unlink / transfer /
   primary-change. Per-op atomicity requirements:
   - **Link** — insert identity, insert provenance for every
     org the new identity reaches, upsert memberships, append
     to `principal_dirty`. One transaction.
   - **Unlink** — delete identity (CASCADE drops provenance),
     collapse any now-unprovenanced memberships for the
     affected `user_id`, append to `principal_dirty`. One
     transaction.
   - **Transfer** — move identity row, re-key provenance rows,
     recompute memberships for **both** `user_src` and
     `user_dst`, append both to `principal_dirty`. All in one
     transaction. There must be no intermediate state where
     `user_src` keeps org access whose provenance has moved.
   - **Set-primary** — flip booleans, append `principal_dirty`
     (no membership change).
   The principal cache key includes the row's `updated_at`
   from `principal_dirty`, so the next request re-stamps
   before serving; no forced re-login.
5. Audit: `IDENTITY_LINK`, `IDENTITY_LINK_REJECTED`,
   `IDENTITY_UNLINK`, `IDENTITY_SET_PRIMARY`,
   `IDENTITY_ADMIN_LINK`, `IDENTITY_TRANSFER`,
   `IDENTITY_CLAIM_CONFLICT`. Every row carries
   `subject_user_id` (the user whose identity set changed) in
   addition to `actor_user_id`, so the target-scoped query in
   §2.1 / §2.3 can find admin-authored events.

### Slice B — Self-service page wired up

1. Replace the local `useIdentities` stub in
   [identities-page.tsx](frontend/src/account/identities-page.tsx)
   with `GET /me/identities`.
2. Wire `useLinkIdentity` / `useUnlinkIdentity` /
   `useSetPrimaryIdentity` to the real endpoints. Drop the
   "deferred" toast theatre.
3. Rename rail entry → "My GitHub logins". Update breadcrumb
   from "Account · Identities" to "Account · My GitHub logins".

### Slice C — Directory enrichment

1. Extend `GET /users` response with `identities[]`. Payload
   grows by ~80–150 bytes per user (3 identities × ~30–50
   bytes); at the default `GET /users` page size this is
   negligible. **Server-side pagination must be in place from
   day one of this slice** — no client-side "load everything"
   path — so the payload growth doesn't compound with row
   count.
2. Add `GitHub logins` column + row expander (chevron rotates
   `▸` → `▾` on open) to
   [users-page.tsx](frontend/src/directory/users-page.tsx).
3. Extend search to match `github_login` across all identities.
   Slice C keeps the client-side filter; see §6 for the
   server-side promotion trigger.
4. Conditional `Manage →` button when caller has `admin.write`.
5. Remove the top-right "1 identity" chip (UI-only — lands
   with the directory change since both touch chrome).

### Slice D — Admin user detail

1. `/admin/users` index page (table + search; reuses
   `GET /users` shape).
2. `/admin/users/:id` detail page.
3. Admin handlers: `POST /admin/users/{user_id}/identities`,
   `POST /admin/identities/{github_user_id}/transfer`,
   `DELETE /admin/users/{user_id}/identities/{github_user_id}`.
4. Audit trail panel.

---

## 5. Rejected alternatives

### 5.1 "Two tabs in Directory/Users: GH and System"

Rejected. Tabs imply peer operations of equivalent privilege,
which these are not:

- Self-service identity management is per-user, requires OAuth
  proof-of-possession, and is a daily op.
- System-user / cross-user identity management is admin-only,
  destructive, and rare.

Collapsing them forces one of two bad outcomes:
- (a) hide self-service behind an Admin nav group → wrong for
  the 49 non-admin devs;
- (b) duplicate the rail entry → confusing.

The right split is page-by-actor (§2), with the **Directory row
expander** giving the visible "system user ↔ GitHub identities"
connection the proposal was reaching for.

### 5.2 "Admin links a system user to a GH account from the Directory"

Rejected as the **default** front door. Admin-link exists as
break-glass only ([§3.0.2](linear-projects-idea.md)) precisely
because it bypasses OAuth proof. Making it the obvious path
inverts the trust model. It lives on `#/admin/users/:id`,
behind `admin.write`, with a louder audit verb that also
surfaces in the target user's own audit log.

### 5.3 "Edit memberships from the user page"

Rejected. Memberships are **derived** from identity provenance
(§3.0.2.b). Direct membership writes would silently desync from
the GitHub-orgs stamper on the next tick. Adjust identities
instead; memberships follow.

---

## 6. Open questions

- **Search performance on identity-login match.** The current
  `GET /users?org_id=…` fanout filters client-side. Adding
  identity-login matching to the same client-side filter is
  fine for the slice-C ship. Promote to a server-side
  `GET /users?q=…` that joins `dp_user_identities` when **any**
  of these trip:
  - `GET /users` p95 latency > 200 ms in production telemetry;
  - total `dp_users` row count > 300;
  - the directory page TTI (time-to-interactive) regresses
    beyond 500 ms.
  Whichever fires first opens the follow-up ticket.
- **`/admin/users` index vs. reusing Directory.** If we ship
  the `Manage →` button on the directory rows (§2.2), do we
  need a separate `/admin/users` index at all? Probably yes —
  for audit-only navigation ("show me every user with an
  admin-linked identity") that doesn't fit a search box. Punt
  to slice D scoping.
- **Account menu vs. Account rail group.** Today there's an
  Account rail group with one entry. Either move it under the
  user menu (tidier) or keep the group and add `Profile` /
  `Preferences` later. No strong preference; keep the rail
  group until there's a second entry to justify the menu.
