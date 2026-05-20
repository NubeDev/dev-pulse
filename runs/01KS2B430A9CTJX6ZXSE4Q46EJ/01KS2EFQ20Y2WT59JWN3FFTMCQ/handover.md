## Done

- Added SCOPE.md §8.2 "Leaderboard report kind (cross-cutting primitive)" folding ORG-REPORTS.md §1–§5 (vision, subject axis, envelope, response, org-scope interaction table, two-endpoint note, §4 non-goals reconciliation), trimmed to SCOPE.md house style.
- Added SCOPE.md §15.15 "Leaderboard semantics (Phase 3+)" folding ORG-REPORTS.md §6.1–§6.10 as ten Decision-format entries (§15.15.1–§15.15.10), each with revisit triggers, matching the §15.7–§15.14 style.
- The two-endpoint shape (decision 9 `my_standing` + decision 10 `subject_ids` small-N) is called out in both the §8.2 "Endpoint shape — note" subsection and the §15.15 preamble so it survives future edits.
- Picked up two footnotes flagged by earlier stage handovers: §15.15.2 now states the reconciliation identity holds across the full result set, not per page; §15.15.10 now states cursor-with-subject_ids is a typed `400 pagination_disabled_for_subject_ids`, not a silent drop.
- Deleted ORG-REPORTS.md per the §8 promotion path ("This file can then be deleted or kept as design rationale; the locked text lives in SCOPE.md").
- Committed as a5df8e9 on branch codeless/org-leaderboard.

## Next

- (none — this was the final stage of the job)

## What you need to know

- SCOPE.md grew from 643 to ~970 lines; §8.2 was inserted between §8.1 and the "Aggregation functions" subheading; §15.15 was appended after §15.14.
- The `__unlabeled__` synthetic bucket is documented in §15.15.8 but the code uses `dp_memberships.home_org` (UUID); that mapping note from runs/01KS2C17F0PJ65K5PE67D3V297/handover.md is not re-stated in SCOPE.md (it's an implementation detail).
- References to ORG-REPORTS.md remain inside `.codeless/jobs/org-leaderboard/` and prior `runs/*/handover.md` snapshots — those are historical artefacts of the in-flight job, not user-facing docs, and were intentionally left alone.

## Open questions

- (none)
