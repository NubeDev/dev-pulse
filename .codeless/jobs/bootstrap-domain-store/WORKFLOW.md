# Workflow — bootstrap-domain-store

This file is the per-stage discipline for the bootstrap-domain-store
job. It is re-read at the top of every stage by the runner. The
authoritative project-wide rules are in
[../../../../codeless-workspace/CLAUDE.md](../../../../codeless-workspace/CLAUDE.md);
this file only adds rules specific to this job.

## What to read at the top of every stage

In order:

1. `SCOPE.md` in this directory.
2. `TODO.md` at the repo root — the relevant §Phase section for the
   current stage.
3. The previous stage's `handover.md` if one exists.
4. The starter consumer rules referenced from TODO.md:
   - `/home/user/code/rust/starter/DOCS/howto/using-starter-as-a-library.md`
   - `starter/examples/notes/` as the shape to mimic.
5. For any stage that touches schema: TODO §0.1–§0.5 in full.

Do not skip step 4 even if the previous stage already touched
starter. Drift between this repo and starter conventions is the
single most expensive class of mistake we can make.

## Sequencing

- Stages 1–2 (workspace scaffolding, boundary script) can be done
  in a single batch if the scaffolding is mechanical; commit each
  as its own stage commit per the closing trio.
- Stage 3 is a REVIEW gate. Stop after stage 2's push, write the
  handover explaining the crate layout choices and any starter-*
  paths that needed adjustment, and wait for approval.
- Stages 4–6 (domain, store, schema) must each be their own stage
  commit. The schema stage in particular must not be batched with
  the store impl — the migration files are reviewed separately.
- Stage 7 is the second REVIEW gate. Write the handover with the
  schema diagram (text is fine) and confirm the answers to the
  open questions in `SCOPE.md`.
- Stage 8 runs after the schema is approved.

## Per-stage discipline

- **Read before writing.** Every stage starts by reading the
  inputs above and the files it will edit, even if they are
  empty stubs from a previous stage.
- **Verify before committing.** Run `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo test --workspace` from the worktree root. All three
  must pass. From stage 2 onward, also run
  `./scripts/check-boundaries.sh`.
- **One concept per file (CLAUDE.md R3).** If a file accumulates
  a second unrelated struct or trait, split before committing.
- **No drive-by refactors (CLAUDE.md R4).** This job is
  scaffolding + schema. Do not "while I'm here" anything in
  starter or in `ai-runner/`.
- **No half-finished implementations (CLAUDE.md R4).** If a
  stage cannot land cleanly, mark it `[!]` in the session doc
  and halt. Do not commit a stub with a `TODO` and move on.

## REVIEW gate behaviour

At a REVIEW gate, the stage that produced the gate still runs
its closing trio in full — checks, docs, git — so the work to be
reviewed is on the branch. The handover at a REVIEW gate must
contain:

- A one-paragraph summary of what landed.
- A bullet list of the decisions made for any open questions in
  `SCOPE.md` (with brief reasoning).
- Anything the user should look at specifically before approving.

Do not auto-advance past a REVIEW gate. The runner pauses; wait.

## Anti-patterns specific to this job

- **Do not edit any file under `../starter/`.** This is the
  starter-as-library rule from TODO.md §1. If starter is missing
  an API you need, stop the stage with `[!]` and call it out at
  the next REVIEW.
- **Do not edit `../ai-runner/`.** Patches there are tracked in
  `../../codeless-workspace/ai-runner.PATCHES.md` and are not part
  of dev-pulse work.
- **Do not introduce a `user_id` column on `activity_events`.**
  TODO §0.2 is explicit; attribution lives in `event_actors`.
- **Do not introduce a single global cursor.** TODO §0.3 is
  explicit; per-(org, repo, resource_kind) only.
- **Do not introduce naive timestamps.** All timestamp columns
  are `timestamptz`.
- **Do not add a "user is deleted" boolean.** TODO §0.5 uses a
  `deleted_at timestamptz NULL` for soft-delete.

## Closing trio — the last three todos of every stage

Every stage's todo checklist ends with the same three items, in
order. The user watches these tick over in the `Stages` overview;
they are how the user confirms a long-running stage actually
landed instead of just looking like it did. Do **not** rename or
reorder them.

1. `checks` — run the stage's `verify:` list (or `verify_cmd`).
   For this job, that means `cargo fmt --check`,
   `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace`, and from stage 2 onward
   `./scripts/check-boundaries.sh`. Every step must pass. On
   failure: stop, fix, re-run; do not advance to `docs`.
2. `docs` — update `handover.md` for the next stage and the
   active session doc, in the same worktree, so the fresh agent
   that opens the next stage has the context it needs.
3. `git` — stage the changes (`git add -A` from the worktree
   root, or specific paths if the stage was surgical), commit
   with the message `stage N: <one-line title from
   template.yaml>` so the history mirrors the template stages
   one-for-one, and push to the job's branch
   (`codeless/bootstrap-domain-store`) so the work is
   recoverable even if the worktree is wiped.

A stage is not "done" until all three todos are green and the
push succeeds. If `checks` or `git` fails, fix the cause and
retry — do not mark the stage `[x]`, do not advance, and never
`--force` or `--no-verify`. If a stage genuinely produced no
change, say so in the handover and mark `git` as
`skipped — no diff`, but the next stage's commit must include
any side-effect files the investigation touched.
