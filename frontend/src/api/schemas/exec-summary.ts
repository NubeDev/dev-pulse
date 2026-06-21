/**
 * Project Executive Summary — schemas (SCOPE-PROJECT-EXECUTIVE-SUMMARY.md).
 *
 * Mirrors the REST surface in §3.2 and the section payloads in §3.1.
 * Every long-text field on the form is markdown; numbers + dates +
 * enums are typed strictly so the form can render the right control.
 */

import { z } from "zod";
import { isoDateTime, uuid } from "./common.js";

// ---------------------------------------------------------------------------
// Section payloads — one shape per tab. All fields are optional on the
// wire so partial saves carry only what the user changed (§4.3).
// ---------------------------------------------------------------------------

export const ExecSummarySummarySchema = z.object({
  product_name: z.string().nullable().optional(),
  part_number: z.string().nullable().optional(),
  target_release_date: z
    .string()
    .regex(/^\d{4}-\d{2}-\d{2}$/, "expected YYYY-MM-DD")
    .nullable()
    .optional(),
  objective: z.string().nullable().optional(),
  problem: z.string().nullable().optional(),
  value: z.string().nullable().optional(),
  differentiators: z.string().nullable().optional(),
  success_criteria: z.string().nullable().optional(),
});
export type ExecSummarySummary = z.infer<typeof ExecSummarySummarySchema>;

export const ExecSummaryScopeSchema = z.object({
  in_scope: z.string().nullable().optional(),
  out_of_scope: z.string().nullable().optional(),
  assumptions: z.string().nullable().optional(),
  dependencies: z.string().nullable().optional(),
  constraints: z.string().nullable().optional(),
});
export type ExecSummaryScope = z.infer<typeof ExecSummaryScopeSchema>;

/** Closed set of supported field protocols (spec §3.1 — ~12 values). */
export const PROTOCOL_OPTIONS = [
  "BACnet MS/TP",
  "BACnet IP",
  "Modbus RTU",
  "Modbus TCP",
  "KNX",
  "LonWorks",
  "M-Bus",
  "MQTT",
  "OPC UA",
  "Zigbee",
  "Z-Wave",
  "LoRaWAN",
] as const;
export type Protocol = (typeof PROTOCOL_OPTIONS)[number];

export const ExecSummaryRequirementsSchema = z.object({
  must_have: z.string().nullable().optional(),
  optional: z.string().nullable().optional(),
  user_interaction: z.string().nullable().optional(),
  architecture: z.string().nullable().optional(),
  protocols: z.array(z.string()).optional(),
  power: z.string().nullable().optional(),
  mounting: z.string().nullable().optional(),
  certification: z.string().nullable().optional(),
  /** LoRa details — free text, e.g. "AU915, SF7–SF12" (feedback #3). */
  lora: z.string().nullable().optional(),
  /** WiFi details — free text, e.g. "2.4 GHz b/g/n, WPA2" (feedback #3). */
  wifi: z.string().nullable().optional(),
  /** General free-text notes on the Requirements section (feedback #3). */
  notes: z.string().nullable().optional(),
});
export type ExecSummaryRequirements = z.infer<
  typeof ExecSummaryRequirementsSchema
>;

export const ExecSummaryHardwareSchema = z.object({
  hardware_features: z.string().nullable().optional(),
  physical_notes: z.string().nullable().optional(),
  enclosure: z.string().nullable().optional(),
  mounting_type: z.string().nullable().optional(),
  operating_env: z.string().nullable().optional(),
});
export type ExecSummaryHardware = z.infer<typeof ExecSummaryHardwareSchema>;

export const ExecSummaryCommercialSchema = z.object({
  rrp_cents: z.number().int().nullable().optional(),
  oem_price_cents: z.number().int().nullable().optional(),
  target_gp_pct: z.number().nullable().optional(),
  revenue_model: z.string().nullable().optional(),
  channel_strategy: z.string().nullable().optional(),
  target_market: z.string().nullable().optional(),
  volume_assumptions: z.string().nullable().optional(),
});
export type ExecSummaryCommercial = z.infer<
  typeof ExecSummaryCommercialSchema
>;

export const EXEC_SUMMARY_STATUSES = ["draft", "in_review", "approved"] as const;
export const ExecSummaryStatusSchema = z.enum(EXEC_SUMMARY_STATUSES);
export type ExecSummaryStatus = z.infer<typeof ExecSummaryStatusSchema>;

export const ExecSummaryApprovalSchema = z.object({
  status: ExecSummaryStatusSchema,
  reviewer: z.string().nullable().optional(),
  approver: z.string().nullable().optional(),
  review_notes: z.string().nullable().optional(),
  approval_notes: z.string().nullable().optional(),
  submitted_at: isoDateTime.nullable().optional(),
  approved_at: isoDateTime.nullable().optional(),
});
export type ExecSummaryApproval = z.infer<typeof ExecSummaryApprovalSchema>;

// ---------------------------------------------------------------------------
// Attached files (Hardware images + Documents)
// ---------------------------------------------------------------------------

export const ExecSummaryImageDtoSchema = z.object({
  id: uuid,
  project_id: uuid,
  url: z.string(),
  filename: z.string(),
  content_type: z.string(),
  caption: z.string().nullable().optional(),
  ord: z.number().int(),
  created_at: isoDateTime,
});
export type ExecSummaryImageDto = z.infer<typeof ExecSummaryImageDtoSchema>;

export const ExecSummaryDocumentDtoSchema = z.object({
  id: uuid,
  project_id: uuid,
  url: z.string(),
  title: z.string(),
  doc_type: z.string().nullable().optional(),
  notes: z.string().nullable().optional(),
  required_action: z.string().nullable().optional(),
  uploaded_by: z.string().nullable().optional(),
  filename: z.string(),
  content_type: z.string(),
  created_at: isoDateTime,
});
export type ExecSummaryDocumentDto = z.infer<
  typeof ExecSummaryDocumentDtoSchema
>;

export const ExecSummaryChangelogEntrySchema = z.object({
  id: uuid,
  project_id: uuid,
  version: z.string(),
  changed_at: z
    .string()
    .regex(/^\d{4}-\d{2}-\d{2}$/, "expected YYYY-MM-DD"),
  changed_by: z.string(),
  summary: z.string(),
  /** True when the entry carries a content snapshot and can be
   *  restored. Always present on responses from the snapshot-aware
   *  backend (0057+). */
  has_snapshot: z.boolean(),
  created_at: isoDateTime,
});
export type ExecSummaryChangelogEntry = z.infer<
  typeof ExecSummaryChangelogEntrySchema
>;

// ---------------------------------------------------------------------------
// Completion (spec §3.5)
// ---------------------------------------------------------------------------

export const EXEC_SUMMARY_SECTIONS = [
  "summary",
  "scope",
  "requirements",
  "hardware",
  "commercial",
  "documents",
  "approval",
  "changelog",
] as const;
export type ExecSummarySectionId = (typeof EXEC_SUMMARY_SECTIONS)[number];

export const ExecSummaryCompletionSchema = z.object({
  percent: z.number().int().min(0).max(100),
  sections: z.record(z.string(), z.boolean()),
});
export type ExecSummaryCompletion = z.infer<
  typeof ExecSummaryCompletionSchema
>;

// ---------------------------------------------------------------------------
// Top-level DTO — the GET response
// ---------------------------------------------------------------------------

export const ExecSummaryDtoSchema = z.object({
  project_id: uuid,
  summary: ExecSummarySummarySchema,
  scope: ExecSummaryScopeSchema,
  requirements: ExecSummaryRequirementsSchema,
  hardware: ExecSummaryHardwareSchema,
  commercial: ExecSummaryCommercialSchema,
  approval: ExecSummaryApprovalSchema,
  images: z.array(ExecSummaryImageDtoSchema),
  documents: z.array(ExecSummaryDocumentDtoSchema),
  changelog: z.array(ExecSummaryChangelogEntrySchema),
  completion: ExecSummaryCompletionSchema,
  /** Section ids marked "N/A" by the user. Server already OR's
   *  these into `completion.sections` — kept on the envelope so the
   *  UI can render the per-section badge without recomputing. */
  skipped_sections: z.array(z.string()),
  updated_at: isoDateTime,
});
export type ExecSummaryDto = z.infer<typeof ExecSummaryDtoSchema>;

// ---------------------------------------------------------------------------
// Write requests — partial section payloads (§4.3 PATCH)
// ---------------------------------------------------------------------------

export interface PatchExecSummaryRequest {
  summary?: ExecSummarySummary;
  scope?: ExecSummaryScope;
  requirements?: ExecSummaryRequirements;
  hardware?: ExecSummaryHardware;
  commercial?: ExecSummaryCommercial;
  approval?: Pick<
    ExecSummaryApproval,
    "reviewer" | "approver" | "review_notes" | "approval_notes"
  >;
  /** Replace the user-marked "N/A" set wholesale. Empty array
   *  clears every skip; omitted leaves it untouched. */
  skipped_sections?: string[];
}

export interface AddChangelogEntryRequest {
  version: string;
  changed_at: string;
  changed_by: string;
  summary: string;
}

/** Body for restoring a previous revision. Same shape as
 *  {@link AddChangelogEntryRequest} — it describes the *new* entry
 *  that records the roll-back. */
export type RestoreChangelogEntryRequest = AddChangelogEntryRequest;

export interface ApproveExecSummaryRequest {
  approval_notes?: string | null;
}
