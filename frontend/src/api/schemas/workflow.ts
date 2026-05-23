import { z } from "zod";
import { isoDateTime, uuid } from "./common.js";

// ---------------------------------------------------------------------------
// Pins (SCOPE-PROJECTS §6.4)
// ---------------------------------------------------------------------------

export const PIN_CAP = 20;
export const PIN_RENDER_CAP = 50;
export const TAGS_GROUP_BY_CAP = 50;
export const TAG_LINK_WARN_THRESHOLD = 500;

export const PinKindSchema = z.enum(["repo", "tag"]);
export type PinKind = z.infer<typeof PinKindSchema>;

export const PinDtoSchema = z.object({
  kind: PinKindSchema,
  target_id: uuid,
  position: z.number().int(),
  pinned_at: isoDateTime,
});
export type PinDto = z.infer<typeof PinDtoSchema>;

export const AddPinRequestSchema = z.object({
  kind: PinKindSchema,
  target_id: uuid,
});
export type AddPinRequest = z.infer<typeof AddPinRequestSchema>;

export const PinKeyDtoSchema = z.object({
  kind: PinKindSchema,
  target_id: uuid,
});
export type PinKeyDto = z.infer<typeof PinKeyDtoSchema>;

export const ReorderRequestSchema = z.object({
  order: z.array(PinKeyDtoSchema),
});
export type ReorderRequest = z.infer<typeof ReorderRequestSchema>;

// ---------------------------------------------------------------------------
// Tags (SCOPE-PROJECTS §7)
// ---------------------------------------------------------------------------

export const TagScopeKindSchema = z.enum(["user", "team", "org"]);
export type TagScopeKind = z.infer<typeof TagScopeKindSchema>;

export const TagLinkKindSchema = z.enum(["repo", "issue", "user", "team"]);
export type TagLinkKind = z.infer<typeof TagLinkKindSchema>;

export const TagDtoSchema = z.object({
  id: uuid,
  scope_kind: TagScopeKindSchema,
  scope_id: uuid,
  name: z.string(),
  color: z.string(),
  description: z.string().nullable().optional(),
  created_by: uuid,
  created_at: isoDateTime,
  archived_at: isoDateTime.nullable().optional(),
  visible_link_count: z.number().int(),
});
export type TagDto = z.infer<typeof TagDtoSchema>;

export const TagLinkDtoSchema = z.object({
  id: uuid,
  tag_id: uuid,
  kind: TagLinkKindSchema,
  target_id: uuid,
  added_by: uuid,
  added_at: isoDateTime,
});
export type TagLinkDto = z.infer<typeof TagLinkDtoSchema>;

export const TagDetailResponseSchema = z.object({
  tag: TagDtoSchema,
  links: z.array(TagLinkDtoSchema),
  links_page: z.number().int(),
  links_page_size: z.number().int(),
});
export type TagDetailResponse = z.infer<typeof TagDetailResponseSchema>;

export const CreateTagRequestSchema = z.object({
  scope_kind: TagScopeKindSchema,
  scope_id: uuid,
  name: z.string(),
  color: z.string(),
  description: z.string().nullable().optional(),
});
export type CreateTagRequest = z.infer<typeof CreateTagRequestSchema>;

export const UpdateTagRequestSchema = z.object({
  name: z.string().optional(),
  color: z.string().optional(),
  description: z.string().nullable().optional(),
  archived: z.boolean().optional(),
});
export type UpdateTagRequest = z.infer<typeof UpdateTagRequestSchema>;

export const LinkRequestItemSchema = z.object({
  kind: TagLinkKindSchema,
  target_id: uuid,
});
export type LinkRequestItem = z.infer<typeof LinkRequestItemSchema>;

export const LinkBatchRequestSchema = z.object({
  items: z.array(LinkRequestItemSchema),
});
export type LinkBatchRequest = z.infer<typeof LinkBatchRequestSchema>;

export const LinkBatchResponseSchema = z.object({
  linked: z.array(TagLinkDtoSchema),
  warning: z.string().optional(),
});
export type LinkBatchResponse = z.infer<typeof LinkBatchResponseSchema>;

// ---------------------------------------------------------------------------
// App install banner (§13.6)
// ---------------------------------------------------------------------------

export const AppInstallBannerOrgDtoSchema = z.object({
  org_id: uuid,
  login: z.string(),
  name: z.string().nullable().optional(),
  writes_available: z.boolean(),
  manage_url: z.string().optional(),
  admin_copy_text: z.string(),
});
export type AppInstallBannerOrgDto = z.infer<typeof AppInstallBannerOrgDtoSchema>;

export const AppInstallBannerResponseSchema = z.object({
  request_issues_write: z.boolean(),
  orgs: z.array(AppInstallBannerOrgDtoSchema),
});
export type AppInstallBannerResponse = z.infer<typeof AppInstallBannerResponseSchema>;
