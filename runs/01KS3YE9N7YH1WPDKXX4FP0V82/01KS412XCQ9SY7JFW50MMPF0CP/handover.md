## Done

- Added crates/dp-store-pg/migrations/dp/0023_project_board_links.sql (dp_project_board_links + dp_project_board_items, indexes, FK cascades).
- Added crates/dp-store-pg/migrations/dp/0024_drop_repo_project_link.sql (DROP TABLE IF EXISTS dp_repo_project_link).
- Added crates/dp-domain/src/board_link.rs with BoardLink, BoardLinkUpsert, BoardItem, BoardItemMirrorOutcome; wired into dp-domain/src/lib.rs.
- Extended dp-domain/src/store.rs Store trait with list_board_links, get_board_link, create_board_link, delete_board_link, refresh_board_link_cache, list_board_items_for_issue, get_board_item, record_board_item_result (all with default impls so existing fakes stay green).
- Implemented the new methods on PgStore in crates/dp-store-pg/src/store.rs; removed the now-dead pg overrides for get/upsert/delete_repo_project_link so the trait defaults take over. Added row_to_board_link / row_to_board_item helpers.
- Added integration test board_link_crud_and_item_outcomes in crates/dp-store-pg/tests/integration.rs covering create (with picker cache), natural-key conflict, multi-link fan-out, success/failure outcomes with aggregate roll-up, refresh_board_link_cache COALESCE behaviour, cascade delete, and the NotFound delete path.
- Committed as "stage 7: migrations 0023 + 0024 — ...".

## Next

- (none) — fresh session picks up the next stage.

## What you need to know

- dp_issue_dates.mirror_node_id / mirror_synced_at / mirror_error are deliberately NOT dropped by 0023, contrary to the literal §8 wording. The §3.10 mirror still writes through them via OctocrabProjectV2Mirror + PATCH /issues/{id}/dates; dropping the columns ahead of the slice-B mirror rewire would strand the live mirror with no place to record outcomes. A later stage that rewires OctocrabProjectV2Mirror to fan out across dp_project_board_items should drop them.
- The §3.10 REST surface in crates/dp-rest/src/repo_project_link.rs is still wired into the server and OpenAPI but now hits the trait defaults at runtime (GET → 404, PUT → 500). It compiles, the OpenAPI snapshot test passes unchanged, but those endpoints are dead behaviour. The follow-up stage that introduces /projects/{id}/board-links should delete the module, prune dp-rest/src/lib.rs + openapi.rs + dp-server/src/lib.rs wiring, and regenerate the snapshot.
- record_board_item_result uses an empty-string sentinel for item_node_id when a Failure lands before any Success (NOT NULL column). last_synced_at IS NULL is the canonical "never successfully mirrored" signal; the next Success UPSERT overwrites the sentinel with the real PVTI_…
- create_board_link maps the natural-key UNIQUE collision to StoreError::Conflict so the §7.3 POST handler can render 409 "already linked".
- cargo build --workspace + cargo test --workspace --lib + cargo test -p dp-rest --test openapi_snapshot all green. The new integration test is #[ignore]'d behind Docker, matching the rest of the integration suite.

## Open questions

- Stage scope deviates from spec §8 by deferring the mirror_* column drop — confirm the next stage owner agrees and schedules the drop alongside the OctocrabProjectV2Mirror rewire.
