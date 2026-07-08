# TODO — No way to update an existing user

Ref: https://github.com/NubeDev/dev-pulse/issues/14

## Open

- [ ] **There is no endpoint to update an existing user.** A user's
  core attributes (`name`, `email`, `login`) are immutable through the
  API. The only mutation to a user row is the single-field
  `PUT /admin/users/{id}/role`
  ([crates/dp-rest/src/admin.rs:913](../../crates/dp-rest/src/admin.rs#L913),
  handler `set_user_role` at
  [crates/dp-rest/src/admin.rs:824](../../crates/dp-rest/src/admin.rs#L824)),
  which updates **only** the `role` field. There is no
  `PUT /users/{id}` or `PATCH /users/{id}` for general edits.

  This is consistent and intentional across every layer, which is why
  it's a missing feature rather than a bug:

  - **Routes:** the full user surface is `GET /users`
    ([crates/dp-rest/src/directory.rs:336](../../crates/dp-rest/src/directory.rs#L336)),
    `POST /home-org` (membership flip, not the user row),
    `POST /admin/users/{id}/anonymise` (irreversible GDPR pseudonymisation,
    [crates/dp-rest/src/admin.rs:900](../../crates/dp-rest/src/admin.rs#L900)),
    `GET /admin/users/{id}/export`, and
    `PUT /admin/users/{id}/role`. No general write route.
  - **Store trait:** `crates/dp-domain/src/store.rs:294-370` exposes
    `upsert_user` (`:299`, used only by the fetcher/reconciler for
    ingestion, not over HTTP), `set_user_role` (`:356`), and
    `pseudonymise_user` (`:370`). A grep for
    `update_user|edit_user|patch_user|modify_user` across `crates/`
    returns nothing.
  - **Authz:** the `users` resource is registered with actions
    `["read", "admin"]` only
    ([crates/dp-server/src/auth/policy.rs:69-79](../../crates/dp-server/src/auth/policy.rs#L69-L79)).
    Editable resources (`pins`, `tags`, `projects`, …) all carry a
    `"write"` action; `users` does not.
  - **Audit:** the pinned verb vocabulary
    ([crates/dp-rest/src/audit.rs:43-60](../../crates/dp-rest/src/audit.rs#L43-L60))
    has `user.anonymise`, `user.export`, `user.role_set`, and
    `user.identities_read` — no `user.update`.
  - **Frontend:** `frontend/src/api/dev-pulse-api.ts` has `listUsers`
    (`:365`), `setUserRole` (`:406`), `anonymiseUser` (`:396`),
    `exportUser` (`:400`), `setHomeOrg` (`:370`) — no
    `updateUser`/`patchUser`. Schemas in
    [frontend/src/api/schemas/directory.ts](../../frontend/src/api/schemas/directory.ts)
    (`UserDtoSchema` `:27-35`, `SetUserRoleRequestSchema` `:38-41`)
    likewise have no `UpdateUserRequestSchema`.
  - **Scope doc:** `users.md:193` explicitly states the directory
    surface is read-only — "What it does NOT do. Any write. Period."

  | CRUD op | Status | Note |
  |---|---|---|
  | Create | Implicit only | Via `upsert_user` during reconciler ingestion; no `POST /users`. |
  | Read | Yes | `GET /users`, `GET /admin/users/{id}/export`, `GET /admin/users/{id}/identities`. |
  | Update | **No (role-only)** | Only `PUT /admin/users/{id}/role`; no way to edit name/email/login. |
  | Delete | Destructive only | `POST /admin/users/{id}/anonymise` is irreversible pseudonymisation, not a true delete. |

  **Proposed fix:** add a `PATCH /users/{id}` (or `PUT`) endpoint that
  accepts editable fields (e.g. `name`, `email`). This requires:

  1. A new store method (e.g. `update_user`) in
     [crates/dp-domain/src/store.rs](../../crates/dp-domain/src/store.rs)
     plus a pg implementation.
  2. A `"write"` action on the `users` authz resource
     ([crates/dp-server/src/auth/policy.rs:69-79](../../crates/dp-server/src/auth/policy.rs#L69-L79)).
  3. A new audit verb (e.g. `user.update`) in
     [crates/dp-rest/src/audit.rs](../../crates/dp-rest/src/audit.rs).
  4. A route + handler in
     [crates/dp-rest/src/directory.rs](../../crates/dp-rest/src/directory.rs)
     (or `admin.rs` if admin-only).
  5. Frontend client function + `UpdateUserRequestSchema` in
     [frontend/src/api/schemas/directory.ts](../../frontend/src/api/schemas/directory.ts),
    and a UI surface to edit users.
