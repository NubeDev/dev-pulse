# Workflow — triage-slice-2

This job is **one big slice** but spans backend schema changes,
authn changes, write-path mounting, and a substantial frontend
push. The risk profile is uneven — schema + identity + CAS writes
are the parts that bite if rushed; the frontend list pane is
mechanical. Drive each stage accordingly.

## Sequencing

- **Stages 1 → 3** are the load-bearing backend foundation. Each
  must end on green `cargo test` for every crate touched. **Do
  not advance** past stage 3 with a failing test, even if it's
  "obviously the next stage's fix" — every stage is recoverable
  from its own commit only if it actually committed.
- **REVIEW gate after stage 3** (= template stage 4). Operator
  confirms the migration chain, the deprecation strategy for
  `dp_users.github_id`, and the nonce-based OAuth state design
  before the write surface is exposed.
- **Stages 5 → 7** are the write + read endpoints. They depend on
  the identity model but do not depend on each other; if a stage
  blocks on a GitHub-side ambiguity (Projects v2 field IDs,
  octocrab error mapping), park it in the handover and pick up
  the next.
- **Stage 8** is the OpenAPI / audit / bulk inbox cleanup. It's
  the "everything that doesn't fit in a single feature stage but
  must ship together" pile. Do not skip it; the OpenAPI gap is
  what made the slice-1 progress log call out registration as
  TODO.
- **REVIEW gate after stage 8** (= template stage 9). Backend is
  done. Confirm OpenAPI surface + policy registry + migration
  chain before frontend stages start depending on them.
- **Stages 10 → 11** are the frontend backbone and the dates /
  saved-views polish. Land them in order — the identity manager
  changes how every authenticated request looks, so it goes
  first.
- **Stage 12** is tech debt + handover. The job is not done until
  the slice-3 brief is written and `make build` is green from a
  clean target dir.

## Per-stage discipline

- **Read before write.** Top of every stage: re-read
  `linear-projects-idea.md` for the section the stage names
  (§0 / §3.0 / §3.8 / §5.4 / etc.) and re-read `SCOPE.md`. The
  stage description is the *outcome*; the doc + scope are the
  *specification*.
- **Edit-tool stale buffer hazard** (user-memory note). After
  every edit to a Rust file, verify via terminal (`grep` / `sed`
  / `stat`) that the change actually hit disk — not just
  `read_file`. When a compile error claims a freshly added
  member doesn't exist, suspect the buffer before chasing
  imports or cfg gates. Re-apply via a python heredoc with
  `assert src.count(old) == 1` if needed.
- **Migration discipline.** Forward-only, sequential numbering
  from the current head (0012 — confirm in stage 1). Backfills
  go in the same `.sql` file as the schema change. Run
  `make migrate` and verify the output before committing.
- **Octocrab.** Every GitHub-touching call goes through the
  existing `dp-fetcher` client abstraction; do not add a second
  HTTP client. Mock the client in unit tests via the existing
  trait surface; integration tests use the same recording layer
  the reconciler tests use.
- **Frontend.** `pnpm typecheck` is the type gate; `pnpm lint`
  catches the style; `make build` is the final sign-off.
  Mocks-first: extend `frontend/src/workflow/mocks.ts` before
  touching components so the UI is testable without a backend
  running.

## REVIEW gate behaviour

The two REVIEW gates are explicit pauses. At each:

- The stage that *led* to the gate still completes the closing
  trio. REVIEW gates **pause the next stage**; they do not skip
  the current one's commit + push.
- In `handover.md` for the next stage, write:
  - The decisions the operator needs to confirm (numbered).
  - Any open question from `SCOPE.md` you've answered, with the
    answer and the reasoning.
  - The exact migration filenames that landed and the order they
    run in.
  - Any test that's flaky or quarantined, with a one-line cause.
- Do **not** start the next stage until the operator's reply
  lands. If the operator green-lights silently (no objections in
  the gate), proceed.

## Anti-patterns specific to this job

- **Do not bump `dp_issues.version` from the reconciler.** Slice
  1.5 splits the counter precisely so the reconciler bumps
  `external_version` only. Adding a `version` bump in the
  reconciler reintroduces the own-write-unread bug — the very
  thing this slice fixes.
- **Do not put the session id in OAuth `state`.** §3.0.2.a is
  the standard CSRF-safe pattern; `state` is an opaque nonce
  looked up server-side. Any deviation is a security regression.
- **Do not auto-revoke `dp_memberships` on unlink without checking
  provenance.** §3.0.2.b is the invariant; collapse only when no
  `dp_membership_identities` row remains for the
  `(user_id, org_id)`.
- **Do not block the local date save on the GraphQL mirror push.**
  §3.10 is explicit: local upsert is synchronous, mirror is
  best-effort. A failing GraphQL call writes to `mirror_error`
  and returns 200 on the local upsert.
- **Do not pin to `dp_users.github_id` in any new code.** Every
  new read goes through `dp_user_identities WHERE is_primary`.
  The column is deprecated by this job; reads grandfathered into
  it must be migrated before stage 8 closes.
- **Do not advance a stage on a flaky test.** The pre-existing
  `dp-fetcher` failures from §0 are stage 12's job; if a stage
  touches them by accident, fix the cause or revert the touch.
- **Do not skip OpenAPI registration to "come back later".** The
  slice-1 gap is precisely what stage 8 closes. Adding more
  unregistered handlers re-creates the bug we're paying off.
- **Do not mass-refactor `dp-store-pg`** to "make room" for the
  new methods. Add new methods alongside the existing ones; the
  store trait is already large and a slice-2 refactor blows the
  blast radius of this job.

## Closing trio — the last three todos of every stage

Every stage's todo checklist ends with the same three items, in
order. The user watches these tick over in the `Stages` overview;
they are how the user confirms a long-running stage actually
landed instead of just looking like it did. Do **not** rename or
reorder them.

1. `checks` — run the stage's `verify:` list (or `verify_cmd`).
   Every step must pass. On failure: stop, fix, re-run; do not
   advance to `docs`.
2. `docs` — update `handover.md` for the next stage and the active
   session doc, in the same worktree, so the fresh agent that opens
   the next stage has the context it needs (per SCOPE Constraint 2:
   anything that must survive a stage boundary is on disk, not in
   the agent's head).
3. `git` — stage the changes (`git add -A` from the worktree root,
   or specific paths if the stage was surgical), commit with the
   message `stage N: <one-line title from template.yaml>` so the
   history mirrors the template stages one-for-one, and push to
   the job's branch (`codeless/triage-slice-2`) so the work is
   recoverable even if the worktree is wiped.

A stage is not "done" until all three todos are green and the push
succeeds. If `checks` or `git` fails, fix the cause and retry — do
not mark the stage `[x]`, do not advance, and never `--force` or
`--no-verify`. If a stage genuinely produced no change (e.g. an
investigation stage that only updated `SCOPE.md` and that doc was
already current), say so in the handover and mark `git` as
`skipped — no diff`, but the next stage's commit must include any
side-effect files the investigation touched.

## Verify recipes per stage area

- **Backend stage:**
  - `cargo test -p dp-domain -p dp-store-pg -p dp-rest -p dp-server --lib`
  - `cargo test -p dp-fetcher --lib` (after stage 12; expect known
    failures triaged before that)
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `make migrate` against a fresh DB (the existing
    `crates/dp-store-pg/docker-compose.yml`); confirm the
    migration chain applies clean.
- **Frontend stage:**
  - `cd frontend && pnpm typecheck`
  - `cd frontend && pnpm lint`
  - `make build`
- **End-to-end (stage 12):**
  - `cargo test --workspace`
  - `make build`
  - Hit `/me/queue`, `/me/identities`, `/issues/{id}/timeline`,
    `/repos/{id}/sync-status`, `/reports/issues`,
    `PATCH /issues/{id}/dates` with curl through the dev server;
    record the responses in `handover.md`.

## Cost & wall-clock

This job is sized for a multi-hour run. Stage 7 (dates + Projects
v2 mirror) and stages 10-11 (frontend) are the longest. If the
runner reports approaching the cost cap mid-stage, **finish the
stage's closing trio** (so the commit lands) before stopping; the
next worktree boot picks up cleanly from the last commit. Do not
skip the trio to "save tokens" — recovery without a commit is
manual rebase territory.
