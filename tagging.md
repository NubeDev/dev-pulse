# Tagging — Design Proposal

> Goal: one tag surface that works for **repos, users, and issues**, supports both
> single-token tags (`iot`, `dashboard`) and key:value tags (`priority:high`,
> `team:backend`), and **stays in sync with GitHub where GitHub supports it** —
> while still letting users add tags locally where it doesn't.

---

## 1. What we already have

Existing infra (don't rebuild):

- `dp_tags` — cross-org tag entity, scope ∈ {user, team, org}, soft-delete,
  case-insensitive per-scope name uniqueness.
- `dp_tag_links` — polymorphic edges to `repo | issue | user | team`,
  per-target reverse indexes, CHECK-enforced exactly-one-target.
- `dp_issues.labels` — `JSONB` array of GitHub label strings, already
  populated by the fetcher.
- `dp-fetcher` client has `list_labels(owner, repo)` and `get_label`.
- `dp_issue_mutations` + `pending_remote` CAS — proven outbound write path
  to GitHub with optimistic concurrency and a webhook buffer for in-flight
  conflicts (migrations `0007` / `0009`).
- `crates/dp-rest/src/tags.rs` — 7 routes wired (`GET/POST /tags`,
  `GET/PATCH /tags/{id}`, `POST/DELETE /tags/{id}/links`, `GET /me/tags`),
  visibility + scope-member auth, transactional batch link/unlink,
  500-link soft warning.

What's **missing** for this proposal:

- Key:value as a first-class concept (today it's just an opaque `name` string).
- Any awareness that some tags should be **mirrored to GitHub**.
- A `repo_topic` fetch (we get issue labels, not repo topics).
- A sync pipeline for tag ↔ GitHub-label / GitHub-topic.

> **Sibling slice — coordinate, don't collide.** The Projects v2
> dates mirror (`/memories/session/projectv2-mirror-plan.md`,
> migration `0021_issue_github_node_id.sql` already shipped) is
> the other outbound write surface landing around the same time.
> Both work flows through the same fetcher worker loop and the
> same `(issues, write)` permission gate. Land Projects v2 first;
> the §5 reconcilers here then plug into a worker loop whose
> pacing primitives (per-repo concurrency cap, error grouping) are
> already proven by the dates mirror. Do **not** run the two slices
> in parallel — they share `crates/dp-fetcher/src/worker/*` and
> `crates/dp-rest/src/state.rs`.

---

## 2. What "sync with GitHub" actually means

GitHub has **three** native concepts that overlap with our notion of a tag.
We sync the ones we can; the rest are local-only.

| Target kind | GitHub primitive          | Supports `:`? | Sync direction        |
|-------------|---------------------------|---------------|-----------------------|
| `issue`     | **Issue label**           | Yes           | Bidirectional         |
| `repo`      | **Repo topic**            | **No**¹       | Bidirectional w/ fallback |
| `user`      | *(none)*                  | n/a           | Local-only            |
| `team`      | *(none)*                  | n/a           | Local-only            |

¹ Topic regex is `[a-z0-9][a-z0-9-]*`, max 50 chars. A key:value tag like
`priority:high` **cannot** become a topic — it stays local and we mark the
link `unsupported`. The tag itself still exists; only the GitHub mirror is
skipped.

This is the central rule: **the tag is ours, the mirror is best-effort.**

---

## 3. Tag name grammar — single vs key:value

Both shapes live in the same `name` column. We add two derived columns
populated by a generated expression + a CHECK, so callers can filter
without parsing strings:

```
name = 'priority:high'   →  kind='kv',     key='priority', value='high'
name = 'iot'             →  kind='single', key=NULL,       value=NULL
name = 'team:backend:v2' →  kind='kv',     key='team',     value='backend:v2'   -- split on FIRST colon
```

Rules:

- Lowercase normalised on write (matches GitHub label & topic conventions).
- `name` ≤ 50 chars (the GitHub topic ceiling — keeps everything portable).
- Allowed chars: `[a-z0-9:_-]`. **No spaces, no slashes.**
- A `:` makes it kv. **`kv` tags are never sync-able as repo topics**, only
  as issue labels.
- Per-scope case-insensitive uniqueness already enforced by the existing
  `dp_tags_scope_name_uniq` index — no change needed.

### Regex — stricter than the first draft

The v1 grammar deliberately tightens past what GitHub topics allow so
the **same string** is valid for both label and topic use:

```
^[a-z0-9](?:[a-z0-9_-]*[a-z0-9])?(?::[a-z0-9](?:[a-z0-9_-]*[a-z0-9])?)?$
```

- No trailing `:` (`team:` rejected).
- No leading/trailing `-` or `_` in either segment; no `--` runs (every
  `-` must be followed by `[a-z0-9]`).
- At most one `:` for **new** tags. kv tags whose value contains a
  further `:` (e.g. `team:platform:infra`) are rejected at create time;
  the split-on-first-colon rule applies only to the *backfill* of
  pre-existing rows.
- Length ≤ 50 (the GitHub topic ceiling).

This closes the §3 footgun the reviewer flagged: one regex covers both
surfaces, no "valid here but unsupported there" ambiguity for *new* tags.

### Why split on the first colon, not require a single `:`?

Real-world labels look like `area:auth/oauth` or `team:platform:infra`. The
team-agreed convention is "everything before the first colon is the key" —
that's all the UI needs to render `key=team` chips.

---

## 4. Schema changes

One new migration, e.g. `0023_tag_sync.sql` (next odd slot for the
projects-issues track per `STAGE-1-COORDINATION.md` — `0019` is
taken by user identities, `0021` by the Projects v2 mirror's
`dp_issues.github_node_id`).

### 4.1 `dp_tags` additions

Order matters — columns first, then backfill, **then** the composite
invariant CHECK. Closes the reviewer's #1 (no link between `kind` and
`key/value`).

```sql
-- 1. Add columns. kind defaults to 'single' so existing rows are
--    valid immediately; the backfill upgrades the kv ones.
ALTER TABLE dp_tags
    ADD COLUMN kind        TEXT NOT NULL DEFAULT 'single'
        CHECK (kind IN ('single', 'kv')),
    ADD COLUMN key         TEXT NULL,
    ADD COLUMN value       TEXT NULL,
    -- which GitHub primitive (if any) this tag mirrors to when linked
    ADD COLUMN sync_mode   TEXT NOT NULL DEFAULT 'auto'
        CHECK (sync_mode IN ('auto', 'local_only'));

-- 2. Backfill kind/key/value from existing names.
UPDATE dp_tags SET
    kind  = CASE WHEN position(':' in name) > 0 THEN 'kv' ELSE 'single' END,
    key   = CASE WHEN position(':' in name) > 0 THEN split_part(name, ':', 1) ELSE NULL END,
    value = CASE WHEN position(':' in name) > 0 THEN substring(name from position(':' in name) + 1) ELSE NULL END;

-- 3. Lock in the invariant. Future writes with kind='kv' AND
--    key IS NULL (or kind='single' AND key IS NOT NULL) are rejected.
ALTER TABLE dp_tags
    ADD CONSTRAINT dp_tags_kind_kv_invariant CHECK (
        (kind = 'kv'     AND key IS NOT NULL AND value IS NOT NULL)
     OR (kind = 'single' AND key IS NULL     AND value IS NULL)
    );

-- Cheap filter for `GET /tags?key=priority`
CREATE INDEX dp_tags_key_idx ON dp_tags (scope_kind, key) WHERE archived_at IS NULL;
```

- `sync_mode='auto'` — try to mirror per the table in §2.
- `sync_mode='local_only'` — never push, never read from GitHub. Useful
  for private editorial tags like `user:phoenix-attention`.

### 4.2 `dp_tag_links` additions

The link is where mirror state actually lives — the *same* tag may be
synced on issue A and local-only on issue B (e.g. label was deleted on B).

Reminder: `dp_tag_links` already carries
`kind TEXT NOT NULL CHECK (kind IN ('repo','issue','user','team'))`
from migration `0005`. The reviewer's #2 ("CHECK references missing
`kind`") was a doc oversight — the column is there; the constraint below
is valid as written.

```sql
ALTER TABLE dp_tag_links
    ADD COLUMN sync_state    TEXT NOT NULL DEFAULT 'local_only'
        CHECK (sync_state IN (
            'local_only',          -- not eligible (user/team target, or sync_mode=local_only)
            'unsupported',          -- target supports sync but this tag shape can't (e.g. kv → repo topic)
            'pending_push',         -- we created locally; needs push to GitHub
            'pending_pull',         -- we observed on GitHub; needs domain reconcile
            'synced',               -- mirror confirmed
            'conflict',             -- diverged; needs operator attention
            'remote_missing'        -- pull confirmed absence; awaiting N-cycle quarantine before delete
        )),
    -- GH label node_id for issue/repo-label links. NULL for topic links
    -- (the tag name *is* the topic — don't store it twice; reviewer
    -- nit on §4.2) and for local-only kinds.
    ADD COLUMN external_ref  TEXT NULL,
    ADD COLUMN last_synced_at TIMESTAMPTZ NULL,
    ADD COLUMN sync_error    TEXT NULL,
    -- Counter for the N-cycle quarantine in §5.1 step 5 — increments
    -- on each consecutive *complete* pull that confirms remote
    -- absence. Resets to 0 on any pull that re-observes the link.
    ADD COLUMN remote_missing_streak INTEGER NOT NULL DEFAULT 0;

-- Structural guarantee: user/team links can never escape local_only.
-- Defense-in-depth against a buggy push worker (§7.2).
ALTER TABLE dp_tag_links
    ADD CONSTRAINT dp_tag_links_user_team_local_only CHECK (
        kind IN ('repo', 'issue')
     OR (kind IN ('user', 'team') AND sync_state = 'local_only')
    );

CREATE INDEX dp_tag_links_pending_push_idx
    ON dp_tag_links (added_at)
    WHERE sync_state IN ('pending_push', 'pending_pull', 'conflict', 'remote_missing');
```

The partial index keeps the sync-worker scan cost proportional to the
backlog, not the total tag-link count.

### 4.3 No new mutation table

Reuse `dp_issue_mutations` for issue-label pushes (it already round-trips
to GitHub's `PATCH /issues` with optimistic CAS). For **repo topics** add
a thin sibling — `dp_repo_mutations` — modelled on `dp_issue_mutations`.
That stays out of this doc; it's a follow-up migration when the issue
path is proven.

---

## 5. Sync pipeline

Two reconcilers, both idempotent, both running in the existing fetcher
worker loop.

### 5.1 Pull (GitHub → us) — runs after every repo fetch

For each `repo`:

1. `client.list_labels(owner, repo)` → set of `(name, color)`.
2. For each label not yet represented as an **org-scope** tag in the
   repo's org, create it with `sync_mode='auto'`.
3. For each linked issue's `labels` JSONB array, ensure a
   `dp_tag_links` row exists with `sync_state='synced'`.
4. For repo topics (new `list_topics` client call needed):
   create/refresh `dp_tag_links(kind=repo, target_repo_id=…)` rows
   with `sync_state='synced'`.
5. Anything we previously marked `synced` that GitHub no longer has →
   move to `sync_state='remote_missing'` and bump
   `remote_missing_streak`. **Do not delete on a single observation.**
   Closes the reviewer's #3 (silent data loss on transient fetch
   failures):
   - The bump only happens when the current `list_labels` call returned
     **200 with a complete page set** — no rate-limit truncation, no
     5xx mid-pagination, no scoped-token 404. Partial responses leave
     rows untouched.
   - After `remote_missing_streak >= 3` consecutive confirmed-absent
     pulls (~three poll cycles), the worker deletes the link and writes
     `tag.unlink_remote` to the audit log. The tag survives.
   - Any pull that re-observes the link resets the streak to 0 and
     restores `sync_state='synced'`.
   - During quarantine the UI shows a grey dot — "missing on GitHub,
     awaiting confirmation."

### 5.2 Push (us → GitHub) — runs from `pending_push` queue

For each `dp_tag_links` row in `pending_push`:

- If `kind='issue'` and tag is `single` or `kv` → enqueue a
  `dp_issue_mutations` row of kind `set_labels` with the desired
  label set. Existing CAS / pending-remote / webhook-buffer machinery
  handles concurrency. On 200 OK: `sync_state='synced'`, fill
  `external_ref` with the GitHub label node_id.
- If `kind='repo'`:
  - tag is `single` and name matches topic regex → push via
    `PUT /repos/{owner}/{repo}/topics` (replace-all semantics).
    **GitHub's topics API has no ETag/version** — concurrent pushers
    will silently lose updates without serialisation. Mitigation: the
    push worker takes a **per-repo Postgres advisory lock**
    (`pg_advisory_xact_lock(hashtext('topics:' || repo_id::text))`)
    for the read-merge-write cycle. Cross-replica safe, no extra
    table, dropped at commit. Addresses reviewer #4.
  - else → `sync_state='unsupported'`. Surfaced in the UI as a
    "local-only" pill on the chip.
- If `kind='user'` or `kind='team'` → `sync_state='local_only'`
  permanently. Never touched by the push worker.

### 5.3 Conflict handling

Conflict = we hold `synced` for `(tag, target)` but the next pull sees
a *different* label set or a *different* topic set on the remote.

1. On detection: `sync_state='conflict'`, store the diverged remote
   payload in `sync_error` as `{"remote": …, "local": …}` JSON.
2. UI shows a yellow dot with two explicit actions (reviewer §5.3
   nit — what does the user actually *do*?):
   - **Keep mine** → flip back to `pending_push`. Next push wins.
   - **Take remote** → adopt remote, flip to `synced`.
3. If neither action is taken within one full poll cycle (~5 min by
   default), the worker auto-converges to **remote** and clears
   `sync_error`. Matches how `dp_issues` itself handles GitHub-truth
   drift today.

---

## 6. API additions to `crates/dp-rest/src/tags.rs`

Additive — no breaking changes.

### 6.1 New fields on existing DTOs

`TagDto` gains:
- `kind: "single" | "kv"`
- `key: Option<String>`
- `value: Option<String>`
- `sync_mode: "auto" | "local_only"`

`TagLinkDto` gains:
- `sync_state: "local_only" | "unsupported" | "pending_push" | "pending_pull" | "synced" | "conflict" | "remote_missing"`
- `external_ref: Option<String>`
- `last_synced_at: Option<DateTime<Utc>>`
- `origin: "local" | "github_label" | "github_topic"` (§7.1)

### 6.2 New query params on `GET /tags`

| Param           | Effect                                                  |
|-----------------|---------------------------------------------------------|
| `?key=priority` | Only kv tags whose `key = 'priority'`                   |
| `?value=high`   | Combine with `key` — exact `key:value` lookup           |
| `?kind=kv`      | Only kv (or `?kind=single`)                             |
| `?sync_state=conflict` | Operator filter — surface broken mirrors          |

### 6.3 New routes

```
POST   /tags/{id}/links/resync   — admin-only; force re-pull from GitHub
                                    for every link of this tag. Rate-
                                    limited to one call per tag per 60s
                                    via a `last_resync_at TIMESTAMPTZ`
                                    column on `dp_tags`, checked in the
                                    handler. Stops operators from
                                    hammering GitHub (reviewer §6.3).
GET    /repos/{id}/tags           — convenience: list tags linked to a repo
GET    /issues/{id}/tags          — same for an issue
GET    /users/{id}/tags           — same for a user
```

The three `GET /<resource>/{id}/tags` already have efficient indexes
(`dp_tag_links_{repo,issue,user}_idx`). They're just response-shaping —
no new store method needed beyond `list_tags_for_target(kind, id)`.

### 6.4 `CreateTagRequest` validation

- Normalise `name` to lowercase on entry.
- Reject names not matching `^[a-z0-9][a-z0-9:_-]{0,49}$` →
  `400 tag_name_invalid`.
- Set `kind/key/value` server-side from the name. **Client never
  sends them on create** — pure derived state.

---

## 7. Resolved design decisions

These were the open questions from the first draft. Picking the answer
that ages best, not the one that's cheapest to ship.

### 7.1 Per-repo vs org-scope sync — **org-scope, with provenance**

GitHub labels are per-repo; we collapse them to **one org-scope tag**
per `lower(name)`. The UI shows one `bug` chip across 30 repos instead
of 30 separate entities.

To avoid the "label exists on only one repo but now looks org-wide"
problem, we keep provenance on the **link**, not the tag:

```sql
ALTER TABLE dp_tag_links
    ADD COLUMN origin TEXT NOT NULL DEFAULT 'local'
        CHECK (origin IN ('local', 'github_label', 'github_topic'));
```

- `GET /tags` returns the union (one row per name).
- `GET /repos/{id}/tags` filters to links where the repo participates,
  so a repo only "sees" the labels it actually owns.
- Renaming an org-scope tag rewrites the label on **every repo that
  has a `github_label`-origin link to it** — see §7.1.1 below for
  the collision handling the reviewer flagged.

This is the right long-term shape: the chip is a *concept*, the
links are the *facts*.

#### 7.1.1 Rename collisions (reviewer #7 — most likely real-world break)

Renaming an org-scope tag from `bug` → `defect` fans out to
`PATCH /repos/{o}/{r}/labels/bug` on every repo with a `github_label`
origin link. GitHub returns **422** when the target name already
exists on that repo (e.g. that repo already had its own `defect`).

Handling, per repo, executed inside the rename mutation:

1. Pre-flight `GET /repos/{o}/{r}/labels/{new_name}`.
2. If **404** (free) → `PATCH` the rename. On 200 → link
   `sync_state='synced'`, `external_ref` updated.
3. If **200** (collision) → do **not** PATCH. Instead:
   - Delete the old label on that repo (`DELETE /labels/{old_name}`)
     — GitHub auto-removes it from every issue.
   - Re-apply the new label name to every issue that previously
     carried the old one, via the existing `dp_issue_mutations`
     `set_labels` path.
   - Mark the link `sync_state='synced'`, `external_ref` = the
     pre-existing label's node_id.
   - Audit verb: `tag.rename_merged` (distinct from `tag.rename`)
     so the operator can find these after the fact.
4. If any per-repo step fails with non-422 → that link goes to
   `conflict`, others continue. **Partial rename is acceptable** —
   the chip is one entity, the mirrors are best-effort.

#### 7.1.2 Cross-repo divergence under one org-scope tag (reviewer #8)

Two repos can legitimately ship `bug` with different colors or
descriptions. Collapsing to one chip means *one* color wins on the
DP side; the per-repo definitions on GitHub keep their own colors
untouched until a user explicitly recolours the DP chip (§7.3).

Making this honest rather than silent:

- The pull reconciler records the per-repo `(color, description)` it
  sees on each `github_label`-origin link in two new optional columns:

  ```sql
  ALTER TABLE dp_tag_links
      ADD COLUMN remote_color       TEXT NULL,
      ADD COLUMN remote_description TEXT NULL;
  ```

- `GET /tags/{id}` surfaces a `divergence_count` field: the number of
  `github_label`-origin links whose `remote_color` ≠ the tag's
  current `color`. UI shows "7 of 30 repos differ" with a drill-down.
- A user explicit recolour (§7.3) triggers the same fan-out as a
  rename, with the same §7.1.1 collision rules — reviewer #5: the
  N-call fan-out is *expected and explicit*, paced through the same
  worker queue, never silent.

### 7.2 User tags ↔ GitHub — **stay local, forever**

No native primitive maps cleanly. Mirroring to gists / READMEs is a
leak-of-private-state risk (a `user:phoenix-attention` tag is
editorial, not something we want pushed to a public README).

Decision: `kind IN ('user','team')` links are **structurally**
`sync_state='local_only'`. The CHECK constraint enforcing this lives
in §4.2 alongside the other `dp_tag_links` additions — single
migration, single place to read.

Defense-in-depth: even a buggy reconciler can't PII-leak to GitHub.

### 7.3 Color reconciliation — **GitHub wins on first sight, user wins after**

A new `dp_tags` column tracks intent:

```sql
ALTER TABLE dp_tags
    ADD COLUMN color_source TEXT NOT NULL DEFAULT 'user'
        CHECK (color_source IN ('user', 'github', 'default'));
```

- Tag created locally → `color_source='user'`. Pull never overwrites.
- Tag created by the pull reconciler from a GitHub label →
  `color_source='github'`. Subsequent pulls **do** refresh the color
  (so a rename-and-recolour on GitHub propagates).
- The instant a human edits the color via `PATCH /tags/{id}` → flip
  to `color_source='user'`. From then on, the local color is truth
  and the **push** fans out: one `PATCH /repos/{o}/{r}/labels/{name}`
  per repo with a `github_label`-origin link. This is N calls for an
  org-scope tag linked across N repos — the reviewer's #5 was right
  that §7.4 didn't cover this. The calls go through the same paced
  worker queue (§7.4), so the fan-out is explicit and rate-bounded,
  not hidden.
- Topics have no color, so this only applies to `github_label`-origin
  links.

### 7.4 Bulk re-tag — **transactional batch + worker-side pacing**

Reviewer #6 was right to push back on the framing: GitHub's issue-label
PATCH is genuinely per-issue, and the label-definition PATCH is
per-repo. "Coalescing" doesn't reduce call count for those endpoints.
What the worker actually buys us is **pacing, grouping, and unified
failure reporting** — not call reduction.

The one place we *do* reduce calls is GitHub's `set_labels` endpoint
on a single issue: N tag-link rows targeting the same issue collapse
to **one** `PATCH /issues/{n}` with the merged label set. That's a
real saving for bulk operations.

Worker loop:

- Reads `pending_push` ordered by `(target_repo_id, target_issue_id,
  added_at)`.
- Same-issue rows → one `set_labels` mutation. (Real call reduction.)
- Same-repo / different-issue rows → stay separate, paced under a
  configurable per-repo concurrency cap (default 4). (Pacing.)
- A label-definition `PATCH` (rename or recolour) is one call per
  repo, paced under the same cap. (Pacing.)
- On success: one UPDATE flips every grouped row to `synced`.
- On failure: every grouped row goes to `conflict` with the **same**
  `sync_error` string — one alert, not N. (Failure grouping.)

No SQL triggers; pure worker-loop property, tuneable without a
migration.

### 7.5 Tag deletion — **archive only, with a quarterly hard-prune**

`PATCH /tags/{id}` with `archived: true` already exists. We add **no**
`DELETE /tags/{id}` to the public API. A `dp-cli admin prune-tags
--archived-before 90d` command (out of scope here, follow-up) does
the hard delete in batches, with `tag_links` going first.

Orphan audit rows (reviewer nit on §7.5): the audit table keeps
`subject_id = tag.id` after the row is gone. We make this honest by
**copying the tag's final `name` into the prune-time audit row**
(`tag.hard_delete` verb, payload `{name, scope_kind, scope_id}`), so
`GET /tags/{id}/history` still resolves the human-readable name when
the row itself is gone. The dangling FK from older audit rows is
intentional — they pre-date the prune and were already user-visible
with just the UUID.

Rationale: audit log answers "when did the `phoenix` initiative end?"
forever, and accidental deletes are recoverable for 90 days.

### 7.6 Tag history — **append-only audit table**

Tags drift (rename, recolour, scope changes). The existing audit
infrastructure already records `tag.update` / `tag.archive` /
`tag.link` / `tag.unlink` verbs (per `dp-rest/src/tags.rs` docstrings).
We don't need a separate `dp_tag_history` table — the audit log
**is** the history. Surface it as `GET /tags/{id}/history` in the
same follow-up that ships the prune command.

---

## 8. Rollout order

1. Migration `0023_tag_sync.sql` — all schema (§4 + §7.1 + §7.2 + §7.3).
   No code paths flipped yet.
2. Wire `kind/key/value/sync_mode/color_source` into `Tag` domain +
   DTO. Backfill verified by a one-shot script asserting
   `count(*) where kind='kv'` matches `count(*) where name like '%:%'`.
3. `GET /tags` filter params + the three `GET /<resource>/{id}/tags`
   convenience routes.
4. Pull-side reconciler: issue labels first, then repo topics.
   Color-source rules (§7.3) live here.
5. Push-side reconciler: issue labels via `dp_issue_mutations`, with
   the coalescing loop from §7.4.
6. `dp_repo_mutations` table + topic push.
7. Conflict UI surfacing in frontend (yellow dot on chips with
   `sync_state='conflict'`).
8. Follow-up: `GET /tags/{id}/history`, `dp-cli admin prune-tags`.

Each step ships independently; the existing tag UI keeps working at
every point because every change is additive.

---

## 9. Other GitHub primitives to leverage

§2–§8 cover the tag ↔ label/topic story. GitHub ships **two more**
typed primitives that already exist on the issues we fetch and
that the workflow surface (triage + projects) currently ignores:
**Issue Types** and **Milestones**. They are not tags — they have
their own grammar, their own GitHub API, and their own UX role —
but they belong in this document because the question "what
classification do we surface on an issue row?" has *one* answer
for the user even though the storage has three. Defining the
boundaries here keeps the chips from collapsing into a single
opaque blob downstream.

### 9.1 The classification primitives, side-by-side

| Primitive       | GitHub source                  | Cardinality on an issue | DP storage today           | Sync direction |
|-----------------|--------------------------------|-------------------------|----------------------------|----------------|
| **Label**       | repo `labels`                  | 0..N                    | `dp_issues.labels` JSONB   | Bidirectional via §2–§8 (tags) |
| **Issue type**  | org-level issue types (GraphQL only — no REST surface lists them) | 0..1                    | *not fetched yet*          | Read-only mirror (no DP-side mutation in v1) |
| **Milestone**   | repo `milestones`              | 0..1                    | *not fetched yet*          | Read-only mirror (no DP-side mutation in v1) |

The asymmetry is deliberate:

- **Labels** are the user's free-form classification — already
  bidirectional in this doc because users *want* to add new ones.
- **Issue types** are an org-admin concept (Bug / Feature / Task,
  defined once per org). Letting DP create them would mean a new
  GitHub App permission and a per-org admin UI; not worth it for
  v1. We **read** them so the triage chip is honest.
- **Milestones** are repo-scoped, due-date-bearing, and already
  modelled in dev-pulse via `dp_projects` + per-issue
  `dp_issue_dates`. The DP project is the **strictly-larger**
  concept (cross-repo, cross-org, polymorphic membership). We
  mirror milestones **in** so a project can adopt one as its
  source of truth for `due_at` if the user wants; we do not
  mirror them **out** because the user already has DP projects
  for the cross-repo case.

### 9.2 What we surface and where

This document defines the **storage and sync** of types and
milestones. The **issue-row chip order** is a triage-surface
concern and lives in the workflow/triage spec — adding a chip
here without updating that spec leaves two sources of truth.
Cross-reference the triage spec when you wire the chips; do not
encode row layout in this document.

Triage rail gains two sections:

- **Types** — one entry per org-defined type (count = open
  issues with that type, viewer-filtered).
- **Milestones** — one entry per *active* milestone (state=open
  on GitHub), grouped by repo, with progress (`closed/total`)
  and due-date relative label. Closed milestones live behind a
  "show closed" toggle.

Project detail page gains a **Milestones** card (§9.5 below) that
lists milestones from any linked repo and lets the user adopt one
as the project's primary milestone.

### 9.3 Storage

One migration, in the next free odd slot per the
`STAGE-1-COORDINATION.md` convention (do not hard-code a number
here — the on-disk migration list is the source of truth, and
concrete numbers in long-lived design docs drift the moment
another branch lands).

```sql
-- Org-level issue types. Refreshed by the fetcher on org tick.
-- GraphQL is the only surface that lists these, so we key on
-- the opaque node id, not the numeric databaseId.
CREATE TABLE dp_issue_types (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id         UUID NOT NULL REFERENCES dp_orgs(id) ON DELETE CASCADE,
    github_node_id TEXT NOT NULL,            -- e.g. "IT_kwDOABCD..."
    name           TEXT NOT NULL,            -- "Bug", "Feature", "Task"
    description    TEXT NULL,
    color          TEXT NULL,                -- semantic palette name (see §7.2)
    is_enabled     BOOLEAN NOT NULL DEFAULT TRUE,
    fetched_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, github_node_id)
);
CREATE INDEX dp_issue_types_org_idx ON dp_issue_types (org_id) WHERE is_enabled;

-- Repo milestones. Refreshed per-repo by the fetcher. The
-- `github_node_id` follows the precedent set by migration
-- `0021_issue_github_node_id.sql` (Projects v2 mirror) — node
-- ids are how every GraphQL join in this codebase reconciles
-- REST-fetched rows with GraphQL-only surfaces.
CREATE TABLE dp_milestones (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    repo_id        UUID NOT NULL REFERENCES dp_repos(id) ON DELETE CASCADE,
    github_number  INTEGER NOT NULL,         -- repo-scoped `number`
    github_node_id TEXT NOT NULL,            -- for Projects v2 / GraphQL joins
    title          TEXT NOT NULL,
    description    TEXT NULL,
    state          TEXT NOT NULL CHECK (state IN ('open', 'closed')),
    -- GitHub's `due_on` is a calendar date, not a timestamp.
    -- Storing it as TIMESTAMPTZ forces a timezone interpretation
    -- ("UTC midnight") that displays as the *previous day* west
    -- of UTC. DATE keeps it tz-agnostic; the §9.5 follow-the-
    -- milestone path doesn't need finer precision.
    due_on         DATE NULL,
    open_issues    INTEGER NOT NULL DEFAULT 0,
    closed_issues  INTEGER NOT NULL DEFAULT 0,
    created_at     TIMESTAMPTZ NOT NULL,
    updated_at     TIMESTAMPTZ NOT NULL,
    closed_at      TIMESTAMPTZ NULL,
    fetched_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- §9.4 N=3 quarantine counter; same shape as `dp_tag_links`
    -- in §4.2. Reset to 0 on any pull that re-observes the row.
    remote_missing_streak INTEGER NOT NULL DEFAULT 0,
    UNIQUE (repo_id, github_number)
);
CREATE INDEX dp_milestones_repo_state_idx ON dp_milestones (repo_id, state);
CREATE INDEX dp_milestones_due_idx ON dp_milestones (due_on) WHERE state = 'open';

-- Per-issue pointers. Both nullable, both populated by the fetcher.
ALTER TABLE dp_issues
    ADD COLUMN issue_type_id UUID NULL REFERENCES dp_issue_types(id) ON DELETE SET NULL,
    ADD COLUMN milestone_id  UUID NULL REFERENCES dp_milestones(id) ON DELETE SET NULL;

CREATE INDEX dp_issues_milestone_idx ON dp_issues (milestone_id) WHERE milestone_id IS NOT NULL;
CREATE INDEX dp_issues_type_idx      ON dp_issues (issue_type_id) WHERE issue_type_id IS NOT NULL;

-- Project ↔ milestone adoption (§9.5). `due_at_overridden`
-- flips to TRUE when the user edits `due_at` directly while a
-- primary milestone is set; the fetcher checks this flag before
-- re-syncing and `[Adopt]` / `[Re-sync]` clears it.
ALTER TABLE dp_projects
    ADD COLUMN primary_milestone_id UUID NULL REFERENCES dp_milestones(id) ON DELETE SET NULL,
    ADD COLUMN due_at_overridden    BOOLEAN NOT NULL DEFAULT FALSE;
```

Why each choice:

- **`issue_type_id` / `milestone_id` as FKs, not denormalised
  text**, because the renames are frequent enough on GitHub
  (especially milestone titles) that a JSONB-of-strings would
  drift fast. The FK forces the fetcher to converge.
- **`ON DELETE SET NULL`** so a GitHub-side deletion of a
  milestone doesn't cascade to issues — the issue keeps its
  history, the chip just disappears.
- **No `dp_projects.milestone_links`** many-to-many table. A
  project adopting *one* milestone is the load-bearing case (§9.5);
  if multi-milestone projects become real, that's a follow-up
  migration with its own scope review.

### 9.4 Fetcher additions

Same worker loop, same pacing primitives as the §5 reconcilers.
The "do not run in parallel with the Projects v2 dates mirror"
constraint from §1 applies here too.

Per-org tick (new):

1. `client.list_issue_types(org)` — GraphQL, returns the org's
   defined types.
2. Upsert into `dp_issue_types`. Disabled types stay in the table
   with `is_enabled = false` so historical issue rows still
   resolve their chip.

Per-repo tick (additions to the existing flow):

1. `client.list_milestones(owner, repo, state=all)` — REST, cheap.
2. Upsert into `dp_milestones`. Milestones absent from the
   response follow the **same N=3 quarantine pattern as §5.1
   step 5**, with the same "complete page set, no 5xx, no
   token-scope 404" guard before the streak counter advances.
   A partial GraphQL response or a downgraded token looks like
   absence too; diverging the quarantine semantics across
   primitives invites silent data loss without buying anything.
   The streak counter for milestones lives on the row as
   `remote_missing_streak INTEGER NOT NULL DEFAULT 0` (same
   shape as `dp_tag_links` in §4.2).
3. Per-issue fetch already returns `milestone` and `type` fields;
   resolve to the local FK by `(repo_id, github_number)` and
   `(org_id, github_id)` respectively. No new API call.

The `client.list_issue_types` and `client.list_milestones`
methods are the only new fetcher surface — the issue body itself
already carries both fields in the payloads we discard today.

### 9.5 Projects ↔ milestones (the "adopt a milestone" affordance)

A DP project's `due_at` (and Meta block) currently live in
`dp_projects` as free-floating timestamps the user types in.
Once milestones are fetched, the project detail page gains:

- A **Milestones** card listing every milestone from any
  `dp_project_repo_links` repo, with `closed/total` progress and
  relative due-date label.
- Each row has an `[Adopt as primary]` button. **Adoption is
  idempotent**: every click of `[Adopt]` re-copies the
  milestone's current `due_on` into the project's `due_at` and
  resumes the follow-the-milestone behaviour. This is the only
  affordance — there is no separate `[Follow again]` action.
  Adopting:
  1. Sets `dp_projects.primary_milestone_id = <milestone.id>`.
  2. Copies the milestone's `due_on` into the project's `due_at`.
  3. From this point on, a fetcher tick that observes a new
     `due_on` on the milestone re-syncs the project's `due_at`.
- **Local override behaviour.** If the user edits `due_at`
  directly on the project while a primary milestone is adopted,
  the override is sticky — fetcher ticks stop overwriting it
  and the Meta block's `Due` cell renders `Apr 12 · overrides
  milestone "v2.4"` with an inline `[Re-sync]` action.
  `[Re-sync]` is functionally identical to `[Adopt as primary]`
  on the same milestone: it re-copies the current `due_on` and
  resumes follow. The user therefore has exactly one verb
  ("adopt this milestone's date, now") whether they're starting
  fresh or recovering from an override.
- The Meta block's `Due` cell renders `Apr 12 · from milestone
  "v2.4"` when adoption is active and the override is not set,
  and the milestone title is a link back to the Milestones card.

How "override is set" is detected: the project carries an
existing `version: int` (CAS token from the issues write path,
§8). A user-initiated `PATCH /projects/{id}` that touches
`due_at` while `primary_milestone_id` is non-null sets a
sibling `due_at_overridden BOOLEAN NOT NULL DEFAULT FALSE`
column. The fetcher checks this flag before re-syncing;
`[Adopt]` / `[Re-sync]` clears it back to `FALSE`. One flag,
one verb, no third state.

Why this shape:

- **One milestone per project, not many.** A project is the
  cross-cutting concept; if a project genuinely spans multiple
  milestones, those milestones each track their own work
  independently and the project's `due_at` is the *latest* of
  them — which is the same answer as "no primary milestone, use
  the user-typed value." Multi-milestone wins nothing here.
- **No outbound writes.** Adopting a milestone is a DP-side
  pointer change; we never `PATCH /repos/{o}/{r}/milestones/{n}`.
  The user changes milestone dates on GitHub (their existing
  workflow), DP follows.
- **Smart view: "Due in current milestone".** Once primary
  milestones exist, the triage rail's `Due this week` smart view
  gains a sibling `Current milestone` that scopes to issues
  whose milestone matches the user's pinned project's primary
  milestone. Cheap once the FK is in place. Edge cases:
  - **No pinned project, or no pin with a primary milestone** —
    the rail entry stays visible but disabled, with hover text
    "Pin a project with an adopted milestone to use this view."
    Clicking is a no-op. Hiding the entry entirely makes the
    feature undiscoverable for new users.
  - **Multiple pinned projects with primary milestones** — the
    view unions the milestones (`milestone_id IN (m1, m2, …)`).
    No picker UI; the user already expressed intent by pinning
    every project they care about. If the union grows past a
    reasonable cap (working assumption: 10 milestones), the
    rail entry surfaces a "(N milestones)" badge so the
    cardinality is honest.

### 9.6 Labels — the existing-data-on-the-floor fix

`dp_issues.labels` is already populated by the fetcher (`JSONB`
array of label name strings) and not surfaced anywhere in the
triage or project UI. This is the cheapest win in this doc.

Three additive changes, all read-only:

1. **`IssueListItem` DTO gains `labels: string[]`.** The column
   is already in the SELECT — just stop discarding it.
2. **Triage row renders up to 3 label chips** between the title
   and the assignees, with `+N more` overflow. Colour is resolved
   from the org-scope tag of matching name (§7.1 — same chip the
   tag surface renders); labels without a corresponding tag row
   fall back to the muted-border default.
3. **Triage rail gains a `Labels` section** mirroring `Saved
   views` (§3 of the existing tagging surface) — one entry per
   org-scope tag of `kind='single'`, count = open issues carrying
   that label, viewer-filtered. Clicking sets the list query to
   `?labels=<name>`.

This **deliberately overlaps** with the §2–§8 tag-as-label sync.
That overlap is fine: §2–§8 makes the *tag entity* the source of
truth for chip metadata (colour, description, scope); §9.6 makes
the *issue's GitHub label string* render as that same chip even
before the §2–§8 push-side reconciler runs. When the user later
edits the chip's colour, §7.3's fan-out propagates to GitHub —
no separate code path.

**Bootstrap sequencing — be honest about what ships when.** On
day one, `dp_tags` carries no rows for any GitHub label, so
every §9.6 chip renders with the muted-border fallback. The
**§5.1 pull reconciler** is what creates the org-scope tag rows
from observed labels and populates their colours. So:

- §9.6 ships the chip-render component and the data-on-the-floor
  fix (labels in `IssueListItem`), with the fallback colour.
- §5.1 ships the colours by populating `dp_tags` rows.

These are **sequenced, not independent**. §9.6 is safe to land
before §5.1 (the fallback colour is honest, not broken), but
"ships behind nothing" was the wrong framing — it ships behind
the §9.9 step 11 ordering. §5.1 must follow for the chips to
look like the design intent.

### 9.7 Filter and group-by axes

Triage's existing group-by axes (`status` / `assignee` / `repo`)
gain three more:

| Axis        | Bucket key                          | Empty-state label              |
|-------------|-------------------------------------|--------------------------------|
| `type`      | `issue_type.name` or `"Untyped"`    | "No issue type assigned"       |
| `milestone` | `milestone.title` or `"No milestone"` | "Not in any milestone"       |
| `label`     | one row per label, issues repeat    | "Unlabelled"                   |

`label` is the only axis where a single issue appears in **more
than one** group — same double-counting semantics as the §7.7
tag-as-report-dimension contract. **Reuse §7.7's chosen UX
pattern verbatim** (the "all orgs combined" de-dup footnote
shape) — do not introduce a second disclosure idiom for the same
problem. If §7.7's pattern is updated, §9.7's footer updates
with it; one source of truth.

Filter params (additive to `GET /issues`):

- `?type_id=<uuid>` — exact match.
- `?milestone_id=<uuid>` — exact match.
- `?labels=bug,p1` — comma-separated; all-of semantics (issue
  must carry every named label).
- `?no_milestone=true` — escape hatch for the "Untyped" bucket
  use case as a flat filter.

**Divergence from GitHub's own URL grammar.** GitHub's issue
search UI uses `+` as the label separator (e.g.
`?labels=bug+p1`); dev-pulse uses `,`. Both are all-of; the
separator is the only difference. We pick `,` for readability
and consistency with the other comma-separated params on
`GET /issues`. **Anyone pasting a GitHub URL will get an
"unknown label" result** because `+` is URL-decoded to space,
not split. Surface this as a soft hint in the filter chip when
the parsed label list contains whitespace.

### 9.8 What stays out of scope

Explicit non-goals so the §9 surface doesn't grow without a
review:

- **No DP-side creation of issue types.** Org admins define them
  on GitHub; DP reads only. If a v1 deployment wants to mass-
  apply a type, they do it on GitHub and the next fetcher tick
  picks it up.
- **No DP-side milestone CRUD.** Same reason — milestone admin
  is a GitHub-side workflow; DP-side creation would need a new
  permission and a per-repo admin UI for one cross-repo concept
  that DP projects already cover.
- **No milestone ↔ tag sync.** A milestone is *not* a tag (it's
  due-date-bearing, repo-scoped, 0..1 per issue). Modelling it
  as a `kv` tag would lose the due date and the progress count.
  They stay separate primitives.
- **No issue-type ↔ tag sync.** An issue type is *not* a label
  (it's org-scoped, 0..1, with its own colour from the org's
  type palette). Mirroring it into a tag would create a chip the
  user can't edit through the tag surface without it diverging
  from GitHub on the next pull. Stays separate.
- **Milestones and issue types are NOT first-class targets for
  `dp_tag_links`.** The `dp_tag_links.kind` CHECK constraint
  stays `IN ('repo', 'issue', 'user', 'team')` — no
  `'milestone'`, no `'issue_type'`. A tag can link to an *issue
  that belongs to a milestone*, but the milestone itself is not
  link-able. Adding either kind to the CHECK would imply
  symmetry between tags and these primitives that this section
  explicitly rejects (different cardinality, different sync
  semantics, different ownership).

### 9.9 Rollout order (extends §8)

Slotting in after the §8 sequence:

9.  Schema migration (next free odd slot per
    `STAGE-1-COORDINATION.md` — do not pick a number until the
    branch lands, see §9.3) — schema only.
10. Fetcher: `list_issue_types` + `list_milestones`, populate
    `dp_issue_types` / `dp_milestones`, resolve per-issue FKs.
11. `IssueListItem` carries `labels`, `type`, `milestone`. Triage
    row renders the three chips in the §9.2 order. **Ships
    behind nothing — purely additive read-side.**
12. Triage rail: Types and Milestones sections; Labels section
    (the §9.6 read-side half of §2–§8).
13. `GET /issues` filter params (`type_id`, `milestone_id`,
    `labels`, `no_milestone`). Group-by axes added.
14. Project detail page: Milestones card + `[Adopt as primary]`.
    `dp_projects.primary_milestone_id` becomes load-bearing for
    the Meta `Due` cell.
15. Smart view: "Current milestone" (depends on §9.5 adoption).

Each step ships independently — the §2–§8 tag-sync surface and
this §9 classification surface share zero code paths and one
shared chip-render component (the Label chip, §9.6 step 2).
