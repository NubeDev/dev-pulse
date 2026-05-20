## Done

- Added `GET /admin/runs` (paginated `FetchRun` projection), `POST /admin/users/:id/anonymise` (pseudonymisation cascade + audit), and `GET /admin/users/:id/export` (streamed JSON dump) handlers in `crates/dp-rest/src/admin.rs`.
- Updated existing `POST /admin/refresh` to emit an `admin.refresh` audit_log row (Phase 2 missed it). Refresh now threads `Extension<Principal>` and routes errors through `ApiError`.
- Widened `AdminState` with `Arc<dyn Store>`; `admin_router` accepts the new combined state.
- Added two `Store` methods with default impls — `list_fetch_runs(limit, offset)` and `list_event_actor_rows_for_user_page(user_id, offset, limit)` — and implemented both in `PgStore` with `LIMIT`/`OFFSET` SQL.
- Added `tokio-stream` + `bytes` to dp-rest deps.
- Unit tests cover refresh-audit, refresh-validation, paginated runs (newest-first across pages), anonymise cascade + audit on success, no-audit on failure, export streaming shape (parses as JSON, includes user/memberships/events, folds multi-role rows for one event), export 404-before-stream, and a pure `fold_event_actors_page` test.
- All 21 dp-rest unit tests + the rest of the workspace lib tests pass; `scripts/check-boundaries.sh` is clean.
- Committed as `54f105f` on `codeless/phase-4-http-auth-openapi`.

## Next

- Stage 6: webhook receiver / HMAC-authenticated `POST /webhooks/github` plus the worker plumbing — read TODO.md stage 6 in a fresh session.

## What you need to know

- `AdminState` constructor signature is now `AdminState::new(scheduler, store)`. Anything that builds it (composition root, future tests) needs both.
- Export streaming hand-splices its JSON header (`{"user":…,"memberships":…,"events":[`) rather than relying on serde_json's `Value` key order — `serde_json::Value::Object` sorts keys alphabetically so a trim-the-tail trick on a `json!` macro produced `…,"user":…}` instead of `…]}`.
- `list_event_actor_rows_for_user_page` contract is `(ts ASC, event_id ASC)`. The export handler depends on consecutive same-`event_id` rows being adjacent so `fold_event_actors_page` can collapse multi-role rows into one `ExportEvent` with a `roles` array. PgStore + the test fake both honour this order.
- Audit policy on the new routes: refresh audits even on coalesce ("ran:false") because operator intent is the auditable event; anonymise audits **after** cascade so a failed cascade leaves no trail; export audits **before** streaming starts (the authorised request itself is the audit-worthy event).
- New utoipa DTOs (`FetchRunDto`, `ExportEvent`, `MembershipDto`, `UserExport`) use `#[schema(value_type = String)]` on `FetchRunKind` / `EventKind` / `MembershipRole` and `value_type = Vec<String>` on `Vec<ActorRole>` because the dp-domain types don't derive `ToSchema` (§0.6 boundary — dp-domain stays HTTP-shape-free).
- 500MB / OOM target met by paging at `EXPORT_PAGE_SIZE = 500` rows; an mpsc channel of capacity 4 between the producer task and the response stream caps pending memory at ~4 pages.

## Open questions

- (none)
