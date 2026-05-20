## Done

- Added `crates/dp-rest/src/openapi.rs` with `#[derive(OpenApi)] struct DevPulseApi` aggregating all 14 utoipa-annotated handlers (reports x5, directory x4, admin x4, plus a docs-only `POST /webhooks/github` stub) and every request/response schema; re-exported `DevPulseApi` from the crate root for `dp-server::build()` to hand to `ServerBuilder::with_openapi`.
- Added `#[utoipa::path]` to the previously-unannotated `admin::refresh` handler and `ToSchema` derive to `RefreshResponse` so the aggregator picks it up.
- Created `crates/dp-rest/tests/openapi_snapshot.rs` + generated `crates/dp-rest/tests/openapi.snapshot.json` pinning the document; regenerates on `cargo test -p dp-rest -- --update-openapi-snapshot` or `UPDATE_OPENAPI_SNAPSHOT=1`.
- `cargo build --workspace` clean; `cargo test -p dp-rest` 22/22 pass (21 unit + 1 snapshot).
- Committed as `stage 6: OpenAPI aggregation in dp-rest::openapi` on `codeless/phase-4-http-auth-openapi`.

## Next

- Stage 7 (per TODO.md Phase 4 sequence — a fresh session will pick it up).

## What you need to know

- The webhook receiver in `dp-fetcher::webhook::router` is intentionally NOT utoipa-annotated; `openapi.rs` carries a private `webhook_github_stub()` whose only purpose is to render the path into the spec. `dp-server` still mounts the real router.
- `RefreshResponse` is an untagged enum — utoipa 5 serialises it as `oneOf` over the two variants.
- The snapshot file uses `to_pretty_json()` output with a trailing newline; any drift surfaces as a `panic!` pointing at the first diverging line plus the regen command.

## Open questions

- (none)
