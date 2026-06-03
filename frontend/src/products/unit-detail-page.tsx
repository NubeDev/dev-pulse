/**
 * `#/units/{id}` — serialised unit detail page (§7.4, P2).
 *
 * Shows the unit's serial number, status, QR code, EOL timeline, and
 * provides controls to patch the unit's status / customer / shipped_at.
 */

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { QRCodeSVG } from "qrcode.react";

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

import { api } from "../api/client.js";
import type { UnitDto, UnitStatus } from "../api/schemas/products.js";
import { PageHeading } from "../components/page-heading.jsx";
import {
  productDetailTabRoute,
  runDetailRoute,
} from "../routes.js";

import { EolDialog } from "./eol/eol-dialog.js";
import {
  UNIT_STATUSES,
  UNIT_STATUS_LABEL,
  UNIT_STATUS_VARIANT,
} from "./runs/run-shared.js";
import {
  useUnit,
  usePatchUnit,
  useUnitEol,
} from "./use-manufacturing-data.js";

export function UnitDetailPage({ unitId }: { unitId: string }): JSX.Element {
  const unit = useUnit(unitId);

  if (unit.isPending) {
    return (
      <div className="px-4 lg:px-6">
        <div
          className="flex items-center gap-2 py-4 text-sm text-muted-foreground"
          data-testid="unit-detail-loading"
        >
          <Spinner /> Loading unit…
        </div>
      </div>
    );
  }

  if (unit.isError) {
    return (
      <div className="px-4 lg:px-6">
        <Alert variant="destructive" data-testid="unit-detail-error">
          <AlertTitle>Couldn't load unit</AlertTitle>
          <AlertDescription>{unit.error.message}</AlertDescription>
        </Alert>
      </div>
    );
  }

  if (!unit.data) {
    return (
      <div className="px-4 lg:px-6">
        <Alert data-testid="unit-detail-missing">
          <AlertTitle>Unit not found</AlertTitle>
          <AlertDescription>
            This unit either doesn't exist or you don't have access to it.{" "}
            <a className="underline" href="#/products">
              Back to products
            </a>
            .
          </AlertDescription>
        </Alert>
      </div>
    );
  }

  return <UnitDetailBody unit={unit.data} />;
}

function UnitDetailBody({ unit }: { unit: UnitDto }): JSX.Element {
  const patch = usePatchUnit(unit.id, unit.run_id);
  const eolQ = useUnitEol(unit.id);
  const [eolOpen, setEolOpen] = useState(false);

  // Fetch the product for name + model display.
  const productQ = useQuery({
    queryKey: ["products", "detail", unit.product_id],
    queryFn: () => api.getProduct(unit.product_id),
    staleTime: 60_000,
  });

  const onStatus = (next: UnitStatus): void => {
    if (next === unit.status) return;
    patch.mutate({
      expected_version: unit.version,
      status: next,
      customer_id: unit.customer_id ?? null,
      built_at: unit.built_at ?? null,
      shipped_at: next === "shipped" ? (unit.shipped_at ?? new Date().toISOString()) : (unit.shipped_at ?? null),
    });
  };

  const backHref = unit.run_id
    ? runDetailRoute(unit.run_id)
    : productDetailTabRoute(unit.product_id, "units");

  const qrSvgUrl = api.unitQrSvgUrl(unit.id);

  return (
    <div className="flex flex-col gap-4 px-4 lg:px-6" data-testid="unit-detail">
      <PageHeading
        title={
          <span className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span className="font-mono" data-testid="unit-detail-serial">
              {unit.serial_number}
            </span>
            <Badge variant={UNIT_STATUS_VARIANT[unit.status]}>
              {UNIT_STATUS_LABEL[unit.status]}
            </Badge>
          </span>
        }
        description={
          <span className="flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
            {productQ.data ? (
              <a
                className="underline"
                href={productDetailTabRoute(unit.product_id, "overview")}
              >
                {productQ.data.name} · {productQ.data.model_number}
              </a>
            ) : (
              <span>{unit.product_id}</span>
            )}
            {" · "}
            <a className="underline" href={backHref}>
              {unit.run_id ? "Back to run" : "Back to product units"}
            </a>
          </span>
        }
      />

      {patch.isError && (
        <Alert variant="destructive" data-testid="unit-patch-error">
          <AlertTitle>Couldn't update unit</AlertTitle>
          <AlertDescription>{patch.error.message}</AlertDescription>
        </Alert>
      )}

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        {/* Status control */}
        <Card className="gap-2 py-4">
          <CardHeader className="px-4">
            <CardTitle className="text-sm">Details</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3 px-4">
            <div className="flex flex-col gap-1.5">
              <Label className="text-xs font-medium text-muted-foreground">
                Status
              </Label>
              <Select
                value={unit.status}
                onValueChange={(v) => onStatus(v as UnitStatus)}
                disabled={patch.isPending}
              >
                <SelectTrigger data-testid="unit-status-select">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {UNIT_STATUSES.map((s) => (
                    <SelectItem key={s} value={s}>
                      {UNIT_STATUS_LABEL[s]}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {unit.built_at ? (
              <div className="flex flex-col gap-0.5">
                <span className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
                  Built at
                </span>
                <span className="text-sm">
                  {new Date(unit.built_at).toLocaleString()}
                </span>
              </div>
            ) : null}

            {unit.shipped_at ? (
              <div className="flex flex-col gap-0.5">
                <span className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
                  Shipped at
                </span>
                <span className="text-sm">
                  {new Date(unit.shipped_at).toLocaleString()}
                </span>
              </div>
            ) : null}

            {unit.customer_id ? (
              <div className="flex flex-col gap-0.5">
                <span className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
                  Customer
                </span>
                <span className="font-mono text-sm">{unit.customer_id}</span>
              </div>
            ) : null}
          </CardContent>
        </Card>

        {/* QR card */}
        <Card className="gap-2 py-4" data-testid="unit-qr">
          <CardHeader className="px-4">
            <CardTitle className="text-sm">QR label</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col items-start gap-3 px-4">
            {unit.qr_url ? (
              <QRCodeSVG value={unit.qr_url} size={160} />
            ) : (
              <p className="text-sm text-muted-foreground">
                QR unavailable (no token configured)
              </p>
            )}
            <div className="flex gap-3">
              <a
                href={qrSvgUrl}
                download
                className="text-sm underline hover:text-foreground"
              >
                Download SVG
              </a>
              <a
                href={qrSvgUrl}
                target="_blank"
                rel="noreferrer"
                className="text-sm underline hover:text-foreground"
              >
                Print label
              </a>
            </div>
          </CardContent>
        </Card>
      </div>

      {/* EOL timeline */}
      <Card className="gap-2 py-4">
        <CardHeader className="px-4">
          <div className="flex items-center justify-between">
            <CardTitle className="text-sm">EOL test timeline</CardTitle>
            <Button
              size="sm"
              onClick={() => setEolOpen(true)}
              data-testid="record-eol-button"
            >
              Record EOL
            </Button>
          </div>
        </CardHeader>
        <CardContent className="px-4" data-testid="unit-eol-timeline">
          {eolQ.isError ? (
            <Alert variant="destructive">
              <AlertDescription>{eolQ.error.message}</AlertDescription>
            </Alert>
          ) : eolQ.isPending ? (
            <div className="flex flex-col gap-2">
              {Array.from({ length: 2 }).map((_, i) => (
                <Skeleton key={i} className="h-16 rounded-md" />
              ))}
            </div>
          ) : (eolQ.data ?? []).length === 0 ? (
            <p className="py-6 text-center text-sm text-muted-foreground">
              No EOL reports yet.
            </p>
          ) : (
            <ul className="flex flex-col gap-3">
              {(eolQ.data ?? []).map((report) => (
                <li
                  key={report.id}
                  className="rounded-md border p-3"
                  data-testid="unit-eol-report"
                >
                  <div className="flex flex-wrap items-center gap-2">
                    <Badge
                      variant={
                        report.result === "pass" ? "default" : "destructive"
                      }
                    >
                      {report.result === "pass" ? "Pass" : "Fail"}
                    </Badge>
                    {report.station ? (
                      <span className="text-sm text-muted-foreground">
                        Station: {report.station}
                      </span>
                    ) : null}
                    {report.firmware ? (
                      <span className="font-mono text-xs text-muted-foreground">
                        FW: {report.firmware}
                      </span>
                    ) : null}
                    <span className="ml-auto text-xs text-muted-foreground">
                      {new Date(report.tested_at).toLocaleString()}
                    </span>
                  </div>

                  {report.tested_by ? (
                    <div className="mt-1 text-xs text-muted-foreground">
                      Tester: {report.tested_by}
                    </div>
                  ) : null}

                  {report.notes ? (
                    <div className="mt-1.5 text-sm">{report.notes}</div>
                  ) : null}

                  {report.measurements != null &&
                  Object.keys(
                    report.measurements as Record<string, unknown>,
                  ).length > 0 ? (
                    <div className="mt-2 grid grid-cols-2 gap-1 rounded-md bg-muted/40 px-2 py-1.5 text-xs sm:grid-cols-3">
                      {Object.entries(
                        report.measurements as Record<string, unknown>,
                      ).map(([k, v]) => (
                        <div key={k} className="flex gap-1">
                          <span className="font-medium text-muted-foreground">
                            {k}:
                          </span>
                          <span className="font-mono">
                            {String(v ?? "—")}
                          </span>
                        </div>
                      ))}
                    </div>
                  ) : null}
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>

      <EolDialog
        unitId={unit.id}
        runId={unit.run_id ?? null}
        open={eolOpen}
        onOpenChange={setEolOpen}
      />
    </div>
  );
}
