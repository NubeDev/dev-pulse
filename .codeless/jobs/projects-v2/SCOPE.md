# Scope — projects-v2

> The full design lives in [linear-projects-v2.md](../../../linear-projects-v2.md).
> Companion docs: [linear-projects-idea.md](../../../linear-projects-idea.md)
> (triage workbench + §3.10 mirror this work replaces),
> [SCOPE-PROJECTS.md](../../../SCOPE-PROJECTS.md) (CAS contract,
> audit, permission pairs). This file is the trimmed brief; read
> all three linked docs before stage 1.

## Goal

Land a first-class Projects surface in dev-pulse, owned by us,
optionally mirroring to GitHub Projects v2 boards. Replace the
§3.10 admin pane (per-repo board link, node-id paste field) with
a project-scoped surface accessed from the main sidebar and the
workflow detail pane. End state: a user can create a project,
add 10 issues from triage, edit a date on one of them, and see
the date appear on every linked GitHub board within ~5 seconds
— without ever pasting a `PVT_…` node id.

## In scope

- **Storage** (linear-projects-v2.md §5, §8):
  - Migration 0022 — `dp_projects` + `dp_project_issues`.
  - Migration 0023 — `dp_project_board_links`.
  - Migration 0024 — `DROP TABLE dp_repo_project_link` (per §11
    of linear-projects-v2.md — never shipped to a real
    deployment, no data preservation needed).
  - Domain types: `Project`, `ProjectUpsert`, `BoardLink`,
    `BoardLinkUpsert`. CAS via `version` column on
    `dp_projects` (mirrors §8.2 of SCOPE-PROJECTS.md).
  - `Store` trait methods: project CRUD, membership add/remove
    (including bulk add), board-link CRUD, mirror status
    accessors. All with pg impls + unit tests.

- **REST surface** (linear-projects-v2.md §7):
  - §7.1 project CRUD: list / get / create / patch / archive.
  - §7.2 membership: list issues in project, bulk add, single
    delete, "what's this issue's project?" lookup.
  - §7.3 board picker + link CRUD: org-scoped
    `GET /orgs/{org_id}/projects-v2`, plus CRUD on
    `/projects/{id}/board-links`.
  - All routes gated on the new `(projects, read|write)` pairs
    (linear-projects-v2.md §9.1). Defaults mirror
    `(issues, ·)`.
  - Audit verbs per linear-projects-v2.md §9.3:
    `project.{create,update,archive}`,
    `project.issue.{add,remove}`,
    `project.board.{link,unlink}`.
  - OpenAPI registration in `crates/dp-rest/src/openapi.rs`;
    regenerate the snapshot at the end of the job.

- **GitHub Projects v2 mirror fan-out** (linear-projects-v2.md
  §7.4, §11):
  - Keep every `gh_projectv2_*` method in
    `crates/dp-fetcher/src/client/mod.rs` unchanged.
  - Add `gh_list_org_projectv2` analogous to today's
    `gh_list_repo_projectv2` (called by the new picker
    endpoint).
  - Rewire `OctocrabProjectV2Mirror::mirror_dates` to fan out
    over **all** `dp_project_board_links` rows for the issue's
    project, writing to each. Failures land per-link on
    `last_mirror_error`; the existing
    `dp_issue_dates.mirror_synced_at` / `mirror_error`
    columns carry the most recent outcome across all links.
  - The §3.10 lazy node-id resolve path (`IssueNodeIdRef`,
    migration 0021, `set_issue_github_node_id`) is unchanged.

- **Frontend slice A** (linear-projects-v2.md §6):
  - New sidebar section between Workflow and Directory with
    sub-items `Active (N)`, `Backlog (N)`, `Done`. Counts come
    from `GET /projects?status=&count_only=1`.
  - `#/projects` list page (§6.2): table with name / status /
    progress bar / due date / lead / org; `[+ New project]`
    modal (name, description, lead).
  - `#/projects/{id}` detail page (§6.3): header, description
    (markdown), meta block (start / due / lead / % closed),
    linked boards section (slice B fills this), issue list
    using the existing triage row component filtered to
    `project_id`.
  - Workflow detail-pane integration (§6.5): a Project chip
    above the dates editor — `[+ Add to project]` when none,
    or `● <name>   [Change…] [Remove]` when set. Quick-pick
    autocomplete over the issue's org's active projects.
  - Bulk add from triage (§6.6): the existing
    selection-checkbox toolbar gains `[Add to project ▾]`.
    Single endpoint call; render per-row outcomes.

- **Frontend slice B** (linear-projects-v2.md §6.4, §9.4):
  - Link-a-board dialog launched from the project detail page.
    Board dropdown sourced from
    `GET /orgs/{org_id}/projects-v2`; field dropdowns from
    the selected board's `dateFields`. **No node-id paste
    field on this dialog** — failures show a helpful error
    plus `[Open GitHub project settings]` link.
  - Per-link mirror status surfaced on the project detail
    page: `Last sync: 14:23:07 ✓` or
    `Last sync: failed — <message>` plus `[Re-link]`.
  - Move the existing admin pane from `#/admin/projects` to
    `#/admin/project-sync` and rename the sidebar entry to
    `Admin · Project sync`. It becomes the escape hatch only;
    paste-node-id fallback stays here.

- **Cleanup**:
  - Zero references to `dp_repo_project_link` /
    `repo_project_link_router` / `RepoProjectLinkDto` /
    `PutRepoProjectLinkRequest` in the codebase outside
    migration 0024.
  - `cargo test --workspace` green; UPDATE_OPENAPI_SNAPSHOT
    regen; `pnpm exec tsc --noEmit` clean.
  - Append a progress-log entry to linear-projects-v2.md §0
    summarising what landed and what was deferred.

## Out of scope

- **Two-way mirror.** GitHub-side date edits don't pull back
  into dev-pulse. Stub task type can stay where it is; no
  webhook subscription, no projection table.
- **Project templates** / **automation rules** /
  **milestones** / **timeline (Gantt) view** /
  **project-level dependencies** — all listed in §4 of
  linear-projects-v2.md, all deferred.
- **Cross-org projects.** `dp_projects.org_id` is NOT NULL; one
  project belongs to exactly one org.
- **Reports `project_id` dimension** (§10) — slice C, deferred
  to a follow-up job.
- **Search-by-name on `#/projects`** — slice C.
- **Quick-pick autocomplete in `[+ Add to project]`** — slice C
  (plain dropdown is fine for v1).
- Any `starter-*` edits. If a starter API is missing, compose
  around it in dev-pulse or stop the stage with a `[!]` and
  surface it at the next REVIEW.

## Constraints

- **MSRV 1.78**, `cargo clippy --workspace --all-targets -D
  warnings`, `cargo fmt --check`, `cargo test --workspace`,
  `./scripts/check-boundaries.sh` all green before any stage
  commits. Frontend: `pnpm exec tsc --noEmit` clean.
- **CAS contract** (SCOPE-PROJECTS.md §8.2): every project
  PATCH carries `expected_version`; mismatch returns
  `409 stale_local_version` with `{ current_version }` body.
  Same shape as `PATCH /issues/{id}`.
- **Audit log parity** with §8.5 of SCOPE-PROJECTS.md. Bulk
  add lands one audit row per issue for transparency.
- **Permission pairs**: add `(projects, read)` and
  `(projects, write)` to the policy engine; defaults mirror
  `(issues, read|write)`.
- **No node-id paste field on the primary `Link a board`
  dialog.** If the GraphQL picker fails (no `project` scope,
  GitHub 5xx, no boards), show a helpful error with a link
  out — never a paste box. The paste box lives only on the
  retired admin page at `#/admin/project-sync`.
- **No two-way sync.** The checkbox in the dialog mockup
  (linear-projects-v2.md §6.4) is rendered but disabled with
  "coming soon" copy.
- **No `--force`, no `--no-verify`** — see
  [../../../../codeless-workspace/CLAUDE.md](../../../../codeless-workspace/CLAUDE.md)
  and ADDING-JOB.md Hard rule 5. If a hook fails, fix the
  cause.
- **Naming**: the user-facing surface is `Projects` (top-level
  sidebar). The GitHub-side concept is `GitHub board` or
  `Projects v2 board` in copy. The DB column for the board
  node id is `github_board_node_id` (not `project_node_id`)
  to remove the naming collision.

## Open questions (resolve at the first REVIEW gate)

1. **Issue belongs to many projects?** (§14.4 of
   linear-projects-v2.md). Storage allows it (composite PK
   `(project_id, issue_id)`). Recommend: **allow many in
   storage; show only the most-recently-added on the detail
   pane chip, with a `+N more` pill that expands to all**.
   Decision needed before slice A's frontend stage so the
   chip component can be specced.
2. **`dp_projects.name` uniqueness scope.** §5 says `UNIQUE
   (org_id, lower(name))`. Confirm that's what we want
   (rather than globally unique, which would conflict with
   cross-org duplicate names like "Q3 stability"). Default:
   per-org unique.
3. **Status enum vs lookup table.** Use a `CHECK
   (status IN ('active','backlog','done','archived'))`
   constraint plus a `text` column — same pattern as
   `event_actors.role` in `bootstrap-domain-store`. Pg enums
   are heavier and harder to migrate. Confirm at REVIEW.
4. **Lead user immutable?** No — lead can change. `created_by`
   is immutable. Confirm at REVIEW (matches §9.2).
5. **Counts in sidebar — separate endpoint or query param?**
   §6.1 says `count_only=1` query param. Confirm at REVIEW;
   alternative is `GET /projects/counts`.

Default for every open question is the answer noted above —
the REVIEW gate exists to challenge them, not to find new
options.

## Acceptance checklist

Tick at the final stage:

- [ ] `cargo test --workspace` green.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
      clean.
- [ ] `pnpm exec tsc --noEmit` clean.
- [ ] OpenAPI snapshot regenerated and committed.
- [ ] `./scripts/check-boundaries.sh` green.
- [ ] User can: create a project, add 10 issues from triage,
      edit a date on one, see it mirror to a linked GitHub
      board within 5s, see "Synced HH:mm:ss to <board name>"
      in the editor.
- [ ] `grep -r 'repo_project_link\|RepoProjectLinkDto\|PutRepoProjectLinkRequest' crates/ frontend/src` returns
      hits only in migration 0024.
- [ ] linear-projects-v2.md §0 has a new progress-log entry
      summarising what landed.
