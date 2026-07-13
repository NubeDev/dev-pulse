import { z } from "zod";
import { isoDateTime, uuid } from "./common.js";

export const OrgDtoSchema = z.object({
  id: uuid,
  github_id: z.number().int(),
  login: z.string(),
  name: z.string().nullable().optional(),
});
export type OrgDto = z.infer<typeof OrgDtoSchema>;

export const TeamDtoSchema = z.object({
  id: uuid,
  org_id: uuid,
  github_id: z.number().int(),
  slug: z.string(),
  name: z.string(),
});
export type TeamDto = z.infer<typeof TeamDtoSchema>;

// Operator-controlled role tier (DOCS/SCOPE-AUTHZ-USERS.md §3).
// Older payloads without `role` are tolerated and default to "reader"
// so a frontend deployed against a slightly-older server doesn't 5xx.
export const UserRoleSchema = z.enum(["reader", "writer", "admin"]);
export type UserRole = z.infer<typeof UserRoleSchema>;

export const UserDtoSchema = z.object({
  id: uuid,
  github_id: z.number().int(),
  login: z.string(),
  name: z.string().nullable().optional(),
  email: z.string().nullable().optional(),
  role: UserRoleSchema,
});
export type UserDto = z.infer<typeof UserDtoSchema>;

// Body of PUT /admin/users/{id}/role.
export const SetUserRoleRequestSchema = z.object({
  role: UserRoleSchema,
});
export type SetUserRoleRequest = z.infer<typeof SetUserRoleRequestSchema>;

// Body of PUT /admin/users/{id} (issue #14). Each field is optional:
// omit to leave unchanged, send `null` to clear. `login` and `role`
// are not editable here (login is GitHub-owned; role has its own
// endpoint).
export const UpdateUserRequestSchema = z.object({
  name: z.string().nullable().optional(),
  email: z.string().nullable().optional(),
});
export type UpdateUserRequest = z.infer<typeof UpdateUserRequestSchema>;

// Wire shape of GET /admin/users/{id}/identities (same as
// GET /me/identities — see dp_rest::me_identities).
export const AdminUserIdentityDtoSchema = z.object({
  id: z.string(),
  provider: z.string(),
  email: z.string().nullable().optional(),
  display_name: z.string().nullable().optional(),
  linked_at: isoDateTime,
  is_primary: z.boolean(),
});
export type AdminUserIdentityDto = z.infer<typeof AdminUserIdentityDtoSchema>;

export const AdminUserIdentitiesResponseSchema = z.object({
  identities: z.array(AdminUserIdentityDtoSchema),
  primary_id: z.string().nullable().optional(),
});
export type AdminUserIdentitiesResponse = z.infer<
  typeof AdminUserIdentitiesResponseSchema
>;

export const MembershipDtoSchema = z.object({
  user_id: uuid,
  org_id: uuid,
  role: z.string(),
  joined_at: isoDateTime,
  home_org: uuid.nullable().optional(),
});
export type MembershipDto = z.infer<typeof MembershipDtoSchema>;

export const SetHomeOrgRequestSchema = z.object({
  user_id: uuid,
  org_id: uuid,
});
export type SetHomeOrgRequest = z.infer<typeof SetHomeOrgRequestSchema>;
