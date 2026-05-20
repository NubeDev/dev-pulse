## Done

- Implemented `dp_rest::tags` module wiring SCOPE-PROJECTS §7.5 routes: `GET /tags`, `GET /me/tags`, `POST /tags`, `GET /tags/{id}` (with `links_page` pagination), `PATCH /tags/{id}`, `POST /tags/{id}/links`, `DELETE /tags/{id}/links`.
- Batch link/unlink are transactional all-or-nothing with new `ApiError::Batch` (422 `batch_rejected`) carrying `items: [{index, code, message}, ...]`; nothing commits on per-item failure.
- Viewer-filtered link counts (§7.4) — `TagDto.visible_link_count` reflects only links the caller can see; `filter_visible_links` is the single chokepoint.
- Five new audit verbs pinned: `tag.create`, `tag.update`, `tag.archive` (verb split so archive is queryable), `tag.link`, `tag.unlink` (one audit row per linked target, per §7.6).
- Archive-only retirement (`PATCH … archived: true`); no `DELETE /tags/{id}` exists.
- 500-link soft warning (§13.5): `TAG_LINK_WARN_THRESHOLD = 500` lives in `dp_domain::tag`; batch-link responses set `warning: "tag_link_count_high"` once exceeded — operation still commits.
- New `ApiError::Forbidden` variant; `TagLinkKind` gets `Hash`.
- `dp-server::build` merges `tags_router` into the protected fragment.
- OpenAPI doc + snapshot regenerated; all 15 new unit tests pass plus full workspace test suite.
- Committed as `bf02f66` with message starting with the stage title.

## Next

- Stage 6 of 13 (next session picks up). Per the job plan that should be the issues CRUD / optimistic-CAS write path + `0007_issues_optimistic_cas.sql` migration that the stage-2 `IssueMutation` trait methods already reserved space for.

## What you need to know

- Visibility model is pragmatic: org-scope uses `list_memberships_for_user`; team-scope is approximated as "any team in a visible org" (because team-membership isn't a v1 entity). Documented inline; tracked as §12 follow-up.
- Repo-link visibility filter currently returns `HashSet::new()` (i.e. no repo links are *required* to pass the viewer filter), because `dp-domain::Store` doesn't yet expose `list_repos_for_org`. Issue and user links are conservatively visible. Refine when the repo-listing primitive lands.
- The store trait already had all the methods this stage needed (stage-2 work). The Postgres impl for tag methods is the default no-op fallback from `dp_domain::store` — production wiring of `PgStore` tag methods is a later stage's concern.
- `record_audit_log` is called once per link in batch ops — a 100-row batch writes 100 audit rows. Matches §7.6 literal reading.
- OpenAPI snapshot is at `crates/dp-rest/tests/openapi.snapshot.json`; regenerate with `UPDATE_OPENAPI_SNAPSHOT=1 cargo test -p dp-rest --test openapi_snapshot`.

## Open questions

- Per-item visibility checks for `link_targets` currently only validate kind + dedupe. A target_id pointing at a repo/team the caller can't see will be accepted (and silently filtered out of their later `visible_link_count`). Tightening this requires `get_repo` / `get_team` on the store trait — flagged for a future stage.
- The race-on-store-conflict path in `link_targets` synthesises a single `index: 0` per-item error when the unique index fires; if real-world telemetry shows this misleads users, switch to re-fetching links and emitting precise indices.
