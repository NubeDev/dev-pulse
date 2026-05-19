# Workflow — copilot-smoke-todo

One stage. The point of the job is exercising the runner pipeline,
not coding work.

## Per-stage discipline

- Read `SCOPE.md` first. The exact text to append is in there;
  copy it verbatim including the blank lines.
- Open `TODO.md`, append the new section at the end of the file,
  save, and verify the diff is +6 lines / -0 lines (the section
  plus surrounding blank lines).

## Closing trio — the last three todos of every stage

Every stage's todo checklist ends with the same three items, in
order. Do not rename or reorder them.

1. `checks` — there is no code change, so mark this `skipped — no
   code change` in the handover. Do not run cargo against the
   workspace; this job is markdown-only by design.
2. `docs` — update `handover.md`. The `## Done` section must name
   the file changed (`TODO.md`) and the resulting commit hash
   (`git rev-parse HEAD`). An empty `## Done` fails the docs rail.
3. `git` — `git add TODO.md`, commit with the exact message
   `stage 1: copilot smoke test — append TODO.md section`, and
   push to `codeless/copilot-smoke-todo`. Never `--force`, never
   `--no-verify`.

A stage is not done until all three todos are green and the push
succeeds.

## Anti-patterns

- Touching any file other than `TODO.md` and the codeless
  `runs/.../handover.md` the runtime maintains.
- Editing existing TODO.md content under the guise of "tidying".
- Leaving `## Done` empty or writing "n/a" — it must name the file
  and the commit hash, or the docs rail fails.
