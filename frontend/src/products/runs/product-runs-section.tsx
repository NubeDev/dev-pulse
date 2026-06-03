/**
 * Product detail → Runs tab (§7.4, P2).
 *
 * Table of production runs for a product (code, status, qty
 * planned / built / pass / fail), a "New run" dialog, and row →
 * run detail (`#/runs/{id}`). `org_id` for the create flow is taken
 * from the product itself (each run belongs to the product's org).
 */

import { useEffect, useState } from "react";
import { PlusIcon } from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";

import type { ProductDto, RunStatus } from "../../api/schemas/products.js";
import { navigate, runDetailRoute } from "../../routes.js";
import { useCreateRun, useProductRuns } from "../use-manufacturing-data.js";

import {
  RUN_STATUSES,
  RUN_STATUS_LABEL,
  RUN_STATUS_VARIANT,
} from "./run-shared.js";

export function ProductRunsSection({
  product,
}: {
  product: ProductDto;
}): JSX.Element {
  const runs = useProductRuns(product.id);
  const [newOpen, setNewOpen] = useState(false);

  const rows = runs.data ?? [];

  return (
    <div className="flex flex-col gap-4" data-testid="product-runs">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium">
          Production runs{" "}
          <span className="text-muted-foreground">({rows.length})</span>
        </h3>
        <Button
          variant="outline"
          size="sm"
          onClick={() => setNewOpen(true)}
          data-testid="product-run-new"
        >
          <PlusIcon className="mr-1.5 h-4 w-4" /> New run
        </Button>
      </div>

      {runs.isError ? (
        <Alert variant="destructive" data-testid="product-runs-error">
          <AlertTitle>Couldn't load runs</AlertTitle>
          <AlertDescription>{runs.error.message}</AlertDescription>
        </Alert>
      ) : runs.isPending ? (
        <div className="flex flex-col gap-2">
          {Array.from({ length: 3 }).map((_, i) => (
            <Skeleton key={i} className="h-11 rounded-md" />
          ))}
        </div>
      ) : rows.length === 0 ? (
        <div
          className="rounded-lg border-2 border-dashed border-border bg-muted/20 px-4 py-10 text-center text-sm text-muted-foreground"
          data-testid="product-runs-empty"
        >
          No production runs yet. Start a run to allocate serial numbers
          and track build / test progress.
        </div>
      ) : (
        <div className="rounded-md border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Run</TableHead>
                <TableHead>Status</TableHead>
                <TableHead className="text-right">Planned</TableHead>
                <TableHead className="text-right">Built</TableHead>
                <TableHead className="text-right">Pass</TableHead>
                <TableHead className="text-right">Fail</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((r) => (
                <TableRow
                  key={r.id}
                  className="cursor-pointer"
                  onClick={() => navigate(runDetailRoute(r.id))}
                  data-testid="product-run-row"
                >
                  <TableCell className="font-mono">{r.run_code}</TableCell>
                  <TableCell>
                    <Badge variant={RUN_STATUS_VARIANT[r.status]}>
                      {RUN_STATUS_LABEL[r.status]}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {r.qty_planned}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {r.qty_built}
                  </TableCell>
                  <TableCell className="text-right tabular-nums text-emerald-600">
                    {r.qty_passed}
                  </TableCell>
                  <TableCell className="text-right tabular-nums text-destructive">
                    {r.qty_failed}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}

      <NewRunDialog
        open={newOpen}
        onOpenChange={setNewOpen}
        product={product}
      />
    </div>
  );
}

function NewRunDialog({
  open,
  onOpenChange,
  product,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  product: ProductDto;
}): JSX.Element {
  const create = useCreateRun(product.id);
  const [runCode, setRunCode] = useState("");
  const [qtyPlanned, setQtyPlanned] = useState("0");
  const [status, setStatus] = useState<RunStatus>("planned");
  const [notes, setNotes] = useState("");

  useEffect(() => {
    if (!open) return;
    setRunCode("");
    setQtyPlanned("0");
    setStatus("planned");
    setNotes("");
    create.reset();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const qty = Number(qtyPlanned);
  const qtyValid = !Number.isNaN(qty) && qty >= 0;
  const canSubmit =
    runCode.trim().length > 0 && qtyValid && !create.isPending;

  const onSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    if (!canSubmit) return;
    create.mutate(
      {
        org_id: product.org_id,
        manufacturer_id: product.manufacturer_id ?? null,
        run_code: runCode.trim(),
        status,
        qty_planned: qty,
        notes: notes.trim() || null,
      },
      {
        onSuccess: (run) => {
          onOpenChange(false);
          navigate(runDetailRoute(run.id));
        },
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md" data-testid="new-run-dialog">
        <DialogHeader>
          <DialogTitle>New production run</DialogTitle>
          <DialogDescription>
            A run reserves serial numbers and tracks build / test
            progress for {product.name}.
          </DialogDescription>
        </DialogHeader>

        <form className="flex flex-col gap-4" onSubmit={onSubmit}>
          <div className="flex flex-col gap-2">
            <Label htmlFor="new-run-code">Run code</Label>
            <Input
              id="new-run-code"
              data-testid="new-run-code"
              value={runCode}
              onChange={(e) => setRunCode(e.target.value)}
              placeholder="RUN-2026-01"
              maxLength={200}
              autoFocus
              required
            />
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="new-run-qty">Quantity planned</Label>
            <Input
              id="new-run-qty"
              data-testid="new-run-qty"
              type="number"
              min={0}
              value={qtyPlanned}
              onChange={(e) => setQtyPlanned(e.target.value)}
            />
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="new-run-status">Status</Label>
            <Select
              value={status}
              onValueChange={(v) => setStatus(v as RunStatus)}
            >
              <SelectTrigger id="new-run-status" data-testid="new-run-status">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {RUN_STATUSES.map((s) => (
                  <SelectItem key={s} value={s}>
                    {RUN_STATUS_LABEL[s]}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="new-run-notes">Notes</Label>
            <Textarea
              id="new-run-notes"
              data-testid="new-run-notes"
              rows={3}
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              placeholder="Optional run notes…"
            />
          </div>

          {create.isError && (
            <Alert variant="destructive" data-testid="new-run-error">
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
              data-testid="new-run-submit"
              disabled={!canSubmit}
            >
              {create.isPending ? "Creating…" : "Create run"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
