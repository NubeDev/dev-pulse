## Done

- Added `tags: Vec<Uuid>` and `repos: Vec<Uuid>` additive fields to `ReportRequest` (`crates/dp-reports/src/envelope.rs`) and `GroupBy::Tag` variant, with `MAX_TAGS_FOR_GROUP_BY_TAG = 50` const per §7.7.
- New `crates/dp-reports/src/tag_filter.rs` module materialises the §7.7 metric × link-kind mapping table: `is_issue_centric_event_kind`, `tag_link_kinds_match_event_kind`, `empty_reason_for_tag_filter`, and the locked literal `EMPTY_REASON_TAG_KIND_MISMATCH = "tag links do not match metric attribution"`. 14 unit tests cover every link-kind × event-kind cell including the exhaustive EventKind classification sweep and the empty-link-set edge case.
- `dp_reports` re-exports the new module + constants from `lib.rs`.
- Extended `ReportQuery` (REST) with `tags` and `repos` comma-separated UUID params; `to_request()` plumbs them through.
- Added `ReportResponse.empty_reason: Option<String>` (`skip_serializing_if = Option::is_none`) so the field is absent on the wire for non-§7.7 paths.
- All five report handlers (`user`, `team`, `org`, `home-org-split`, `freshness`) now call `validate_tag_filter` (returns 400 with stable codes `group_by_tag_requires_tags` / `tags_filter_over_cap`) and the four data handlers short-circuit via `empty_reason_for_request` → `empty_response` when the tag filter mismatches the requested metric.
- `count_rows` rejects `GroupBy::Tag` with `400 group_by_tag_unsupported` (the per-tag row builder lands in a later stage; this prevents "no data" being confused with "not wired").
- FakeStore in `crates/dp-rest/src/reports.rs` tests grew a `tag_links` Mutex + `resolve_tag_targets` override; 6 new `#[tokio::test]`s pin: the cap-too-big 400, the require-tags 400, the issue-only-on-commit-metric empty_reason literal echo (with `rows: []`), the satisfiable issue-on-issue case (field absent), the repo-link-tag-on-commit case (field absent), and the no-tag-filter baseline.
- OpenAPI snapshot regenerated (`crates/dp-rest/tests/openapi.snapshot.json`) — `ReportResponse` now documents the optional `empty_reason` property.
- Full workspace `cargo test` passes; `bash scripts/check-boundaries.sh` clean.
- Committed as `4c38eba` with the stage title.

## Next

- Stage 7 of 13 picks up next. Based on the job goal (SCOPE-PROJECTS §8, §13.1–§13.7, §15.6 promotion), the natural next move is the GitHub Issues CRUD synchronous write path: `0007_issues_optimistic_cas.sql` migration + the `IssueMutation` store impl + the §8.2 optimistic-CAS REST handlers the stage-2 trait stubs already reserved.

## What you need to know

- `count_rows` short-circuit for `GroupBy::Tag` is intentional — frontends that ask for `group_by=tag` will get `400 group_by_tag_unsupported` until a later stage wires the per-tag row builder. Tests pin this so it doesn't silently flip.
- `empty_reason_for_request` calls `Store::resolve_tag_targets` with empty visibility allow-lists. The §7.7 check only needs which *kinds* the tag carries (not which targets are viewer-visible), so empty allow-lists are correct for this gate. The same call site can pass real allow-lists once stage-7+ repo/team visibility primitives land — no signature change required.
- `empty_response` `debug_assert_eq!`s the reason literal against `EMPTY_REASON_TAG_KIND_MISMATCH` so any future caller passing a new literal trips in debug builds. If a second empty-reason ever lands, lift the assertion to a typed enum.
- `ReportResponse.empty_reason` uses `skip_serializing_if = "Option::is_none"` — existing tests (`every_handler_*`) keep passing because the field is absent on the wire when None. New tests use `v.get("empty_reason").is_none()` rather than `v["empty_reason"].is_null()` to match.
- The §13.7 / §13.1–§13.6 decision lock-in and the SCOPE-PROJECTS → SCOPE.md promotion mentioned in the overall job goal are NOT done by this stage — they are explicitly job-wide; only the §15.6 envelope additivity (per the stage description) is implemented here.

## Open questions

- The §7.7 mapping treats "tag with zero resolved links" as a §7.7 mismatch (same empty_reason). If product wants those distinguished (e.g. `tag_has_no_links` vs `tag_links_do_not_match_metric_attribution`), promote `empty_reason` to a typed enum before §13 locks the wire form.
- `GroupBy::Tag` returning `group_by_tag_unsupported` is a stopgap; the per-tag row builder + actual `repos`/`tags` SQL predicates are stage-7+ work. Flagged in tests as a deliberate pin.
