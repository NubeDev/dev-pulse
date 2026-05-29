/**
 * Field-level validation rules for the executive summary.
 *
 * The backend's completion booleans are section-level
 * (see `project_exec_summary.rs` in dp-store-pg, the `SELECT ... AS
 * c_summary, AS c_scope, ...` block). The rules below mirror those
 * SQL predicates one-for-one so the Validate tab can tell the user
 * exactly which inputs are blocking each section, not just which
 * section is red.
 *
 * Each rule returns a stable `key` (used as React key + the
 * `data-validation-key` attribute the section pages tag onto their
 * matching input so "Jump to field" can scroll-and-focus it) plus
 * which patch-path satisfies it. `kind` decides whether the Validate
 * tab can edit the value inline or needs to send the user to the
 * section tab.
 */

import type {
  ExecSummaryDto,
  ExecSummarySectionId,
} from "../../api/client.js";
import { SECTIONS } from "./shared.js";

export type FieldKind =
  | "text" // short single-line input — editable inline
  | "number" // numeric input — editable inline
  | "markdown" // long-form markdown — jump to section
  | "array" // tag/multi-select — jump to section
  | "upload"; // file upload — jump to section

export interface MissingField {
  /** Stable identifier — also the value the section page tags onto
   *  its matching input via `data-validation-key="…"` so the
   *  jump-to-field flow can scroll-and-focus the input. */
  key: string;
  /** Section this field lives in — used both for grouping and to
   *  decide which tab to open when jumping. */
  sectionId: ExecSummarySectionId;
  /** Human-readable field label (matches what the section form
   *  shows). */
  label: string;
  /** Short hint about what's expected. */
  hint?: string;
  /** How the Validate tab should let the user fix it. */
  kind: FieldKind;
}

function isFilled(v: string | null | undefined): boolean {
  return typeof v === "string" && v.trim().length > 0;
}

function isFilledNumber(v: number | null | undefined): boolean {
  return typeof v === "number" && Number.isFinite(v);
}

/** Compute the missing-field list for a DTO. Skipped sections (the
 *  user's explicit N/A set) yield an empty list — matching the
 *  backend's `with_skips` behaviour. */
export function computeMissingFields(data: ExecSummaryDto): MissingField[] {
  const skipped = new Set(data.skipped_sections);
  const out: MissingField[] = [];

  if (!skipped.has("summary")) {
    const s = data.summary;
    if (!isFilled(s.product_name)) {
      out.push({
        key: "summary.product_name",
        sectionId: "summary",
        label: "Product name",
        kind: "text",
      });
    }
    if (!isFilled(s.objective)) {
      out.push({
        key: "summary.objective",
        sectionId: "summary",
        label: "Objective",
        hint: "What the product is meant to achieve.",
        kind: "markdown",
      });
    }
    if (!isFilled(s.success_criteria)) {
      out.push({
        key: "summary.success_criteria",
        sectionId: "summary",
        label: "Success criteria",
        hint: "How you'll know it worked.",
        kind: "markdown",
      });
    }
  }

  if (!skipped.has("scope")) {
    const s = data.scope;
    if (!isFilled(s.in_scope)) {
      out.push({
        key: "scope.in_scope",
        sectionId: "scope",
        label: "In scope",
        kind: "markdown",
      });
    }
    if (!isFilled(s.out_of_scope)) {
      out.push({
        key: "scope.out_of_scope",
        sectionId: "scope",
        label: "Out of scope",
        kind: "markdown",
      });
    }
  }

  if (!skipped.has("requirements")) {
    const r = data.requirements;
    if (!isFilled(r.must_have)) {
      out.push({
        key: "requirements.must_have",
        sectionId: "requirements",
        label: "Must-have requirements",
        kind: "markdown",
      });
    }
    if (!r.protocols || r.protocols.length === 0) {
      out.push({
        key: "requirements.protocols",
        sectionId: "requirements",
        label: "Protocols",
        hint: "Pick at least one supported field protocol.",
        kind: "array",
      });
    }
  }

  if (!skipped.has("hardware")) {
    const h = data.hardware;
    const hasFeatures = isFilled(h.hardware_features);
    const hasImage = data.images.length > 0;
    if (!hasFeatures && !hasImage) {
      out.push({
        key: "hardware.hardware_features",
        sectionId: "hardware",
        label: "Hardware features or image",
        hint: "Either describe features or attach at least one image.",
        kind: "markdown",
      });
    }
  }

  if (!skipped.has("commercial")) {
    const c = data.commercial;
    if (!isFilledNumber(c.rrp_cents)) {
      out.push({
        key: "commercial.rrp_cents",
        sectionId: "commercial",
        label: "RRP",
        hint: "Recommended retail price.",
        kind: "number",
      });
    }
    if (!isFilledNumber(c.target_gp_pct)) {
      out.push({
        key: "commercial.target_gp_pct",
        sectionId: "commercial",
        label: "Target GP %",
        kind: "number",
      });
    }
  }

  if (!skipped.has("documents") && data.documents.length === 0) {
    out.push({
      key: "documents.any",
      sectionId: "documents",
      label: "At least one document",
      hint: "Upload a brief, BOM, datasheet or any supporting file.",
      kind: "upload",
    });
  }

  if (!skipped.has("changelog") && data.changelog.length === 0) {
    out.push({
      key: "changelog.any",
      sectionId: "changelog",
      label: "At least one change-log entry",
      kind: "upload",
    });
  }

  // Approval is gated by status rather than a field — surfaced
  // contextually by the existing Submit / Approve buttons, so the
  // Validate tab deliberately skips it.

  return out;
}

/** Group the flat missing list by section so the UI can render
 *  section headers with field rows nested under them. Preserves the
 *  declared SECTIONS order. */
export function groupMissingBySection(
  missing: readonly MissingField[],
): Array<{
  sectionId: ExecSummarySectionId;
  label: string;
  fields: MissingField[];
}> {
  const bySection = new Map<ExecSummarySectionId, MissingField[]>();
  for (const m of missing) {
    const list = bySection.get(m.sectionId);
    if (list) list.push(m);
    else bySection.set(m.sectionId, [m]);
  }
  return SECTIONS.filter((s) => bySection.has(s.id)).map((s) => ({
    sectionId: s.id,
    label: s.label,
    fields: bySection.get(s.id)!,
  }));
}
