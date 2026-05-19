## Done

- Reviewed stages 3–5 (webhook receiver, worker, handlers) against Layer-1 rulebook
- Ran scripts/check-boundaries.sh (clean) and `cargo test -p dp-fetcher --lib` (57 passed)
- Confirmed replay dedup via `delivery_id` unique + idempotent upsert via `external_id`
- Confirmed latency histogram + `fetch_runs` row written per drain

## Next

- Stage 7: reconciler riding on the same handlers (per-(org,repo,resource_kind) cursors + etag conditional GETs, runs every 4h)

## What you need to know

- PASS sentinel emitted below — runtime will continue
- No patches proposed (review stage forbids it); any nits land in a later ramp step
- Worker uses `FOR UPDATE SKIP LOCKED` via `Store::claim_webhooks` so reconciler can safely share the worker dispatch path
- Route is intentionally outside `with_principal`; HMAC is the auth surface — preserve when reconciler/backfill compose into the same app

## Open questions

- (none)

PASS: webhook receive→enqueue→drain path is HMAC-fail-closed, dep-clean (zero starter_* in dp-fetcher), replay-idempotent on external_id, and emits the latency histogram + fetch_runs row the rulebook requires.
