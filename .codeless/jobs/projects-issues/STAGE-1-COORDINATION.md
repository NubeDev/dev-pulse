# Stage 1 — Envelope-additivity coordination with `codeless/org-leaderboard`

> Pinned coordination note for the `projects-issues` job.
> Required deliverable of WORKFLOW.md stage 1. Names which branch
> lands first, the exact §15.6 fields each job adds, the
> migration-numbering convention, and the dashboard-01 shell
> files both jobs may touch.

## TL;DR

1. **`codeless/org-leaderboard` lands first.** It is further along
   (stage 1 + 2 closed, stage-3 scaffold imminent) and its
   §15.6 changes are smaller and additive-only. `projects-issues`
   rebases off `main` after the leaderboard merge and inherits the
   `repos: Vec<RepoId>` field instead of adding it.
2. **No structural overlap in §15.6** — both jobs add only new
   optional `Vec<_>` fields and obey the §15.6 "additive, never
   repurpose" revisit rule.
3. **Migrations split odd/even** off the current head `0003_*`:
   leaderboard takes the next **even** numbers (`0004_*`,
   `0006_*`, …), projects-issues takes the next **odd** numbers
   (`0005_*`, `0007_*`, …). Whoever merges second renumbers
   *only its own* migrations forward by one if a collision exists
   (i.e. shift its `0005_*` → `0007_*`); never edit the other
   job's filenames.
4. **Dashboard-01 shell collisions are limited to two files**:
   `frontend/src/components/app-sidebar.tsx` and
   `frontend/src/routes.ts`. Both jobs append new route /
   sidebar items; neither edits existing ones. Merge conflicts
   are mechanical.

## 1. Which branch lands first

Snapshot at `2026-05-20` (this branch's HEAD = `49bbca1`,
`codeless/org-leaderboard` HEAD = `e89bab6`):

- `codeless/org-leaderboard` has closed stage 1
  (`STAGE-1-COMPOSABILITY.md`) and the stage-2 REVIEW gate, and is
  scoped to start stage 3 next session (scaffold
  `LeaderboardEnvelope` for `subject = user`, single-org). Its
  §15.6 touch is the **smaller** of the two (one inherited field,
  one separate envelope type — see §2 below).
- `codeless/projects-issues` (this branch) has only stage 0 + 1
  scaffolded. Its §15.6 touch is **larger** (two new fields on
  `ReportEnvelope`, one new `GroupBy::Tag` variant, a metric ×
  link-kind table, and a new `empty_reason` value).

Bigger surface waits for smaller surface. **Order: leaderboard
merges first, projects-issues rebases off main, then both
promotion stages (leaderboard stage 11, projects-issues stage 13)
sequence so SCOPE.md is rewritten exactly once per merge.**

If the leaderboard branch is still open when `projects-issues`
reaches stage 6 (the §15.6 stage), `projects-issues` stage 6 adds
its fields directly to `ReportEnvelope` without waiting; the
leaderboard branch then rebases its `LeaderboardEnvelope` field
list (which already names `repos`) onto the now-existing
`ReportEnvelope.repos` field — no code change, just a
documentation reconciliation in its envelope comment block.

## 2. Exact §15.6 fields each job adds, by name

### `codeless/org-leaderboard` adds

Per ORG-REPORTS.md §3 (lines 66–101), leaderboard introduces a
**separate** `LeaderboardEnvelope` struct that **conceptually
extends** §15.6 but does not modify the `ReportEnvelope` struct
itself. The fields it names:

- inherited from §15.6 (read-only references, no edits required):
  `window`, `org_scope` (= `ScopeMode` per §15.6), `repos`
  (Option<Vec<RepoId>> — see overlap note below), `teams`,
  `actor_roles`, `tz` (part of `Window` in §15.6).
- new on `LeaderboardEnvelope` (not on `ReportEnvelope`):
  - `subject: SubjectKind`
  - `rank_by: MetricId`
  - `also_compute: Option<Vec<MetricId>>`
  - `subject_ids: Option<Vec<SubjectId>>`
  - `include_bots: bool`
  - `page: PageRequest`

The only place leaderboard work touches `ReportEnvelope` directly
is **the `repos` field**, which it lists as "inherited from
§15.6" — but §15.6 does not currently carry `repos`. So whichever
branch lands first defines `ReportEnvelope.repos` (see §3 below
for who).

### `codeless/projects-issues` adds

Per SCOPE-PROJECTS.md §7.7 (lines 397–444), projects-issues makes
three additive changes to `ReportEnvelope` itself:

- `tags: Vec<TagId>` — new optional filter field. Empty = no tag
  filter. Union semantics, **never** widens `users` / `teams` /
  `orgs`.
- `repos: Vec<RepoId>` — new optional filter field. Required to
  land *with or before* `tags`, because tag links of kind `repo`
  cannot otherwise be expressed.
- `group_by: Vec<GroupBy>` gains a `Tag` variant (the enum
  itself, not the field). When `GroupBy::Tag` is present, `tags`
  is required and capped at 50.

Plus one response-level addition (not on the request envelope):

- a new `empty_reason` value `"tag links do not match metric
  attribution"` for the issue-only-tag-on-commit-metric case.

### Overlap and resolution

Both jobs want a `repos: Vec<RepoId>` (or `Option<Vec<RepoId>>`)
field on `ReportEnvelope`.

- **Field name**: `repos` (matches the existing `orgs` / `users` /
  `teams` naming convention in §15.6).
- **Type**: `Vec<RepoId>` (not `Option<Vec<…>>`) — matches the
  existing convention "empty = no filter" used by every other
  list field in §15.6. The leaderboard branch's `Option<Vec<_>>`
  spelling is a doc-only artefact in ORG-REPORTS.md §3; the Rust
  struct it lands should use `Vec<RepoId>` for consistency.
- **Owner**: whichever job's §15.6 stage hits trunk first. Per
  §1, that is **leaderboard stage 3**. `projects-issues` stage 6
  then asserts the field exists and adds only `tags` + the
  `GroupBy::Tag` variant + the `empty_reason` value.
- **Fallback**: if `projects-issues` stage 6 runs before
  leaderboard stage 3 has merged, `projects-issues` adds `repos`
  itself; leaderboard's later rebase finds the field already
  present and re-points its `LeaderboardEnvelope` doc-block at
  the now-existing `ReportEnvelope.repos`.

No other §15.6 field names collide. No existing §15.6 field
changes meaning under either job. The §15.6 "additive, never
repurpose" revisit rule (SCOPE.md lines 429–432) is honoured by
both.

## 3. Migration-numbering convention

Current head of `crates/dp-store-pg/migrations/dp/` is `0003_*`
(`0001_init.sql`, `0002_merge_synthetic_duplicate_users.sql`,
`0003_smart_merge_duplicate_users.sql`).

Convention for the next migrations from these two jobs:

- **`codeless/org-leaderboard` takes the next *even* slots**:
  `0004_*`, `0006_*`, … (leaderboard's current plan does not call
  out a migration explicitly; if its stage-3 scaffold needs one
  it claims `0004`, otherwise it leaves `0004` reserved and the
  even-numbering shifts only if it ever uses one).
- **`codeless/projects-issues` takes the next *odd* slots**:
  `0005_*` (pins + tags + tag_links per stage 3), `0007_*`
  (issues write-path columns per stage 9), `0009_*` if a third is
  needed.

Why odd/even (and not a contiguous range per job): the schema
diffs are independent and we want either branch to be mergeable
without touching the other's migration list. If both branches
race a `0004_*` filename, the second-to-merge branch shifts
**its own** migration forward by one (its `0005_*` → `0007_*`),
keeping its filenames contiguous within its own numbering family,
and updates its own `sqlx`/`include_str!` registration accordingly.

Concretely, the file names this job is committed to:

- `0005_user_pins_tags_tag_links.sql` — stage 3 (pins + tags +
  tag_links per §6.3 / §7.2; includes the per-scope expression
  unique index on `lower(name)` and the polymorphic CHECK
  constraints from SCOPE-PROJECTS.md §7.2).
- `0007_issues_optimistic_cas.sql` — stage 9 (`issues.version`,
  `pending_remote`, `pending_remote_at`, `pending_remote_actor`
  per §8.2).

Leaderboard's slots stay open at `0004`, `0006`.

## 4. Dashboard-01 shell files both jobs may touch

Inventory of the dashboard-01 shell (`frontend/src/`):

- `routes.ts` — top-level route registry.
- `layout/app-shell.tsx` — shell frame.
- `components/app-sidebar.tsx` — left nav.
- `components/nav-main.tsx` — main-nav item rendering.
- `components/site-header.tsx` — top header.

What each job touches:

| File                                       | leaderboard | projects-issues | nature of edit                                      |
|--------------------------------------------|-------------|-----------------|-----------------------------------------------------|
| `frontend/src/routes.ts`                   | **yes**     | **yes**         | append new routes; do not edit existing             |
| `frontend/src/components/app-sidebar.tsx`  | **yes**     | **yes**         | append new sidebar entries; do not edit existing    |
| `frontend/src/components/nav-main.tsx`     | unlikely    | unlikely        | only edit if the existing nav primitive needs new affordances; if it does, the *first* job to need them owns the edit and the second job consumes the primitive |
| `frontend/src/layout/app-shell.tsx`        | no          | no              | no change expected                                  |
| `frontend/src/components/site-header.tsx`  | no          | no              | no change expected                                  |

Concretely, the two **expected** collision files are `routes.ts`
and `app-sidebar.tsx`, and the collisions are mechanical (both
jobs append entries; conflict markers resolve by keeping both).

Project-specific new surfaces:

- **projects-issues** stage 11 adds: Pin sidebar (in
  `app-sidebar.tsx`, above existing nav per §6.1 render cap), Tag
  manager page (new route in `routes.ts`, new directory under
  `frontend/src/` — naming TBD at stage 11, not in the shell),
  Issue CRUD forms (consumed from inside report pages and the new
  tag manager — no new top-level route).
- **leaderboard** frontend (not in its current stage list, but
  implied by ORG-REPORTS.md §5 / §6.9): a leaderboard page and a
  `my_standing` page — both new routes, both new sidebar entries.

Coordination rule: when editing `routes.ts` or `app-sidebar.tsx`,
**append, never re-order**, and group your additions in a single
contiguous block so the merge conflict region is one hunk per
file.

## 5. What this stage does NOT decide

- The §12 open questions in SCOPE-PROJECTS.md (pin cap, tag name
  charset, cross-scope promotion, webhook-vs-fetcher race,
  Projects v2 import) — those are the stage-2 REVIEW gate.
- Whether the leaderboard `repos` field is `Vec<RepoId>` vs
  `Option<Vec<RepoId>>` *in the Rust struct that lands* — §2
  states a preference (`Vec<RepoId>`, matches existing convention)
  but the leaderboard branch owns the final call when its stage 3
  lands. Either choice is compatible with projects-issues' §7.7
  semantics.
- Migration **content** — only filenames are reserved here.
- Frontend file names for the Pin sidebar component, Tag manager
  page, or Issue CRUD forms — those are stage-11 decisions; this
  note only enumerates the shared shell files.

## 6. References

- `SCOPE.md` §15.6 (lines 394–438) — current `ReportEnvelope`.
- `SCOPE-PROJECTS.md` §7.7 (lines 397–444) — projects-issues
  envelope additions.
- `SCOPE-PROJECTS.md` §13.1–§13.7 (lines 651–800) — decisions to
  be locked.
- `ORG-REPORTS.md` §3 (lines 66–101) — leaderboard envelope.
- `ORG-REPORTS.md` §6.1–§6.10 — leaderboard decisions.
- `.codeless/jobs/org-leaderboard/STAGE-1-COMPOSABILITY.md` —
  prior stage-1 note from the other branch.
- `crates/dp-store-pg/migrations/dp/` — current migration head.
- `frontend/src/{routes.ts,layout,components}` — dashboard-01
  shell inventory.
