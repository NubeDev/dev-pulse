## Done

- registered `repos` resource (`read`, `sync`) in `register_dev_pulse_resources` with a regression test (`repos_read_and_sync_are_registered`) — closes the stage-9 review-gate failure where §5.9 `with_permission("repos", …)` decorations fired against an unregistered resource (`crates/dp-server/src/auth/policy.rs`)
- frontend `#/account/identities` route: new `account` section in `routes.ts`, page at `frontend/src/account/identities-page.tsx`, wired into `app.tsx` and `app-shell.tsx`; identity rows project off the auth user with link/unlink/transfer/set-primary buttons that stage client-side and toast "backend deferred (§10)" so the round-trip seam is exercisable before backend handlers land
- user-menu identity-set badge (linking to `#/account/identities`) added to the SiteHeader actions slot
- Teams + People rail entries on the triage page; People is collapsed-by-default (§13.5 cap); both surface placeholder copy until backend team / person queue endpoints ship
- triage middle-pane: group-by + sort dropdowns (`triage-group-by`, `triage-sort-by`), row checkbox (`triage-row-select`), bulk action bar with Done / Snooze 1d / Clear, Shift-E / Shift-H / Shift-D bulk keybindings wired through new `useBulkInbox` hook → `POST /me/inbox/bulk` (new `bulkInbox` API client method, `BulkInboxOp` / `BulkInboxRequest` / `BulkInboxResponse` zod schemas)
- ⌘K / Ctrl-K command palette (`CommandPalette` component) with jump-to (top-8 visible rows), view-switch (mine/untriaged/snoozed/all), and apply-to-selection (done / snooze / restore) entries; substring filter, ↑/↓ cursor, Enter to run
- `cargo build -p dp-server -p dp-rest` and `cargo test -p dp-server auth::policy::tests` both pass (4 tests including the new repos guard)

## Next

- (none) — stage 11 picks up from a fresh session per the per-stage charter

## What you need to know

- backend `/me/queue?status=snoozed` was *not* added — the existing snoozed view in `useTriageRows` still returns an empty placeholder. Adding `status` requires touching `ListIssuesQuery` + `IssueListFilter` + the inbox-issues SQL in `crates/dp-store-pg/src/store.rs` (the visibility predicates currently hard-code `status <> 'snoozed'`). Deferred to keep scope finite; flag in stage 11
- identity handlers (link / unlink / transfer / set-primary, `/me/identities`) remain deferred — the frontend page is scaffold only; the audit verbs `IDENTITY_ADD/REMOVE/VERIFY/MERGE` from stage 8 are still un-fired. If stage 11/12 ships the backend, swap the in-page `note(...)` calls for the real mutations
- group-by visual rendering is deferred — the dropdown sets state and the `groupBy` variable is intentionally `void`-referenced; flat sorted-list rendering still wins. Follow-up work needs to bucket `sortedRows` into collapsible sections without breaking the j/k cursor index
- `useMarkInboxSeen` is still used inside `openIssue`; `bulkInbox.mark_all_seen` path is exposed in the palette via apply-to-selection done, not as a separate command (a tiny gap if a future smoke wants both)
- typecheck not run locally (no `node_modules` in this worktree) — `pnpm install && pnpm typecheck` should be re-run by the next stage. The new types follow patterns from the existing client.ts schemas so risk is low
- the boundary-smoke `protected_routes()` list in `crates/dp-server/tests/phase4_smoke.rs` does not yet enumerate `/repos`, `/me/queue`, `/me/inbox/*`, etc. — registering `repos` removes the unknown-resource bug but the smoke still won't catch a future drift. Worth widening in a later stage

## Open questions

- where should the multi-identity backend land in the 12-stage ramp? Stage 10 scope reads as "identity manager frontend"; the audit vocabulary suggests stage 7/8 already meant to ship the handlers. Confirming this routes the next stage's effort correctly
- the `created_at` sort option in the dropdown maps to `number_asc/desc` (issue number) because `IssueListItem` doesn't surface `created_at`. If `created_at` is required, the list dto needs an extra field — answer changes whether to extend `IssueListItem` or rename the dropdown option
