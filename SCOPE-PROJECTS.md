# dev-pulse — Scope (Projects, Issues & Pins) — *design rationale*

> **Status: promoted.** The normative scope for the workflow
> surface now lives in [SCOPE.md](SCOPE.md) §16 (pinned repos &
> tags), §17 (project tags), §18 (GitHub Issues CRUD), §19 (auth
> implications), and the §15.15–§15.21 decisions block.
>
> This document is **retained as design rationale**, not deleted.
> It captures the vision, the comparison against GitHub Projects v2,
> conflict-handling walk-throughs, and the open questions that led
> to the locked decisions. When the two documents disagree,
> [SCOPE.md](SCOPE.md) is the normative source — this file is the
> design diary. New decisions and surface-shape edits land in
> [SCOPE.md](SCOPE.md); this file is updated only when its rationale
> drifts from what was actually built.
>
> SCOPE.md §20 carries the §13.x → §15.x decision mapping.
>
> ---
>
> Original framing follows.
>
> Companion to [SCOPE.md](SCOPE.md). Covers the **workflow** half of
> dev-pulse: pinned repos, home-grown project tags, and GitHub Issues
> CRUD. The reporting half stays in [SCOPE.md](SCOPE.md); the two
> documents share entities, auth, and the access gate.

---

## 1. Vision

The base product (SCOPE.md) tells a manager *what is happening* across
their GitHub orgs. This document covers what happens *next*: the user
spots something in a report and needs to **act** — group related work,
file a follow-up, reassign an issue, close a stale one — without
leaving dev-pulse and without losing the cross-org lens that is the
product's core differentiator.

Three concrete capabilities:

1. **Pinned favourites** — fast-access shortcuts for the handful of
   repos (and tags) a given user actually works with day-to-day.
2. **Project tags** — a lightweight, *home-grown*, cross-org way to
   group repos / issues / users / teams into a "project," because
   GitHub's own Projects feature is org-scoped and cannot model the
   cross-org work dev-pulse is built around.
3. **Issue CRUD** — synchronous, user-initiated GitHub Issues
   create / read / update / close / reopen / comment, with optimistic
   local updates and an audit trail.

This is the **action surface**. Reports remain read-only and
fetcher-backed; this surface introduces the first user-initiated
writes to GitHub.

---

## 2. Why a separate document

Workflow features change shape faster than reporting features and have
materially different constraints:

- They introduce **writes to GitHub** — new auth scopes, new audit
  verbs, new failure modes.
- They are **per-user state** in a way reports are not — pins and
  personal tags belong to a user, not to an org.
- They cross the line from *observing* developer activity to
  *participating* in it, which has its own product framing (§4
  below) that the reporting non-goals in SCOPE.md §4 do not cover.

Keeping these in their own scope document means a reviewer can sign
off on reporting without also having to weigh in on issue mutation,
and vice versa.

---

## 3. Goals

### Primary goals

1. **Per-user pinned repos and tags** — a small, ordered list the user
   curates themselves, surfaced as the default filter on the
   issue-management views and as a sidebar quick-list.
2. **Home-grown project tags** — users can create tags, attach them
   to repos / issues / users / teams, and use them as a filter and
   grouping dimension everywhere a report or list accepts one. Tags
   are **cross-org by construction** — a single tag can span repos in
   multiple orgs.
3. **GitHub Issues CRUD** — the operations a manager actually needs
   to act on what a report shows: create, edit (title/body/labels/
   assignees/milestone), close, reopen, comment. Synchronous,
   user-initiated, optimistic with reconciliation.

### Secondary goals

- **Tag-driven Gantt** (later phase) — once tags exist, an additive
  schedule table on top of them gives a project-style timeline view
  without users having to re-enter dates they already have in
  GitHub milestones and issue timestamps.
- **One-way GitHub Projects v2 import** (later phase) — read-only
  mirror of an existing GitHub Project as a dev-pulse tag, for users
  who already organise work in Projects. Never the system of record.

---

## 4. Non-goals (for now)

Enforced by design choices, not just stated intent:

- **No bulk issue mutations in v1.** Per-item only. Bulk close,
  bulk relabel, bulk reassign — all deferred. Keeps the audit log
  unambiguous and the rate-limit story simple.
- **No PR mutations.** Read-only on PRs in v1 (the reporting
  surface already covers them). PR comments, merges, reviews from
  inside dev-pulse are out.
- **No discussions, reactions, or attachments.**
- **No label / milestone administration.** We *use* whatever
  labels and milestones already exist on the repo; we don't create
  or rename them from dev-pulse.
- **No draft issues, no issue templates, no @-mention
  autocomplete from our user table.** All deferred.
- **GitHub Projects v2 is not the system of record.** If we ever
  integrate, it is one-way *import* into a tag (§3 secondary).
  We do not push tag changes back into a GitHub Project.
- **Tags do not replace GitHub labels.** Labels stay where they
  are (on the repo, in GitHub); tags are dev-pulse-side metadata
  that can span repos and orgs in ways labels structurally cannot.
- **No surveillance creep.** Pins and tags are user-curated. The
  system does not infer "what project a user belongs to" from
  their activity and assign tags automatically — that would
  reintroduce exactly the perf-ranking dynamic SCOPE.md §4 rules
  out.

---

## 5. Key entities

Extends [SCOPE.md §5](SCOPE.md). New entities introduced here:

- **Pin** — a per-user reference to a repo *or* a tag, with a
  position for ordering. Lives only in the dev-pulse DB.
- **Tag** — a named, coloured, scoped grouping. Scope is one of
  `user` (private), `team`, or `org` (shared). A tag is an opaque
  bucket; the meaning comes from what's linked to it.
- **TagLink** — polymorphic edge between a tag and one of:
  `repo`, `issue`, `user`, `team`. A tag with no links is legal
  (an empty project).
- **IssueMutation** — an audit record of a user-initiated write to
  a GitHub issue: actor, target, operation, before/after diff,
  resulting GitHub delivery id (when known).
- **Local issue version** — a monotonically increasing integer on
  the local `issues` row, bumped on every fetched update *and*
  every optimistic local write. Serves as the optimistic-
  concurrency token for the §8 write path (see §8.3) and the
  reconciler-vs-optimistic guard (see §13.7). Not a separate
  entity, but called out here because it is load-bearing.
- **(Future) TagSchedule** — per-tag, per-item start/due/dependency
  rows that drive the Gantt view. Additive; absent in v1.

---

## 6. Pinned repos & tags

### 6.1 Behaviour

- A user can pin any **repo** they can see (per the SCOPE.md §15.11
  access gate) and any **tag** that is visible to them (per §7.4
  below).
- Pins are **ordered** — the user controls position via drag /
  reorder. Newly-added pins go to the end.
- Pinning a **tag** is equivalent to pinning every repo currently
  linked to it, for the purposes of "what shows up in my sidebar
  list" — but it stays a *single* pin in the data model, so if the
  tag gains a repo tomorrow, that repo appears in the user's
  sidebar without further action. This is the headline reason to
  pin tags rather than repos.
- The **pin cap** (working assumption: 20 pins per user) protects
  the *data model*, not the rendered sidebar — a single tag pin
  can expand to many entries. A separate **render cap** (working
  assumption: 50 entries) governs the sidebar: above it the over-
  flow collapses into a "…and N more" disclosure that opens the
  full list. Both caps live in `dp-config` (§13.5).
- Over-the-cap pins are rejected with a clear error, not silently
  dropped.

### 6.2 Surfaces

- **Sidebar quick-list** on every page: pinned items in the user's
  chosen order.
- **Default repo filter** on the Issues view and the Workbench
  dashboard (a new home page that aggregates pinned items'
  recent activity).
- Pins are **not** a report dimension — they are personal UI
  state. They do not appear in the §15.6 envelope and do not
  affect anyone else's view.

### 6.3 Storage

One table:

```
user_pins(
  user_id     UUID  NOT NULL,
  kind        TEXT  NOT NULL CHECK (kind IN ('repo','tag')),
  target_id   UUID  NOT NULL,
  position    INT   NOT NULL,
  pinned_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (user_id, kind, target_id)
)
-- unique (user_id, position) enforced at write time, not as a
-- DB constraint, to allow atomic reorder.
```

### 6.4 API

- `GET    /me/pins`                — ordered list with hydrated targets.
- `POST   /me/pins`                — `{ kind, target_id }`, appends.
- `DELETE /me/pins/{kind}/{id}`    — remove.
- `PUT    /me/pins/order`          — full ordered id list, atomic.

### 6.5 Audit

Verbs added to the §15.13 vocabulary in SCOPE.md:
`pin.add`, `pin.remove`, `pin.reorder`.

---

## 7. Project tags (home-grown)

### 7.1 Why home-grown and not GitHub Projects

GitHub Projects v2 was considered and rejected as the system of
record for v1:

- **Org-scoped.** A Project belongs to one org or one user.
  dev-pulse's headline goal (SCOPE.md §3) is *cross-org /
  cross-company* views. A grouping primitive that can't span orgs
  fights the product.
- **GraphQL-only**, separate API surface from the REST `octocrab`
  flow in SCOPE.md §15.4 — new client wrapper, new rate-limit
  bucket math, new error vocabulary.
- **Cannot tag users or teams** — Projects items are limited to
  Issues / PRs / draft items. We want to group people too
  ("Phoenix squad"), and Projects structurally can't.
- **Separate App permission** (`project: write`) with a separate
  per-install consent step.
- **External state we don't own** — cache invalidation, deleted-
  project edge cases, API version churn (Projects has had two
  incompatible API generations in recent years).

Home-grown wins on all four points: cross-org natively, one
storage backend, polymorphic over four target kinds, no extra
GitHub scope, fully owned schema.

We keep the **one-way Projects import** door open (§3 secondary)
for users who already organise work in Projects — but as a
read-only mirror into a tag, not as the system of record.

### 7.2 Storage

```
tags(
  id              UUID PRIMARY KEY,
  scope_kind      TEXT NOT NULL CHECK (scope_kind IN ('user','team','org')),
  -- Exactly one of the three scope_*_id columns is non-NULL,
  -- matching scope_kind. Enforced by CHECK below; gives us real
  -- FKs and ON DELETE CASCADE per kind.
  scope_user_id   UUID REFERENCES users(id) ON DELETE CASCADE,
  scope_team_id   UUID REFERENCES teams(id) ON DELETE CASCADE,
  scope_org_id    UUID REFERENCES orgs(id)  ON DELETE CASCADE,
  name            TEXT NOT NULL,
  color           TEXT NOT NULL,            -- semantic name: 'indigo', 'red', ...
  description     TEXT,
  created_by      UUID NOT NULL REFERENCES users(id),
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  archived_at     TIMESTAMPTZ,
  CHECK (
    (scope_kind = 'user' AND scope_user_id IS NOT NULL
      AND scope_team_id IS NULL AND scope_org_id IS NULL) OR
    (scope_kind = 'team' AND scope_team_id IS NOT NULL
      AND scope_user_id IS NULL AND scope_org_id IS NULL) OR
    (scope_kind = 'org'  AND scope_org_id  IS NOT NULL
      AND scope_user_id IS NULL AND scope_team_id IS NULL)
  )
);
-- Case-insensitive per-scope uniqueness; expression index, not a
-- column-list UNIQUE constraint.
CREATE UNIQUE INDEX tags_scope_name_uniq
  ON tags (scope_kind, COALESCE(scope_user_id, scope_team_id, scope_org_id), lower(name));

tag_links(
  id           UUID PRIMARY KEY,
  tag_id       UUID NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  kind         TEXT NOT NULL CHECK (kind IN ('repo','issue','user','team')),
  -- Exactly one target_* column is non-NULL, matching kind.
  target_repo_id  UUID REFERENCES repos(id)  ON DELETE CASCADE,
  target_issue_id UUID REFERENCES issues(id) ON DELETE CASCADE,
  target_user_id  UUID REFERENCES users(id)  ON DELETE CASCADE,
  target_team_id  UUID REFERENCES teams(id)  ON DELETE CASCADE,
  added_by     UUID NOT NULL REFERENCES users(id),
  added_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
  CHECK (
    (kind = 'repo'  AND target_repo_id  IS NOT NULL
      AND target_issue_id IS NULL AND target_user_id IS NULL AND target_team_id IS NULL) OR
    (kind = 'issue' AND target_issue_id IS NOT NULL
      AND target_repo_id IS NULL AND target_user_id IS NULL AND target_team_id IS NULL) OR
    (kind = 'user'  AND target_user_id  IS NOT NULL
      AND target_repo_id IS NULL AND target_issue_id IS NULL AND target_team_id IS NULL) OR
    (kind = 'team'  AND target_team_id  IS NOT NULL
      AND target_repo_id IS NULL AND target_issue_id IS NULL AND target_user_id IS NULL)
  )
);
CREATE UNIQUE INDEX tag_links_tag_target_uniq
  ON tag_links (tag_id, kind,
                COALESCE(target_repo_id, target_issue_id, target_user_id, target_team_id));
```

Notes:

- `scope_kind = 'user'` is the "personal tag" case — only the
  owner can see it.
- `archived_at` is soft-delete on the tag itself. Archived tags
  don't appear in pickers but their links survive so historical
  reports filtered by the tag still resolve.
- **Soft-deleted link targets** (user pseudonymised per SCOPE.md
  §0.5, repo removed from an install, issue deleted on GitHub)
  are filtered at query time but the `tag_links` row stays for
  audit. A periodic integrity job (admin task) hard-prunes rows
  whose target has been gone longer than the §0.5 retention
  window.
- No global tag namespace. Two orgs can both have a tag named
  "Phoenix" without colliding (the unique index is per-scope).
- **`color`** is a semantic palette name (`indigo`, `red`,
  `teal`, …), **not** a frontend design-system token id —
  decouples stored rows from design-token churn. The frontend
  maps the semantic name to its current token at render time.

### 7.3 Polymorphism — why it's the whole point

A single tag can simultaneously link:

- **repos** across multiple orgs (cross-org grouping — the §3
  goal Projects cannot meet),
- **issues** that are not yet pulled out into a project ("anything
  blocking the Phoenix migration"),
- **users** who are working on it ("the Phoenix squad" — used as a
  shortcut filter on reports), and
- **teams** as a coarser grouping.

Reports (SCOPE.md §15.6) gain `tags: Vec<TagId>` as an additive
optional field on `ReportEnvelope` (per its §15.6 revisit rule —
additive, never repurpose existing). When provided, the report
unions the link targets into the existing `users` / `teams` /
*(implicit repos via orgs)* filters.

### 7.4 Visibility & permissions

A tag is **visible** to a user iff its scope is visible to them:

- `scope_kind = 'user'` — only the owner.
- `scope_kind = 'team'` — any user the §15.11 policy lets see the
  team.
- `scope_kind = 'org'` — any user the §15.11 policy lets see the
  org.

A tag's **links** are **filtered at query time** to only those
the viewer can see. The viewer never sees a `tag_links` row for
a repo / issue / user / team they have no access to — but the
tag itself is not denied. This avoids the awkward "tag exists for
some people, vanishes for others" UI failure.

**Link counts** in `GET /tags` and `GET /tags/{id}` are reported
**after** the viewer-visibility filter — i.e. the count the
viewer would see if they expanded the tag, not the true count.
Reporting the true count would leak the existence of repos /
issues / users / teams the viewer has no access to.

**Default tag scope.** New-tag UI defaults to `org` scope (when
the viewer is in exactly one visible org) or prompts the viewer
to pick (when they are in several); `user` scope is the
opt-in. The product framing (§1) is cross-org grouping for
managers — defaulting to the shared artefact prevents the
"I made it but my teammate can't see it" surprise.

**Mutation:**
- Anyone who can see a tag can **propose** a link, but only the
  tag's `scope` members can **commit** one. User-scope tags:
  owner only. Team/org-scope: any member.
- Edit (rename / recolour / archive): scope members only.
- Hard delete: never via API (only archive). DB cleanup of
  archived tags is an admin job.

### 7.5 API

- `GET    /tags`                 — list visible tags, with viewer-
  filtered link counts; paginated per the SCOPE.md §15.6
  envelope page contract.
- `POST   /tags`                 — create.
- `PATCH  /tags/{id}`            — rename / recolour / archive.
- `GET    /tags/{id}`            — single tag; links are paginated
  separately (`?links_page=…`) to keep a single response bounded
  even for tags near the §13.5 500-link soft warning.
- `POST   /tags/{id}/links`      — `[{kind, target_id}, ...]`,
  batch. **Transactional all-or-nothing**: any per-item validation
  failure (target not visible, wrong kind, duplicate) rejects the
  whole batch with a per-item error array, no partial commit.
- `DELETE /tags/{id}/links`      — batch unlink, same all-or-
  nothing semantics.
- `GET    /me/tags`              — convenience: tags I own or am a
  scope member of.

### 7.6 Audit

New verbs: `tag.create`, `tag.update`, `tag.archive`, `tag.link`,
`tag.unlink`. Each `tag.link` / `tag.unlink` records the
`(tag_id, kind, target_id)` tuple.

### 7.7 Tags as a reports dimension

Two additive changes to the SCOPE.md §15.6 envelope, both
governed by its "additive, never repurpose" revisit rule:

1. **`tags: Vec<TagId>`** — new optional filter field. A tag in
   this list contributes an **additional `OR`-predicate** to the
   report's WHERE clause; it does **not** widen or redefine the
   existing `users` / `teams` / `orgs` filters. The predicate is
   the union of the tag's visible link targets resolved to the
   metric's natural attribution column (see (3) below).
2. **`repos: Vec<RepoId>`** — new optional filter field. Required
   to land *before or with* this surface, because tag links of
   kind `repo` cannot otherwise be expressed: the §15.6 envelope
   has no other repo-level filter, and silently widening to the
   whole org is the opposite of what a `repo`-linked tag asks
   for. Tracked as a SCOPE.md §15.6 follow-up (not edited from
   this document).
3. **`group_by: Vec<GroupBy>`** gains a `Tag` variant. Grouping
   by tag produces one row per tag in the `tags` filter; the
   filter is **required** when `GroupBy::Tag` is present, capped
   at a configurable maximum (working assumption: 50). "All
   visible tags" as a default is rejected — it is a query and a
   UI footgun once a deployment accumulates hundreds of tags.

**Metric × link-kind mapping** (resolves "what does a tag with
only `issue` links mean for `commits authored`?"):

| Tag link kind | Contributes to                                            |
|---------------|-----------------------------------------------------------|
| `repo`        | every metric — filters on `activity_events.repo_id`.      |
| `user`        | every metric — filters on `event_actors.user_id`.         |
| `team`        | every metric — expands to team members at query time.     |
| `issue`       | **only** issue-centric metrics (issues opened/closed/     |
|               | commented/assigned per SCOPE.md §15.7). Ignored for       |
|               | commit-, PR-, review-, and workflow-centric metrics.      |

An `issue`-linked tag with no other link kinds, queried against
a commit metric, produces an empty result with an explicit
`empty_reason = "tag links do not match metric attribution"`
in the response — not a silent zero.

**Double-counting rule:** an event counted toward tag A is
**also** counted toward tag B if both tags link the same target.
Tags overlap by design; per-tag totals do not have to sum to the
overall total. This falls out naturally from the union semantics
in (1) and is surfaced in the UI the same way as the SCOPE.md
§8.1 "all orgs combined" de-dup note.

---

## 8. GitHub Issues CRUD

### 8.1 In-scope operations (v1)

Per-issue:

- **Create** — title (required), body, labels, assignees,
  milestone. Repo selected from the viewer's accessible repos.
- **Update** — same fields as create, plus state transitions
  (close / reopen). Partial updates only — the API takes a diff,
  not the full issue.
- **Comment** — add a comment. Edit/delete of comments deferred.

Everything else (bulk ops, PRs, discussions, reactions, label/
milestone admin) is non-goal per §4.

### 8.2 Write path (synchronous, user-initiated)

The path is built around a **local `version: int` column** on
`issues`, monotonically bumped on every fetched update *and*
every optimistic local write. It is the optimistic-concurrency
token; nothing in this path relies on GitHub returning a 409,
because the Issues REST API does not support `If-Match` /
`If-Unmodified-Since` and does not 409 on stale state.

1. UI loads the form, captures the current `issues.version` as
   `expected_version`, and submits it back on POST.
2. UI POST → `dp-rest` handler.
3. §15.11 policy check: viewer can see the target repo.
4. **Permission check** against the per-org App installation: does
   it carry `issues: write`? If not, return `403
   writes_not_available_for_org` with a UI-friendly message.
5. **Optimistic CAS** — in a short transaction, update the local
   row only `WHERE id = ? AND version = expected_version`,
   setting `version = version + 1`, `pending_remote = true`,
   `pending_remote_at = now()`, `pending_remote_actor = ?`. Zero
   rows updated ⇒ stale; return `409 stale_local_version` with
   the current row so the UI can reload and re-prompt. **No
   row-lock is held across the network call.**
6. **Synchronous GitHub call** via the SCOPE.md §15.4 octocrab
   wrapper (same rate-limit guard, same retry rules).
7. On success — clear `pending_remote`, record the GitHub
   delivery id on the `IssueMutation` audit row, commit.
8. On failure — re-apply the pre-mutation field values *and* bump
   `version` again (so any concurrent reader sees a change),
   clear `pending_remote`, surface the GitHub error verbatim
   (422 validation / 403 scope / 5xx upstream). Audit row
   recorded with `failed` status either way.

The scheduled fetcher (SCOPE.md §10) is unchanged. It will
re-observe the mutation when it ticks (or sooner, via the
issue's webhook) and reconcile any drift between optimistic and
authoritative state, subject to the reconciler guard in §13.7.

### 8.3 Conflict handling

- **Stale local write (CAS miss in §8.2 step 5):** reject with
  `409 stale_local_version`, return the current row, ask the UI
  to reload and re-prompt the user. The local row is trustworthy
  here because the webhook-driven reconciler keeps it close to
  authoritative GitHub state.
- **Concurrent dev-pulse writers on the same issue:** resolved
  by the CAS in §8.2 step 5 — the second writer's
  `expected_version` no longer matches and they get a clean
  `409`. No locks held across the GitHub round-trip; no
  connection-pool exposure to upstream stalls.
- **GitHub-side concurrent edit between form load and submit:**
  the webhook for that edit will bump `issues.version` locally
  before the submit arrives (in the typical case); when it does,
  the CAS misses and the user is asked to reload. When the
  webhook *loses* the race (sub-second submit), the local row is
  silently overwritten — same last-write-wins behaviour as the
  GitHub web UI, and the reconciler will pick up the
  authoritative state shortly. Documented limit, not a bug.
- **Webhook arrives mid-flight** with the same change: see §13.7
  — the reconciler does not touch a row with
  `pending_remote = true` younger than the timeout; on the next
  reconciliation pass it confirms or overwrites.

### 8.4 Permissions surfaced honestly

If the per-org App install was granted **read-only**:

- The UI shows a clearly-labelled "writes not available for
  `org-x`" banner on every issue in that org's repos.
- Create / edit / comment controls are visibly disabled with
  hover text explaining why and pointing at the org admin docs.
- The API still returns `403 writes_not_available_for_org` if a
  caller bypasses the UI, with `Retry-After`-style guidance in
  the body.

No silent failures. No surprise 500s. The org-admin who scoped
the install down made a choice and the UI respects it.

### 8.5 Audit

New verbs on top of SCOPE.md §15.13:
`issue.create`, `issue.update`, `issue.close`, `issue.reopen`,
`issue.comment`.

Every row records:

- `actor` (dev-pulse user),
- `target` (repo + issue number, plus our internal issue id),
- `diff` — JSON of mutated fields, `{ before, after }`, with
  `before` omitted on create,
- `result` — `committed` / `failed` / `pending_remote_timeout`,
- `github_delivery_id` when available,
- `error` — verbatim GitHub error for `failed` rows.

**`pending_remote_timeout`** fires when a mutation has been in
`pending_remote = true` for longer than
`issues.pending_remote_timeout_secs` (default 60s, in
`dp-config`) — i.e. the synchronous handler crashed or its
request was killed between §8.2 step 5 and step 7. A background
sweeper (re-using the reconciler's schedule) finds these rows,
rolls them back to the pre-mutation values, bumps `version`,
clears the flag, and writes the audit row with this status. The
UI shows a "mutation timed out — please retry" toast on next
view. No data is held in the pending state indefinitely.

This satisfies the SCOPE.md §9 transparency requirement: a user
can request a full export of mutations they performed and a full
export of mutations performed *against* an issue they own.

---

## 9. Auth implications

Layered on SCOPE.md §15.1 (GitHub App) and §15.10 (operator
OAuth login):

- The GitHub App's **default permission set** gains `issues:
  write`. Existing read-only installs are not auto-upgraded —
  org admins re-consent when they want writes (GitHub's
  permission-change flow handles this).
- Operator OAuth scope is unchanged — write authority is
  delegated through the App install, **not** the user's OAuth
  token. This keeps personal tokens out of the write path and
  means revoking a user inside dev-pulse (SCOPE.md §0.5) also
  revokes their ability to mutate GitHub via the tool.
- The §15.11 access gate is the only authorisation check for
  visibility. Mutation adds an *additional* check (§8.2 step 3)
  against the App install's scope.

---

## 10. Out of scope for this document

- The Gantt view itself — schema sketch only (`TagSchedule` in
  §5). UI, dependency resolution, critical-path rendering
  deferred to a later scope doc.
- GitHub Projects v2 import — flagged as a §3 secondary goal,
  full shape deferred.
- Comment editing/deletion, reactions, attachments — see §4.
- Label / milestone management — see §4.
- Notification / digest emails on tag activity — out.

---

## 11. Success criteria

This surface is successful when:

1. A user can pin a tag named *Phoenix* covering 7 repos across
   3 orgs, and the Issues view defaults to those repos with no
   further configuration.
2. Creating an issue from a report row takes one click and one
   form, and the resulting issue appears on GitHub within the
   page render — not on the next fetcher tick.
3. An org admin who scoped the App install read-only sees no
   broken UI and no surprise errors — just clearly-labelled
   disabled controls.
4. The §15.13 audit log answers "who closed issue #1234, when,
   and what did they change?" with one query.
5. The cross-org `tags` filter on reports produces results that a
   user can manually reconcile against the linked repos in under
   a minute (same trust bar as SCOPE.md §11.4).
6. No write originates from the scheduled fetcher. Every GitHub
   write has a `dp-rest` request id and an actor in the audit log.

---

## 12. Open questions

- **Pin cap** — working assumption 20 per user (§6.1). Validate
  with first deployment.
- **Tag name length / character set** — what does the UI need to
  render cleanly? Emoji in tag names: yes/no?
- **Cross-scope tag promotion** — can a user-scope tag be
  promoted to team or org scope? (Probably yes, as a single
  admin action, but it interacts with `tag_links` ownership.)
- **Issue webhook latency under load** — does step 7 of §8.2
  reliably win the race against a fetcher tick? If not, we need
  a short polling fallback on the affected issue.
- **GitHub Projects v2 import** — if/when this lands, does it
  populate `tag_links` directly or sit in a side-table that
  feeds a tag? (Side-table is the safer answer; defer.)
- **Mobile** — is any of this expected to work on a phone? Pins
  and quick-comment yes; create-issue form probably no.

---

## 13. Decisions

Locked decisions specific to this surface. Anything not listed is
open (§12).

### 13.1 Project grouping is home-grown tags, not GitHub Projects v2

- **Decision:** dev-pulse's project-grouping primitive is the
  `tags` + `tag_links` schema in §7.2. GitHub Projects v2 is
  **not** the system of record.
- **Why:** cross-org by construction (Projects is org-scoped),
  polymorphic over repos/issues/users/teams (Projects is
  issue/PR/draft-only), one storage backend, no extra GitHub
  scope, fully owned schema. Detailed comparison in §7.1.
- **Revisit if:** the first target deployment is GitHub
  Enterprise-only AND already uses Projects v2 heavily AND
  refuses to maintain a parallel tagging structure — then
  consider promoting the §3 secondary "Projects import" goal to
  v1 and treating Projects as a read-only source for tag links.
  The system-of-record stays dev-pulse either way.

### 13.2 Tag links are polymorphic across four kinds

- **Decision:** `tag_links.kind ∈ {repo, issue, user, team}` —
  no more, no less for v1.
- **Why:** these four cover every "what is this project made of?"
  question the §7 use-cases need. Adding PRs as a fifth kind is
  tempting but PRs are tightly coupled to their repo — filtering
  by repo gets you the PRs anyway.
- **Revisit if:** a use-case appears that genuinely needs PR-level
  tagging independent of repo (e.g. "tag the long-lived release
  PRs across all repos in a project"). Add as a fifth kind;
  existing rows untouched.

### 13.3 Issue writes are synchronous user-initiated only; MCP mutations out for v1

- **Decision:** the only path from dev-pulse to a GitHub write is
  a `dp-rest` handler responding to a user request. The fetcher
  never writes. Background jobs never write. **The MCP surface
  (SCOPE.md §15.14, Phase 5) is read-only for v1.** Exposing
  mutations over MCP requires a principal model that MCP
  clients (API token / delegated OAuth, not a session cookie)
  can satisfy with the same audit guarantees as the REST path —
  that design is its own scope item, not a Phase 5 task.
- **Why:** keeps the audit story clean (every write has an
  actor whose authority and identity we can prove), keeps blast
  radius bounded (a fetcher bug cannot mutate GitHub), keeps
  rate-limit accounting simple (write budget is user-traffic-
  shaped, not schedule-shaped).
- **Revisit if:** (a) a use-case appears that genuinely needs a
  scheduled mutation (e.g. "auto-close stale issues after 90d")
  — treat it as a new feature with its own scope review; do not
  retrofit the fetcher; or (b) Phase 5 MCP picks up enough
  traction that delegated-mutation becomes a real ask — open a
  follow-up scope doc covering the principal model first.

### 13.4 Optimistic local writes with reconciler-backed truth

- **Decision:** §8.2 step 5 applies the mutation to the local
  store *before* the GitHub call returns; step 8 rolls forward
  (re-applies pre-mutation values, bumps `version`) on failure.
  Optimistic concurrency uses a local `issues.version` int as
  the CAS token; no DB row-lock is held across the GitHub
  round-trip. The fetcher / webhook reconciler is the final
  source of truth and may overwrite the optimistic row subject
  to §13.7.
- **Why:** UI responsiveness — a closed issue should look closed
  immediately, not after a 600ms round-trip. CAS-on-version
  preserves correctness without exposing connection-pool slots
  to upstream stalls.
- **Revisit if:** reconciler-vs-optimistic drift becomes a
  user-visible bug pattern (e.g. flickering state). The
  alternative is pessimistic-write (wait for GitHub before
  updating the local row), which is simpler but slower.

### 13.7 Reconciler defers to in-flight optimistic writes

- **Decision:** the fetcher / webhook reconciler **must not**
  overwrite an `issues` row where `pending_remote = true` and
  `pending_remote_at` is younger than
  `issues.pending_remote_timeout_secs` (default 60s, §8.5).
  Webhook payloads for such rows are buffered, not applied,
  until the flag clears or the timeout sweeper rolls the row
  back. After the flag clears, the *next* fetcher tick (or the
  buffered webhook payload, replayed) becomes authoritative.
- **Why:** without this rule, a fetcher tick that races the
  GitHub round-trip in §8.2 will write the pre-mutation state
  back over the optimistic row, the UI flickers to old values,
  then the next tick re-applies the truth. The CAS in §8.2
  step 5 protects writers from each other; this decision
  protects writers from the reconciler.
- **Revisit if:** the timeout default (60s) is wrong for the
  first production deployment — too short causes spurious
  rollbacks under upstream slowness, too long delays
  reconciliation of genuinely-stuck rows. Tunable in
  `dp-config`.

### 13.5 Pin cap, sidebar render cap, and tag scope cap

- **Decision (working assumption, soft-locked):**
  - **20 pins per user** (data-model cap; §6.1).
  - **50 rendered sidebar entries per user** after tag expansion
    (UI cap; overflow collapses into "…and N more").
  - **No hard cap on tag-links per tag**, but a warning surfaces
    above **500 links on one tag** (signal of misuse — the user
    probably wants two tags). Check fires on insert; the
    response carries the warning, the operation still commits.
  - **50-tag cap on `tags` filter when `GroupBy::Tag` is
    requested** (§7.7).
- **Why:** data-model cap protects writes; render cap protects
  the UI; link warning protects query performance; group-by cap
  protects report cost. All four numbers are exposed in
  `dp-config` for tuning.
- **Revisit if:** first deployment hits any limit naturally.

### 13.6 GitHub App permission: `issues: write` becomes default

- **Decision:** the App manifest declares `issues: write` in its
  default permission set. Existing installs that consented to
  read-only must re-consent to gain write; until they do, the
  §8.4 "writes not available" path applies.
- **Why:** the alternative — two App registrations, one
  read-only and one writable — doubles the install ceremony and
  fragments the webhook delivery.
- **Migration for existing read-only installs.** Promoting the
  default permission set triggers GitHub's per-install reconsent
  flow for every existing org install. The rollout:
  1. Ship the manifest change behind a `dp-config` flag
     (`github.app.request_issues_write`, default `true` in new
     deployments).
  2. On the first authenticated view after upgrade, surface a
     persistent (dismissible) banner naming each of the
     viewer's orgs whose install is still read-only, with a
     deep-link to the install's permissions page and copy-able
     text for the viewer to send their org admin.
  3. The §8.4 "writes not available" affordance is the
     steady-state fallback; the banner is the one-shot prompt.
  4. No grace period in code — the §8 surface is gated on the
     install permission, not on a calendar date.
- **Revisit if:** a target deployment's security policy forbids
  any App with write scope. Then we set
  `github.app.request_issues_write = false`, which hard-disables
  the §8 surface and the tag-link kind `issue` (the rest of
  tagging still works).

---

## 14. Cross-references to SCOPE.md

For convenience:

- §4 / §4.1 / §4.2 of [SCOPE.md](SCOPE.md) — entry points that
  cross-reference this document.
- §5 — key entities (this doc extends, does not replace).
- §9 — privacy / transparency constraints; the audit additions
  in §6.5, §7.6, §8.5 satisfy these for the workflow surface.
- §10 — fetcher; unchanged in scope, but §8.2 + §13.7 add a
  reconciler-side rule about deferring to in-flight optimistic
  writes.
- §15.1 — GitHub App; §9 above amends the default permission set.
- §15.4 — octocrab rate-limit wrapper; reused verbatim for
  issue writes.
- §15.6 — `ReportEnvelope`; §7.7 adds two additive optional
  fields (`tags: Vec<TagId>` and `repos: Vec<RepoId>`) per its
  revisit rule. The `repos` addition is a SCOPE.md follow-up
  that must land before or alongside this surface; tag links of
  kind `repo` cannot otherwise be expressed by the envelope.
- §15.10 — operator OAuth login; §9 above explains why write
  authority delegates through the App, not the user token.
- §15.11 — access gate; the single authorisation check for
  visibility in this surface too.
- §15.13 — audit vocabulary; §6.5, §7.6, §8.5 extend it.
