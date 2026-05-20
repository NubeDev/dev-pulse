# Workflow — projects-issues

## Sequencing

- **Stage 1 (envelope-additivity coordination)** is non-negotiable
  and must precede any code. The `org-leaderboard` worktree is
  also extending `ReportEnvelope` (§15.6) and may also need
  migration numbers above `0002_*`. The stage 1 deliverable is a
  one-page note in the job dir naming:
  - which branch lands first (this one or `codeless/org-leaderboard`);
  - the exact §15.6 fields each job adds, by name;
  - the migration-number convention (e.g. this job takes the next
    odd numbers, the other takes the next even — or any other
    scheme that prevents collision);
  - the dashboard-01 shell files both jobs may touch (sidebar,
    routing, top-level page registry).
  If the leaderboard branch has already merged when this stage
  runs, the note still exists but its recommendations narrow to
  "rebase off main, here is what changed."
- Stages 3–7 can run sequentially without blocking on the
  leaderboard branch; the §15.6 additions in stage 6 only need to
  rebase against whatever shape `ReportEnvelope` has at that
  point.
- **Stage 7 (first REVIEW gate)** is the hard gate before any
  GitHub write code lands. Do not proceed to stage 8 without it.
- Stages 8–10 (App permission, write path, reconciler guard) must
  land in order — each depends on the prior stage's invariants.
- Stage 11 (frontend wiring) depends on every backend stage
  through 10 being green.
- **Stage 12 (second REVIEW gate)** is the surface review against
  the §11 success criteria.
- Stage 13 (promotion into SCOPE.md) only runs after stage 12
  signs off and (ideally) after the leaderboard branch has either
  merged or signalled its own promotion timing, so the SCOPE.md
  rewrite happens once, not twice.

## Per-stage discipline

- Re-read this file and `SCOPE.md` at the start of every stage.
  The decisions §13.1–§13.7 in SCOPE-PROJECTS.md are the contract;
  every behaviour change must cite the §x it implements.
- Before writing code in a stage, list the primitives the stage
  reuses from SCOPE.md: §15.1, §15.4, §15.6, §15.10, §15.11,
  §15.13. If a primitive doesn't exist, stop and flag it — do not
  invent a parallel one.
- The §15.11 access gate is the **single** visibility check.
  Mutation adds the §8.2 step 4 App-install scope check **on top
  of** the gate, not in place of it.
- Tests are part of the stage, not a follow-up. Specifically:
  - §6 stages — pin-cap rejection, atomic reorder.
  - §7 stages — viewer-filtered link counts (the table must show
    a real "user can see tag, cannot see some links" fixture),
    per-scope name uniqueness, archive-doesn't-cascade-links,
    batch all-or-nothing transactional semantics.
  - §7.7 stage — every row of the metric × link-kind mapping
    table has a test; the `empty_reason` path is one of them.
  - §8 / §9 / §10 stages — every §8.3 race case has a test;
    `pending_remote_timeout` rollback is tested; the §13.7
    reconciler-vs-optimistic guard has a test fixture for each
    of "fetcher tick mid-flight," "webhook arrives mid-flight,"
    and "handler crash → timeout sweeper."

## REVIEW gates

- **After stage 7** (before any GitHub writes). Handover must
  include: the §11 access-gate audit (every new query / mutation
  path traced back to §15.11 with no parallel checks), a
  green/red mark against §13.1–§13.5, the §15.6 envelope diff in
  its merged form (this job's additions + whatever leaderboard
  landed if it landed), and a fixture proving the viewer-filtered
  link-count behaviour (§7.4).
- **After stage 12** (surface review before promotion).
  Handover must include: a walkthrough of each §11 success
  criterion against the deployed code, a green/red mark against
  §13.6 and §13.7, and a recorded transcript of each of the §8.3
  conflict cases on a live system.

REVIEW gates still commit + push the stage that led to them; they
only pause the *next* stage.

## Anti-patterns specific to this job

- **Do not** ship a write path through the fetcher. §13.3 is
  load-bearing: the audit story breaks the moment a non-user
  actor can mutate GitHub.
- **Do not** add `If-Match` / `If-Unmodified-Since` to the GitHub
  call hoping for a 409. The Issues REST API does not honour
  them. The local `version` CAS is the only guard.
- **Do not** report true tag-link counts (§7.4). Filter to the
  viewer's visibility *before* counting, every time.
- **Do not** silently widen a `repo`-linked tag to the whole org
  when `ReportEnvelope.repos` is absent. The §15.6 follow-up to
  add `repos` lands *before or with* the tag-as-reports-dimension
  stage; if it has not, fail the report with a clear error rather
  than producing the wrong answer.
- **Do not** mutate GitHub labels or milestones (§4). dev-pulse
  *uses* them; the org admin manages them in GitHub.
- **Do not** auto-assign tags based on user activity (§4 "no
  surveillance creep"). Tags are user-curated. Period.
- **Do not** hard-delete tags. Archive only (§7.4 mutation
  rules). DB cleanup is an admin job.
- **Do not** retrofit MCP write tools when stage 11's REST
  surface lands. §13.3 explicitly defers MCP mutation; opening
  it requires its own scope doc.

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
   the next stage has the context it needs (per SCOPE Constraint:
   anything that must survive a stage boundary is on disk, not in
   the agent's head).
3. `git` — stage the changes (`git add -A` from the worktree root,
   or specific paths if the stage was surgical), commit with the
   message `stage N: <one-line title from template.yaml>` so the
   history mirrors the template stages one-for-one, and push to
   the job's branch (`codeless/projects-issues`) so the work is
   recoverable even if the worktree is wiped.

A stage is not "done" until all three todos are green and the push
succeeds. If `checks` or `git` fails, fix the cause and retry — do
not mark the stage `[x]`, do not advance, and never `--force` or
`--no-verify`. If a stage genuinely produced no change (e.g. an
investigation stage that only updated `SCOPE.md` and that doc was
already current), say so in the handover and mark `git` as
`skipped — no diff`, but the next stage's commit must include any
side-effect files the investigation touched.
