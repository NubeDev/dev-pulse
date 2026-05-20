/**
 * Shared report layout — composes the shadcn dashboard-01 block
 * primitives in the order the brief calls out:
 *
 *   1. PageHeading lockup
 *   2. Filter Card (entity selectors + WindowPicker)
 *   3. Data-as-of Alert
 *   4. SectionCards (KPI tiles built from the per-kind totals)
 *   5. ChartAreaInteractive (summed per-bucket series)
 *   6. DataTable with three-lens Tabs in its toolbar
 *
 * Every direct child wraps itself in `px-4 lg:px-6` to line up with
 * the block's gutter — `SectionCards` and `DataTable` carry it
 * internally; the heading, filter Card, banner, and chart are wrapped
 * here so the columns stay flush across the whole page.
 *
 * Each report page builds its `kpis`, `chart`, and `table` from its
 * own data + selector wiring; this shell handles the layout +
 * empty-state prompt + lens toggle.
 */

import { useMemo, type ReactNode } from "react";
import type { ColumnDef } from "@tanstack/react-table";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import type { ChartConfig } from "@/components/ui/chart";
import {
  ChartAreaInteractive,
} from "@/components/chart-area-interactive";
import {
  DataTable,
  type DataTableTab,
} from "@/components/data-table";
import { SectionCards, type SectionCard } from "@/components/section-cards";

import type { CountRow, DataAsOf, ScopeMode } from "../api/client.js";
import { PageHeading } from "../components/page-heading.jsx";
import { DataAsOfBanner } from "./data-as-of.jsx";
import { LENSES } from "./lens-tabs.jsx";
import { ACTIVITY_KINDS } from "./activity-types.js";

/** Per-kind series payload: rows are date-keyed `CountRow`s; the
 *  shell combines them into a single date-bucketed chart series and
 *  derives KPI deltas. */
export interface PerKind {
  rows: ReadonlyArray<CountRow>
  loading: boolean
}

export interface ReportShellProps {
  /** Page heading title. */
  title: string
  /** Optional muted description rendered under the heading. */
  description?: ReactNode
  /** Filter card body — typically `<EntitySelect> + <WindowPicker />`
   *  laid out in the shared `FILTER_GRID_CLASS` grid. */
  filters: ReactNode
  /** True until the entity selector has resolved a target (no entity
   *  → show the empty-state prompt instead of the data panes). */
  ready: boolean
  /** Prompt to show in place of the data panes when `!ready`. */
  emptyPrompt: ReactNode
  /** Per-kind query envelope keyed by `ACTIVITY_KINDS[i].key`. */
  perKind: ReadonlyMap<string, PerKind>
  /** Latest `data_as_of` envelope from any settled query. */
  dataAsOf: DataAsOf | null
  /** Loading flag for the data-as-of banner. */
  dataAsOfLoading: boolean
  /** Stable test id for the wrapping element (default "report-shell"). */
  testId?: string
  /** Three-lens toggle state — required when `tabs` is undefined the
   *  default `LENSES` set is used. */
  lens: ScopeMode
  onLensChange: (next: ScopeMode) => void
  /** Subject noun for the headline (e.g. user login, team name). */
  subjectLabel: string | null
}

interface ActivityRow {
  kind: string
  label: string
  total: number
  trend: ReadonlyArray<CountRow>
  loading: boolean
}

function buildRows(
  perKind: ReadonlyMap<string, PerKind>,
): ActivityRow[] {
  return ACTIVITY_KINDS.map((k) => {
    const entry = perKind.get(k.key)
    const rows = entry?.rows ?? []
    const sorted = [...rows].sort((a, b) => a.key.localeCompare(b.key))
    const total = rows.reduce((acc, r) => acc + r.count, 0)
    return {
      kind: k.key,
      label: k.label,
      total,
      trend: sorted,
      loading: entry?.loading ?? false,
    }
  })
}

/** Build the SectionCards: total events + top 3 activity types. */
function buildKpis(rows: ReadonlyArray<ActivityRow>): SectionCard[] {
  const grandTotal = rows.reduce((acc, r) => acc + r.total, 0)
  const grandDelta = computeDelta(
    rows.flatMap((r) => r.trend.map((t) => t.count)),
    rows[0]?.trend.length ?? 0,
  )
  const top = [...rows]
    .filter((r) => r.total > 0)
    .sort((a, b) => b.total - a.total)
    .slice(0, 3)

  const cards: SectionCard[] = [
    {
      description: "Total events",
      value: grandTotal.toLocaleString(),
      delta: grandDelta,
      footerTitle: deltaFooter(grandDelta),
      footerDescription: "Across all tracked activity kinds.",
      testId: "kpi-total",
    },
  ]
  for (const row of top) {
    const delta = computeDelta(row.trend.map((t) => t.count), row.trend.length)
    cards.push({
      description: row.label,
      value: row.total.toLocaleString(),
      delta,
      footerTitle: deltaFooter(delta),
      footerDescription: `${row.trend.length}-bucket trend`,
      testId: `kpi-${row.kind}`,
    })
  }
  // Pad to four tiles when fewer than 3 active kinds.
  while (cards.length < 4) {
    cards.push({
      description: "—",
      value: "0",
      footerDescription: "No activity in this slot.",
      testId: `kpi-empty-${cards.length}`,
    })
  }
  return cards
}

/** Compare the last-half sum to the first-half sum; emit a signed
 *  percentage. Returns undefined if the series has < 2 buckets or
 *  the baseline is zero. */
function computeDelta(values: number[], _buckets: number): string | undefined {
  if (values.length < 2) return undefined
  const mid = Math.floor(values.length / 2)
  const a = values.slice(0, mid).reduce((s, v) => s + v, 0)
  const b = values.slice(mid).reduce((s, v) => s + v, 0)
  if (a === 0 && b === 0) return undefined
  if (a === 0) return "+100%"
  const pct = ((b - a) / a) * 100
  const sign = pct >= 0 ? "+" : ""
  return `${sign}${pct.toFixed(1)}%`
}

function deltaFooter(delta: string | undefined): ReactNode {
  if (!delta) return "Flat across window"
  return delta.startsWith("-") ? `Down ${delta.slice(1)} vs prior half` : `Up ${delta.slice(1) || delta} vs prior half`
}

/** Build the chart data: sum all per-kind counts per bucket key. */
function buildChartSeries(rows: ReadonlyArray<ActivityRow>): Array<{ date: string; events: number }> {
  const totals = new Map<string, number>()
  for (const r of rows) {
    for (const t of r.trend) {
      totals.set(t.key, (totals.get(t.key) ?? 0) + t.count)
    }
  }
  return [...totals.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([date, events]) => ({ date, events }))
}

const CHART_CONFIG: ChartConfig = {
  events: { label: "Events", color: "var(--accent-info)" },
}

const LENS_TABS: DataTableTab[] = LENSES.map((l) => ({
  value: l.value,
  label: l.label,
}))

// Cell components factored out so they can carry stable display
// names — keeps Tabler / lint quiet in the column defs below.
function TotalCell(value: number): ReactNode {
  return <span className="font-medium tabular-nums">{value.toLocaleString()}</span>
}

import { Skeleton } from "@/components/ui/skeleton"
import { Sparkline } from "./trend-sparkline.jsx"

const COLUMNS: ColumnDef<ActivityRow, unknown>[] = [
  {
    accessorKey: "label",
    header: "Activity",
    cell: ({ row }) => <span className="font-medium">{row.original.label}</span>,
  },
  {
    accessorKey: "total",
    header: () => <div className="text-right">Total</div>,
    cell: ({ row }) => (
      <div className="text-right">
        {row.original.loading ? (
          <Skeleton className="ml-auto h-4 w-10" />
        ) : (
          TotalCell(row.original.total)
        )}
      </div>
    ),
  },
  {
    id: "trend",
    header: () => <div className="text-right">Trend</div>,
    cell: ({ row }) => (
      <div className="flex justify-end">
        {row.original.loading ? (
          <Skeleton className="h-8 w-24" />
        ) : (
          <span className="inline-flex h-8 w-24 items-center justify-end">
            <Sparkline
              points={row.original.trend.map((t) => ({ key: t.key, value: t.count }))}
              width={96}
              height={32}
              ariaLabel={`${row.original.label} trend, ${row.original.trend.length} buckets, total ${row.original.total}`}
            />
          </span>
        )}
      </div>
    ),
  },
]

export function ReportShell({
  title,
  description,
  filters,
  ready,
  emptyPrompt,
  perKind,
  dataAsOf,
  dataAsOfLoading,
  testId = "report-shell",
  lens,
  onLensChange,
  subjectLabel,
}: ReportShellProps): JSX.Element {
  const rows = useMemo(() => buildRows(perKind), [perKind])
  const kpis = useMemo(() => buildKpis(rows), [rows])
  const chartSeries = useMemo(() => buildChartSeries(rows), [rows])

  const headline = useMemo(() => {
    if (!subjectLabel) return ""
    const top = [...rows]
      .filter((r) => r.total > 0)
      .sort((a, b) => b.total - a.total)
      .slice(0, 3)
    if (top.length === 0) return `${subjectLabel} had no recorded activity in the selected window.`
    const lensLabel = LENSES.find((l) => l.value === lens)?.label ?? ""
    const parts = top.map((r) => `${r.total.toLocaleString()} ${r.label.toLowerCase()}`)
    const joined = parts.length === 1
      ? parts[0]
      : `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`
    return `${subjectLabel} recorded ${joined} (${lensLabel}).`
  }, [rows, subjectLabel, lens])

  return (
    <div data-testid={testId} className="flex flex-col gap-4 md:gap-6">
      <div className="px-4 lg:px-6">
        <PageHeading title={title} description={description} />
      </div>

      <div className="px-4 lg:px-6">
        <Card>
          <CardHeader>
            <CardTitle className="text-base font-medium">Filters</CardTitle>
          </CardHeader>
          <CardContent>{filters}</CardContent>
        </Card>
      </div>

      <div className="px-4 lg:px-6">
        <DataAsOfBanner data={dataAsOf} loading={dataAsOfLoading} />
      </div>

      {!ready ? (
        <div className="px-4 lg:px-6">
          <Card>
            <CardContent className="pt-6 text-sm text-muted-foreground">
              {emptyPrompt}
            </CardContent>
          </Card>
        </div>
      ) : (
        <>
          <SectionCards cards={kpis} />

          <div className="px-4 lg:px-6">
            <ChartAreaInteractive
              testId="report-chart"
              title="Activity over time"
              description={subjectLabel ? `Events per bucket · ${subjectLabel}` : "Events per bucket"}
              data={chartSeries}
              config={CHART_CONFIG}
            />
          </div>

          <DataTable
            testId="activity-table"
            data={rows}
            columns={COLUMNS}
            tabs={LENS_TABS}
            activeTab={lens}
            onTabChange={(v) => onLensChange(v as ScopeMode)}
            getRowId={(r) => r.kind}
            toolbar={
              <span data-testid="headline" className="hidden text-sm text-muted-foreground md:inline">
                {headline}
              </span>
            }
            emptyMessage="No activity in this window."
          />
        </>
      )}
    </div>
  )
}
