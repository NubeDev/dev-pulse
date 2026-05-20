## Done

- reviewed stages 0–1 diff (SCOPE §15.10–§15.14, TODO §6 auth-choice update, job SCOPE/WORKFLOW scaffolding)
- verified R1/R2/R4/R5 + wire-format invariants hold on the locked decisions

## Next

- (none) — fresh session picks up stage 3

## What you need to know

- no code lands until stage 3; this stage is a decisions-only blocking gate
- key decisions: GitHub OAuth via starter-auth-oauth, starter-authz StaticRbacEngine with `oauth.github_orgs intersects auth.github.allow_orgs`, allow-list in dp-config, one DevPulseApi OpenAPI doc in dp-rest, audit verbs pinned to 8-item enum
- HMAC webhook + OAuth login/callback + starter-auth-users session routes are the only routes outside `with_principal`

## Open questions

- (none) — §0 inputs and §15.10–§15.14 decisions are locked

PASS: prerequisite decisions are recorded and consistent with R-boundary, single-transport, default-deny authz, and untouched Phase 2/3 wire formats.
