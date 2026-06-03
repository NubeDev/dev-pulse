/**
 * `#/customers/{id}` — read-only customer detail (§7.4, P1 subset).
 *
 * Shows the stored customer fields. Shipped-units / open-RMA rollups
 * are P2/P3 — a placeholder note marks where they'll appear once
 * those features ship.
 */

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

import type { CustomerDto } from "../../api/schemas/products.js";
import { Markdown } from "../../components/markdown.jsx";
import { PageHeading } from "../../components/page-heading.jsx";
import { navigate, rmaDetailRoute } from "../../routes.js";
import { useRmaList } from "../use-manufacturing-data.js";
import { useCustomer } from "../use-products-data.js";
import {
  RMA_STATUS_LABEL,
  RMA_STATUS_VARIANT,
} from "../rma/rma-shared.js";

export function CustomerDetailPage({
  customerId,
}: {
  customerId: string;
}): JSX.Element {
  const customer = useCustomer(customerId);

  if (customer.isPending) {
    return (
      <div className="px-4 lg:px-6">
        <div
          className="flex items-center gap-2 py-4 text-sm text-muted-foreground"
          data-testid="customer-detail-loading"
        >
          <Spinner /> Loading customer…
        </div>
      </div>
    );
  }

  if (customer.isError) {
    return (
      <div className="px-4 lg:px-6">
        <Alert variant="destructive" data-testid="customer-detail-error">
          <AlertTitle>Couldn't load customer</AlertTitle>
          <AlertDescription>{customer.error.message}</AlertDescription>
        </Alert>
      </div>
    );
  }

  if (!customer.data) {
    return (
      <div className="px-4 lg:px-6">
        <Alert data-testid="customer-detail-missing">
          <AlertTitle>Customer not found</AlertTitle>
          <AlertDescription>
            This customer either doesn't exist or you don't have access
            to it.{" "}
            <a className="underline" href="#/manufacturing/parties">
              Back to parties
            </a>
            .
          </AlertDescription>
        </Alert>
      </div>
    );
  }

  return <CustomerDetailBody customer={customer.data} />;
}

function CustomerRmaRollup({
  customerId,
}: {
  customerId: string;
}): JSX.Element {
  const rmas = useRmaList({ customer_id: customerId });
  const rows = rmas.data ?? [];

  return (
    <Card className="gap-2 py-4" data-testid="customer-rma-rollup">
      <CardHeader className="px-4">
        <CardTitle className="text-sm">
          Returns (RMAs){" "}
          <span className="text-muted-foreground">({rows.length})</span>
        </CardTitle>
      </CardHeader>
      <CardContent className="px-4">
        {rmas.isError ? (
          <Alert variant="destructive" data-testid="customer-rma-error">
            <AlertDescription>{rmas.error.message}</AlertDescription>
          </Alert>
        ) : rmas.isPending ? (
          <div className="flex flex-col gap-2">
            {Array.from({ length: 3 }).map((_, i) => (
              <Skeleton key={i} className="h-10 rounded-md" />
            ))}
          </div>
        ) : rows.length === 0 ? (
          <p
            className="py-6 text-center text-sm text-muted-foreground"
            data-testid="customer-rma-empty"
          >
            No returns for this customer.
          </p>
        ) : (
          <div className="rounded-md border">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>RMA #</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Warranty</TableHead>
                  <TableHead>Created</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {rows.map((r) => (
                  <TableRow
                    key={r.id}
                    className="cursor-pointer"
                    onClick={() => navigate(rmaDetailRoute(r.id))}
                    data-testid={`customer-rma-row-${r.id}`}
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
                    <TableCell className="text-muted-foreground">
                      {new Date(r.created_at).toLocaleDateString("en-AU")}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function Field({
  label,
  value,
}: {
  label: string;
  value: string | null | undefined;
}): JSX.Element {
  return (
    <div className="flex flex-col gap-0.5">
      <span className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
        {label}
      </span>
      <span className="text-sm">{value && value.trim() ? value : "—"}</span>
    </div>
  );
}

function CustomerDetailBody({
  customer,
}: {
  customer: CustomerDto;
}): JSX.Element {
  return (
    <div className="flex flex-col gap-4 px-4 lg:px-6" data-testid="customer-detail">
      <PageHeading
        title={
          <span className="flex items-center gap-2">
            <span data-testid="customer-detail-name">{customer.name}</span>
            {customer.archived_at ? (
              <Badge variant="outline">Archived</Badge>
            ) : null}
          </span>
        }
        description={
          <a className="text-sm underline" href="#/manufacturing/parties">
            Back to parties
          </a>
        }
      />

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <Card className="gap-2 py-4">
          <CardHeader className="px-4">
            <CardTitle className="text-sm">Contact</CardTitle>
          </CardHeader>
          <CardContent className="grid grid-cols-2 gap-3 px-4">
            <Field label="Contact name" value={customer.contact_name} />
            <Field label="Account ref" value={customer.account_ref} />
            <Field label="Email" value={customer.email} />
            <Field label="Phone" value={customer.phone} />
            <Field label="Website" value={customer.website} />
            <Field label="Address" value={customer.address} />
          </CardContent>
        </Card>

        <Card className="gap-2 py-4">
          <CardHeader className="px-4">
            <CardTitle className="text-sm">Notes</CardTitle>
          </CardHeader>
          <CardContent className="px-4">
            {customer.notes && customer.notes.trim() ? (
              <Markdown>{customer.notes}</Markdown>
            ) : (
              <p className="text-sm text-muted-foreground">No notes.</p>
            )}
          </CardContent>
        </Card>
      </div>

      <CustomerRmaRollup customerId={customer.id} />
    </div>
  );
}
