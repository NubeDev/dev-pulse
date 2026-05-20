## Done

- verified stage 1 ("lock Phase 4 open questions") is already committed on this branch as 71b5097
- confirmed all five required decisions land in main SCOPE.md §15.10–§15.14 (operator login via starter-auth-oauth; starter-authz StaticRbacEngine allow-list policy at crates/dp-server/policy/dev-pulse.toml; with_principal + require_permission boundary with documented exceptions; 8-verb audit vocabulary; one DevPulseApi OpenAPI doc with utoipa::path shims) and the mirror in .codeless/jobs/phase-4-http-auth-openapi/handover.md (D4.1–D4.5)

## Next

- stage 2 (per WORKFLOW.md) — typically a REVIEW gate / sign-off, then stage 3 begins coding the report handlers in dp-rest

## What you need to know

- branch `codeless/phase-4-http-auth-openapi` is up to date with origin; working tree clean
- the §0 inputs in TODO.md are read-only per the stage brief and were not touched
- did NOT create an empty re-commit — the stage-1 commit already exists with the exact required message prefix
- pre-reads for stage 3 are enumerated in .codeless/jobs/phase-4-http-auth-openapi/handover.md ("Next: stage 3")

## Open questions

- (none)
