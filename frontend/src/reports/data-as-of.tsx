/**
 * "Data as of <ts>" banner — SCOPE §0.3 / §11.7.
 *
 * Rendered as a shadcn `Alert` (information variant) with a `Badge`
 * for the staleness state (fresh / lagging / stale / pending) and a
 * second `Badge` tagging the source field on the `data_as_of` envelope
 * (headline / reconciler / webhook). This is the per-page freshness
 * banner that sits between the filter Card and the results Card, so
 * the operator sees the data's age before reading totals.
 */

import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

import type { DataAsOf } from "../api/client.js";

export interface DataAsOfBannerProps {
  data: DataAsOf | null | undefined;
  /** Loading shim from the parent query. */
  loading?: boolean;
}

type StalenessBand = "fresh" | "lagging" | "stale" | "pending";

const HOUR_MS = 60 * 60 * 1000;

function bandOf(ageMs: number | null): StalenessBand {
  if (ageMs === null) return "pending";
  if (ageMs < HOUR_MS) return "fresh";
  if (ageMs < 4 * HOUR_MS) return "lagging";
  return "stale";
}

/** Per-band badge palette. We keep colours via Tailwind utility
 *  classes so dark mode picks up the right contrast through the
 *  shared `--background` / `--foreground` tokens. */
const BAND_BADGE: Record<StalenessBand, string> = {
  fresh:
    "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
  lagging:
    "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300",
  stale:
    "border-red-500/30 bg-red-500/10 text-red-700 dark:text-red-300",
  pending: "border-border bg-muted text-muted-foreground",
};

const BAND_LABEL: Record<StalenessBand, string> = {
  fresh: "Fresh",
  lagging: "Lagging",
  stale: "Stale",
  pending: "Pending",
};

function pick(d: DataAsOf): { ts: string | null; source: string } {
  if (d.headline) return { ts: d.headline, source: "headline" };
  if (d.reconciler_latest) return { ts: d.reconciler_latest, source: "reconciler" };
  if (d.webhook_latest) return { ts: d.webhook_latest, source: "webhook" };
  return { ts: null, source: "pending" };
}

function formatTs(rfc3339: string): string {
  const d = new Date(rfc3339);
  if (Number.isNaN(d.getTime())) return rfc3339;
  return d.toLocaleString("en-AU", {
    year: "numeric",
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    timeZoneName: "short",
  });
}

export function DataAsOfBanner({ data, loading }: DataAsOfBannerProps): JSX.Element {
  let band: StalenessBand = "pending";
  let ts: string | null = null;
  let source = "pending";
  let body: JSX.Element;

  if (loading || !data) {
    body = <span>Data as of … (loading)</span>;
  } else {
    const picked = pick(data);
    ts = picked.ts;
    source = picked.source;
    if (!ts) {
      body = (
        <span>Data not yet available (pending first reconciler run)</span>
      );
    } else {
      const ageMs = Date.now() - new Date(ts).getTime();
      band = bandOf(ageMs);
      body = (
        <span className="inline-flex flex-wrap items-center gap-x-2 gap-y-1">
          <strong className="font-medium text-foreground">Data as of</strong>
          <span className="tabular-nums">{formatTs(ts)}</span>
        </span>
      );
    }
  }

  return (
    <Alert
      data-testid="data-as-of"
      role="status"
      aria-live="polite"
      className="flex items-center gap-3"
    >
      <Badge
        variant="outline"
        className={cn("gap-1.5 text-[0.6875rem] uppercase tracking-wider", BAND_BADGE[band])}
      >
        <span
          aria-hidden
          className={cn(
            "size-1.5 rounded-full",
            band === "fresh" && "bg-emerald-500",
            band === "lagging" && "bg-amber-500",
            band === "stale" && "bg-red-500",
            band === "pending" && "bg-muted-foreground",
          )}
        />
        {BAND_LABEL[band]}
      </Badge>
      <AlertDescription className="flex-1">{body}</AlertDescription>
      {ts ? (
        <Badge
          variant="secondary"
          className="text-[0.625rem] uppercase tracking-wider"
        >
          {source}
        </Badge>
      ) : null}
    </Alert>
  );
}
