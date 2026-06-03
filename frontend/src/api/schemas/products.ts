/**
 * Product & Manufacturing — schemas (DOCS/ideas/product-manufacturing.md).
 *
 * Mirrors the dp-rest DTOs in `crates/dp-rest/src/{products,parties,
 * product_manuals}.rs`. P1 surface: products, parties (customers /
 * manufacturers / suppliers), project links, documents, manuals +
 * revisions.
 */

import { z } from "zod";
import { isoDateTime, uuid } from "./common.js";

// ---------------------------------------------------------------------------
// Products
// ---------------------------------------------------------------------------

export const ProductStatusSchema = z.enum(["draft", "active", "eol", "archived"]);
export type ProductStatus = z.infer<typeof ProductStatusSchema>;

export const ProductDtoSchema = z.object({
  id: uuid,
  org_id: uuid,
  name: z.string(),
  model_number: z.string(),
  description: z.string().nullable().optional(),
  manufacturer_id: uuid.nullable().optional(),
  status: ProductStatusSchema,
  serial_prefix: z.string().nullable().optional(),
  serial_format: z.string().nullable().optional(),
  archived_at: isoDateTime.nullable().optional(),
  created_by: uuid.nullable().optional(),
  version: z.number(),
  created_at: isoDateTime,
  updated_at: isoDateTime,
});
export type ProductDto = z.infer<typeof ProductDtoSchema>;

export const ProductListResponseSchema = z.object({
  rows: z.array(ProductDtoSchema),
  total: z.number(),
  limit: z.number(),
  offset: z.number(),
});
export type ProductListResponse = z.infer<typeof ProductListResponseSchema>;

export type ListProductsQuery = {
  org_id?: string;
  status?: ProductStatus;
  q?: string;
  limit?: number;
  offset?: number;
  count_only?: boolean;
};

export type CreateProductRequest = {
  org_id: string;
  name: string;
  model_number: string;
  description?: string | null;
  manufacturer_id?: string | null;
  status?: ProductStatus;
  serial_prefix?: string | null;
  serial_format?: string | null;
};

export type PatchProductRequest = {
  expected_version: number;
  name: string;
  model_number: string;
  description?: string | null;
  manufacturer_id?: string | null;
  status: ProductStatus;
  serial_prefix?: string | null;
  serial_format?: string | null;
};

export type ArchiveProductRequest = { expected_version: number };

// ---------------------------------------------------------------------------
// Product documents
// ---------------------------------------------------------------------------

export const ProductDocumentDtoSchema = z.object({
  id: uuid,
  product_id: uuid,
  url: z.string(),
  title: z.string(),
  doc_type: z.string().nullable().optional(),
  notes: z.string().nullable().optional(),
  uploaded_by: z.string().nullable().optional(),
  created_at: isoDateTime,
});
export type ProductDocumentDto = z.infer<typeof ProductDocumentDtoSchema>;

// ---------------------------------------------------------------------------
// Parties (manufacturers / suppliers share PartyDto; customers add account_ref)
// ---------------------------------------------------------------------------

export const PartyDtoSchema = z.object({
  id: uuid,
  org_id: uuid,
  name: z.string(),
  contact_name: z.string().nullable().optional(),
  email: z.string().nullable().optional(),
  phone: z.string().nullable().optional(),
  address: z.string().nullable().optional(),
  website: z.string().nullable().optional(),
  notes: z.string().nullable().optional(),
  archived_at: isoDateTime.nullable().optional(),
  version: z.number(),
  created_at: isoDateTime,
  updated_at: isoDateTime,
});
export type PartyDto = z.infer<typeof PartyDtoSchema>;

export const PartyListResponseSchema = z.object({
  rows: z.array(PartyDtoSchema),
  total: z.number(),
  limit: z.number(),
  offset: z.number(),
});
export type PartyListResponse = z.infer<typeof PartyListResponseSchema>;

export const CustomerDtoSchema = PartyDtoSchema.extend({
  account_ref: z.string().nullable().optional(),
});
export type CustomerDto = z.infer<typeof CustomerDtoSchema>;

export const CustomerListResponseSchema = z.object({
  rows: z.array(CustomerDtoSchema),
  total: z.number(),
  limit: z.number(),
  offset: z.number(),
});
export type CustomerListResponse = z.infer<typeof CustomerListResponseSchema>;

export type ListPartiesQuery = {
  org_id?: string;
  q?: string;
  include_archived?: boolean;
  limit?: number;
  offset?: number;
  count_only?: boolean;
};

export type CreatePartyRequest = {
  org_id: string;
  name: string;
  contact_name?: string | null;
  email?: string | null;
  phone?: string | null;
  address?: string | null;
  website?: string | null;
  notes?: string | null;
};

export type PatchPartyRequest = {
  expected_version: number;
  name: string;
  contact_name?: string | null;
  email?: string | null;
  phone?: string | null;
  address?: string | null;
  website?: string | null;
  notes?: string | null;
};

export type CreateCustomerRequest = CreatePartyRequest & { account_ref?: string | null };
export type PatchCustomerRequest = PatchPartyRequest & { account_ref?: string | null };
export type ArchivePartyRequest = { expected_version: number };

// ---------------------------------------------------------------------------
// Manuals + revisions
// ---------------------------------------------------------------------------

export const RevisionStatusSchema = z.enum(["draft", "published", "superseded"]);
export type RevisionStatus = z.infer<typeof RevisionStatusSchema>;

export const ManualDtoSchema = z.object({
  id: uuid,
  product_id: uuid,
  title: z.string(),
  created_by: uuid.nullable().optional(),
  version: z.number(),
  created_at: isoDateTime,
  updated_at: isoDateTime,
});
export type ManualDto = z.infer<typeof ManualDtoSchema>;

export const ManualRevisionDtoSchema = z.object({
  id: uuid,
  manual_id: uuid,
  revision: z.string(),
  status: RevisionStatusSchema,
  body_md: z.string(),
  change_note: z.string().nullable().optional(),
  authored_by: uuid.nullable().optional(),
  created_at: isoDateTime,
});
export type ManualRevisionDto = z.infer<typeof ManualRevisionDtoSchema>;

export type CreateManualRequest = { title: string };
export type CreateRevisionRequest = { revision: string; body_md: string; change_note?: string | null };

// ---------------------------------------------------------------------------
// P2 — manufacturing runs, serialised units, EOL
// ---------------------------------------------------------------------------

export const RunStatusSchema = z.enum(["planned", "in_progress", "completed", "cancelled"]);
export type RunStatus = z.infer<typeof RunStatusSchema>;

export const RunDtoSchema = z.object({
  id: uuid,
  org_id: uuid,
  product_id: uuid,
  manufacturer_id: uuid.nullable().optional(),
  run_code: z.string(),
  status: RunStatusSchema,
  qty_planned: z.number(),
  qty_built: z.number(),
  qty_passed: z.number(),
  qty_failed: z.number(),
  next_serial_seq: z.number(),
  started_at: isoDateTime.nullable().optional(),
  completed_at: isoDateTime.nullable().optional(),
  notes: z.string().nullable().optional(),
  version: z.number(),
  created_at: isoDateTime,
  updated_at: isoDateTime,
});
export type RunDto = z.infer<typeof RunDtoSchema>;

export type CreateRunRequest = {
  org_id: string;
  manufacturer_id?: string | null;
  run_code: string;
  status?: RunStatus;
  qty_planned?: number;
  notes?: string | null;
};
export type PatchRunRequest = {
  expected_version: number;
  manufacturer_id?: string | null;
  run_code: string;
  status: RunStatus;
  qty_planned: number;
  started_at?: string | null;
  completed_at?: string | null;
  notes?: string | null;
};

export const UnitStatusSchema = z.enum(["built", "tested", "shipped", "returned", "scrapped"]);
export type UnitStatus = z.infer<typeof UnitStatusSchema>;

export const UnitDtoSchema = z.object({
  id: uuid,
  org_id: uuid,
  product_id: uuid,
  run_id: uuid.nullable().optional(),
  serial_number: z.string(),
  status: UnitStatusSchema,
  customer_id: uuid.nullable().optional(),
  built_at: isoDateTime.nullable().optional(),
  shipped_at: isoDateTime.nullable().optional(),
  qr_url: z.string().nullable().optional(),
  version: z.number(),
  created_at: isoDateTime,
  updated_at: isoDateTime,
});
export type UnitDto = z.infer<typeof UnitDtoSchema>;

export const UnitAllocationDtoSchema = z.object({
  units: z.array(UnitDtoSchema),
  first_seq: z.number(),
  count: z.number(),
});
export type UnitAllocationDto = z.infer<typeof UnitAllocationDtoSchema>;

export type AllocateUnitsRequest = { count: number };
export type PatchUnitRequest = {
  expected_version: number;
  status: UnitStatus;
  customer_id?: string | null;
  built_at?: string | null;
  shipped_at?: string | null;
};

export const EolResultSchema = z.enum(["pass", "fail"]);
export type EolResult = z.infer<typeof EolResultSchema>;

export const EolReportDtoSchema = z.object({
  id: uuid,
  unit_id: uuid,
  result: EolResultSchema,
  station: z.string().nullable().optional(),
  firmware: z.string().nullable().optional(),
  measurements: z.unknown(),
  notes: z.string().nullable().optional(),
  tested_by: z.string().nullable().optional(),
  tested_at: isoDateTime,
});
export type EolReportDto = z.infer<typeof EolReportDtoSchema>;

export type RecordEolRequest = {
  result: EolResult;
  station?: string | null;
  firmware?: string | null;
  measurements?: Record<string, unknown>;
  notes?: string | null;
  tested_by?: string | null;
};

export const RunEolSummaryDtoSchema = z.object({
  run_id: uuid,
  built_count: z.number(),
  pass_count: z.number(),
  fail_count: z.number(),
  notes_md: z.string().nullable().optional(),
  signed_by: uuid.nullable().optional(),
  signed_at: isoDateTime.nullable().optional(),
  version: z.number(),
});
export type RunEolSummaryDto = z.infer<typeof RunEolSummaryDtoSchema>;

export type RunEolSummaryRequest = { notes_md?: string | null; sign_off?: boolean };

export const PublicUnitDtoSchema = z.object({
  serial_number: z.string(),
  model_number: z.string(),
  product_name: z.string(),
  status: UnitStatusSchema,
  manuals: z.array(z.object({ title: z.string(), revision: z.string() })),
});
export type PublicUnitDto = z.infer<typeof PublicUnitDtoSchema>;

// ---------------------------------------------------------------------------
// P3 — Returns / RMA
// ---------------------------------------------------------------------------

export const RmaStatusSchema = z.enum(["open", "received", "diagnosed", "repaired", "replaced", "rejected", "closed"]);
export type RmaStatus = z.infer<typeof RmaStatusSchema>;

export const RmaDtoSchema = z.object({
  id: uuid,
  org_id: uuid,
  unit_id: uuid.nullable().optional(),
  product_id: uuid,
  customer_id: uuid.nullable().optional(),
  rma_number: z.string(),
  under_warranty: z.boolean(),
  status: RmaStatusSchema,
  reason: z.string().nullable().optional(),
  diagnosis: z.string().nullable().optional(),
  resolution: z.string().nullable().optional(),
  received_at: isoDateTime.nullable().optional(),
  resolved_at: isoDateTime.nullable().optional(),
  version: z.number(),
  created_at: isoDateTime,
  updated_at: isoDateTime,
});
export type RmaDto = z.infer<typeof RmaDtoSchema>;

export type ListRmaQuery = {
  org_id?: string;
  status?: RmaStatus;
  product_id?: string;
  customer_id?: string;
  unit_id?: string;
};

export type CreateRmaRequest = {
  org_id: string;
  product_id: string;
  unit_id?: string | null;
  customer_id?: string | null;
  rma_number: string;
  under_warranty?: boolean;
  reason?: string | null;
};

export type PatchRmaRequest = {
  expected_version: number;
  unit_id?: string | null;
  customer_id?: string | null;
  under_warranty: boolean;
  status: RmaStatus;
  reason?: string | null;
  diagnosis?: string | null;
  resolution?: string | null;
  received_at?: string | null;
  resolved_at?: string | null;
};

// ---------------------------------------------------------------------------
// Firmware & Software releases
// ---------------------------------------------------------------------------

export const ReleaseKindSchema = z.enum(["software", "firmware"]);
export type ReleaseKind = z.infer<typeof ReleaseKindSchema>;

export const ReleaseLinkSchema = z.object({
  label: z.string(),
  url: z.string(),
});
export type ReleaseLink = z.infer<typeof ReleaseLinkSchema>;

export const ProductReleaseDtoSchema = z.object({
  id: uuid,
  org_id: uuid,
  product_id: uuid,
  kind: ReleaseKindSchema,
  major: z.number(),
  minor: z.number(),
  version_label: z.string(),
  release_notes: z.string().nullable().optional(),
  released_at: isoDateTime.nullable().optional(),
  links: z.array(ReleaseLinkSchema).optional(),
  archived_at: isoDateTime.nullable().optional(),
  created_by: uuid.nullable().optional(),
  version: z.number(),
  created_at: isoDateTime,
  updated_at: isoDateTime,
});
export type ProductReleaseDto = z.infer<typeof ProductReleaseDtoSchema>;

export type CreateReleaseRequest = {
  kind: ReleaseKind;
  major: number;
  minor: number;
  release_notes?: string | null;
  released_at?: string | null;
  links?: ReleaseLink[];
};

export type PatchReleaseRequest = {
  expected_version: number;
  kind: ReleaseKind;
  major: number;
  minor: number;
  release_notes?: string | null;
  released_at?: string | null;
  links?: ReleaseLink[];
};

export type ArchiveReleaseRequest = { expected_version: number };
