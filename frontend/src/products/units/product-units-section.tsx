/**
 * Product detail → Units tab (§7.4, P2).
 *
 * Serial table across the product's runs. The product → units API is
 * run-scoped, so the simplest correct approach is: list the product's
 * runs, let the user pick one (or "All runs"), and union
 * `api.listRunUnits` for the selected run(s). Columns: serial, status,
 * run; row → unit detail (`#/units/{id}`).
 */

import { useMemo, useState } from "react";
import { useQueries } from "@tanstack/react-query";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

import { api } from "../../api/client.js";
import type { ProductDto, RunDto, UnitDto } from "../../api/schemas/products.js";
import { navigate, unitDetailRoute } from "../../routes.js";
import { manufacturingKeys, useProductRuns } from "../use-manufacturing-data.js";

import {
  UNIT_STATUS_LABEL,
  UNIT_STATUS_VARIANT,
} from "../runs/run-shared.js";

const ALL_RUNS = "__all__";

export function ProductUnitsSection({
  product,
}: {
  product: ProductDto;
}): JSX.Element {
  const runs = useProductRuns(product.id);
  const [runFilter, setRunFilter] = useState<string>(ALL_RUNS);
  const [search, setSearch] = useState("");

  const runRows = useMemo<RunDto[]>(() => runs.data ?? [], [runs.data]);
  const runById = useMemo(() => {
    const m = new Map<string, RunDto>();
    for (const r of runRows) m.set(r.id, r);
    return m;
  }, [runRows]);

  // Which runs to fetch units for — one or all.
  const targetRunIds = useMemo(() => {
    if (runFilter === ALL_RUNS) return runRows.map((r) => r.id);
    return runRows.some((r) => r.id === runFilter) ? [runFilter] : [];
  }, [runFilter, runRows]);

  const unitQueries = useQueries({
    queries: targetRunIds.map((runId) => ({
      queryKey: manufacturingKeys.runUnits(runId),
      queryFn: () => api.listRunUnits(runId),
      staleTime: 30_000,
    })),
  });

  const unitsLoading =
    runs.isPending || unitQueries.some((q) => q.isPending);
  const unitsError =
    runs.error ?? unitQueries.find((q) => q.isError)?.error ?? null;

  const allUnits = useMemo<UnitDto[]>(() => {
    const out: UnitDto[] = [];
    for (const q of unitQueries) {
      if (q.data) out.push(...q.data);
    }
    return out;
  }, [unitQueries]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return allUnits;
    return allUnits.filter((u) =>
      u.serial_number.toLowerCase().includes(q),
    );
  }, [allUnits, search]);

  return (
    <div className="flex flex-col gap-4" data-testid="product-units">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-medium">
          Serialised units{" "}
          <span className="text-muted-foreground">({filtered.length})</span>
        </h3>
        <div className="flex flex-wrap items-center gap-2">
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search serial…"
            className="h-9 w-44"
            data-testid="product-units-search"
          />
          <Select value={runFilter} onValueChange={setRunFilter}>
            <SelectTrigger
              className="h-9 w-48"
              data-testid="product-units-run-filter"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={ALL_RUNS}>All runs</SelectItem>
              {runRows.map((r) => (
                <SelectItem key={r.id} value={r.id}>
                  {r.run_code}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      {unitsError ? (
        <Alert variant="destructive" data-testid="product-units-error">
          <AlertTitle>Couldn't load units</AlertTitle>
          <AlertDescription>{unitsError.message}</AlertDescription>
        </Alert>
      ) : unitsLoading ? (
        <div className="flex flex-col gap-2">
          {Array.from({ length: 4 }).map((_, i) => (
            <Skeleton key={i} className="h-11 rounded-md" />
          ))}
        </div>
      ) : runRows.length === 0 ? (
        <div
          className="rounded-lg border-2 border-dashed border-border bg-muted/20 px-4 py-10 text-center text-sm text-muted-foreground"
          data-testid="product-units-empty"
        >
          No runs yet, so no units. Start a run on the Runs tab and
          allocate serials to populate this list.
        </div>
      ) : filtered.length === 0 ? (
        <div
          className="rounded-lg border-2 border-dashed border-border bg-muted/20 px-4 py-10 text-center text-sm text-muted-foreground"
          data-testid="product-units-none"
        >
          No units match this filter.
        </div>
      ) : (
        <div className="rounded-md border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Serial</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Run</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {filtered.map((u) => (
                <TableRow
                  key={u.id}
                  className="cursor-pointer"
                  onClick={() => navigate(unitDetailRoute(u.id))}
                  data-testid="product-unit-row"
                >
                  <TableCell className="font-mono">
                    {u.serial_number}
                  </TableCell>
                  <TableCell>
                    <Badge variant={UNIT_STATUS_VARIANT[u.status]}>
                      {UNIT_STATUS_LABEL[u.status]}
                    </Badge>
                  </TableCell>
                  <TableCell className="font-mono text-muted-foreground">
                    {u.run_id ? runById.get(u.run_id)?.run_code ?? "—" : "—"}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
    </div>
  );
}
