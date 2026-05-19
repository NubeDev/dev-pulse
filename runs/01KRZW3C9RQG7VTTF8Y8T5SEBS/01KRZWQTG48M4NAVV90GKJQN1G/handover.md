## Done

- Added `dp_fetcher::webhook` module: `router` (POST /webhooks/github via axum), `verify` (HMAC SHA-256 with rotation-aware constant-time compare), `metrics` (`dp_webhook_receipt_seconds` histogram registered against the shared `prometheus::Registry`).
- New public surface: `webhook::router`, `WebhookState`, `WebhookMetrics`, `WebhookSecretSource`, `StaticSecrets`, `verify_signature`, `SignatureError`.
- Route is NOT wrapped in `with_principal` per TODO §4 — HMAC is the auth.
- Response table pinned by tests: 200 happy / 200 replay (StoreError::Conflict) / 400 missing-header / 400 non-JSON / 401 missing-or-bad sig / 500 other backend errors.
- Structured `tracing::info_span!` with `webhook.delivery_id = %delivery_id` field on every request log line.
- Added Cargo deps: workspace `axum`, workspace `prometheus`, `hmac 0.12`, `sha2 0.10`, `hex 0.4`; dev-dep `tower 0.5` (util feature) for `oneshot` in tests.
- 19 webhook unit tests all pass (`cargo test -p dp-fetcher --lib webhook`); `scripts/check-boundaries.sh` still OK (zero `starter_*` imports in dp-fetcher); `cargo build --workspace` clean.
- Committed on `codeless/phase-2-ingestion` as `252748e`.

## Next

- Stage 5: webhook worker — drain `webhook_inbox` via `Store::claim_webhooks`, apply idempotent upserts via `external_id`, fan out multi-actor events into `event_actors` per §0.2, with co-author / squash-merge / bot / unattributed fixture tests per SCOPE §6.

## What you need to know

- `WebhookState` carries `Arc<dyn Store>` + `Arc<dyn WebhookSecretSource>` + `WebhookMetrics`. The bin / `dp-server` composition layer is the side that resolves the secret from `starter-secrets-file` and implements `WebhookSecretSource` — `dp-fetcher` stays starter-free. `StaticSecrets::single` / `StaticSecrets::rotating` is the throwaway impl for wiring before the secrets-file adapter lands.
- The handler reads the raw body as `axum::body::Bytes` (HMAC is over the wire bytes — cannot let `Json` reparse first).
- `WebhookMetrics::for_test()` exists under `cfg(test)` only — production wiring must call `WebhookMetrics::register(&registry)` exactly once (the double-register guard test pins this).
- Heads-up footgun for the next session: the worktree root is `/home/user/.codeless/worktrees/job-01KRZW3C9RQG7VTTF8Y8T5SEBS`, **not** `/home/user/code/rust/dev-pulse/`. Absolute paths to the latter silently edit the wrong checkout. Use relative paths or the worktree-absolute path.

## Open questions

- (none)
