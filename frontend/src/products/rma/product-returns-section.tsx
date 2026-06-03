/**
 * Product detail → Returns tab (§7.4, P3).
 *
 * Table of RMAs for this product: RMA #, status, warranty, customer,
 * created date. "New RMA" button opens NewRmaDialog with product preset.
 * Row → `#/rma/{id}`.
 */

import { useState } from "react";
import { PlusIcon } from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

import type { ProductDto } from "../../api/schemas/products.js";
import { navigate, rmaDetailRoute } from "../../routes.js";
import { useRmaList } from "../use-manufacturing-data.js";

import { NewRmaDialog } from "./new-rma-dialog.js";
import { RMA_STATUS_LABEL, RMA_STATUS_VARIANT } from "./rma-shared.js";

export function ProductReturnsSection({
  product,
}: {
  product: ProductDto;
}): JSX.Element {
  const rmas = useRmaList({ product_id: product.id });
  const [newOpen, setNewOpen] = useState(false);

  const rows = rmas.data ?? [];

  return (
    <div className="flex flex-col gap-4" data-testid="product-returns">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium">
          Returns{" "}
          <span className="text-muted-foreground">({rows.length})</span>
        </h3>
        <Button
          variant="outline"
          size="sm"
          onClick={() => setNewOpen(true)}
          data-testid="product-returns-new"
        >
          <PlusIcon className="mr-1.5 h-4 w-4" /> New RMA
        </Button>
      </div>

      {rmas.isError ? (
        <Alert variant="destructive" data-testid="product-returns-error">
          <AlertTitle>Couldn't load returns</AlertTitle>
          <AlertDescription>{rmas.error.message}</AlertDescription>
        </Alert>
      ) : rmas.isPending ? (
        <div className="flex flex-col gap-2">
          {Array.from({ length: 3 }).map((_, i) => (
            <Skeleton key={i} className="h-11 rounded-md" />
          ))}
        </div>
      ) : rows.length === 0 ? (
        <div
          className="rounded-lg border-2 border-dashed border-border bg-muted/20 px-4 py-10 text-center text-sm text-muted-foreground"
          data-testid="product-returns-empty"
        >
          No returns for this product yet.{" "}
          <button
            type="button"
            className="underline"
            onClick={() => setNewOpen(true)}
          >
            Log a return.
          </button>
        </div>
      ) : (
        <div className="rounded-md border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>RMA #</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Warranty</TableHead>
                <TableHead>Customer</TableHead>
                <TableHead>Created</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((r) => (
                <TableRow
                  key={r.id}
                  className="cursor-pointer"
                  onClick={() => navigate(rmaDetailRoute(r.id))}
                  data-testid={`product-return-row-${r.id}`}
                >
                  <TableCell className="font-mono">{r.rma_number}</TableCell>
                  <TableCell>
                    <Badge variant={RMA_STATUS_VARIANT[r.status]}>
                      {RMA_STATUS_LABEL[r.status]}
                    </Badge>
                  </TableCell>
                  <TableCell>
                    {r.under_warranty ? (
                      <Badge variant="secondary">Warranty</Badge>
                    ) : (
                      <span className="text-muted-foreground">—</span>
                    )}
                  </TableCell>
                  <TableCell>
                    {r.customer_id ? (
                      <span className="font-mono text-xs text-muted-foreground">
                        {r.customer_id.slice(0, 8)}…
                      </span>
                    ) : (
                      <span className="text-muted-foreground">—</span>
                    )}
                  </TableCell>
                  <TableCell className="text-muted-foreground">
                    {new Date(r.created_at).toLocaleDateString()}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}

      <NewRmaDialog
        open={newOpen}
        onOpenChange={setNewOpen}
        product={product}
      />
    </div>
  );
}
