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

One new migration, e.g. `0019_tag_sync.sql` (next odd slot for the
projects-issues track per `STAGE-1-COORDINATION.md`).

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

1. Migration `0019_tag_sync.sql` — all schema (§4 + §7.1 + §7.2 + §7.3).
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
