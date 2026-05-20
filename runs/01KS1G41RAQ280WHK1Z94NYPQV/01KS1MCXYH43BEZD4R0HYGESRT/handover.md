## Done

- Created `crates/dp-server/policy/dev-pulse.toml` — `StaticRbacEngine` config with org-gate allow (`oauth.in_allowed_org == true`) + org-gate deny (`not oauth.in_allowed_org == true`); deny-overrides ensures the deny beats the default `Reader/Writer/Admin` allows that `default_policy = true` brings in.
- Created `dp_server::auth` module with three files:
- `config.rs` — `GitHubAuthConfig { client_id, client_secret_ref, allow_orgs, org_refresh_interval_secs }` + case-insensitive `any_in_allow_list` (empty allow-list fails closed).
- `github_orgs.rs` — `GithubOrgsSource` trait, `StaticGithubOrgsSource` (tests / bin placeholder), `CachedGithubOrgsSource` (TTL cache), and `GithubOrgsStamper` (`PrincipalExtrasLookup` impl that wraps `OAuthPrincipalExtras` and stamps `oauth.github_orgs` + `oauth.in_allowed_org`).
- `policy.rs` — `register_dev_pulse_resources` (registers `reports`/`users`/`orgs`/`teams`/`home_org`/`admin`), `load_static_engine`, and `AwaitingAccessEngine` (rewrites `no_matching_rule` AND the org-gate-deny `explicit_deny` into the wire-stable `awaiting_access` reason).
- dp-rest decorates every protected route in `reports_router`/`directory_router`/`admin_router` via `starter_authz::with_permission`-wrapped sub-router merges (had to use `with_permission` rather than `require_permission` directly — the layer's return-type annotation `FromFnLayer<_, (), ()>` doesn't satisfy axum 0.8's `MethodRouter::layer` Service-impl marker-tuple bound, but `Router::layer(from_fn(closure))` inside `with_permission` works).
- dp-server::build hands `Arc<dyn PolicyEngine>` to the protected fragment as an axum `Extension` so the `require_permission` gate finds both the Principal (from `with_principal`) and the engine on every request.
- dp-rest test helpers seed `Extension<NoopPolicyEngine>` + `Extension<SpiPrincipal{Admin}>` so the per-route gate evaluates Allow in unit tests.
- 13 dp-server lib tests + 21 dp-rest lib tests pass; full workspace `cargo test` green; `scripts/check-boundaries.sh` reports OK.
- Committed as `b7c684e` on `codeless/phase-4-http-auth-openapi`.

## Next

- Stage 10 (`dev-pulse serve` bin wiring) needs to construct the `AppState.policy` via `register_dev_pulse_resources` → `load_static_engine("crates/dp-server/policy/dev-pulse.toml")`, build the real `GithubOrgsSource` (octocrab-backed, using `Client::with_personal_token` against the operator's stored OAuth access token), wrap it in `CachedGithubOrgsSource::new(src, cfg.org_refresh_interval())`, and compose `GithubOrgsStamper::new(OAuthPrincipalExtras::new(identity_store), cached, Arc::new(cfg))` into `AuthState::with_principal_extras(...)`. Then mount on the configured address.

## What you need to know

- The `starter-authz::condition` grammar has no `intersects` operator. The org-gate is expressed via a derived boolean `oauth.in_allowed_org` stamped by `GithubOrgsStamper` (computed from `auth.github.allow_orgs` ∩ `github_orgs`). The policy file is org-agnostic; adding/removing orgs is a `dp-config` edit only.
- Out-of-org users hit both an `explicit_deny` from the org-gate-deny rule (matched_rule = `org-gate-deny-out-of-org`) AND would also be caught by the `no_matching_rule` branch if the rule weren't present — both paths are rewritten to `awaiting_access` by `AwaitingAccessEngine` so the 403 wire body is stable.
- `GithubOrgsStamper` returns `Value::Null` when the inner lookup returns `Null` (user has no linked OAuth identity) — the policy then denies because `in_allowed_org` is absent. That preserves the standard `OAuthPrincipalExtras` "no identity → no oauth block" semantics.
- The real octocrab-backed `GithubOrgsSource` was deliberately deferred to the bin layer because constructing it needs access to the operator's stored OAuth access token (not currently in `oauth_identities`) and would otherwise drag `starter-secrets-file` into `dp-server`. The trait keeps the seam open.
- dp-rest now depends on `starter-authz` (edge-allowed per §0.6). Boundary check still green.

## Open questions

- The user's GitHub OAuth access token is not persisted by `starter-auth-oauth` (`OAuthIdentity` has no `access_token` field). The real `OctocrabGithubOrgsSource` will need a parallel store, or the stamper will have to fetch orgs inside the callback path (one-shot) and cache them for the configured refresh interval. Stage 10's bin wiring needs to pick one — flagged for stage 10 / a follow-up SCOPE note rather than left undecided here.
- The report/directory handlers currently call `audit::record` only in directory + admin; report handlers don't write `report.read` yet (carried over from stage 3). SCOPE D4.4 requires it. Not in stage-9 scope; flag for a future stage to bolt the audit write onto each report handler.
