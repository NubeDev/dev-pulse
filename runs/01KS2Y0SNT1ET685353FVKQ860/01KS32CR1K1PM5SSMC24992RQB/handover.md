## Done

- reviewed stages 4–8 diff against Layer-1 invariants (R1/R2/R4/R5) and the stage-9 gate criteria (OpenAPI, policy registry, migration chain)

## Next

- (none) — gate failed; remediation belongs to a later ramp step, not this stage

## What you need to know

- migration chain 0013 → 0015 (plus 0017 trigger) is intact and ordered
- OpenAPI snapshot grew to cover the new handlers; utoipa decorations present on write / read / inbox / dates / repos / reports
- `crates/dp-server/src/auth/policy.rs::register_dev_pulse_resources` registers: reports, users, orgs, teams, home_org, admin, pins, github_app, issues, tags
- `crates/dp-rest/src/repos.rs:315-321` calls `require_permission("repos", "read")` and `("repos", "sync")` against an unregistered resource
- no `identities` resource is registered and no identity handler exists (only `IDENTITY_*` audit-verb constants in `crates/dp-rest/src/audit.rs:97-109`)

## Open questions

- is the multi-identity surface intended to ship in slice 2 backend (goal text says yes) or deferred to a later slice (code state suggests deferred)? Answer changes whether the fix is "register `identities` + add handlers" or "drop `identities` from the gate criterion"

FAIL: `repos` and `identities` resources are missing from `register_dev_pulse_resources`, so the `("repos", "read")` / `("repos", "sync")` decorations introduced in stages 5–6 violate the R4 trust boundary (require_permission against an unregistered resource) and the stage-9 registry criterion is not met.
