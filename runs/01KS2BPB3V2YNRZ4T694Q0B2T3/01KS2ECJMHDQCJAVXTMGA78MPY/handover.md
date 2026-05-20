## Done

- Reviewed stages 2–6 diff (≈5 200 lines across `dp-domain`, `dp-reports`, `dp-rest`, `dp-server`, migrations) against the rulebook's Layer-1 invariants and SCOPE-PROJECTS §13.1–§13.5.
- Wrote `.codeless/jobs/projects-issues/STAGE-7-REVIEW.md` capturing the audit and verdict.
- Committed (`stage 7: REVIEW …`) and pushed to `codeless/projects-issues` (commit `65d8a66`).
- Ran `cargo test -p dp-reports --lib` (60 pass) and `cargo test -p dp-rest --lib` (52 pass) as part of the audit.

## Next

- Stage 8 (App permission bump) can proceed — REVIEW gate cleared.

## What you need to know

- Sentinel: **PASS: Layer-1 invariants (R1 dependency direction, R2 single transport, R4/R5 trust boundary, §15.6 additive wire format) all hold; §13.1–§13.5 decisions implemented; gaps logged are documented v1 limitations, not regressions.**
- Three follow-ups logged in STAGE-7-REVIEW.md for the stage-12 reviewer (they are *not* blocking stage 8):
- §15.6 additivity confirmed: `ReportRequest.tags` + `ReportRequest.repos` both `#[serde(default)]`; `GroupBy::Tag` new variant; OpenAPI snapshot diff is additive only.
- Leaderboard branch (`codeless/org-leaderboard`) has not yet merged; the STAGE-1-COORDINATION.md fallback applies — this branch owns `ReportRequest.repos`, leaderboard rebases onto it.

## Open questions

- (none for this gate; the three follow-ups above are tracked in STAGE-7-REVIEW.md for stage 12.)
