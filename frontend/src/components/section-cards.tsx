/**
 * Block-derived `SectionCards` — verbatim shadcn dashboard-01 KPI
 * tile grid with the canned demo cards (Revenue, Customers, …)
 * replaced by a single `cards` prop. The Card/Badge/Footer shape
 * (gradient surface, large tabular-nums title, trending delta badge)
 * is preserved exactly so the visual rhythm matches the block.
 */

import type { ReactNode } from "react"
import { IconTrendingDown, IconTrendingUp } from "@tabler/icons-react"

import { Badge } from "@/components/ui/badge"
import {
  Card,
  CardAction,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"

export interface SectionCard {
  description: string
  value: string
  delta?: string
  footerTitle?: ReactNode
  footerDescription?: ReactNode
  testId?: string
}

function isNegative(delta?: string): boolean {
  if (!delta) return false
  return delta.trim().startsWith("-")
}

export function SectionCards({ cards }: { cards: SectionCard[] }) {
  return (
    <div
      data-testid="section-cards"
      className="*:data-[slot=card]:from-primary/5 *:data-[slot=card]:to-card dark:*:data-[slot=card]:bg-card grid grid-cols-1 gap-4 px-4 *:data-[slot=card]:bg-gradient-to-t *:data-[slot=card]:shadow-xs @xl/main:grid-cols-2 @5xl/main:grid-cols-4 lg:px-6"
    >
      {cards.map((card) => {
        const Trend = isNegative(card.delta) ? IconTrendingDown : IconTrendingUp
        return (
          <Card
            key={card.testId ?? card.description}
            data-testid={card.testId}
            className="@container/card"
          >
            <CardHeader>
              <CardDescription>{card.description}</CardDescription>
              <CardTitle className="text-2xl font-semibold tabular-nums @[250px]/card:text-3xl">
                {card.value}
              </CardTitle>
              {card.delta ? (
                <CardAction>
                  <Badge variant="outline">
                    <Trend />
                    {card.delta}
                  </Badge>
                </CardAction>
              ) : null}
            </CardHeader>
            {card.footerTitle || card.footerDescription ? (
              <CardFooter className="flex-col items-start gap-1.5 text-sm">
                {card.footerTitle ? (
                  <div className="line-clamp-1 flex gap-2 font-medium">
                    {card.footerTitle}
                    <Trend className="size-4" />
                  </div>
                ) : null}
                {card.footerDescription ? (
                  <div className="text-muted-foreground">
                    {card.footerDescription}
                  </div>
                ) : null}
              </CardFooter>
            ) : null}
          </Card>
        )
      })}
    </div>
  )
}
