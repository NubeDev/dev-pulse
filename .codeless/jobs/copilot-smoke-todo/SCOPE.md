# Scope — copilot-smoke-todo

## Goal

Prove the codeless Copilot runner works on dev-pulse by performing
one tiny, deterministic edit and completing the full stage lifecycle
(checks, docs, git). No code is touched; no logic changes; the test
is purely about the runner pipeline.

## In scope

- Append exactly one new section to the end of `TODO.md`:

  ```markdown

  ## Copilot smoke test (2026-05-20)

  Submitted via the codeless Copilot runner. This entry exists only
  to verify that the runner can edit a file, write a handover, and
  push a commit on the job branch.
  ```

- Write a non-empty `handover.md` `## Done` section naming the file
  changed and the commit hash.
- Run the closing trio (`checks`, `docs`, `git`) to completion.

## Out of scope

- Any change outside `TODO.md`.
- Any change to Rust source, Cargo files, scripts, or CI.
- Any reformatting of `TODO.md` beyond appending the new section.
- Running cargo test / clippy / fmt against the whole crate — the
  edit is markdown-only, so `checks` is `skipped — no code change`.

## Constraints

- Append only; do not rewrite, reorder, or delete existing TODO.md
  content.
- Commit message: `stage 1: copilot smoke test — append TODO.md section`.
- Push to branch `codeless/copilot-smoke-todo` (the job branch).
- Never `--force`, never `--no-verify`.

## Open questions

None — the edit is fully specified above.
