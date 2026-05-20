/**
 * "Data as of <ts>" banner — SCOPE §0.3 / §11.7.
 *
 * Renders the lens-aware headline timestamp prominently, with a
 * fallback to the latest reconciler/webhook stamp when `headline`
 * is null (a fresh org with no reconciler run yet).
 *
 * Visual: a compact muted pill that lives inside the report card's
 * header — uses Tailwind tokens (`bg-muted`, `text-muted-foreground`)
 * plus a shadcn `Badge` for the source/relative-time delta tag.
 */

import { Badge } from "@nube/starter-ui-kit/components/badge";
import { cn } from "@nube/starter-ui-kit/lib/utils";

import type { DataAsOf } from "../api/client.js";

export interface DataAsOfBannerProps {
  data: DataAsOf | null | undefined;
  /** Loading shim from the parent query. */
  loading?: boolean;
}

function pick(d: DataAsOf): { ts: string | null; source: string } {
  if (d.headline) return { ts: d.headline, source: "headline" };
  if (d.reconciler_latest) return { ts: d.reconciler_latest, source: "reconciler" };
  if (d.webhook_latest) return { ts: d.webhook_latest, source: "webhook" };
  return { ts: null, source: "pending" };
}

function formatTs(rfc3339: string): string {
  // Keep operator-friendly: ISO date+time minus seconds, in viewer TZ.
  const d = new Date(rfc3339);
  if (Number.isNaN(d.getTime())) return rfc3339;
  return d.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    timeZoneName: "short",
  });
}

export function DataAsOfBanner({ data, loading }: DataAsOfBannerProps): JSX.Element {
  return (
    <div
      role="status"
      aria-live="polite"
      data-testid="data-as-of"
      className="inline-flex items-center gap-2 rounded-md bg-muted px-3 py-1.5 text-[0.8125rem] tabular-nums text-muted-foreground"
    >
      <span
        aria-hidden
        className={cn(
          "size-2 shrink-0 rounded-full",
          loading ? "bg-muted-foreground" : "bg-emerald-500",
        )}
      />
      {loading || !data ? (
        <span>Data as of … (loading)</span>
      ) : (() => {
        const { ts, source } = pick(data);
        if (!ts) return <span>Data not yet available (pending first reconciler run)</span>;
        return (
          <span className="inline-flex items-center gap-2">
            <span>
              <strong className="text-foreground">Data as of</strong> {formatTs(ts)}
            </span>
            <Badge variant="secondary" className="uppercase tracking-wider text-[0.625rem]">
              {source}
            </Badge>
          </span>
        );
      })()}
    </div>
  );
}
