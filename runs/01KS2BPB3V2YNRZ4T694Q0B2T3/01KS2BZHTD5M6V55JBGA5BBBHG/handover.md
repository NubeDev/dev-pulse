## Done

- Added migration `crates/dp-store-pg/migrations/dp/0005_user_pins_tags_tag_links.sql` with `dp_user_pins`, `dp_tags`, `dp_tag_links`. Includes the polymorphic CHECK constraints on `dp_tags.scope_*_id` and `dp_tag_links.target_*_id`, the per-scope case-insensitive unique expression index `dp_tags_scope_name_uniq` on `(scope_kind, COALESCE(scope_user_id, scope_team_id, scope_org_id), lower(name))`, the per-target unique index `dp_tag_links_tag_target_uniq`, four partial reverse-lookup indexes (one per link kind), and the `dp_user_pins_user_position_idx` covering the sidebar render order. `(user_id, position)` is deliberately NOT uniqued at the DB level — §6.3 requires that so atomic reorder can rewrite every row in one tx.
- Added domain types: `pin::{Pin, PinKind}`, `tag::{Tag, TagScopeKind}`, `tag_link::{TagLink, TagLinkKind}`, `issue_mutation::{IssueMutation, IssueMutationOp, IssueMutationResult}`. Each has unit tests for JSON round-trip and the helper methods (`scope_id`, `target_id`, `is_archived`, `audit_verb`, `applies_to_non_issue_metrics`, lower-case wire form matching the SQL CHECK).
- Re-exported the new types from `crates/dp-domain/src/lib.rs`.
- Added `Store` trait methods (with default no-op / empty / `Invalid` impls so existing fakes compile): `list_pins_for_user`, `add_pin`, `remove_pin`, `reorder_pins`, `get_tag`, `create_tag`, `update_tag`, `list_tags_visible_to`, `list_tag_links`, `add_tag_links`, `remove_tag_links`, `resolve_tag_targets`, `record_issue_mutation`, `update_issue_mutation_result`, `list_pending_issue_mutations_older_than`.
- `cargo build --workspace` and `cargo test --workspace --lib` green. 31 dp-domain unit tests pass (10 new).
- Committed on `codeless/projects-issues` as `257ca18`.

## Next

- Stage 4: presumably the `dp-store-pg` Postgres implementations of the new `Store` methods, and/or the `0007_issues_optimistic_cas.sql` migration (`dp_issues.version` + `pending_remote*` columns + `dp_issue_mutations` table). Pick up from the stage-list in the job's WORKFLOW.md.

## What you need to know

- All new `Store` methods have **default impls** so dp-reports / dp-rest / dp-mcp / fetcher integration-test fakes did not need touching this stage. The PG backend must override each one when stage 4 lands writes — the default `Invalid("… not supported by this store")` will surface in tests if a handler accidentally exercises the unimplemented backend method.
- `IssueMutation` type ships now, but its **table is reserved for `0007_issues_optimistic_cas.sql`** per the coordination note. The trait methods that touch it will fail until 0007 lands.
- The leaderboard branch has not merged yet. Migration slot 0005 was reserved for this job in the stage-1 coordination note; if leaderboard's 0004 has not merged by the time this branch rebases, that is fine — the slots are non-overlapping by design.
- `position` on `dp_user_pins` has no unique constraint; reorder must rewrite all rows in one tx (`reorder_pins` trait method).
- The previous stage's failure was a handover-diff mismatch (claimed paths that were not in the diff). This handover lists only paths actually changed in commit `257ca18` — verified via `git status` before commit.

## Open questions

- (none)
