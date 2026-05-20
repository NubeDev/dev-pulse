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
import { Alert, AlertDescription } from "@nube/starter-ui-kit/components/alert";
import { Badge } from "@nube/starter-ui-kit/components/badge";
import { cn } from "@nube/starter-ui-kit/lib/utils";

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

/** Band → className groups. Each band has a card/banner surface
 *  class, a dot class, and a human label. The colour cues stay the
 *  same semantic family (green/amber/red/neutral), just expressed via
 *  Tailwind utilities so the page picks up dark-mode tokens. */
const BAND_CLASSES: Record<
  Band,
  { surface: string; dot: string; label: string }
> = {
  fresh: {
    surface:
      "border-emerald-500/40 bg-emerald-50 text-emerald-900 dark:bg-emerald-950/30 dark:text-emerald-100",
    dot: "bg-emerald-500",
    label: "Fresh",
  },
  warning: {
    surface:
      "border-amber-500/40 bg-amber-50 text-amber-900 dark:bg-amber-950/30 dark:text-amber-100",
    dot: "bg-amber-500",
    label: "Lagging",
  },
  stale: {
    surface:
      "border-red-500/40 bg-red-50 text-red-900 dark:bg-red-950/30 dark:text-red-100",
    dot: "bg-red-500",
    label: "Stale",
  },
  pending: {
    surface: "border-border bg-muted text-muted-foreground",
    dot: "bg-muted-foreground",
    label: "Pending",
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
  const overallBand_ = BAND_CLASSES[overall];

  return (
    <Card>
      <CardHeader>
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="grid gap-1">
            <CardTitle className="text-2xl font-semibold tracking-tight">
              Data freshness
            </CardTitle>
            <CardDescription className="text-muted-foreground">
              <code>GET /reports/freshness</code> · per-org reconciler health, webhook lag.
            </CardDescription>
          </div>
        </div>
      </CardHeader>
      <CardContent className="grid gap-5">
        <Alert
          data-testid="freshness-headline"
          data-band={overall}
          aria-live="polite"
          className={cn(overallBand_.surface)}
        >
          <AlertDescription className="flex items-center gap-2 text-current">
            <span
              aria-hidden
              className={cn(
                "inline-block size-2.5 shrink-0 rounded-full",
                overallBand_.dot,
              )}
            />
            <span>
              <strong className="mr-1.5">{overallBand_.label}.</strong>
              {headline}
            </span>
          </AlertDescription>
        </Alert>

        {error ? (
          <Alert variant="destructive" data-testid="freshness-error">
            <AlertDescription>Failed to load freshness: {error}</AlertDescription>
          </Alert>
        ) : null}

        {loading && cards.length === 0 ? (
          <p className="text-muted-foreground">Loading orgs…</p>
        ) : cards.length === 0 ? (
          <p className="text-muted-foreground">
            No orgs tracked yet. Run a fetch or webhook to seed the first one.
          </p>
        ) : (
          <div
            data-testid="freshness-grid"
            className="grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-3.5"
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
  const band = BAND_CLASSES[card.band];
  return (
    <div
      data-testid="freshness-card"
      data-org-id={card.org.id}
      data-band={card.band}
      className={cn(
        "grid gap-2 rounded-md border p-3.5",
        band.surface,
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <strong
          className="truncate text-sm font-semibold"
          title={card.org.login}
        >
          {card.org.name ?? card.org.login}
        </strong>
        <Badge
          variant="outline"
          aria-label={`${band.label} (${card.band})`}
          className="gap-1.5 border-current/30 bg-background/60 text-[0.6875rem] uppercase tracking-wider text-current"
        >
          <span
            aria-hidden
            className={cn("size-2 rounded-full", band.dot)}
          />
          {band.label}
        </Badge>
      </div>
      {card.org.name ? (
        <code className="text-xs text-muted-foreground">{card.org.login}</code>
      ) : null}
      <div className="grid gap-0.5 tabular-nums">
        <span className="text-lg font-semibold">
          {card.ts === null ? "—" : `last updated ${formatRelative(card.ageMs)}`}
        </span>
        {card.ts ? (
          <span title={card.ts} className="text-xs text-muted-foreground">
            {formatAbsolute(card.ts)}
          </span>
        ) : (
          <span className="text-xs text-muted-foreground">
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
