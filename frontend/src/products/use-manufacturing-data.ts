/**
 * React-Query bindings for the §7.4 P2 manufacturing surface —
 * production runs, serialised units, and EOL reports. Mirrors the
 * sibling `use-products-data.ts`: a stable key factory + thin
 * query/mutation hooks with the same cache discipline (30s lists,
 * 15s detail, invalidate-on-write).
 *
 * Cache keys live under `["manufacturing", …]` so a logout flush
 * clears them alongside the other section caches.
 *
 * Invalidation discipline: an allocate / EOL / sign-off mutation
 * touches the run counters *and* its unit list, so the write hooks
 * invalidate both the run detail and its units to keep the stat
 * cards (built / pass / fail / yield) live.
 */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { api } from "../api/client.js";
import type {
  AllocateUnitsRequest,
  CreateRmaRequest,
  CreateRunRequest,
  EolReportDto,
  ListRmaQuery,
  PatchRmaRequest,
  PatchRunRequest,
  PatchUnitRequest,
  RecordEolRequest,
  RmaDto,
  RunDto,
  RunEolSummaryDto,
  RunEolSummaryRequest,
  UnitAllocationDto,
  UnitDto,
} from "../api/schemas/products.js";

/** Stable cache keys — the invalidation surface for the mutation
 *  hooks below. */
export const manufacturingKeys = {
  runsForProduct: (productId: string) =>
    ["manufacturing", "runs", "for-product", productId] as const,
  run: (runId: string) => ["manufacturing", "run", runId] as const,
  runUnits: (runId: string) =>
    ["manufacturing", "run-units", runId] as const,
  runEolSummary: (runId: string) =>
    ["manufacturing", "run-eol-summary", runId] as const,
  unit: (unitId: string) => ["manufacturing", "unit", unitId] as const,
  unitEol: (unitId: string) => ["manufacturing", "unit-eol", unitId] as const,
  rmaList: (q: ListRmaQuery) => ["manufacturing", "rma", "list", q] as const,
  rma: (id: string) => ["manufacturing", "rma", "detail", id] as const,
};

// ---------------------------------------------------------------------------
// Runs — read
// ---------------------------------------------------------------------------

/** Runs belonging to a product (Runs tab on the product page). */
export function useProductRuns(productId: string | null) {
  return useQuery<RunDto[]>({
    queryKey: productId
      ? manufacturingKeys.runsForProduct(productId)
      : ["manufacturing", "runs", "for-product", "(none)"],
    queryFn: () =>
      productId ? api.listProductRuns(productId) : Promise.resolve([]),
    enabled: !!productId,
    staleTime: 30_000,
  });
}

/** Single run read for the run detail page. Returns `null` when the
 *  run does not exist so the page renders a clean "not found". */
export function useRun(runId: string | null) {
  return useQuery<RunDto | null>({
    queryKey: runId
      ? manufacturingKeys.run(runId)
      : ["manufacturing", "run", "(none)"],
    queryFn: () => (runId ? api.getRun(runId) : Promise.resolve(null)),
    enabled: !!runId,
    staleTime: 15_000,
  });
}

// ---------------------------------------------------------------------------
// Runs — write
// ---------------------------------------------------------------------------

export function useCreateRun(productId: string) {
  const qc = useQueryClient();
  return useMutation<RunDto, Error, CreateRunRequest>({
    mutationFn: (body) => api.createRun(productId, body),
    onSuccess: () => {
      qc.invalidateQueries({
        queryKey: manufacturingKeys.runsForProduct(productId),
      });
    },
  });
}

export function usePatchRun(runId: string, productId?: string | null) {
  const qc = useQueryClient();
  return useMutation<RunDto, Error, PatchRunRequest>({
    mutationFn: (body) => api.patchRun(runId, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: manufacturingKeys.run(runId) });
      if (productId) {
        qc.invalidateQueries({
          queryKey: manufacturingKeys.runsForProduct(productId),
        });
      }
    },
  });
}

// ---------------------------------------------------------------------------
// Units (run-scoped) — read + allocate
// ---------------------------------------------------------------------------

/** Serialised units belonging to a run. */
export function useRunUnits(runId: string | null) {
  return useQuery<UnitDto[]>({
    queryKey: runId
      ? manufacturingKeys.runUnits(runId)
      : ["manufacturing", "run-units", "(none)"],
    queryFn: () => (runId ? api.listRunUnits(runId) : Promise.resolve([])),
    enabled: !!runId,
    staleTime: 30_000,
  });
}

/** Reserve N serials against a run. Invalidates the run (counters)
 *  and its unit list so the freshly minted serials appear. */
export function useAllocateUnits(runId: string) {
  const qc = useQueryClient();
  return useMutation<UnitAllocationDto, Error, AllocateUnitsRequest>({
    mutationFn: (body) => api.allocateUnits(runId, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: manufacturingKeys.run(runId) });
      qc.invalidateQueries({ queryKey: manufacturingKeys.runUnits(runId) });
      qc.invalidateQueries({
        queryKey: manufacturingKeys.runEolSummary(runId),
      });
    },
  });
}

// ---------------------------------------------------------------------------
// Units (single) — read + write
// ---------------------------------------------------------------------------

/** Single unit read for the unit detail page. Returns `null` when the
 *  unit does not exist. */
export function useUnit(unitId: string | null) {
  return useQuery<UnitDto | null>({
    queryKey: unitId
      ? manufacturingKeys.unit(unitId)
      : ["manufacturing", "unit", "(none)"],
    queryFn: () => (unitId ? api.getUnit(unitId) : Promise.resolve(null)),
    enabled: !!unitId,
    staleTime: 15_000,
  });
}

export function usePatchUnit(unitId: string, runId?: string | null) {
  const qc = useQueryClient();
  return useMutation<UnitDto, Error, PatchUnitRequest>({
    mutationFn: (body) => api.patchUnit(unitId, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: manufacturingKeys.unit(unitId) });
      if (runId) {
        qc.invalidateQueries({ queryKey: manufacturingKeys.runUnits(runId) });
        qc.invalidateQueries({ queryKey: manufacturingKeys.run(runId) });
      }
    },
  });
}

// ---------------------------------------------------------------------------
// EOL reports (unit timeline) + run sign-off summary
// ---------------------------------------------------------------------------

/** EOL reports for a unit, newest-first (server order). */
export function useUnitEol(unitId: string | null) {
  return useQuery<EolReportDto[]>({
    queryKey: unitId
      ? manufacturingKeys.unitEol(unitId)
      : ["manufacturing", "unit-eol", "(none)"],
    queryFn: () => (unitId ? api.listUnitEol(unitId) : Promise.resolve([])),
    enabled: !!unitId,
    staleTime: 15_000,
  });
}

/** Record a fresh EOL report against a unit. Invalidates the unit's
 *  timeline + the unit (status may flip) and, when known, the
 *  owning run's counters / sign-off snapshot. */
export function useRecordEol(unitId: string, runId?: string | null) {
  const qc = useQueryClient();
  return useMutation<EolReportDto, Error, RecordEolRequest>({
    mutationFn: (body) => api.recordEol(unitId, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: manufacturingKeys.unitEol(unitId) });
      qc.invalidateQueries({ queryKey: manufacturingKeys.unit(unitId) });
      if (runId) {
        qc.invalidateQueries({ queryKey: manufacturingKeys.run(runId) });
        qc.invalidateQueries({ queryKey: manufacturingKeys.runUnits(runId) });
        qc.invalidateQueries({
          queryKey: manufacturingKeys.runEolSummary(runId),
        });
      }
    },
  });
}

/** Run EOL sign-off summary (built/pass/fail snapshot + signer). */
export function useRunEolSummary(runId: string | null) {
  return useQuery<RunEolSummaryDto | null>({
    queryKey: runId
      ? manufacturingKeys.runEolSummary(runId)
      : ["manufacturing", "run-eol-summary", "(none)"],
    queryFn: () =>
      runId ? api.getRunEolSummary(runId) : Promise.resolve(null),
    enabled: !!runId,
    staleTime: 15_000,
  });
}

export function useUpsertRunEolSummary(runId: string) {
  const qc = useQueryClient();
  return useMutation<RunEolSummaryDto, Error, RunEolSummaryRequest>({
    mutationFn: (body) => api.upsertRunEolSummary(runId, body),
    onSuccess: () => {
      qc.invalidateQueries({
        queryKey: manufacturingKeys.runEolSummary(runId),
      });
      qc.invalidateQueries({ queryKey: manufacturingKeys.run(runId) });
    },
  });
}

// ---------------------------------------------------------------------------
// RMA / Returns (P3)
// ---------------------------------------------------------------------------

/** List RMAs with optional filters. */
export function useRmaList(q: ListRmaQuery) {
  return useQuery<RmaDto[]>({
    queryKey: manufacturingKeys.rmaList(q),
    queryFn: () => api.listRma(q),
    staleTime: 30_000,
  });
}

/** Single RMA read for the detail page. Returns `null` when not found. */
export function useRma(id: string | null) {
  return useQuery<RmaDto | null>({
    queryKey: id
      ? manufacturingKeys.rma(id)
      : ["manufacturing", "rma", "detail", "(none)"],
    queryFn: () => (id ? api.getRma(id) : Promise.resolve(null)),
    enabled: !!id,
    staleTime: 15_000,
  });
}

export function useCreateRma() {
  const qc = useQueryClient();
  return useMutation<RmaDto, Error, CreateRmaRequest>({
    mutationFn: (body) => api.createRma(body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["manufacturing", "rma"] });
    },
  });
}

export function usePatchRma(id: string) {
  const qc = useQueryClient();
  return useMutation<RmaDto, Error, PatchRmaRequest>({
    mutationFn: (body) => api.patchRma(id, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: manufacturingKeys.rma(id) });
      qc.invalidateQueries({ queryKey: ["manufacturing", "rma"] });
    },
  });
}
