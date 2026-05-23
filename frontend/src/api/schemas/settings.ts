import { z } from "zod";
import { isoDateTime, uuid } from "./common.js";

// ---------------------------------------------------------------------------
// Per-user settings (Account → Settings)
// ---------------------------------------------------------------------------

export const SettingDtoSchema = z.object({
  key: z.string(),
  label: z.string(),
  help: z.string(),
  is_secret: z.boolean(),
  has_value: z.boolean(),
  value: z.string().nullable(),
  updated_at: isoDateTime.nullable().optional(),
});
export type SettingDto = z.infer<typeof SettingDtoSchema>;

export const PutSettingRequestSchema = z.object({
  value: z.string(),
});
export type PutSettingRequest = z.infer<typeof PutSettingRequestSchema>;

export const TestGithubPatResponseSchema = z.discriminatedUnion("ok", [
  z.object({
    ok: z.literal("true"),
    login: z.string(),
    name: z.string().nullable(),
    account_type: z.string().nullable(),
  }),
  z.object({
    ok: z.literal("false"),
    code: z.string(),
    message: z.string(),
  }),
]);
export type TestGithubPatResponse = z.infer<typeof TestGithubPatResponseSchema>;
