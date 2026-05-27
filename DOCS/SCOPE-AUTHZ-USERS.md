# SCOPE — Authz role management + GitHub identity admin

> Adds operator-controlled **roles** and an admin **users** management
> page to dev-pulse. Sidebar entries are hidden by role tier so a
> Reader never sees the Admin section. Companion to [users.md](../users.md)
> §2.3 (admin user-detail surface) and the `starter-authz` integration
> already wired in [crates/dp-server/policy/dev-pulse.toml](../crates/dp-server/policy/dev-pulse.toml).

---

## 0. TL;DR

dev-pulse already runs `starter-authz` with built-in Reader / Writer /
Admin roles, but **a user's role is hardcoded at session mint**: there
is no UI to change it and no DB column to store it. Operators currently
hand-edit the CLI-seeded admin account and rely on the org-gate for
everyone else.

This change:

1. Adds a persisted `role` column on `dp_users` (`reader` | `writer` |
   `admin`, default `reader`).
2. Wires `Principal.role` from that column at session-mint time.
3. Adds `PUT /admin/users/{id}/role` (and `GET /admin/users/{id}/identities`)
   gated by a new `users:admin` permission.
4. Reshapes `#/admin/users` from a GDPR-only page into a real
   management table — role select per row, linked-GitHub-logins
   column, plus the existing Export / Anonymise actions.
5. Hides sidebar entries the current user can't reach by role tier,
   with a hash-route guard so typing `#/admin/...` as a Reader
   redirects to `#/reports` instead of rendering a 403 flash.

Out of scope: `DbPolicyEngine` migration, `/v1/authz/*` admin REST
surface, admin-initiated identity linking / force-unlink, tenants /
teams (Phase 7 of `starter-authz` SCOPE-EXT).

---

## 1. Why role tier, not per-item authz checks

The user picked **role-based gating** during scope nail-down. The
backend's `with_permission(...)` decorations are the authoritative
boundary; the sidebar gate is a UX convenience. Three tiers (`reader`
< `writer` < `admin`) give us a single `>=` comparison in the client,
no per-item probe round-trip, and a one-line `minRole:` annotation on
each nav entry.

Per-item `(resource, action)` checks would require either a
`POST /v1/authz/check` round-trip per item or a synthesised permission
manifest in `/auth/me`. Both are appropriate when the policy is
edited at runtime (the `DbPolicyEngine` story). dev-pulse's policy is
file-backed and edited by the operator; tiering matches that reality.

---

## 2. Data model

### 2.1 New column

```sql
ALTER TABLE dp_users
    ADD COLUMN role TEXT NOT NULL DEFAULT 'reader'
        CHECK (role IN ('reader', 'writer', 'admin'));
```

- Default `reader` so freshly-OAuth'd users land on the least-priv
  tier. The org-gate rules in
  [dev-pulse.toml](../crates/dp-server/policy/dev-pulse.toml) still
  apply on top — an out-of-org `reader` is still 403.
- The CLI-seeded break-glass admin (`dev-pulse create-admin`) is
  backfilled to `admin` by the same migration so the
  `break-glass-admin-bypass` rule keeps working unchanged.
- No history table. Role changes are infrequent and the audit row
  (`user.role_set`, §4) is the durable record.

### 2.2 Domain type

```rust
// crates/dp-domain/src/lib.rs
pub enum Role { Reader, Writer, Admin }
```

`Role` lives in `dp-domain` (not `dp-rest`) because both the store
layer and the principal-mint path need it. Serialises as the lowercase
string the policy engine already expects.

---

## 3. REST surface

| Method | Path | Permission | Purpose |
|--------|------|------------|---------|
| GET    | `/users` (existing) | `users:read` | Add `role` field to `UserDto`. |
| PUT    | `/admin/users/{id}/role` | `users:admin` | Body `{ "role": "reader" \| "writer" \| "admin" }`. |
| GET    | `/admin/users/{id}/identities` | `users:admin` | Same shape as `/me/identities`. |

`users:admin` is a **new action** on the already-registered `users`
resource. Registered in
[crates/dp-server/src/auth/policy.rs](../crates/dp-server/src/auth/policy.rs)
`register_dev_pulse_resources`. The built-in `Admin` role's blanket
allow (from `default_policy = true`) covers it; `Reader` and `Writer`
do not.

### 3.1 Self-demotion guard

`PUT /admin/users/{id}/role` returns `409 { "error": "cannot_self_demote" }`
when:

- `id == principal.actor_user_id`, AND
- the current role is `admin`, AND
- the requested role is not `admin`.

This prevents an admin locking themselves out with one click.
Break-glass recovery for the genuinely-locked-out case is still the
CLI-seeded admin path (existing behaviour).

### 3.2 Audit

Two new audit verbs in [dp-rest/src/audit.rs](../crates/dp-rest/src/audit.rs):

- `user.role_set` — written on every successful role mutation.
  Payload carries `{ target_user_id, from_role, to_role }`.
- `user.identities_read` — admin-side identity inspection. Read
  auditing is uncommon in dev-pulse but justified here: it surfaces
  whether the operator was looking at a specific user before a
  destructive action.

---

## 4. Frontend

### 4.1 Sidebar gating ([frontend/src/layout/app-shell.tsx](../frontend/src/layout/app-shell.tsx))

Add `minRole?: "reader" | "writer" | "admin"` to `NAV_MAIN` items
(default `reader`). Filter `NAV_MAIN` by
`roleAtLeast(auth.user.role, item.minRole)` before passing to
`<AppSidebar>`.

| Section | minRole | Why |
|---------|---------|-----|
| Reports | reader | default landing |
| Directory | reader | read-only browse |
| Account | reader | self-service |
| Projects | reader | read mostly; project mutations gate server-side |
| Workflow | writer | triage edits issues |
| Admin | admin | every sub-item is admin-only today |

Sub-items inherit the section's `minRole`; no per-sub-item override
needed in this slice.

### 4.2 Route guard ([frontend/src/app.tsx](../frontend/src/app.tsx))

When the hash resolves to a section the user can't reach, redirect to
`#/reports` (the user's lowest-common-denominator landing). The
backend already 403s — this is purely so a typo doesn't render a
permission-denied flash.

### 4.3 `#/admin/users` page reshape ([frontend/src/admin/users-page.tsx](../frontend/src/admin/users-page.tsx))

Replace the single-user `Select` + GDPR-pair with a table:

| Login | Email | Role (select) | Linked logins | Actions |
|-------|-------|---------------|---------------|---------|

- **Role select** — three-option `<Select>`. On change, optimistic
  `PUT /admin/users/{id}/role`; rolls back on error with an inline
  toast. Disabled when the row is the current admin (self-protection
  matches the server guard).
- **Linked logins** — chips, populated by a one-shot fan-out of
  `GET /admin/users/{id}/identities` keyed on the rendered slice.
  For ≤50 users this is a single React Query batch on mount; lazy
  per-row only if that becomes a perf problem.
- **Actions** — keep the existing Export and Anonymise flows.
- **Filter row** — a role filter (`All / Reader / Writer / Admin`)
  and the existing login search.

The GDPR `AlertDialog` retype-to-confirm is preserved verbatim.

### 4.4 `auth.user.role`

Already plumbed via `/auth/me` and the `useAuth()` hook — the value
just becomes meaningful once the backend populates it from the DB
column instead of a hardcoded default.

---

## 5. Tests

- `dp-server/tests/phase4_smoke.rs`:
  - Reader → `PUT /admin/users/{id}/role` → 403.
  - Admin → `PUT /admin/users/self/role` demoting to reader → 409.
  - Admin → `PUT /admin/users/{other}/role` → 200; `GET /users` reflects.
- Frontend Playwright (existing admin-users spec):
  - Sidebar Admin entry hidden for non-admin role.
  - Role select changes the value on reload.

---

## 6. Migration / rollout

1. Migration adds the column with default `reader`.
2. CLI-seeded admin backfilled to `admin` in the same migration.
3. Existing sessions keep working — `Principal.role` is re-derived on
   the next request from the new column.
4. No breaking wire changes: `UserDto` gains a field (clients ignore
   unknowns), no existing endpoint changes shape.

---

## 7. Hard rules

| Rule | Summary |
|------|---------|
| AR1  | `role` lives on `dp_users`; never duplicated in session state. |
| AR2  | Server is the authority on permissions; the sidebar gate is UX. |
| AR3  | Self-demotion from `admin` is refused server-side AND client-side. |
| AR4  | Role mutations write `user.role_set` audit rows. |
| AR5  | Adding a new resource action requires a `register_spec(...)` edit; the policy compile catches drift. |
