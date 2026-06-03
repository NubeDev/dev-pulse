/**
 * `#/rma` — Returns / RMA list page (§7.4, P3).
 *
 * Status filter via `?status=` query param. Table shows RMA #,
 * status, warranty, product name, customer, created date.
 * Row → `#/rma/{id}`.
 */

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { PlusIcon } from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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
import type { RmaStatus } from "../../api/schemas/products.js";
import { PageHeading } from "../../components/page-heading.jsx";
import {
  navigate,
  rmaDetailRoute,
  rmaListRoute,
  rmaStatusOf,
  useRoute,
  type RmaStatusRoute,
} from "../../routes.js";
import { useRmaList } from "../use-manufacturing-data.js";

import { NewRmaDialog } from "./new-rma-dialog.js";
import {
  RMA_STATUSES,
  RMA_STATUS_LABEL,
  RMA_STATUS_VARIANT,
} from "./rma-shared.js";

const ALL = "__all__";

export function RmaListPage(): JSX.Element {
  const route = useRoute();
  const statusFilter = rmaStatusOf(route) as RmaStatus | null;
  const [newOpen, setNewOpen] = useState(false);

  const rmaQuery = useRmaList(
    statusFilter ? { status: statusFilter } : {},
  );

  // Product name lookup map — best-effort, 200 limit
  const productsQuery = useQuery({
    queryKey: ["products", "list-for-rma"],
    queryFn: () => api.listProducts({ limit: 200 }),
    staleTime: 60_000,
  });
  const productNames = useMemo(() => {
    const m = new Map<string, string>();
    for (const p of productsQuery.data?.rows ?? []) {
      m.set(p.id, p.name);
    }
    return m;
  }, [productsQuery.data]);

  const rows = rmaQuery.data ?? [];

  const onStatusChange = (v: string): void => {
    if (v === ALL) {
      navigate(rmaListRoute());
    } else {
      navigate(rmaListRoute(v as RmaStatusRoute));
    }
  };

  return (
    <div className="flex flex-col gap-4 px-4 lg:px-6" data-testid="rma-list">
      <PageHeading
        title="Returns"
        trailing={
          <div className="flex items-center gap-2">
            <Select
              value={statusFilter ?? ALL}
              onValueChange={onStatusChange}
            >
              <SelectTrigger className="h-9 w-44" data-testid="rma-status-filter">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ALL}>All statuses</SelectItem>
                {RMA_STATUSES.map((s) => (
                  <SelectItem key={s} value={s}>
                    {RMA_STATUS_LABEL[s]}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button
              size="sm"
              onClick={() => setNewOpen(true)}
              data-testid="rma-new"
            >
              <PlusIcon className="mr-1.5 h-4 w-4" /> New RMA
            </Button>
          </div>
        }
      />

      {rmaQuery.isError ? (
        <Alert variant="destructive" data-testid="rma-list-error">
          <AlertTitle>Couldn't load returns</AlertTitle>
          <AlertDescription>{rmaQuery.error.message}</AlertDescription>
        </Alert>
      ) : rmaQuery.isPending ? (
        <div className="flex flex-col gap-2">
          {Array.from({ length: 5 }).map((_, i) => (
            <Skeleton key={i} className="h-11 rounded-md" />
          ))}
        </div>
      ) : rows.length === 0 ? (
        <div
          className="rounded-lg border-2 border-dashed border-border bg-muted/20 px-4 py-10 text-center text-sm text-muted-foreground"
          data-testid="rma-list-empty"
        >
          No returns{statusFilter ? ` with status "${RMA_STATUS_LABEL[statusFilter]}"` : ""}.{" "}
          <button
            type="button"
            className="underline"
            onClick={() => setNewOpen(true)}
          >
            Log a new return.
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
                <TableHead>Product</TableHead>
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
                  data-testid={`rma-row-${r.id}`}
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
                  <TableCell className="max-w-[180px] truncate">
                    {productNames.get(r.product_id) ?? (
                      <span className="font-mono text-xs text-muted-foreground">
                        {r.product_id.slice(0, 8)}…
                      </span>
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
                    {new Date(r.created_at).toLocaleDateString("en-AU")}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}

      <NewRmaDialog open={newOpen} onOpenChange={setNewOpen} />
    </div>
  );
}
