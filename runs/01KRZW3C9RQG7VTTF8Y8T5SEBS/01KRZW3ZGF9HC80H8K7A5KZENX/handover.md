## Done

- Added SCOPE.md §15 Decisions section with four locked Phase 2 decisions and a §15.5 noting TODO §0.1–§0.6 as read-only inputs
- §15.1 GitHub App over PAT (rationale: webhook delivery requires App, per-install rate-limit bucket scales with org count, audit story, growth into write scopes); revisit if GitHub changes App rate-limit policy or a deployment refuses Apps
- §15.2 Backfill default 90 days, configurable via `starter-config` `backfill.window_days`; revisit on first deployment / deeper-history request / storage budget
- §15.3 Webhook HMAC secret at `secrets://github/webhook_hmac` in starter-secrets-file with current+previous overlap rotation; receiver tries current first, falls back to previous with `webhook.hmac.rotated_fallback` metric; mismatch on both → 401 fail-closed, never enqueued; `webhook_inbox.delivery_id` uniqueness survives rotation so in-flight redeliveries are safe
- §15.4 Octocrab rate-limit headroom: single client wrapper in dp-fetcher tracks primary + secondary buckets, pauses when `remaining < 100` until reset, threshold exposed as `github.ratelimit.min_remaining` in starter-config, honours `Retry-After` on 429; webhook ingest path unaffected
- Committed as `cc4c258` with message starting `stage 1:`

## Next

- Stage 2 picks up implementation of the octocrab client wrapper in dp-fetcher (the place §15.4 lands as code) and/or the webhook receiver route (§15.3 lands as code). A fresh session will pick the next stage from TODO Phase 2

## What you need to know

- SCOPE.md previously had no Decisions section; §15 is new and sits after §14 (Out of scope)
- Wording is deliberately specific so later stages can be greppable: `secrets://github/webhook_hmac`, config keys `backfill.window_days` and `github.ratelimit.min_remaining`, metric `webhook.hmac.rotated_fallback`, threshold default `100`, overlap-cycle length tied to reconciler interval (4h per TODO §0.3)
- TODO §6 risks list still shows unchecked boxes for "GitHub App vs PAT" and "Backfill bound" — left unchanged because the Decisions section is now the canonical record; if you want the TODO checkboxes ticked too, that's a one-line edit per item
- No code changes this stage — pure decision capture per the stage brief

## Open questions

- Operator login (humans logging into dev-pulse) is still open per SCOPE §12; §15.1 only resolves *fetcher* auth to GitHub
- Per-org TZ source (TODO §6) is untouched — not in Phase 2 scope
- Materialised `event_actor_facts` table (TODO Phase 1 / §6) deferred to first load test as previously planned
