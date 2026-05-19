## Done

- Added `crates/dp-fetcher/src/client/mod.rs` — `Client` wrapper with `for_installation` (App-auth via octocrab, per-installation token minted+cached by octocrab) and `with_personal_token` (test path). `get_conditional<T>` sends `If-None-Match` from cursor etag, parses `ETag` on response, and routes status into typed `Fetched::{NotModified, Ok}` / `ClientError::{Unauthorized, PrimaryRateLimit, SecondaryRateLimit, Server, Client, Deserialize, Transport, Jwt}`. Typed helpers `list_pull_requests`, `list_issues`, `list_commits` exposed for reconciler/backfill so raw octocrab never leaks past this crate.
- Added `crates/dp-fetcher/src/client/ratelimit.rs` — `classify()` turns `x-ratelimit-*`, `x-ratelimit-resource: secondary`, `retry-after`, and raw 429 into `RateLimitSignal::{Ok, PrimaryExhausted, SecondaryRateLimit}`; unit-tested across six header shapes.
- Added `crates/dp-fetcher/src/client/credentials.rs` — `AppCredentials` (with `Debug` redaction) + `InstallationCredentials` value type. PEM held in `SecretString`. Boundary-rationale doc inline. No `starter_*` imports; the bin/dp-server layer is the one that resolves from `starter-secrets-file` and constructs these.
- Added `crates/dp-fetcher/tests/client_wiremock.rs` — six wiremock-backed tests pinning happy / 304 / 401 / 403-secondary / 429 / 5xx branches.
- Added `crates/dp-fetcher/src/client/test_keys/test_rsa.pem` — throwaway 2048-bit RSA for unit tests.
- Updated `crates/dp-fetcher/Cargo.toml` — adds octocrab 0.43 (default-features=false, with `default-client`, `rustls`, `rustls-ring`, `retry`, `timeout`, `tracing`), jsonwebtoken 9, secrecy 0.10, http 1, http-body-util 0.1, bytes 1, tokio (sync+time+rt+macros), tracing, thiserror, serde/serde_json, async-trait, chrono, uuid. Dev: wiremock 0.6, tokio rt-multi-thread, serde_json.
- Updated `crates/dp-fetcher/src/lib.rs` — `pub mod client;` plus stage-3 narrative.
- `cargo test --offline -p dp-fetcher` → 15/15 pass. `cargo build --offline --workspace` → clean. `scripts/check-boundaries.sh` → OK.
- Commit `506b49e`: "stage 3: octocrab client wrapper in dp-fetcher::client …".

## Next

- (none) — next stage is a fresh session.

## What you need to know

- Octocrab handles installation-token minting + caching when given `.app(AppId, EncodingKey)` followed by `.installation(InstallationId)`. We deliberately do NOT mint JWTs ourselves; that minimizes attack surface and keeps token refresh transparent to reconciler/backfill.
- Wiremock tests use `with_personal_token` to avoid stubbing the `/app/installations/{id}/access_tokens` exchange. Same wrapper code paths exercise after the token boundary, so the test shortcut doesn't reduce coverage of the rate-limit + conditional-GET logic.
- Boundary rule held: zero `starter_*` imports in `dp-fetcher` (verified by `scripts/check-boundaries.sh`). `InstallationCredentials` is the agreed contract for the bin/server composition layer to bridge starter-secrets-file → fetcher without breaking §0.6.
- Typed list helpers currently return `serde_json::Value` rather than octocrab model types. Rationale: the reconciler will deserialize into dp-domain entities, not octocrab models, so we don't want to leak octocrab's `PullRequest`/`Issue` shape through the wrapper. Tighten to dp-domain types when reconciler lands.
- The TODO §0.6 boundary check allows `starter_spi` AND `starter_store_postgres` in dp-store-pg (the boundary script was widened earlier; the TODO prose says only starter_spi). Not changed in this stage.

## Open questions

- (none)
