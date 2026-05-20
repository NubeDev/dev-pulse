## Done

- implemented stage 4: pins surface per SCOPE-PROJECTS §6.4 / §6.5 / §13.5
- new `dp-rest::pins` module with four routes — `GET /me/pins`, `POST /me/pins`, `DELETE /me/pins/{kind}/{target_id}`, `PUT /me/pins/order` — all keyed off `Principal.actor_user_id` (no admin-on-behalf path in v1)
- atomic reorder: REST pre-checks set-equality (`reorder_set_mismatch`), `PgStore::reorder_pins` runs the check + per-row `UPDATE … SET position` inside one transaction
- §13.5 pin cap exposed as `dp_domain::PIN_CAP = 20` and re-exported from `dp_rest::pins`; enforced both at REST (`pin_cap_exceeded` 400) and inside `PgStore::add_pin` (same tx as the insert)
- three pinned audit verbs (`pin.add`, `pin.remove`, `pin.reorder`) in `dp_rest::audit`, each written *after* the mutation succeeds
- new `ApiError::Conflict` (409, used by re-pin) and `ApiError::NotFound` (404, used by missing-on-delete)
- `pins` resource (`read|write`) registered in `dp_server::auth::policy::register_dev_pulse_resources`; org-gate wildcard rules cover it without a policy TOML edit
- `pins_router` merged into the protected fragment in `dp_server::build`
- OpenAPI document updated (paths + schemas + `pins` tag); snapshot regenerated under `UPDATE_OPENAPI_SNAPSHOT=1`
- 8 new unit tests in `dp_rest::pins::tests` (list order, append at end + audit, cap rejection, duplicate 409, delete + audit, 404 on missing, atomic reorder, set-mismatch); whole-workspace `cargo test` green; `scripts/check-boundaries.sh` green
- committed on `codeless/projects-issues` (`d98ade5`)

## Next

- stage 5 (tags REST surface) — `GET/POST/PATCH/DELETE /tags`, `POST/DELETE /tags/{id}/links`, plus visibility filter per §7.4 and the §13.5 500-link warning

## What you need to know

- `PinKind` now derives `Hash`; this is additive and trait-default-safe but is a new derive on a public type
- `dp-config` does not yet exist as a crate; `PIN_CAP` lives in `dp-domain` as a single source of truth with a doc-comment saying it eventually moves into `dp-config` (matching §13.5)
- `GET /me/pins` returns the bare `PinDto` (no hydrated repo/tag payload yet) — §6.4 calls for hydration; deferred to a follow-up since `Store` has no `get_repo` and adding one widens the trait beyond this stage
- `position` is never auto-compacted on `DELETE`; gaps are only compacted by the next `PUT /me/pins/order` (matches §6.3 row-by-row commentary)
- `PUT /me/pins/order` audits `target = "count:<n>"` rather than the full key list to keep `dp_audit_log.target` small; the verb is meaningful at row granularity (one row = one reorder)

## Open questions

- (none)
