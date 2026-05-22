/**
 * Catalogue of view templates surfaced by the create wizard.
 *
 * Templates come in three flavours:
 *
 *   * `single` — creates ONE view, pre-fills name / group /
 *                filter / sort. The user can rename in step 2.
 *   * `batch`  — fans out into N pre-baked views in one click
 *                (e.g. the 8-gate progression: G1..G8 each
 *                becomes its own tab). Each batched view
 *                carries its own name; the wizard's name field
 *                is hidden.
 *   * `custom` — starts from the toolbar's current group /
 *                filter / sort.
 *
 * Categories are orthogonal to the template choice — the wizard
 * exposes an OPTIONAL categories editor in step 2 that works on
 * top of any template (including batch: each fanned-out view
 * gets the same category sections). When categories are non-empty
 * the wizard forces `group_by = "tag:category"` so the workbench
 * renders collapsible sections, one per category.
 *
 * The 8-gate progression intentionally keeps its per-tab filter
 * empty (May 2026 amendment) so the per-gate chip doesn't clutter
 * the filter bar.
 */

import {
  AlertOctagonIcon,
  FlagIcon,
  ListChecksIcon,
  ListIcon,
  SparklesIcon,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

import type { ProjectViewFilterClause } from "../../api/client.js";

import { CATEGORY_TAG_KEY } from "./category-utils.js";

/** Shape consumed by the per-view writers (batch + single). */
export interface ViewTemplateSeed {
  name: string;
  groupBy: string | null;
  filterClauses: ProjectViewFilterClause[];
}

export type ViewTemplateKind = "single" | "batch" | "custom";

export interface ViewTemplate {
  id: string;
  label: string;
  description: string;
  Icon: LucideIcon;
  kind: ViewTemplateKind;
  /** Populated when `kind === "single"`. */
  seed?: ViewTemplateSeed;
  /** Populated when `kind === "batch"`. */
  batch?: ViewTemplateSeed[];
}

const GATES: Array<{ short: string; label: string }> = [
  { short: "G1", label: "Executive Summary" },
  { short: "G2", label: "Proof of Concept" },
  { short: "G3", label: "MVP Build" },
  { short: "G4", label: "Client Acceptance" },
  { short: "G5", label: "Product Refinement" },
  { short: "G6", label: "Production Ready" },
  { short: "G7", label: "Go-To-Market" },
  { short: "G8", label: "Scale & Support" },
];

/** Canonical template list — the same array drives the wizard's
 *  tile picker. Order matters; the most useful templates sit
 *  first. */
export const VIEW_TEMPLATES: ViewTemplate[] = [
  {
    id: "gate-progression",
    label: "Gate progression (G1–G8)",
    description:
      "Creates 8 separate tabs — one per gate from Executive Summary through Scale & Support.",
    Icon: FlagIcon,
    kind: "batch",
    batch: GATES.map((g) => ({
      name: g.short,
      groupBy: null,
      filterClauses: [],
    })),
  },
  {
    id: "simple-list",
    label: "Simple list",
    description:
      "A single view with no preset filter. Add optional categories in the next step to get collapsible sections.",
    Icon: ListIcon,
    kind: "single",
    seed: {
      name: "All issues",
      groupBy: null,
      filterClauses: [],
    },
  },
  {
    id: "status-and-blocked",
    label: "Open vs closed",
    description:
      "Groups by open/closed so stalled tickets are one click away.",
    Icon: ListChecksIcon,
    kind: "single",
    seed: {
      name: "Open vs closed",
      groupBy: "status",
      filterClauses: [],
    },
  },
  {
    id: "blocked-only",
    label: "Blocked only",
    description:
      "Single tab filtered to `label:blocked`, grouped by gate so you see where work has stalled.",
    Icon: AlertOctagonIcon,
    kind: "single",
    seed: {
      name: "Blocked",
      groupBy: "tag:gate",
      filterClauses: [{ dim: "label", value: "blocked" }],
    },
  },
  {
    id: "custom",
    label: "Custom",
    description:
      "Starts from the current group / filter / sort. The tab's icon is auto-picked from its name.",
    Icon: SparklesIcon,
    kind: "custom",
  },
];

/** The `group_by` spec used by every categorised view — matches
 *  the server-side bucketing dimension for kv tags. */
export const CATEGORISED_GROUP_BY = `tag:${CATEGORY_TAG_KEY}`;

/** Pre-baked category packs surfaced as quick-add chips in step
 *  2's optional categories editor. The user can edit/remove
 *  freely after applying a pack. */
export interface CategoryPack {
  id: string;
  label: string;
  categories: string[];
}

export const CATEGORY_PACKS: CategoryPack[] = [
  {
    id: "engineering",
    label: "Engineering",
    categories: ["Hardware", "Firmware", "Software"],
  },
  {
    id: "quality",
    label: "Quality & ops",
    categories: ["Testing", "Compliance", "Operations"],
  },
  {
    id: "launch",
    label: "Launch",
    categories: ["Manufacturing", "Compliance", "GTM"],
  },
];
