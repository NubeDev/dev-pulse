/**
 * `#/rma/{id}` — RMA detail page (§7.4, P3).
 *
 * Header: RMA # + status Badge + warranty Badge. Horizontal status
 * stepper + Select to change status. Status change auto-sets
 * received_at / resolved_at when transitioning for the first time.
 * Markdown fields for diagnosis + resolution (editable with Save).
 * Reason shown read-only. Linked product / unit / customer cards.
 * CAS PATCH (expected_version), 409 → refetch + banner.
 */

import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { Textarea } from "@/components/ui/textarea";

import { api } from "../../api/client.js";
import type { RmaDto, RmaStatus } from "../../api/schemas/products.js";
import { isDpRestError } from "../../api/error.js";
import { Markdown } from "../../components/markdown.jsx";
import { PageHeading } from "../../components/page-heading.jsx";
import {
  customerDetailRoute,
  navigate,
  rmaListRoute,
  unitDetailRoute,
} from "../../routes.js";
import { useRma, usePatchRma } from "../use-manufacturing-data.js";

import {
  RMA_STATUSES,
  RMA_STATUS_LABEL,
  RMA_STATUS_VARIANT,
  RMA_TERMINAL_STATUSES,
} from "./rma-shared.js";

export function RmaDetailPage({ rmaId }: { rmaId: string }): JSX.Element {
  const rma = useRma(rmaId);

  if (rma.isPending) {
    return (
      <div className="px-4 lg:px-6">
        <div
          className="flex items-center gap-2 py-4 text-sm text-muted-foreground"
          data-testid="rma-detail-loading"
        >
          <Spinner /> Loading RMA…
        </div>
      </div>
    );
  }

  if (rma.isError) {
    return (
      <div className="px-4 lg:px-6">
        <Alert variant="destructive" data-testid="rma-detail-error">
          <AlertTitle>Couldn't load RMA</AlertTitle>
          <AlertDescription>{rma.error.message}</AlertDescription>
        </Alert>
      </div>
    );
  }

  if (!rma.data) {
    return (
      <div className="px-4 lg:px-6">
        <Alert data-testid="rma-detail-missing">
          <AlertTitle>RMA not found</AlertTitle>
          <AlertDescription>
            This return either doesn't exist or you don't have access to it.{" "}
            <a className="underline" href={rmaListRoute()}>
              Back to returns
            </a>
            .
          </AlertDescription>
        </Alert>
      </div>
    );
  }

  return <RmaDetailBody rma={rma.data} />;
}

function RmaDetailBody({ rma }: { rma: RmaDto }): JSX.Element {
  const patch = usePatchRma(rma.id);
  const conflict = isDpRestError(patch.error) && patch.error.status === 409;

  const [diagnosis, setDiagnosis] = useState(rma.diagnosis ?? "");
  const [resolution, setResolution] = useState(rma.resolution ?? "");

  // Seed fields from fresh server data after a refetch
  useEffect(() => {
    setDiagnosis(rma.diagnosis ?? "");
    setResolution(rma.resolution ?? "");
  }, [rma.diagnosis, rma.resolution]);

  // Product, unit, customer lookups
  const productQuery = useQuery({
    queryKey: ["products", "detail", rma.product_id],
    queryFn: () => api.getProduct(rma.product_id),
    staleTime: 60_000,
  });
  const unitQuery = useQuery({
    queryKey: ["units", "detail", rma.unit_id],
    queryFn: () => (rma.unit_id ? api.getUnit(rma.unit_id) : Promise.resolve(null)),
    enabled: !!rma.unit_id,
    staleTime: 60_000,
  });
  const customerQuery = useQuery({
    queryKey: ["customers", "detail", rma.customer_id],
    queryFn: () =>
      rma.customer_id
        ? api.getCustomer(rma.customer_id)
        : Promise.resolve(null),
    enabled: !!rma.customer_id,
    staleTime: 60_000,
  });

  /** Build the full PATCH upsert body, overriding the changed field(s). */
  const buildPatch = (overrides: Partial<{
    status: RmaStatus;
    under_warranty: boolean;
    diagnosis: string | null;
    resolution: string | null;
    received_at: string | null;
    resolved_at: string | null;
  }>) => {
    const nextStatus = overrides.status ?? rma.status;
    const now = new Date().toISOString();

    // Auto-set timestamps when transitioning
    let receivedAt = rma.received_at ?? null;
    if (nextStatus === "received" && !receivedAt) {
      receivedAt = now;
    }
    let resolvedAt = rma.resolved_at ?? null;
    if (RMA_TERMINAL_STATUSES.has(nextStatus) && !resolvedAt) {
      resolvedAt = now;
    }

    return {
      expected_version: rma.version,
      unit_id: rma.unit_id ?? null,
      customer_id: rma.customer_id ?? null,
      under_warranty: overrides.under_warranty ?? rma.under_warranty,
      status: nextStatus,
      reason: rma.reason ?? null,
      diagnosis: overrides.diagnosis !== undefined ? overrides.diagnosis : (rma.diagnosis ?? null),
      resolution: overrides.resolution !== undefined ? overrides.resolution : (rma.resolution ?? null),
      received_at: overrides.received_at !== undefined ? overrides.received_at : receivedAt,
      resolved_at: overrides.resolved_at !== undefined ? overrides.resolved_at : resolvedAt,
    };
  };

  const onStatusChange = (next: RmaStatus): void => {
    if (next === rma.status) return;
    patch.mutate(buildPatch({ status: next }));
  };

  const onWarrantyToggle = (): void => {
    patch.mutate(buildPatch({ under_warranty: !rma.under_warranty }));
  };

  const onSaveDiagnosis = (): void => {
    patch.mutate(buildPatch({ diagnosis: diagnosis.trim() || null }));
  };

  const onSaveResolution = (): void => {
    patch.mutate(buildPatch({ resolution: resolution.trim() || null }));
  };

  return (
    <div className="flex flex-col gap-4 px-4 lg:px-6" data-testid="rma-detail">
      <PageHeading
        title={
          <span className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span className="font-mono" data-testid="rma-detail-number">
              {rma.rma_number}
            </span>
            <Badge variant={RMA_STATUS_VARIANT[rma.status]}>
              {RMA_STATUS_LABEL[rma.status]}
            </Badge>
            {rma.under_warranty && (
              <Badge variant="secondary">Warranty</Badge>
            )}
          </span>
        }
        description={
          <a
            className="text-sm text-muted-foreground underline"
            href={rmaListRoute()}
          >
            Back to returns
          </a>
        }
        trailing={
          <Select
            value={rma.status}
            onValueChange={(v) => onStatusChange(v as RmaStatus)}
          >
            <SelectTrigger
              className="h-9 w-44"
              data-testid="rma-status-select"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {RMA_STATUSES.map((s) => (
                <SelectItem key={s} value={s}>
                  {RMA_STATUS_LABEL[s]}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        }
      />

      {/* Status stepper */}
      <div className="flex flex-wrap items-center gap-1 overflow-x-auto">
        {RMA_STATUSES.map((s, i) => {
          const isCurrent = s === rma.status;
          const isPast =
            RMA_STATUSES.indexOf(rma.status) > i &&
            rma.status !== "rejected"; // rejected is not a linear step
          return (
            <div key={s} className="flex items-center gap-1">
              {i > 0 && (
                <span className="text-muted-foreground">›</span>
              )}
              <button
                type="button"
                onClick={() => onStatusChange(s)}
                className={[
                  "rounded-full px-2.5 py-0.5 text-xs font-medium transition-colors",
                  isCurrent
                    ? "bg-primary text-primary-foreground"
                    : isPast
                    ? "bg-muted text-muted-foreground line-through"
                    : "bg-muted/50 text-muted-foreground hover:bg-muted",
                ].join(" ")}
              >
                {RMA_STATUS_LABEL[s]}
              </button>
            </div>
          );
        })}
      </div>

      {/* Conflict / error banners */}
      {conflict && (
        <Alert variant="destructive" data-testid="rma-detail-conflict">
          <AlertTitle>This RMA changed underneath you</AlertTitle>
          <AlertDescription>
            Someone else updated this return. The latest values have been
            reloaded — re-apply your change.
          </AlertDescription>
        </Alert>
      )}
      {patch.isError && !conflict && (
        <Alert variant="destructive" data-testid="rma-detail-patch-error">
          <AlertTitle>Couldn't update RMA</AlertTitle>
          <AlertDescription>{patch.error.message}</AlertDescription>
        </Alert>
      )}

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        {/* Summary card */}
        <Card className="gap-2 py-4">
          <CardHeader className="px-4">
            <CardTitle className="text-sm">Summary</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3 px-4">
            <InfoRow label="RMA #">
              <span className="font-mono">{rma.rma_number}</span>
            </InfoRow>
            <InfoRow label="Warranty">
              <button
                type="button"
                className="flex items-center gap-1.5 text-sm underline"
                onClick={onWarrantyToggle}
                disabled={patch.isPending}
                data-testid="rma-warranty-toggle"
              >
                {rma.under_warranty ? (
                  <Badge variant="secondary">Yes</Badge>
                ) : (
                  <span className="text-muted-foreground">No</span>
                )}
              </button>
            </InfoRow>
            {rma.received_at && (
              <InfoRow label="Received">
                {new Date(rma.received_at).toLocaleString("en-AU")}
              </InfoRow>
            )}
            {rma.resolved_at && (
              <InfoRow label="Resolved">
                {new Date(rma.resolved_at).toLocaleString("en-AU")}
              </InfoRow>
            )}
            <InfoRow label="Created">
              {new Date(rma.created_at).toLocaleString("en-AU")}
            </InfoRow>
          </CardContent>
        </Card>

        {/* Linked entities */}
        <Card className="gap-2 py-4">
          <CardHeader className="px-4">
            <CardTitle className="text-sm">Linked</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3 px-4">
            <InfoRow label="Product">
              {productQuery.isPending ? (
                <Skeleton className="h-4 w-32" />
              ) : productQuery.data ? (
                <a
                  className="underline"
                  href={`#/products/${productQuery.data.id}`}
                >
                  {productQuery.data.name}
                </a>
              ) : (
                <span className="font-mono text-xs text-muted-foreground">
                  {rma.product_id}
                </span>
              )}
            </InfoRow>
            <InfoRow label="Unit / Serial">
              {rma.unit_id ? (
                unitQuery.isPending ? (
                  <Skeleton className="h-4 w-24" />
                ) : unitQuery.data ? (
                  <a
                    className="font-mono underline"
                    href={unitDetailRoute(unitQuery.data.id)}
                  >
                    {unitQuery.data.serial_number}
                  </a>
                ) : (
                  <span className="font-mono text-xs text-muted-foreground">
                    {rma.unit_id}
                  </span>
                )
              ) : (
                <span className="text-muted-foreground">—</span>
              )}
            </InfoRow>
            <InfoRow label="Customer">
              {rma.customer_id ? (
                customerQuery.isPending ? (
                  <Skeleton className="h-4 w-28" />
                ) : customerQuery.data ? (
                  <a
                    className="underline"
                    href={customerDetailRoute(customerQuery.data.id)}
                  >
                    {customerQuery.data.name}
                  </a>
                ) : (
                  <span className="font-mono text-xs text-muted-foreground">
                    {rma.customer_id}
                  </span>
                )
              ) : (
                <span className="text-muted-foreground">—</span>
              )}
            </InfoRow>
          </CardContent>
        </Card>
      </div>

      {/* Reason (read-only) */}
      {rma.reason && (
        <Card className="gap-2 py-4" data-testid="rma-reason-card">
          <CardHeader className="px-4">
            <CardTitle className="text-sm">Reason (customer-reported)</CardTitle>
          </CardHeader>
          <CardContent className="px-4">
            <p className="text-sm text-muted-foreground">{rma.reason}</p>
          </CardContent>
        </Card>
      )}

      {/* Diagnosis */}
      <Card className="gap-2 py-4" data-testid="rma-diagnosis-card">
        <CardHeader className="px-4">
          <CardTitle className="text-sm">Diagnosis</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-3 px-4">
          <div className="flex flex-col gap-2">
            <Label htmlFor="rma-diagnosis">Diagnosis (markdown)</Label>
            <Textarea
              id="rma-diagnosis"
              data-testid="rma-diagnosis"
              rows={4}
              value={diagnosis}
              onChange={(e) => setDiagnosis(e.target.value)}
              placeholder="Root cause, component failure, test results…"
            />
            {diagnosis.trim() && (
              <div className="rounded-md border bg-background p-3">
                <Markdown>{diagnosis}</Markdown>
              </div>
            )}
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={onSaveDiagnosis}
            disabled={patch.isPending}
            data-testid="rma-save-diagnosis"
          >
            Save diagnosis
          </Button>
        </CardContent>
      </Card>

      {/* Resolution */}
      <Card className="gap-2 py-4" data-testid="rma-resolution-card">
        <CardHeader className="px-4">
          <CardTitle className="text-sm">Resolution</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-3 px-4">
          <div className="flex flex-col gap-2">
            <Label htmlFor="rma-resolution">Resolution (markdown)</Label>
            <Textarea
              id="rma-resolution"
              data-testid="rma-resolution"
              rows={4}
              value={resolution}
              onChange={(e) => setResolution(e.target.value)}
              placeholder="What was done: repair, replacement, rejection reason…"
            />
            {resolution.trim() && (
              <div className="rounded-md border bg-background p-3">
                <Markdown>{resolution}</Markdown>
              </div>
            )}
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={onSaveResolution}
            disabled={patch.isPending}
            data-testid="rma-save-resolution"
          >
            Save resolution
          </Button>
        </CardContent>
      </Card>
    </div>
  );
}

function InfoRow({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}): JSX.Element {
  return (
    <div className="flex items-start gap-2">
      <span className="w-24 shrink-0 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
        {label}
      </span>
      <span className="text-sm">{children}</span>
    </div>
  );
}
