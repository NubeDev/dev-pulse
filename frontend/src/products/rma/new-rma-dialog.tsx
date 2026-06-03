/**
 * New RMA dialog — used from both the RMA list page (no product preset)
 * and the product detail Returns tab (product fixed).
 *
 * Fields: product (Select if not preset), RMA #, under_warranty
 * (Switch), customer (Select, optional), unit (Select, optional —
 * derived from the product's runs → units), reason (Textarea).
 */

import { useEffect, useMemo, useState } from "react";
import { useQueries, useQuery } from "@tanstack/react-query";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";

import { api } from "../../api/client.js";
import type { ProductDto, UnitDto } from "../../api/schemas/products.js";
import { navigate, rmaDetailRoute } from "../../routes.js";
import { manufacturingKeys } from "../use-manufacturing-data.js";
import { useCreateRma } from "../use-manufacturing-data.js";

const NONE = "__none__";

export function NewRmaDialog({
  open,
  onOpenChange,
  product,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  product?: ProductDto | null;
}): JSX.Element {
  const create = useCreateRma();

  const [rmaNumber, setRmaNumber] = useState("");
  const [reason, setReason] = useState("");
  const [underWarranty, setUnderWarranty] = useState(false);
  const [selectedProductId, setSelectedProductId] = useState<string>(
    product?.id ?? NONE,
  );
  const [selectedCustomerId, setSelectedCustomerId] = useState<string>(NONE);
  const [selectedUnitId, setSelectedUnitId] = useState<string>(NONE);

  // Reset form on open
  useEffect(() => {
    if (!open) return;
    setRmaNumber("");
    setReason("");
    setUnderWarranty(false);
    setSelectedProductId(product?.id ?? NONE);
    setSelectedCustomerId(NONE);
    setSelectedUnitId(NONE);
    create.reset();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  // When parent product changes (e.g. dialog re-opened with different product)
  useEffect(() => {
    setSelectedProductId(product?.id ?? NONE);
    setSelectedUnitId(NONE);
  }, [product?.id]);

  // Product list (if product not preset)
  const productsQuery = useQuery({
    queryKey: ["products", "list-for-new-rma"],
    queryFn: () => api.listProducts({ limit: 200 }),
    staleTime: 60_000,
    enabled: !product,
  });

  // Customer list
  const customersQuery = useQuery({
    queryKey: ["customers", "list-for-new-rma"],
    queryFn: () => api.listCustomers({ limit: 200 }),
    staleTime: 60_000,
  });

  // Runs for the selected product (to build unit picker)
  const resolvedProductId =
    selectedProductId !== NONE ? selectedProductId : null;

  const runsQuery = useQuery({
    queryKey: ["products", "runs-for-new-rma", resolvedProductId],
    queryFn: () =>
      resolvedProductId
        ? api.listProductRuns(resolvedProductId)
        : Promise.resolve([]),
    staleTime: 30_000,
    enabled: !!resolvedProductId,
  });

  const runIds = useMemo(
    () => runsQuery.data?.map((r) => r.id) ?? [],
    [runsQuery.data],
  );

  // Fan-out: units for each run
  const unitQueries = useQueries({
    queries: runIds.map((runId) => ({
      queryKey: manufacturingKeys.runUnits(runId),
      queryFn: () => api.listRunUnits(runId),
      staleTime: 30_000,
    })),
  });

  const allUnits = useMemo<UnitDto[]>(() => {
    const out: UnitDto[] = [];
    for (const q of unitQueries) {
      if (q.data) out.push(...q.data);
    }
    return out;
  }, [unitQueries]);

  // Derived org_id from product
  const resolvedProduct = product ?? productsQuery.data?.rows.find(
    (p) => p.id === resolvedProductId,
  );
  const orgId = resolvedProduct?.org_id ?? "";

  const canSubmit =
    rmaNumber.trim().length > 0 &&
    resolvedProductId !== null &&
    orgId.length > 0 &&
    !create.isPending;

  const onSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    if (!canSubmit || !resolvedProductId) return;
    create.mutate(
      {
        org_id: orgId,
        product_id: resolvedProductId,
        unit_id: selectedUnitId !== NONE ? selectedUnitId : null,
        customer_id: selectedCustomerId !== NONE ? selectedCustomerId : null,
        rma_number: rmaNumber.trim(),
        under_warranty: underWarranty,
        reason: reason.trim() || null,
      },
      {
        onSuccess: (rma) => {
          onOpenChange(false);
          navigate(rmaDetailRoute(rma.id));
        },
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg" data-testid="new-rma-dialog">
        <DialogHeader>
          <DialogTitle>New Return (RMA)</DialogTitle>
          <DialogDescription>
            Log a customer return. The RMA number must be unique per org.
          </DialogDescription>
        </DialogHeader>

        <form className="flex flex-col gap-4" onSubmit={onSubmit}>
          {/* Product selector — fixed if product prop provided */}
          {product ? (
            <div className="flex flex-col gap-1.5">
              <Label className="text-xs text-muted-foreground">Product</Label>
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium">{product.name}</span>
                <Badge variant="outline" className="font-mono text-xs">
                  {product.model_number}
                </Badge>
              </div>
            </div>
          ) : (
            <div className="flex flex-col gap-2">
              <Label htmlFor="new-rma-product">Product</Label>
              <Select
                value={selectedProductId}
                onValueChange={(v) => {
                  setSelectedProductId(v);
                  setSelectedUnitId(NONE);
                }}
              >
                <SelectTrigger id="new-rma-product" data-testid="new-rma-product">
                  <SelectValue placeholder="Select a product…" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={NONE} disabled>
                    Select a product…
                  </SelectItem>
                  {productsQuery.data?.rows.map((p) => (
                    <SelectItem key={p.id} value={p.id}>
                      {p.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}

          {/* RMA number */}
          <div className="flex flex-col gap-2">
            <Label htmlFor="new-rma-number">RMA #</Label>
            <Input
              id="new-rma-number"
              data-testid="new-rma-number"
              value={rmaNumber}
              onChange={(e) => setRmaNumber(e.target.value)}
              placeholder="RMA-2026-001"
              maxLength={200}
              autoFocus
              required
            />
          </div>

          {/* Under warranty */}
          <div className="flex items-center gap-2">
            <Checkbox
              id="new-rma-warranty"
              data-testid="new-rma-warranty"
              checked={underWarranty}
              onCheckedChange={(v) => setUnderWarranty(v === true)}
            />
            <Label htmlFor="new-rma-warranty" className="cursor-pointer">
              Under warranty
            </Label>
          </div>

          {/* Customer selector */}
          <div className="flex flex-col gap-2">
            <Label htmlFor="new-rma-customer">Customer (optional)</Label>
            <Select
              value={selectedCustomerId}
              onValueChange={setSelectedCustomerId}
            >
              <SelectTrigger id="new-rma-customer" data-testid="new-rma-customer">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NONE}>Unassigned</SelectItem>
                {customersQuery.data?.rows.map((c) => (
                  <SelectItem key={c.id} value={c.id}>
                    {c.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* Unit picker — only shown when a product is selected and has units */}
          {resolvedProductId && allUnits.length > 0 && (
            <div className="flex flex-col gap-2">
              <Label htmlFor="new-rma-unit">Unit / Serial (optional)</Label>
              <Select
                value={selectedUnitId}
                onValueChange={setSelectedUnitId}
              >
                <SelectTrigger id="new-rma-unit" data-testid="new-rma-unit">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={NONE}>Not matched</SelectItem>
                  {allUnits.map((u) => (
                    <SelectItem key={u.id} value={u.id}>
                      <span className="font-mono">{u.serial_number}</span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}

          {/* Reason */}
          <div className="flex flex-col gap-2">
            <Label htmlFor="new-rma-reason">
              Reason / fault description (optional)
            </Label>
            <Textarea
              id="new-rma-reason"
              data-testid="new-rma-reason"
              rows={3}
              value={reason}
              onChange={(e) => setReason(e.target.value)}
              placeholder="Customer-reported fault…"
            />
          </div>

          {create.isError && (
            <Alert variant="destructive" data-testid="new-rma-error">
              <AlertTitle>Create failed</AlertTitle>
              <AlertDescription>{create.error.message}</AlertDescription>
            </Alert>
          )}

          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={() => onOpenChange(false)}
              disabled={create.isPending}
            >
              Cancel
            </Button>
            <Button
              type="submit"
              data-testid="new-rma-submit"
              disabled={!canSubmit}
            >
              {create.isPending ? "Creating…" : "Create RMA"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
