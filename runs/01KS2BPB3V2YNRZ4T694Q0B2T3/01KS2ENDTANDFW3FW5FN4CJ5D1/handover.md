## Done

- Added `dp_domain::app_install::{OrgAppInstall, AppInstallPermissions}` (fail-closed default) plus `Store::get_org_app_install` trait method (default `Ok(None)`).
- Added `dp_rest::app_permissions` module: `GitHubAppConfig` (carries `request_issues_write` + App slug), `require_issues_write` §8.4 write-gate, `app_manifest_permissions` helper, `GET /me/app-install-banner` handler returning per-org `writes_available` + copy-able admin text + per-install `manage_url` deep-link.
- Added `ApiError::WritesNotAvailable` → 403 `writes_not_available_for_org` with org_login + manage_url body.
- Wired `github_app: Arc<GitHubAppConfig>` through `dp_server::AppState` / `dp_rest::AppState`; bin reads `[github.app]` (sub-table of `[github]`) in `dp-config`; default `request_issues_write = true` per §13.6 step 1.
- Registered `github_app` resource in `dp-server/src/auth/policy.rs`.
- Regenerated `crates/dp-rest/tests/openapi.snapshot.json` (additive only: +114 lines, 1 new path, 3 new schemas).
- Locked the decision additively in `SCOPE.md` as §15.15 (cross-refs SCOPE-PROJECTS §8.4 / §13.6).
- Tests: 9 new `app_permissions::tests::*` cases (gate, manifest, copy-text, manage_url); workspace `cargo test` green; boundary check OK.
- Committed as `3206d41` with the stage title in the subject.

## Next

- Stage 9 (per the run plan) — likely the §8.2 optimistic-CAS issue write path proper, which will mount real `POST/PATCH/...` issue handlers that route through `dp_rest::require_issues_write(...)` before touching octocrab. The `Store::get_org_app_install` trait method's default returns `Ok(None)` — the Postgres backend override + the corresponding migration land in a later stage.

## What you need to know

- Bin layer surface: `[github.app]` is a TOML sub-table of `[github]` (not a top-level block). Example: `[github.app]\nrequest_issues_write = true\nslug = "dev-pulse"`.
- `GitHubAppConfig` is re-exported from `dp-server` so the bin (which does not depend on `dp-rest` directly) can name it.
- The §8.4 banner endpoint and the §8.4 403 share the *same* `dp_rest::require_issues_write` verdict — never duplicate the gate logic in a future handler.
- `AppInstallPermissions::READ_ONLY` is the fail-closed default; a missing `OrgAppInstall` row is treated the same as `issues_write = false` (banner / 403 both say "writes not available").
- The §13.6 hard-disable escape hatch (`request_issues_write = false`) also omits `issues` from `app_manifest_permissions(...)` so the GitHub consent screen does not even show the write scope.
- The openapi snapshot test only honours the `UPDATE_OPENAPI_SNAPSHOT=1` env var when invoked through `--test openapi_snapshot`, not via the default `cargo test -p dp-rest` runner (which rejects the `--update-openapi-snapshot` argument because it's a `#[test]`-style flag the lib runner doesn't accept).

## Open questions

- (none) — the deferred items (Postgres backend `get_org_app_install` impl, install-callback writer) belong to later stages of this same job, not stage 8.
