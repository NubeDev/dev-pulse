/**
 * Shared labels / badge variants for §7.4 P3 Returns / RMA.
 * Mirrors runs/run-shared.ts — kept tiny + dependency-free.
 */

import type { RmaStatus } from "../../api/schemas/products.js";

export const RMA_STATUS_LABEL: Record<RmaStatus, string> = {
  open: "Open",
  received: "Received",
  diagnosed: "Diagnosed",
  repaired: "Repaired",
  replaced: "Replaced",
  rejected: "Rejected",
  closed: "Closed",
};

export const RMA_STATUSES: RmaStatus[] = [
  "open",
  "received",
  "diagnosed",
  "repaired",
  "replaced",
  "rejected",
  "closed",
];

export const RMA_STATUS_VARIANT: Record<
  RmaStatus,
  "default" | "secondary" | "outline" | "destructive"
> = {
  open: "default",
  received: "secondary",
  diagnosed: "secondary",
  repaired: "default",
  replaced: "default",
  rejected: "destructive",
  closed: "outline",
};

/** Terminal statuses — moving to one of these sets resolved_at if null. */
export const RMA_TERMINAL_STATUSES = new Set<RmaStatus>([
  "repaired",
  "replaced",
  "rejected",
  "closed",
]);
