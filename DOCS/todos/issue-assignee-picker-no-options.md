# TODO — Issue assignee picker shows NO options

## Resolved

- [x] **Fixed:** added `u.role` to the `SELECT` in
  `list_users_for_org_impl`
  ([crates/dp-store-pg/src/store/orgs.rs:180](../../crates/dp-store-pg/src/store/orgs.rs#L180)),
  mirroring `list_users_impl`. The org-scoped query now returns 200
  with a populated `role`, so the assignee / project-lead pickers
  populate. Regression test: `list_users_for_org_includes_role` in
  [crates/dp-store-pg/tests/integration.rs](../../crates/dp-store-pg/tests/integration.rs)
  (runs under `cargo test -p dp-store-pg -- --ignored`). The picker's
  frontend already surfaces a fetch failure ("Failed to load
  members.") so the silent-empty state is doubly covered.

  Secondary note (repo collaborators who aren't full GitHub org
  members are still excluded, since the query inner-joins
  `dp_memberships`) remains an open product decision — see below.

## Original diagnosis

- [ ] **`GET /users?org_id=…` 500s, so the assignee picker silently
  renders zero options.** Reproduced live against
  https://dev-pulse.fly.dev/ (`dev@dev.com`): opening any issue's
  "Assignees" field
  ([frontend/src/workflow/issues-page.tsx:1123-1124](../../frontend/src/workflow/issues-page.tsx#L1123-L1124),
  `<UserLoginsPicker orgId={issue.org_id} .../>`) shows an empty
  dropdown even though the org has 100+ known users.

  **Root cause:** `list_users_for_org_impl` in
  [crates/dp-store-pg/src/store/orgs.rs:178-191](../../crates/dp-store-pg/src/store/orgs.rs#L178-L191)
  selects
  ```sql
  SELECT u.id, u.github_id, u.login, u.email, u.name, u.deleted_at
  FROM dp_users u JOIN dp_memberships m ON m.user_id = u.id
  WHERE m.org_id = $1 AND u.deleted_at IS NULL ORDER BY u.login
  ```
  — it omits the `role` column. But the shared row mapper
  `row_to_user`
  ([crates/dp-store-pg/src/store/rows.rs:94-113](../../crates/dp-store-pg/src/store/rows.rs#L94-L113))
  unconditionally does `r.try_get("role")` (line 100) and fails the
  whole row (and thus the whole request) when that column isn't in
  the result set. The sibling query `list_users_impl` (no org filter,
  [crates/dp-store-pg/src/store/users.rs:73-82](../../crates/dp-store-pg/src/store/users.rs#L73-L82))
  *does* select `role` and works fine — confirmed live:
  `GET /users` → 200 with the full directory;
  `GET /users?org_id=<uuid>` → `{"error":"internal error","code":"store_error"}`.

  The frontend never surfaces this failure. `useOrgUsers`
  ([frontend/src/components/user-picker.tsx:32-39](../../frontend/src/components/user-picker.tsx#L32-L39))
  is a plain `useQuery` with no error UI, and the options list is
  built from `users.data ?? []`
  ([frontend/src/components/user-picker.tsx:87](../../frontend/src/components/user-picker.tsx#L87)) —
  a failed fetch just renders as "no options" with zero indication
  anything went wrong. Same code path backs both `UserPicker`
  (project `lead_user_id`) and `UserLoginsPicker` (issue
  `assignees`), so project-lead pickers are equally broken.

  **Fix:** add `u.role` to the `SELECT` in `list_users_for_org_impl`
  (`orgs.rs:180`), mirroring `list_users_impl`. That's the crash fix;
  should be a one-line change plus a regression test asserting
  `GET /users?org_id=` returns 200 with a populated role.

  **Separate, secondary issue to confirm after the fix ships:**
  `list_users_for_org_impl` inner-joins `dp_memberships`, which is
  only populated by the GitHub org-membership reconciler
  (`upsert_membership_impl` called from
  [crates/dp-fetcher/src/reconciler/mod.rs:393](../../crates/dp-fetcher/src/reconciler/mod.rs#L393)).
  A `dp_users` row can exist (e.g. from issue/PR authorship) without
  a matching `dp_memberships` row for the org, so repo
  collaborators/authors who aren't full GitHub org members will
  still be excluded from the picker even once the 500 is fixed —
  and there is no manual "add org/project member" UI to work around
  it. Worth a follow-up decision on whether that's intended scope
  (org members only) or whether the picker should fall back to repo
  collaborators too.
