# Scope — projects-issues

> Full design rationale lives in
> [`SCOPE-PROJECTS.md`](../../../SCOPE-PROJECTS.md) at the repo root.
> This file is the trimmed brief; do not duplicate the design — point
> at it.

## Goal

Ship the workflow half of dev-pulse: per-user pinned repos and tags,
home-grown cross-org project tags (with tags-as-a-reports-dimension),
and synchronous user-initiated GitHub Issues CRUD on an
optimistic-CAS write path with a reconciler guard. This is the
**action surface** — the first user-initiated writes from dev-pulse
to GitHub. Reports stay read-only and fetcher-backed.

## In scope

- **Pins** (§6) — `user_pins` table, REST `/me/pins`, sidebar
  quick-list, default repo filter, audit verbs, pin cap from
  `dp-config` (§13.5).
- **Tags** (§7) — `tags` + `tag_links` polymorphic over
  repo/issue/user/team, per-scope case-insensitive uniqueness,
  archive (no hard delete), viewer-filtered link counts (§7.4),
  batch transactional link/unlink, audit verbs.
- **Tags as a reports dimension** (§7.7) — additive `tags` and
  `repos` fields on `ReportEnvelope` (§15.6) following its
  "additive, never repurpose" rule; `GroupBy::Tag` capped at 50
  with required filter; the metric × link-kind mapping table;
  `empty_reason` for the issue-only-tag-on-commit-metric case.
- **Issue CRUD** (§8) — create / update / close / reopen / comment,
  per-issue only, with the §8.2 optimistic-CAS-on-`version` write
  path (no row-lock across the GitHub round-trip), the §8.3
  conflict cases, the §8.4 writes-not-available affordance, the
  §8.5 audit log incl. `pending_remote_timeout`.
- **Reconciler guard** (§13.7) — fetcher / webhook reconciler must
  not overwrite a row with `pending_remote = true` younger than
  `issues.pending_remote_timeout_secs`; webhook payloads buffered
  and replayed.
- **App permission bump** (§13.6) — `issues: write` becomes the
  default scope behind a `dp-config` flag; one-shot migration
  banner; §8.4 steady-state affordance.
- **Frontend wiring** — pin sidebar with overflow, tag manager
  respecting §7.4 default scope, Issue CRUD forms with the §8.3
  stale-version UX, writes-not-available banner.
- **Promotion** — fold SCOPE-PROJECTS.md into SCOPE.md as new §
  sections (§6/§7/§8/§9 mirrored) and a §15.x Decisions block
  carrying §13.1–§13.7.

## Out of scope

- Bulk issue mutations (§4).
- PR mutations, discussions, reactions, attachments (§4).
- Label / milestone administration (§4).
- Draft issues, issue templates, @-mention autocomplete (§4).
- GitHub Projects v2 as system of record (§13.1); one-way Projects
  import is a §3 secondary goal, deferred.
- MCP mutations (§13.3) — MCP stays read-only in v1; delegated
  mutation needs its own scope doc covering the principal model.
- Tag-driven Gantt (`TagSchedule`) — §3 secondary, schema sketch
  only, no UI.
- Cross-scope tag promotion (user → team/org) — §12 open question.
- Notification / digest emails on tag activity.
- Mobile UX for the create-issue form (§12).

## Constraints

- Reuse SCOPE.md primitives: §15.1 GitHub App, §15.4 octocrab
  rate-limit wrapper, §15.6 envelope (extended additively only),
  §15.10 operator OAuth, §15.11 access gate, §15.13 audit
  vocabulary. Do **not** invent parallel versions.
- **Write authority delegates through the App install**, not the
  user's OAuth token (§9). Revoking a user inside dev-pulse
  revokes their ability to mutate GitHub.
- **No write originates from the fetcher.** Every GitHub write has
  a `dp-rest` request id and an actor in the audit log (§11.6).
- **Optimistic concurrency uses `issues.version` as the CAS
  token.** No DB row-lock held across the GitHub round-trip
  (§8.2 step 5). The Issues REST API does not support `If-Match` /
  `If-Unmodified-Since`; the local CAS is the only guard.
- **The reconciler defers to in-flight optimistic writes** for
  the timeout window (§13.7). Webhook payloads for such rows are
  buffered, not applied.
- **Tags are not GitHub labels.** Labels stay in GitHub; tags are
  dev-pulse-side metadata that can span repos and orgs (§4).
- **Tag link counts in API responses are viewer-filtered** (§7.4)
  — reporting true counts would leak existence of inaccessible
  targets.
- **`color` on tags is a semantic palette name**, not a frontend
  design-token id — decouples stored rows from token churn.
- **Migration numbering** must be coordinated with the in-flight
  `org-leaderboard` worktree (stage 1 deliverable). Both jobs
  may want sequential numbers above `0002_*`.
- **§15.6 envelope additions** must be coordinated with the
  in-flight `org-leaderboard` worktree — both jobs add fields to
  the same struct, both follow the "additive, never repurpose"
  rule, second-to-land rebases.

## Open questions

The §12 open questions in SCOPE-PROJECTS.md, in stage 2 priority
order:

1. **Pin cap** — working assumption 20 per user (§6.1 / §13.5).
   Validate or pick a number before stage 4.
2. **Tag name length / character set** — emoji yes/no? UI render
   constraints? Decide before stage 5.
3. **Cross-scope tag promotion** — can a user-scope tag be
   promoted to team or org scope? Probably yes but interacts
   with `tag_links` ownership. Decide before stage 5 *or* close
   as out-of-scope for v1.
4. **Issue webhook latency under load** — does §8.2 step 7
   reliably win the race against a fetcher tick? Affects whether
   §13.7's buffer-and-replay is enough or whether a polling
   fallback is needed. Decide before stage 9.
5. **Projects v2 import** — `tag_links` direct vs side-table;
   §13.1 revisit trigger. Closed as deferred unless stage 1
   reveals a deployment that demands it.
6. **Mobile** — closed as out-of-scope for create-issue; pins and
   quick-comment are nice-to-have, not gating.
