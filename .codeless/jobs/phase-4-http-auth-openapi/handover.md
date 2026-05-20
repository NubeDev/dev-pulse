# Phase 4 — Handover

## Stage 1: decisions locked (prose-only)

**Branch:** `codeless/phase-4-http-auth-openapi`

Five decisions recorded in job SCOPE.md §Decisions (D4.1–D4.5) and
main SCOPE.md §15.10–§15.14:

1. **D4.1 Operator login** — GitHub OAuth via `starter-auth-oauth`;
   `github_orgs` stamped by a post-callback wrapper in
   `dp-server::auth` using the Phase 2 octocrab client; cached on
   session, refreshed per `org_refresh_interval` (1h default).
2. **D4.2 Access gate** — `starter-authz` `StaticRbacEngine` with
   one allow rule (`oauth.github_orgs intersects
   auth.github.allow_orgs`); allow-list in `dp-config`.
3. **D4.3 Auth boundary** — `with_principal` + `require_permission`
   on every route except webhook (HMAC), OAuth login/callback, and
   session routes.
4. **D4.4 Audit vocabulary** — 8 verbs pinned as a const enum in
   `dp-rest::audit`; one `record()` helper; no second writer.
5. **D4.5 One OpenAPI doc** — `DevPulseApi` in `dp-rest::openapi`
   with shims for starter-mounted routes; snapshot-tested.

**REVIEW gate:** decisions sign-off required before stage 3 (first
code stage) proceeds.

---

## Next: stage 3 (report handlers in dp-rest::reports)

Pre-reads for the next agent session:
- `crates/dp-domain/src/store.rs` — Store trait methods available
- `crates/dp-reports/src/` — the envelope, lenses, freshness types
- Phase 3 handover (spot-check fixtures, envelope shape §15.6)
- `starter-server` Principal / AppState types
- `utoipa` annotation patterns
