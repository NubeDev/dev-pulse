/**
 * Freshness dashboard — `GET /reports/freshness`.
 *
 * SCOPE §0.3 makes "Data as of <ts>" a first-class affordance; this
 * page is the dedicated operator view of that signal. It renders:
 *
 *   - an overall system-health headline (worst per-org lag wins,
 *     plus the most recent reconciler / webhook stamp), and
 *   - a card grid (one card per org) with "last updated <relative
 *     time>", colour-coded by staleness (green < 1h, yellow < 4h,
 *     red ≥ 4h).
 *
 * The `/reports/freshness` envelope has `rows: null` — every signal
 * lives on `data_as_of`:
 *   - `headline`           — server-picked "most relevant" stamp
 *   - `per_org`            — `{ <org_uuid>: <rfc3339> }` (the cards)
 *   - `reconciler_latest`  — most recent reconciler tick across all
 *                            orgs (the "reconciler health" signal)
 *   - `webhook_latest`     — most recent webhook ingest (the
 *                            "webhook lag" signal)
 *
 * Org names come from `GET /orgs`; we join client-side. An org that
 * appears in `/orgs` but is absent from `per_org` renders as
 * "pending first reconciler run" (SCOPE: absent ≠ stale).
 *
 * No leaderboard affordance — cards are sorted by org login so the
 * order is stable, not by a composite score (§4 design constraint).
 *
 * Smoke harness: `VITE_USE_MOCK_REPORTS=1` short-circuits both the
 * freshness query and the org list so the page renders without
 * dp-server. The colour buckets are exercised by seeding three
 * mock orgs with staleness in each band.
 */

import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@nube/starter-ui-kit/components/card";

import { api } from "../api/client.js";
import type { DataAsOf, OrgDto, ReportResponse } from "../api/client.js";

const USE_MOCK = import.meta.env.VITE_USE_MOCK_REPORTS === "1";

/** Staleness bucket. Drives the card colour + the "<n>m ago" label. */
type Band = "fresh" | "warning" | "stale" | "pending";

const HOUR_MS = 60 * 60 * 1000;

function bandOf(ageMs: number | null): Band {
  if (ageMs === null) return "pending";
  if (ageMs < HOUR_MS) return "fresh";
  if (ageMs < 4 * HOUR_MS) return "warning";
  return "stale";
}

const BAND_STYLE: Record<Band, {
  border: string;
  background: string;
  dot: string;
  label: string;
  textColor: string;
}> = {
  // green
  fresh: {
    border: "oklch(0.78 0.16 145)",
    background: "oklch(0.96 0.04 145)",
    dot: "oklch(0.7 0.18 145)",
    label: "Fresh",
    textColor: "oklch(0.35 0.12 145)",
  },
  // amber
  warning: {
    border: "oklch(0.82 0.14 80)",
    background: "oklch(0.97 0.05 80)",
    dot: "oklch(0.72 0.16 80)",
    label: "Lagging",
    textColor: "oklch(0.4 0.12 80)",
  },
  // red
  stale: {
    border: "oklch(0.78 0.18 25)",
    background: "oklch(0.96 0.05 25)",
    dot: "oklch(0.62 0.2 25)",
    label: "Stale",
    textColor: "oklch(0.4 0.16 25)",
  },
  // neutral
  pending: {
    border: "var(--border)",
    background: "var(--muted)",
    dot: "var(--muted-foreground)",
    label: "Pending",
    textColor: "var(--muted-foreground)",
  },
};

/** Format `ageMs` as a compact relative-time string. The cards are
 *  small, so we want "12m ago" / "3h ago" / "2d ago" — not full
 *  Intl.RelativeTimeFormat output. */
function formatRelative(ageMs: number | null): string {
  if (ageMs === null) return "pending";
  if (ageMs < 0) return "just now";
  const s = Math.floor(ageMs / 1000);
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.floor(h / 24);
  return `${d}d ago`;
}

function formatAbsolute(rfc3339: string): string {
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

interface OrgFreshness {
  org: OrgDto;
  /** RFC3339, or `null` for "absent from per_org". */
  ts: string | null;
  ageMs: number | null;
  band: Band;
}

function buildCards(
  orgs: ReadonlyArray<OrgDto>,
  perOrg: Record<string, string>,
  nowMs: number,
): OrgFreshness[] {
  return [...orgs]
    .sort((a, b) => a.login.localeCompare(b.login))
    .map((org) => {
      const ts = perOrg[org.id] ?? null;
      const ageMs = ts ? nowMs - new Date(ts).getTime() : null;
      return { org, ts, ageMs, band: bandOf(ageMs) };
    });
}

/** Pick the overall system-health band from the per-org cards.
 *  Worst-case wins: a single stale org turns the headline red. A
 *  "pending" org is reported alongside but doesn't tip the band —
 *  a fresh org with one pending neighbour is still "healthy". */
function overallBand(cards: ReadonlyArray<OrgFreshness>): Band {
  if (cards.some((c) => c.band === "stale")) return "stale";
  if (cards.some((c) => c.band === "warning")) return "warning";
  if (cards.some((c) => c.band === "fresh")) return "fresh";
  return "pending";
}

function overallHeadline(
  cards: ReadonlyArray<OrgFreshness>,
  data: DataAsOf,
): string {
  const reconciler = data.reconciler_latest
    ? `reconciler last ran ${formatRelative(Date.now() - new Date(data.reconciler_latest).getTime())}`
    : "no reconciler run recorded";
  const webhook = data.webhook_latest
    ? `webhook last seen ${formatRelative(Date.now() - new Date(data.webhook_latest).getTime())}`
    : "no webhook traffic recorded";
  const fresh = cards.filter((c) => c.band === "fresh").length;
  const warn = cards.filter((c) => c.band === "warning").length;
  const stale = cards.filter((c) => c.band === "stale").length;
  const pending = cards.filter((c) => c.band === "pending").length;
  const total = cards.length;
  if (total === 0) {
    return `No orgs tracked yet — ${reconciler}, ${webhook}.`;
  }
  const parts: string[] = [];
  if (fresh > 0) parts.push(`${fresh} fresh`);
  if (warn > 0) parts.push(`${warn} lagging`);
  if (stale > 0) parts.push(`${stale} stale`);
  if (pending > 0) parts.push(`${pending} pending`);
  return `${parts.join(", ")} of ${total} orgs · ${reconciler} · ${webhook}.`;
}

// ---------------------------------------------------------------------------
// Mock fixtures (stage-6 smoke harness, parity with the other report pages).
// ---------------------------------------------------------------------------

function mockOrgs(): OrgDto[] {
  return [
    { id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", github_id: 1, login: "acme-fresh", name: "Acme (fresh)" },
    { id: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", github_id: 2, login: "acme-lagging", name: "Acme (lagging)" },
    { id: "cccccccc-cccc-cccc-cccc-cccccccccccc", github_id: 3, login: "acme-stale", name: "Acme (stale)" },
    { id: "dddddddd-dddd-dddd-dddd-dddddddddddd", github_id: 4, login: "acme-pending", name: "Acme (pending)" },
  ];
}

function mockFreshness(): ReportResponse<null> {
  const now = Date.now();
  const min = 60 * 1000;
  return {
    rows: null,
    resolved_window: {
      start: new Date(now - 7 * 86_400_000).toISOString(),
      end: new Date(now).toISOString(),
      label: "last_7_days",
      tz: "UTC",
    },
    data_as_of: {
      headline: new Date(now - 8 * min).toISOString(),
      per_org: {
        // Seed one org in each band; the pending org is intentionally
        // omitted from per_org so absent ≠ stale is exercised.
        "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa": new Date(now - 8 * min).toISOString(),
        "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb": new Date(now - 2 * 60 * min).toISOString(),
        "cccccccc-cccc-cccc-cccc-cccccccccccc": new Date(now - 6 * 60 * min).toISOString(),
      },
      reconciler_latest: new Date(now - 8 * min).toISOString(),
      webhook_latest: new Date(now - 30 * 1000).toISOString(),
    },
  };
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export function FreshnessPage(): JSX.Element {
  const freshnessQuery = useQuery({
    queryKey: ["report-freshness"],
    queryFn: () => (USE_MOCK ? Promise.resolve(mockFreshness()) : api.getReportFreshness()),
    // Keep the dashboard live without being chatty — a 30s refresh
    // keeps the "<n>m ago" labels honest without hammering dp-rest.
    refetchInterval: 30_000,
  });

  const orgsQuery = useQuery({
    queryKey: ["orgs"],
    queryFn: () => (USE_MOCK ? Promise.resolve(mockOrgs()) : api.listOrgs()),
  });

  const data = freshnessQuery.data?.data_as_of;
  const orgs = orgsQuery.data ?? [];
  const loading = freshnessQuery.isPending || orgsQuery.isPending;
  const error =
    freshnessQuery.error?.message ?? orgsQuery.error?.message ?? null;

  // `Date.now()` is re-evaluated each render; the 30s refetchInterval
  // re-renders the page so the labels tick over even when the data
  // hasn't changed.
  const cards = useMemo(
    () => buildCards(orgs, data?.per_org ?? {}, Date.now()),
    [orgs, data],
  );

  const overall = data ? overallBand(cards) : "pending";
  const headline = data ? overallHeadline(cards, data) : "Loading freshness…";
  const overallStyle = BAND_STYLE[overall];

  return (
    <Card>
      <CardHeader>
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "flex-start",
            gap: "1rem",
            flexWrap: "wrap",
          }}
        >
          <div>
            <CardTitle>Data freshness</CardTitle>
            <CardDescription>
              <code>GET /reports/freshness</code> · per-org reconciler health, webhook lag.
            </CardDescription>
          </div>
        </div>
      </CardHeader>
      <CardContent style={{ display: "grid", gap: "1.25rem" }}>
        <div
          data-testid="freshness-headline"
          data-band={overall}
          role="status"
          aria-live="polite"
          style={{
            display: "flex",
            alignItems: "center",
            gap: "0.625rem",
            padding: "0.75rem 1rem",
            borderRadius: "var(--radius-md, 0.5rem)",
            border: `1px solid ${overallStyle.border}`,
            background: overallStyle.background,
            color: overallStyle.textColor,
            fontSize: "0.9375rem",
          }}
        >
          <span
            aria-hidden
            style={{
              width: "0.625rem",
              height: "0.625rem",
              borderRadius: "50%",
              background: overallStyle.dot,
              flexShrink: 0,
            }}
          />
          <span>
            <strong style={{ marginRight: "0.375rem" }}>{overallStyle.label}.</strong>
            {headline}
          </span>
        </div>

        {error ? (
          <p data-testid="freshness-error" style={{ color: "oklch(0.5 0.2 25)" }}>
            Failed to load freshness: {error}
          </p>
        ) : null}

        {loading && cards.length === 0 ? (
          <p style={{ color: "var(--muted-foreground)" }}>Loading orgs…</p>
        ) : cards.length === 0 ? (
          <p style={{ color: "var(--muted-foreground)" }}>
            No orgs tracked yet. Run a fetch or webhook to seed the first one.
          </p>
        ) : (
          <div
            data-testid="freshness-grid"
            style={{
              display: "grid",
              gap: "0.875rem",
              gridTemplateColumns: "repeat(auto-fill, minmax(16rem, 1fr))",
            }}
          >
            {cards.map((card) => (
              <OrgFreshnessCard key={card.org.id} card={card} />
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function OrgFreshnessCard({ card }: { card: OrgFreshness }): JSX.Element {
  const style = BAND_STYLE[card.band];
  return (
    <div
      data-testid="freshness-card"
      data-org-id={card.org.id}
      data-band={card.band}
      style={{
        display: "grid",
        gap: "0.5rem",
        padding: "0.875rem 1rem",
        borderRadius: "var(--radius-md, 0.5rem)",
        border: `1px solid ${style.border}`,
        background: style.background,
        color: style.textColor,
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: "0.5rem",
        }}
      >
        <strong
          style={{
            fontSize: "0.9375rem",
            color: "var(--foreground)",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
          title={card.org.login}
        >
          {card.org.name ?? card.org.login}
        </strong>
        <span
          aria-label={`${style.label} (${card.band})`}
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: "0.3125rem",
            padding: "0.125rem 0.5rem",
            borderRadius: "999px",
            background: "color-mix(in oklch, var(--background) 60%, transparent)",
            fontSize: "0.6875rem",
            textTransform: "uppercase",
            letterSpacing: "0.04em",
            color: style.textColor,
          }}
        >
          <span
            aria-hidden
            style={{
              width: "0.5rem",
              height: "0.5rem",
              borderRadius: "50%",
              background: style.dot,
            }}
          />
          {style.label}
        </span>
      </div>
      {card.org.name ? (
        <code
          style={{
            fontSize: "0.75rem",
            color: "var(--muted-foreground)",
          }}
        >
          {card.org.login}
        </code>
      ) : null}
      <div
        style={{
          display: "grid",
          gap: "0.125rem",
          fontVariantNumeric: "tabular-nums",
        }}
      >
        <span
          style={{
            fontSize: "1.125rem",
            fontWeight: 600,
            color: "var(--foreground)",
          }}
        >
          {card.ts === null ? "—" : `last updated ${formatRelative(card.ageMs)}`}
        </span>
        {card.ts ? (
          <span
            title={card.ts}
            style={{
              fontSize: "0.75rem",
              color: "var(--muted-foreground)",
            }}
          >
            {formatAbsolute(card.ts)}
          </span>
        ) : (
          <span style={{ fontSize: "0.75rem", color: "var(--muted-foreground)" }}>
            pending first reconciler run
          </span>
        )}
      </div>
    </div>
  );
}

// Re-export the pure helpers so a smoke test / Storybook harness
// can drive the band buckets without rendering the page.
export const __test__ = {
  bandOf,
  buildCards,
  overallBand,
  formatRelative,
  HOUR_MS,
};
