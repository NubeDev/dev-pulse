# Workflow — projects-v2

This file is the per-stage discipline for the projects-v2 job.
It is re-read at the top of every stage by the runner. The
authoritative project-wide rules are in
[../../../../codeless-workspace/CLAUDE.md](../../../../codeless-workspace/CLAUDE.md);
this file only adds rules specific to this job.

## What to read at the top of every stage

In order:

1. `SCOPE.md` in this directory.
2. `linear-projects-v2.md` at the repo root — the section that
   covers the current stage (§5/§8 storage, §7 REST, §6
   frontend, §9 permissions/audit, §11 migration plan, §12
   phasing).
3. The previous stage's `handover.md` if one exists.
4. For stages touching the §3.10 mirror: re-read
   `linear-projects-idea.md` §3.10 and the existing
   `OctocrabProjectV2Mirror` impl before changing it.
5. For any frontend stage: re-read §6 of linear-projects-v2.md
   in full plus the existing triage workbench row component
   you'll be reusing.

Do not skip step 2. Drift from the design doc is the most
expensive class of mistake we can make on this job — the user
already rejected one shape (§3.10 admin pane), and the cost of
a second wrong shape is a third rewrite.

## Sequencing

- Stages 1 (storage) commits its own migrations + domain +
  store, then halts at stage 2 (REVIEW).
- The REVIEW at stage 2 resolves the 5 open questions in
  `SCOPE.md` — primarily the issue-belongs-to-many-projects
  question, which the slice-A frontend depends on.
- Stages 3 (CRUD) and 4 (membership) are each their own commit;
  do not batch even though they touch the same crate.
- Stage 5 lands the entire slice A frontend in a single stage
  because the sidebar / list / detail / chip / bulk-add only
  make sense together. If it grows past ~800 lines net,
  consider splitting at the chip + bulk-add boundary and stop
  to discuss at the REVIEW.
- Stage 6 is the second REVIEW gate. Stop after stage 5's
  push; the user will exercise slice A end-to-end against the
  running dev server.
- Stages 7–9 land slice B in three commits: storage, REST +
  mirror fan-out, frontend.
- Stage 10 is the third REVIEW gate. The user exercises slice
  B end-to-end.
- Stage 11 (cleanup + acceptance) is mechanical — grep, regen,
  log entry.

## Per-stage discipline

- **Read before writing.** Every stage starts by reading the
  inputs above and the files it will edit.
- **Verify before committing.** Run `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, and `./scripts/check-boundaries.sh`
  from the worktree root. From any stage that touches REST
  also run the OpenAPI snapshot test (regenerate with
  `UPDATE_OPENAPI_SNAPSHOT=1 cargo test -p dp-rest
  openapi_snapshot` and commit the diff). From any stage that
  touches frontend also run `pnpm exec tsc --noEmit` from
  `frontend/`.
- **One concept per file (CLAUDE.md R3).** A `projects` module
  ≠ a `project_board_links` module. Split if a file grows two
  unrelated responsibilities.
- **No drive-by refactors (CLAUDE.md R4).** This job is large
  enough already. Do not "while I'm here" anything outside the
  Projects feature. The existing `IssueDatesEditor` mirror
  status surface is unchanged.
- **No half-finished implementations (CLAUDE.md R4).** If a
  stage cannot land cleanly, mark it `[!]` in the handover
  and halt. Do not commit a stub.

## REVIEW gate behaviour

At a REVIEW gate, the stage that produced the gate still runs
its closing trio in full — checks, docs, git — so the work to
be reviewed is on the branch. The handover at a REVIEW gate
must contain:

- A one-paragraph summary of what landed.
- A bullet list of the decisions made for the open questions
  in `SCOPE.md` (with brief reasoning).
- For slice REVIEW gates (stages 6 and 10): a short script the
  user can follow to exercise the slice in the running app
  (the dev server is at <http://localhost:5173> per usual).
- Anything the user should look at specifically before
  approving.

Do not auto-advance past a REVIEW gate. The runner pauses; wait.

## Anti-patterns specific to this job

- **Do not add a second `github_node_id` column anywhere.** It
  already exists on `dp_issues` from migration 0021. The §3.10
  lazy-resolve path is keeping it populated; do not duplicate
  it on `dp_project_board_links` or elsewhere.
- **Do not break the existing `IssueDatesEditor` mirror status
  surface.** `dp_issue_dates.mirror_synced_at` and
  `mirror_error` remain the single per-issue display columns.
  The new per-link statuses live on
  `dp_project_board_links.last_mirror_at` /
  `last_mirror_error` and surface only on the project detail
  page.
- **Do not fail closed if no token is wired.** If no PAT is
  available, fall through silently — the same behaviour the
  existing mirror has today. The user has explicitly chosen
  this fallback over a noisy error path.
- **Do not edit any file under `../starter/`.** Same rule as
  every other job in this repo.
- **Do not edit `../ai-runner/`.** Patches there are tracked
  in `../../codeless-workspace/ai-runner.PATCHES.md` and are
  not part of dev-pulse work.
- **Do not introduce a paste-node-id field on the primary
  `Link a board` dialog.** The whole point of this job is to
  retire that UX. The fallback paste field stays only on
  `#/admin/project-sync`.
- **Do not retain `dp_repo_project_link`.** Migration 0024
  drops it. There is no production data to preserve (§11 of
  linear-projects-v2.md is explicit). Do not write a data
  migration.
- **Do not pluralise wrongly.** `dp_projects` (table),
  `dp_project_issues` (junction), `dp_project_board_links`
  (junction). Singular owner, plural noun for the table.

## Closing trio — the last three todos of every stage

Every stage's todo checklist ends with the same three items,
in order. The user watches these tick over in the `Stages`
overview; they are how the user confirms a long-running stage
actually landed instead of just looking like it did. Do **not**
rename or reorder them.

1. `checks` — run the stage's `verify:` list (or `verify_cmd`).
   For this job, that means `cargo fmt --check`,
   `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace`, and `./scripts/check-boundaries.sh`.
   For REST stages also `UPDATE_OPENAPI_SNAPSHOT=1 cargo test
   -p dp-rest openapi_snapshot` and commit any diff. For
   frontend stages also `pnpm exec tsc --noEmit` from
   `frontend/`. Every step must pass. On failure: stop, fix,
   re-run; do not advance to `docs`.
2. `docs` — update `handover.md` for the next stage and the
   active session doc, in the same worktree, so the fresh
   agent that opens the next stage has the context it needs.
3. `git` — stage the changes (`git add -A` from the worktree
   root, or specific paths if the stage was surgical), commit
   with the message `stage N: <one-line title from
   template.yaml>` so the history mirrors the template stages
   one-for-one, and push to the job's branch
   (`codeless/projects-v2`) so the work is recoverable even
   if the worktree is wiped.

A stage is not "done" until all three todos are green and the
push succeeds. If `checks` or `git` fails, fix the cause and
retry — do not mark the stage `[x]`, do not advance, and never
`--force` or `--no-verify`. If a stage genuinely produced no
change, say so in the handover and mark `git` as
`skipped — no diff`, but the next stage's commit must include
any side-effect files the investigation touched.
