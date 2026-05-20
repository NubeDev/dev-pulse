/**
 * Freshness dashboard — `GET /reports/freshness`.
 *
 * Single-view status page (no lens toggle, no window filter): a
 * system-health headline Alert with a coloured staleness Badge, then
 * a card grid (one card per org) with "last updated <relative
 * time>", colour-coded by staleness.
 *
 * Per the shared report-page skeleton (stage 3): PageHeading lockup
 * at the top, then a results Card holding the alert and the grid.
 *
 * Org names come from `GET /orgs`; we join client-side. An org that
 * appears in `/orgs` but is absent from `per_org` renders as
 * "pending first reconciler run" (SCOPE: absent ≠ stale).
 */

import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Card,
  CardContent,
} from "@/components/ui/card";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

import { api } from "../api/client.js";
import type { DataAsOf, OrgDto, ReportResponse } from "../api/client.js";
import { PageHeading } from "../components/page-heading.jsx";
import { Skeleton } from "../components/skeleton.jsx";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from "../components/empty.jsx";

const USE_MOCK = import.meta.env.VITE_USE_MOCK_REPORTS === "1";

type Band = "fresh" | "warning" | "stale" | "pending";

const HOUR_MS = 60 * 60 * 1000;

function bandOf(ageMs: number | null): Band {
  if (ageMs === null) return "pending";
  if (ageMs < HOUR_MS) return "fresh";
  if (ageMs < 4 * HOUR_MS) return "warning";
  return "stale";
}

const BAND_CLASSES: Record<
  Band,
  { surface: string; badge: string; dot: string; label: string }
> = {
  fresh: {
    surface:
      "border-emerald-500/40 bg-emerald-50/60 text-emerald-900 dark:bg-emerald-950/30 dark:text-emerald-100",
    badge:
      "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
    dot: "bg-emerald-500",
    label: "Fresh",
  },
  warning: {
    surface:
      "border-amber-500/40 bg-amber-50/60 text-amber-900 dark:bg-amber-950/30 dark:text-amber-100",
    badge:
      "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300",
    dot: "bg-amber-500",
    label: "Lagging",
  },
  stale: {
    surface:
      "border-red-500/40 bg-red-50/60 text-red-900 dark:bg-red-950/30 dark:text-red-100",
    badge:
      "border-red-500/30 bg-red-500/10 text-red-700 dark:text-red-300",
    dot: "bg-red-500",
    label: "Stale",
  },
  pending: {
    surface: "border-border bg-muted/40 text-muted-foreground",
    badge: "border-border bg-muted text-muted-foreground",
    dot: "bg-muted-foreground",
    label: "Pending",
  },
};

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
        "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa": new Date(now - 8 * min).toISOString(),
        "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb": new Date(now - 2 * 60 * min).toISOString(),
        "cccccccc-cccc-cccc-cccc-cccccccccccc": new Date(now - 6 * 60 * min).toISOString(),
      },
      reconciler_latest: new Date(now - 8 * min).toISOString(),
      webhook_latest: new Date(now - 30 * 1000).toISOString(),
    },
  };
}

export function FreshnessPage(): JSX.Element {
  const freshnessQuery = useQuery({
    queryKey: ["report-freshness"],
    queryFn: () => (USE_MOCK ? Promise.resolve(mockFreshness()) : api.getReportFreshness()),
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

  const cards = useMemo(
    () => buildCards(orgs, data?.per_org ?? {}, Date.now()),
    [orgs, data],
  );

  const overall = data ? overallBand(cards) : "pending";
  const headline = data ? overallHeadline(cards, data) : "Loading freshness…";
  const overallBand_ = BAND_CLASSES[overall];

  return (
    <div className="grid gap-6">
      <PageHeading
        title="Data freshness"
        description={
          <>
            <code className="font-mono text-xs">GET /reports/freshness</code> ·
            per-org reconciler health, webhook lag.
          </>
        }
      />

      <Alert
        data-testid="freshness-headline"
        data-band={overall}
        aria-live="polite"
        className={cn("flex items-center gap-3", overallBand_.surface)}
      >
        <Badge
          variant="outline"
          className={cn(
            "gap-1.5 text-[0.6875rem] uppercase tracking-wider",
            overallBand_.badge,
          )}
        >
          <span aria-hidden className={cn("size-1.5 rounded-full", overallBand_.dot)} />
          {overallBand_.label}
        </Badge>
        <AlertDescription className="flex-1 text-current">
          {headline}
        </AlertDescription>
      </Alert>

      {error ? (
        <Alert variant="destructive" data-testid="freshness-error">
          <AlertDescription>Failed to load freshness: {error}</AlertDescription>
        </Alert>
      ) : null}

      <Card>
        <CardContent className="pt-6">
          {loading && cards.length === 0 ? (
            <div className="grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-3">
              <Skeleton className="h-24 w-full" />
              <Skeleton className="h-24 w-full" />
              <Skeleton className="h-24 w-full" />
            </div>
          ) : cards.length === 0 ? (
            <Empty>
              <EmptyHeader>
                <EmptyTitle>No orgs tracked yet</EmptyTitle>
                <EmptyDescription>
                  Run a fetch or webhook to seed the first one.
                </EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : (
            <div
              data-testid="freshness-grid"
              className="grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-3"
            >
              {cards.map((card) => (
                <OrgFreshnessCard key={card.org.id} card={card} />
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

function OrgFreshnessCard({ card }: { card: OrgFreshness }): JSX.Element {
  const band = BAND_CLASSES[card.band];
  return (
    <div
      data-testid="freshness-card"
      data-org-id={card.org.id}
      data-band={card.band}
      className={cn("grid gap-2 rounded-xl border p-4", band.surface)}
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
          className={cn(
            "gap-1.5 text-[0.6875rem] uppercase tracking-wider",
            band.badge,
          )}
        >
          <span aria-hidden className={cn("size-1.5 rounded-full", band.dot)} />
          {band.label}
        </Badge>
      </div>
      {card.org.name ? (
        <code className="text-xs text-muted-foreground">{card.org.login}</code>
      ) : null}
      <div className="grid gap-0.5 tabular-nums">
        <span className="text-base font-semibold">
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

export const __test__ = {
  bandOf,
  buildCards,
  overallBand,
  formatRelative,
  HOUR_MS,
};
