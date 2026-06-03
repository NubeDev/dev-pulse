/**
 * Shared labels / badge variants for §7.4 P2 manufacturing runs and
 * units. Kept tiny + dependency-free so both the section tables and
 * the detail pages can import the same vocabulary.
 */

import type { RunStatus, UnitStatus } from "../../api/schemas/products.js";

export const RUN_STATUS_LABEL: Record<RunStatus, string> = {
  planned: "Planned",
  in_progress: "In progress",
  completed: "Completed",
  cancelled: "Cancelled",
};

export const RUN_STATUSES: RunStatus[] = [
  "planned",
  "in_progress",
  "completed",
  "cancelled",
];

export const RUN_STATUS_VARIANT: Record<
  RunStatus,
  "default" | "secondary" | "outline" | "destructive"
> = {
  planned: "secondary",
  in_progress: "default",
  completed: "outline",
  cancelled: "destructive",
};

export const UNIT_STATUS_LABEL: Record<UnitStatus, string> = {
  built: "Built",
  tested: "Tested",
  shipped: "Shipped",
  returned: "Returned",
  scrapped: "Scrapped",
};

export const UNIT_STATUSES: UnitStatus[] = [
  "built",
  "tested",
  "shipped",
  "returned",
  "scrapped",
];

export const UNIT_STATUS_VARIANT: Record<
  UnitStatus,
  "default" | "secondary" | "outline" | "destructive"
> = {
  built: "secondary",
  tested: "default",
  shipped: "outline",
  returned: "destructive",
  scrapped: "destructive",
};
