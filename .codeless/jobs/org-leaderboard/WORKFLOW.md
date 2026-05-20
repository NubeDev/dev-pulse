# Workflow — org-leaderboard

## Sequencing

- Stage 1 (composability investigation) **must finish and be
  reviewed** before any code stage runs. The whole effort estimate
  hinges on whether §15.7 metrics are additively composable. If
  they are not, the §6.3 `also_compute` work is a metric-layer
  refactor, not a field add — surface that in the stage 1
  handover and propose splitting it into its own stage before the
  REVIEW gate releases.
- Stages 3–5 (scaffold, subject/org-scope fan-out, reconciliation)
  must land in order; each depends on the prior stage's types.
- Stages 6 (cursor), 7 (`also_compute`), and 8 (`subject_ids`) are
  independent of each other once stage 5 lands and could be
  reordered if priorities shift — but do not parallelise them in
  the same worktree.
- Stage 9 (`my_standing`) depends only on the leaderboard SQL
  primitives existing; it does not depend on §6.3 or §6.10. It
  can land before stages 7–8 if scheduling demands, but the
  REVIEW gate at stage 10 still gates promotion.
- Stage 11 (promotion into SCOPE.md) only runs after the second
  REVIEW gate signs off.

## Per-stage discipline

- Re-read this file and `SCOPE.md` at the start of every stage.
  The ten §6.x decisions in ORG-REPORTS.md are the contract;
  every behaviour change must cite the §6.x it implements.
- Before writing code in a stage, list the §15.7 / §8.1 / §15.6
  primitives the stage reuses. If a primitive doesn't exist,
  stop and flag it — do not invent a parallel one.
- Tests are part of the stage, not a follow-up. The §6.2
  reconciliation identity, the §6.5 stale-cursor case, and the
  §6.7 "no composite score on the wire" guarantee each need an
  explicit test.
- Surface parity (REST + MCP + frontend) is verified per stage,
  not at the end. A stage that ships a REST behaviour without
  the matching MCP shape is not done.

## REVIEW gates

- **After stage 1.** Handover must include: a one-paragraph
  answer to each of the SCOPE.md open questions 1 and 2, the
  decision on whether `also_compute` is a field add or a
  refactor, and (if a refactor) a proposed new stage to insert
  before stage 3.
- **After stage 9.** Handover must include: a full §6.1–§6.10
  walk-through with a green/red mark per decision against the
  current code, the §6.2 reconciliation identity output from a
  real fixture, and a screenshot or curl transcript of the
  `my_standing` endpoint refusing an unauthenticated request.

REVIEW gates still commit + push the stage that led to them; they
only pause the *next* stage.

## Anti-patterns specific to this job

- **Do not** project `my_standing` from a full leaderboard
  result by hiding rows. The whole point of §6.9 is that the
  totals and page boundaries are themselves leaks. Build the
  visible-set headline server-side.
- **Do not** add a composite-score field "just for the UI" —
  §6.7. If the UI wants a multi-metric ranking, it issues a new
  request with that metric as `rank_by`.
- **Do not** introduce a leaderboard-local sufficiency
  threshold; defer to SCOPE.md §15.9 (§6.6). Two thresholds for
  the same concept is exactly the §11.4 divergence trap.
- **Do not** silently drop `home_org_label IS NULL` users
  (§6.8). They go into the `__unlabeled__` bucket; suppressing
  them requires an explicit envelope filter.
- **Do not** treat `subject_ids` as "leaderboard with a filter."
  In that mode pagination is disabled and `also_compute` can
  carry every metric of interest (§6.10).

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
   the job's branch (`codeless/org-leaderboard`) so the work is
   recoverable even if the worktree is wiped.

A stage is not "done" until all three todos are green and the push
succeeds. If `checks` or `git` fails, fix the cause and retry — do
not mark the stage `[x]`, do not advance, and never `--force` or
`--no-verify`. If a stage genuinely produced no change (e.g. an
investigation stage that only updated `SCOPE.md` and that doc was
already current), say so in the handover and mark `git` as
`skipped — no diff`, but the next stage's commit must include any
side-effect files the investigation touched.
