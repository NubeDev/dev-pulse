## Done

- Implemented Phase 4 stage 3: five report handlers (`/reports/user/:user_id`, `/team/:team_id`, `/org/:org_id`, `/home-org-split`, `/freshness`) in `crates/dp-rest/src/reports.rs`, all utoipa-annotated and returning `ReportResponse { resolved_window, rows, data_as_of }`.
- Added supporting modules: `state.rs` (`AppState { store: Arc<dyn Store> }`), `error.rs` (`ApiError` with `From<ResolveError>` + `From<StoreError>` and stable JSON `{ error, code }` body).
- Added `utoipa`, `chrono`, `chrono-tz`, `thiserror`, `async-trait` to `dp-rest` deps.
- 8 dp-rest unit tests pass, including the two stage-required ones: `every_handler_echoes_resolved_window_verbatim` and `every_handler_returns_data_as_of_object`. Full workspace builds clean, `scripts/check-boundaries.sh` is green.
- Committed as `c278f9b` on branch `codeless/phase-4-http-auth-openapi`.

## Next

- Stage 4 (per Phase 4 plan): admin / run-log / GDPR handlers in dp-rest — `GET /admin/runs`, `POST /admin/users/:id/anonymise`, `GET /admin/users/:id/export`, `POST /home-org`, plus `POST /webhooks/github` (HMAC, not principal-wrapped).

## What you need to know

- `ReportResponse.rows` is `serde_json::Value` (not a typed generic) — one envelope, five row shapes. The OpenAPI doc would benefit from per-route `body = <Concrete>` overrides when the aggregator lands; the five concrete row types (`CountRow`, `HomeOrgSplitRow`) are already exported with `ToSchema`.
- `ReportQuery` is a flat query struct (axum's `Query`/`serde_urlencoded` can't nest); vector filters are comma-separated. `to_request()` validates and maps to `dp_reports::ReportRequest`.
- `group_by=team` returns 400 (`group_by_team_unsupported`) because the read path needs a `user→team` resolver that this stage doesn't wire — surface as bad-request rather than silent empty.
- `/reports/home-org-split` always forces `ScopeMode::PerOrgSplit`. Inside each (user, org) bucket it dedups on `event_id` so a multi-role row doesn't double-count.
- `/reports/org/:org_id` overwrites `request.orgs` with the path id; `/reports/user/:user_id` and `/reports/team/:team_id` append the path id to the filter without replacing what the caller passed.
- `DataAsOfDto` lives in dp-rest (not dp-domain) — keeps dp-domain free of HTTP-shape concerns. Includes a `headline` field picked via `pick_freshness_headline`.
- ApiError IntoResponse maps Store errors to 500 with a generic message; the real error is logged via `tracing::error!`.
- Tests use an in-module `FakeStore` (same pattern as the existing `admin::tests::TinyStore`); it only seeds rows + freshness, every other method is a stub.

## Open questions

- (none)
