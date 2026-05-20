/**
 * "Data as of <ts>" banner — SCOPE §0.3 / §11.7.
 *
 * Renders the lens-aware headline timestamp prominently, with a
 * fallback to the latest reconciler/webhook stamp when `headline`
 * is null (a fresh org with no reconciler run yet).
 */

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
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: "0.5rem",
        padding: "0.375rem 0.75rem",
        borderRadius: "var(--radius-sm, 0.375rem)",
        background: "var(--muted)",
        color: "var(--muted-foreground)",
        fontSize: "0.8125rem",
        fontVariantNumeric: "tabular-nums",
      }}
    >
      <span
        aria-hidden
        style={{
          width: "0.5rem",
          height: "0.5rem",
          borderRadius: "50%",
          background: loading ? "var(--muted-foreground)" : "oklch(0.7 0.18 145)",
        }}
      />
      {loading || !data ? (
        <span>Data as of … (loading)</span>
      ) : (() => {
        const { ts, source } = pick(data);
        if (!ts) return <span>Data not yet available (pending first reconciler run)</span>;
        return (
          <span>
            <strong style={{ color: "var(--foreground)" }}>Data as of</strong>{" "}
            {formatTs(ts)}
            <span style={{ marginLeft: "0.375rem", opacity: 0.7 }}>· {source}</span>
          </span>
        );
      })()}
    </div>
  );
}
