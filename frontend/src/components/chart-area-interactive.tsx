/**
 * Block-derived `ChartAreaInteractive` — verbatim shadcn
 * dashboard-01 component with the canned 90-day visitor dataset and
 * built-in date-range picker dropped. dev-pulse owns its own window
 * picker (filters Card upstream), so this component just renders the
 * stacked area chart over whatever `data` the page hands it.
 *
 * The Card / ChartContainer / AreaChart / gradient-fill rhythm is
 * preserved exactly so the chart visually reads as the block.
 */

import { Area, AreaChart, CartesianGrid, XAxis } from "recharts"

import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import {
  ChartContainer,
  ChartLegend,
  ChartLegendContent,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "@/components/ui/chart"

export interface ChartAreaInteractiveProps {
  /** Title shown in the card header (e.g. "Activity"). */
  title: string
  /** Muted subtitle line under the title. */
  description?: string
  /**
   * Row data keyed by date (ISO string) plus one numeric column per
   * series in `config`. The XAxis formats `date` as "Apr 12".
   */
  data: Array<{ date: string } & Record<string, number | string>>
  /** Recharts chart config: `{ <seriesKey>: { label, color } }`. */
  config: ChartConfig
  /** Stable test id (e.g. "user-activity-chart"). */
  testId?: string
}

export function ChartAreaInteractive({
  title,
  description,
  data,
  config,
  testId,
}: ChartAreaInteractiveProps) {
  const seriesKeys = Object.keys(config).filter((k) => k !== "visitors")
  return (
    <Card data-testid={testId} className="@container/card">
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        {description ? <CardDescription>{description}</CardDescription> : null}
      </CardHeader>
      <CardContent className="px-2 pt-4 sm:px-6 sm:pt-6">
        <ChartContainer
          config={config}
          className="aspect-auto h-[250px] w-full"
        >
          <AreaChart data={data}>
            <defs>
              {seriesKeys.map((key) => (
                <linearGradient
                  key={key}
                  id={`fill-${key}`}
                  x1="0"
                  y1="0"
                  x2="0"
                  y2="1"
                >
                  <stop
                    offset="5%"
                    stopColor={`var(--color-${key})`}
                    stopOpacity={0.8}
                  />
                  <stop
                    offset="95%"
                    stopColor={`var(--color-${key})`}
                    stopOpacity={0.1}
                  />
                </linearGradient>
              ))}
            </defs>
            <CartesianGrid vertical={false} />
            <XAxis
              dataKey="date"
              tickLine={false}
              axisLine={false}
              tickMargin={8}
              minTickGap={32}
              tickFormatter={(value: string) => {
                const date = new Date(value)
                return date.toLocaleDateString("en-US", {
                  month: "short",
                  day: "numeric",
                })
              }}
            />
            <ChartTooltip
              cursor={false}
              content={
                <ChartTooltipContent
                  labelFormatter={(value) => {
                    return new Date(value as string).toLocaleDateString("en-US", {
                      month: "short",
                      day: "numeric",
                      year: "numeric",
                    })
                  }}
                  indicator="dot"
                />
              }
            />
            {seriesKeys.map((key) => (
              <Area
                key={key}
                dataKey={key}
                type="natural"
                fill={`url(#fill-${key})`}
                stroke={`var(--color-${key})`}
                stackId="a"
              />
            ))}
            {seriesKeys.length > 1 ? (
              <ChartLegend content={<ChartLegendContent />} />
            ) : null}
          </AreaChart>
        </ChartContainer>
      </CardContent>
    </Card>
  )
}
