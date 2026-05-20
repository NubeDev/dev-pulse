## Done

- Implemented Phase 4 stage 4: `dp-rest::directory` with `GET /users`, `GET /orgs`, `GET /teams`, `POST /home-org`, plus `dp-rest::audit` (D4.4 vocabulary + `record()` helper + stub `Principal`).
- Extended `dp-domain::store::Store` with `set_home_org_for_user`, `list_orgs`, `list_teams_for_org`, `list_users_for_org`, `record_audit_log` (default impls keep all existing fakes compiling).
- Added `dp_domain::AuditEntry` mirroring `dp_audit_log` columns.
- Implemented production-grade `set_home_org_for_user` in PgStore (one transaction; existence check + CASE-based UPDATE that flips the target row and clears every other row for the user in a single statement).
- Added 8 unit tests against an in-memory `MemStore` covering list-with-and-without-filter, missing `org_id` → 400, audit-row writes (action pinned to `HOME_ORG_SET`, target `user:<uuid>`), atomic flip (exactly one `home_org = Some` per user post-flip), and no-audit-on-failed-mutation.
- `cargo test -p dp-rest -p dp-domain` → 15 pass; `cargo clippy -p dp-rest -p dp-domain --all-targets --no-deps -- -D warnings` clean; `scripts/check-boundaries.sh` green.
- Committed as `9dbbb1f` with title starting `stage 4: directory handlers + home-org mutation in dp-rest::directory`.

## Next

- Stage 5 (admin handlers): `GET /admin/runs`, `POST /admin/users/:id/anonymise`, `GET /admin/users/:id/export` (streaming); extend the existing `POST /admin/refresh` with audit emission via `audit::record(...)` using the `ADMIN_REFRESH` constant already pinned in this stage.

## What you need to know

- The `Principal` used by handlers is a tiny `dp-rest::audit::Principal { actor_user_id: Uuid }` carried via `axum::Extension`. Stage 9 will replace it with a `starter-auth` derived one; until then, the composition root or tests inject it via `Router::layer(Extension(p))`.
- Audit verbs live as `pub const` in `dp-rest::audit`. Stage 5 should use `ADMIN_REFRESH`, `USER_ANONYMISE`, `USER_EXPORT`, `RUNS_LIST` — they're already defined.
- All new Store methods have safe defaults so existing fakes in `dp-fetcher` and `dp-rest` admin/reports tests didn't need touching. New handlers that need real listing semantics either override or rely on PgStore's real impl.
- Pre-existing clippy warnings in `dp-fetcher` (`unnecessary_lazy_evaluations` on `login_opt.or_else(...)` around line 646) are unrelated to this stage but will trip a workspace-wide `clippy --all-targets -- -D warnings`. Run clippy with `-p dp-rest -p dp-domain --no-deps` to scope it.
- `set_home_org_for_user`'s default impl is non-atomic on purpose (so unrelated test fakes can stay simple); only PgStore and the in-memory MemStore in directory tests guarantee the contract. If a future stage needs atomicity from a non-Pg backend, override there.

## Open questions

- (none)
