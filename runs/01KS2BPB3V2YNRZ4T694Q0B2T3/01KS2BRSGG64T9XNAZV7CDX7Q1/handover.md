## Done

- Wrote `.codeless/jobs/projects-issues/STAGE-1-COORDINATION.md` — one-page coordination note covering: (1) leaderboard merges first; (2) exact §15.6 field lists per job (leaderboard adds `LeaderboardEnvelope` + claims `ReportEnvelope.repos`; projects-issues adds `tags: Vec<TagId>`, `repos: Vec<RepoId>`, `GroupBy::Tag`, and `empty_reason = "tag links do not match metric attribution"`); (3) odd/even migration numbering off the current `0003_*` head — leaderboard takes evens (`0004`, `0006`), this job takes odds (`0005_user_pins_tags_tag_links.sql`, `0007_issues_optimistic_cas.sql`); (4) dashboard-01 shell collision surface limited to `frontend/src/routes.ts` and `frontend/src/components/app-sidebar.tsx`, append-only.
- Committed on `codeless/projects-issues` as `758120f`.

## Next

- Stage 2 (REVIEW gate, no code): confirm the coordination plan and answer/defer the SCOPE-PROJECTS.md §12 open questions (pin cap, tag name charset, cross-scope promotion, webhook-vs-fetcher race, Projects v2 import) before any code lands.

## What you need to know

- `codeless/org-leaderboard` has closed its own stage 1 (`STAGE-1-COMPOSABILITY.md`) and stage 2 REVIEW; it is scoped to start its stage 3 (scaffold `LeaderboardEnvelope`) next. Its envelope changes are smaller and additive-only, which is why it was placed first.
- Both jobs need a `repos` field on `ReportEnvelope`. The note's preferred spelling is `Vec<RepoId>` (matches existing §15.6 convention "empty = no filter") rather than `Option<Vec<RepoId>>` as currently doc'd in ORG-REPORTS.md §3. Final call belongs to whichever job's §15.6 stage hits trunk first.
- Migration filenames reserved here are filename-only; content is up to the implementing stage.
- The leaderboard branch's diff against main also deletes the projects-issues job dir (its branch point pre-dated the scaffold). That's expected — rebase order is "leaderboard merges, projects-issues then rebases off main."

## Open questions

- (none)
