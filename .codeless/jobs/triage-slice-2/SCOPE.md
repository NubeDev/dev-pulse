# Scope — triage-slice-2

## Goal

Land slice 1.5 (the P0 inbox bugs surfaced in the §0 peer review)
and the full slice 2 of the Triage workbench described in
[linear-projects-idea.md](../../../linear-projects-idea.md), end to
end. After this job, an operator across 4 orgs / 100s of repos /
50+ users can:

- See a **true** "My queue" (identity-set scoped, own writes don't
  mark themselves unread, opening a row doesn't lose a concurrent
  edit).
- Link multiple GitHub accounts to one dp-pulse user (work +
  personal + on-call), with membership reconciliation that **does
  not** silently revoke org access when an identity is unlinked.
- Pivot the rail to **Teams** and **People** instead of just orgs.
- Edit issues from the peek panel through real CAS-bounded
  writes (POST /issues, PATCH /issues/{id}, POST comments) backed
  by octocrab.
- See the cross-source timeline, per-repo sync freshness, the
  Reports surface, and per-issue **start / due dates** mirrored
  to GitHub Projects v2 (best-effort, never blocks the local save).
- Drive all of it from the keyboard (`⌘K`, bulk inbox actions,
  group-by / sort, resizable splitters) without dark-mode glitches.

All slice-2 surfaces in the doc must compile, type-check, build,
and pass the existing test suites by the end of the job.

## In scope

### Backend

1. **Slice 1.5 P0 fixes** (§0 progress log)
   - `dp_issues.external_version BIGINT NOT NULL DEFAULT 0` column
     (migration 0013) backfilled from `version`; reconciler bumps
     `external_version`, §8.2 commit path does **not**.
   - `list_inbox_issues` rewritten to identity-set semantics
     (today's "all open everywhere" bug — §0).
   - `POST /me/inbox/seen` wire shape becomes
     `{ entries: [{ issue_id, version }] }` with server-side
     `LEAST(version_seen, current_external_version)`.

2. **Multi-identity model** (§3.0)
   - Migration 0014: `dp_user_identities` (with `is_primary`
     partial unique index) + `dp_membership_identities` provenance
     join. Backfill from existing `dp_users.github_id`. Plan
     deprecation of `dp_users.github_id` (drop comes in a later
     job — document the migration 0016 step in the handover).
   - `GithubOrgsStamper` stamps **set-shaped**
     `Principal.extra.github.{logins, user_ids, orgs}`.
   - `principal_dirty` table drives re-stamp on
     link/unlink/transfer/primary-change without forced re-login.
   - Empty identity set → refuse to mint principal, return 401
     `identity_set_empty`.
   - Membership reconciler enforces §3.0.2.b invariant.
   - Endpoints (gated on new `identities` resource):
     `GET /me/identities`,
     `POST /me/identities/link/start` (302),
     `GET /me/identities/link/callback` (302, consumes nonce from
       server-side `dp_identity_link_pending` table — `state` is
       never the session id),
     `DELETE /me/identities/{github_user_id}` (refuse `is_primary`
       and last-identity),
     `PATCH /me/identities/{github_user_id}/primary`,
     `POST /admin/identities/{github_user_id}/transfer`,
     `POST /admin/users/{user_id}/identities` (admin link, writes
       audit row that surfaces in the **target user's** own log).
   - `dev-pulse link-identity --user <uuid> --github-login <login>`
     CLI for break-glass.

3. **Issue write handlers** (§5.3 — helpers exist, no router)
   - `issues_write_router` mounts `POST /issues`, `PATCH /issues/{id}`,
     `POST /issues/{id}/comments` through
     `acquire_issue_mutation_slot` → octocrab → `commit_*` /
     `rollback_*`. Gated on `(issues, write)`. 409
     `stale_local_version` on CAS miss per §8.3.
   - Integration tests: happy path, CAS miss, GitHub failure
     rollback (the `dp_pending_remote_webhook_buffer` replay path
     must still drain on the rollback edge).

4. **Read endpoints** (§5.4 / §5.6 / §5.9 / §5.10)
   - `GET /issues/{id}/timeline` backed by the §6 guarded
     expression index on `dp_activity_events`.
   - `GET /repos/{id}/sync-status` + `POST /repos/{id}/sync` with
     new `(repos, sync)` auth pair.
   - `GET /reports/issues` with the corrected §5.10 SQL shapes
     (`CROSS JOIN LATERAL jsonb_array_elements_text` for `wip`,
     `jsonb_array_length(...) = 0` for `untriaged`,
     `EXTRACT(EPOCH FROM ...)` for `lead_time`).
   - `/me/queue` gets the keyset cursor on `(updated_at, id)` +
     per-arm `LIMIT $cap` push-down + the new covering index
     `dp_issues_updated_at_idx`.

5. **Dates on issues** (§3.10)
   - Migration 0015: `dp_issue_dates` (PK `issue_id`, both dates
     nullable, `CHECK (start_date IS NULL OR due_date IS NULL OR
     start_date <= due_date)`, mirror provenance columns) and
     optional `dp_repo_project_link`.
   - `PATCH /issues/{id}/dates { start_date?, due_date? }` —
     local upsert is **synchronous**; mirror push enqueued as a
     best-effort task that runs `addProjectV2ItemById` +
     `updateProjectV2ItemFieldValue` GraphQL and writes any
     failure to `mirror_error`. Never blocks the local save.
   - Stub task type for the §3.10 **slice-3 pull-back**; do not
     implement the pull yet.
   - `Due this week` and `Overdue` smart views in `/me/queue`;
     past-due re-bump to `dp_user_issue_state.status = 'inbox'`.

6. **Bulk inbox + audit + OpenAPI** (§3.8 / §5)
   - `POST /me/inbox/bulk { issue_ids, status?, snoozed_until? }`.
   - `dp_user_issue_state` BEFORE UPDATE trigger from §6 actually
     shipped (today the column only sets `updated_at` at INSERT).
   - Register **every** new and previously-omitted handler on
     `DevPulseApi` (`me_queue`, inbox seen/patch/bulk, identities,
     write verbs, timeline, sync-status, reports, dates).
   - Audit vocabulary extended for `IDENTITY_LINKED`,
     `IDENTITY_UNLINKED`, `IDENTITY_PRIMARY_CHANGED`,
     `IDENTITY_TRANSFERRED`, `IDENTITY_CLAIM_CONFLICT`,
     `IDENTITY_ADMIN_LINKED`, `DATE_SET`, `REPO_SYNC_REQUESTED`,
     `BULK_INBOX_SEEN`, `BULK_INBOX_STATE_CHANGED`.

### Frontend

7. **Identity manager** at `#/account/identities` — list, link
   (kicks off OAuth), unlink (with last-identity guard surfaced),
   set-primary, admin-transfer (admin-only). User-menu badge shows
   the active identity set.

8. **Triage rail extensions** (§3.2) — **Teams** and **People**
   entries between Tags and Orgs; People collapsed by default with
   "show all" reveal to respect the §13.5 sidebar render cap;
   Untriaged scoped to `dp_memberships` orgs with the 24h age
   floor; saved views from tags (with counts) per §14.6 slice 3.

9. **Inbox UX completion** — bulk select with `x`, range select
   with shift-click, bulk actions (`e` done-all, `h` snooze-all,
   shift-`E` mark-all-read on visible), snoozed view backed by
   `/me/queue?status=snoozed`, "wake now" affordance.

10. **List pane** — group-by dropdown (`none | repo | assignee |
    label | milestone | state`) with sticky headers + per-group
    sync-freshness badge from `/repos/{id}/sync-status`; sort
    dropdown (`updated_at | created_at | number | assignee |
    repo`); resizable splitters between the three panes (state
    persisted in `localStorage`).

11. **Dates UI** (§3.10) — `start` / `due` date pickers in
    `IssueEditCard`; `Due` column in the list (toggle `g d`);
    `Due this week` + `Overdue` rail entries.

12. **⌘K palette** (§14.5 / §3.5) — jump-to (issue number, repo
    slug, org login, tag name, user login from caller-visible
    users only), view-switch, apply-to-selection (assign, label,
    state). Each apply fans out §8.2 CAS calls with a 4-6
    concurrency cap; 409s requeue with refreshed
    `expected_version`.

13. **Polish** — dark-mode pass on every new surface;
    `PinDto.label` either denormalized server-side (preferred) or
    client-side joined against the repo list so the rail stops
    showing `target_id.slice(0,8)`.

### Tech debt

14. Triage the pre-existing `dp-fetcher` test failures noted in
    §0. Fix in-place or open a tracking issue + quarantine. CI on
    `main` must be green by job end.

## Out of scope

- **Projects v2 pull-back** (§3.10 slice-3) — write-only mirror in
  this job; pull task is a stub.
- **`dp-users.github_id` column drop** — backfill + deprecate now;
  the drop migration is the next job's first step.
- **Saved-view CRUD as a separate object** (§4.2 in the peer
  review) — tag-backed saved views only; first-class object stays
  deferred.
- **SLA / escalation surface, daily digest, on-call rotation
  handoff** (§4.4 / §4.6 / §4.7 in the peer review) — deferred
  with explicit reasoning in the slice-3 brief.
- **Mentions ingestion + `dp_issue_mentions` backfill** — that's
  slice 3 in the original plan; the projection table can be
  created (migration 0012 is already shipped) but the fetcher
  hook stays untouched.
- **Mobile layout** — §13.8 desktop-only stays.
- **Real-time push (SSE/WebSocket)** — refetch-on-focus only.

## Constraints

- **CLAUDE.md applies in full.** R1–R5 hold; no `--no-verify`, no
  `--force`, no skipping the closing trio.
- **Migrations are forward-only** and numbered sequentially from
  the current head (0012). Every migration ships with the SQL the
  doc specifies — including the §6 partial indexes, the
  `BEFORE UPDATE` trigger on `dp_user_issue_state`, the
  `external_version` backfill, and the membership-provenance join.
- **Octocrab calls** route through the existing GitHub client
  abstraction in `crates/dp-fetcher/src/client/`. Do not add a
  second HTTP client. GraphQL for Projects v2 uses the same
  client's GraphQL surface; if missing, add it alongside REST.
- **No new top-level dependencies** other than
  `@tanstack/react-virtual` (already greenlit in §7.4). The
  charting lib for the Reports page can wait for slice 3 —
  render Reports as tables only in this job.
- **OpenAPI registration is non-negotiable.** Every handler this
  job ships **and** every previously-omitted handler (e.g.
  `list_issues`, `me_queue`) lands on `DevPulseApi` before
  REVIEW 2.
- **Wire-shape changes are additive when possible.** The
  `POST /me/inbox/seen` change is the one exception (the current
  shape is racy); call it out in the slice-3 brief so anyone
  pinning to the old shape gets a heads-up.
- **`policy/dev-pulse.toml` resource registry** must list
  `identities` (read+write), `repos` (existing + new `sync`),
  before any router referencing them mounts. Skipping this is the
  same `unknown_resource` bug the slice-1 issues / tags omission
  caused.
- **Tests run after every stage.** `cargo test -p <crate>` for
  every crate touched in the stage, plus `pnpm typecheck` and
  `make build` after any frontend stage. The closing trio's
  `checks` step is where these go.

## Open questions

1. **`dp_users.github_id` deprecation timing.** Backfill in 0014
   then drop in a follow-up migration (next job) — does the
   intermediate state (column still present, every read going
   through `dp_user_identities WHERE is_primary`) need a
   compatibility shim? Resolve in stage 2.
2. **Projects v2 ID discovery.** `dp_repo_project_link` requires
   `gh_project_id` + the two field IDs up front. Do we surface a
   "Link to project" admin flow now, or document that operators
   seed the table by hand for this job and the admin flow comes
   with the pull-back work in slice 3? Resolve in stage 7.
3. **`(repos, sync)` policy default.** Should this be open to any
   org member, or restricted to admins on the repo? Default to
   "any caller with `(repos, read)`" and call it out in REVIEW 1
   for the operator to confirm.
4. **Identity transfer audit.** Does the source user see the
   audit row (their identity vanished without their action)?
   Default: yes, and require a `reason` field on the transfer
   payload that lands in the audit. Confirm in stage 3.

Anything else surfaces in the stage's handover, not silently.
