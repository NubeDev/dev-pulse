## Done

- Added §16 (pinned repos & tags), §17 (project tags — home-grown), §18 (GitHub Issues CRUD), and §19 (auth implications) to SCOPE.md, mirroring SCOPE-PROJECTS.md §6 / §7 / §8 / §9 as the normative scope for the workflow surface.
- Added §15.16–§15.21 to the existing §15 Decisions block in SCOPE.md, promoting SCOPE-PROJECTS §13.1, §13.2, §13.3, §13.4, §13.5, §13.7 in the same Decision format (Decision / Why / Revisit if / Resolves). §13.6 was already landed earlier as §15.15.
- Added a §20 "Relationship to SCOPE-PROJECTS.md" section with the §13.x → §15.x decision mapping table.
- Retained SCOPE-PROJECTS.md (rewrote its header to mark it as "design rationale" pointing at the normative §16–§19 + §20 in SCOPE.md), since 52 files in the tree still cross-reference it. Original content preserved beneath the new status banner.
- Committed as `f0c3d35` with a message starting "stage 13: promote SCOPE-PROJECTS.md into SCOPE.md".

## Next

- (none) — stage 13 is the final stage of this 13-stage job.

## What you need to know

- Existing SCOPE.md §15.x decision numbering (15.1…15.15) was kept intact — all other crates reference these and renumbering would have broken many handlers / tests / openapi snapshots. New decisions were appended as §15.16–§15.21.
- New top-level sections were appended after §15 Decisions as §16–§20, rather than inserted before §15, to preserve the same §15.6 / §15.11 / §15.13 / §15.14 references used throughout the codebase.
- The 52 source files that still mention `SCOPE-PROJECTS` (crates, frontend, migrations, runs/, .codeless/jobs/) were not touched. SCOPE-PROJECTS.md still exists and resolves; it just now declares itself as design rationale and points at the normative §16–§19 in SCOPE.md.
- §15.15 (already-landed §13.6 promotion) was not edited — the §20 mapping table acknowledges it.

## Open questions

- Whether to follow up by sweeping the 52 in-tree references from `SCOPE-PROJECTS.md §X` to `SCOPE.md §16…§19` so the design diary stops being load-bearing. Not in this stage's scope; would be its own job.
