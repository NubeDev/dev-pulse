import { z } from "zod";
import { isoDateTime, uuid } from "./common.js";
import { MembershipDtoSchema } from "./directory.js";
import { UserDtoSchema } from "./directory.js";

export const FetchRunErrorSampleDtoSchema = z.object({
  org: z.string().nullable().optional(),
  repo: z.string().nullable().optional(),
  kind: z.string().nullable().optional(),
  error: z.string(),
});
export type FetchRunErrorSampleDto = z.infer<typeof FetchRunErrorSampleDtoSchema>;

export const FetchRunDtoSchema = z.object({
  id: uuid,
  kind: z.string(),
  started: isoDateTime,
  finished: isoDateTime.nullable().optional(),
  items: z.number().int(),
  errors: z.number().int(),
  partial: z.boolean(),
  error_sample: z.array(FetchRunErrorSampleDtoSchema).nullable().optional(),
});
export type FetchRunDto = z.infer<typeof FetchRunDtoSchema>;

export const ExportEventSchema = z.object({
  event_id: uuid,
  org_id: uuid,
  repo_id: uuid,
  kind: z.string(),
  ts: isoDateTime,
  roles: z.array(z.string()),
});
export type ExportEvent = z.infer<typeof ExportEventSchema>;

export const UserExportSchema = z.object({
  user: UserDtoSchema,
  memberships: z.array(MembershipDtoSchema),
  events: z.array(ExportEventSchema),
});
export type UserExport = z.infer<typeof UserExportSchema>;

export const RefreshResponseSchema = z.discriminatedUnion("ran", [
  z.object({
    ran: z.literal(true),
    items: z.number().int(),
    errors: z.number().int(),
    partial: z.boolean(),
  }),
  z.object({
    ran: z.literal(false),
  }),
]);
export type RefreshResponse = z.infer<typeof RefreshResponseSchema>;

export const ImportRepoRequestSchema = z.object({
  owner: z.string().min(1),
  name: z.string().min(1),
});
export type ImportRepoRequest = z.infer<typeof ImportRepoRequestSchema>;

export const ImportRepoResponseSchema = z.object({
  org_id: uuid,
  repo_id: uuid,
  created: z.boolean(),
});
export type ImportRepoResponse = z.infer<typeof ImportRepoResponseSchema>;
