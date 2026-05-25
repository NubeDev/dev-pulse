/**
 * Block-derived `SiteHeader` — verbatim shadcn dashboard-01 component
 * with the canned GitHub button dropped and the title made dynamic
 * via props. Trigger / separator / h1 layout is preserved.
 *
 * Phase 7 visual polish: subtle slide-in on mount and a 1px
 * leaf→aqua hairline under the header so the brand identity reads
 * even at minimal chrome.
 */

import type { ReactNode } from "react"
import { motion } from "motion/react"

import { Separator } from "@/components/ui/separator"
import { SidebarTrigger } from "@/components/ui/sidebar"

export function SiteHeader({
  title,
  actions,
}: {
  title: string
  actions?: ReactNode
}) {
  return (
    <motion.header
      initial={{ y: -12, opacity: 0 }}
      animate={{ y: 0, opacity: 1 }}
      transition={{ duration: 0.45, ease: [0.22, 1, 0.36, 1] }}
      className="relative flex h-(--header-height) shrink-0 items-center gap-2 border-b bg-background/80 backdrop-blur-md transition-[width,height] ease-linear group-has-data-[collapsible=icon]/sidebar-wrapper:h-(--header-height)"
    >
      <div className="flex w-full items-center gap-1 px-4 lg:gap-2 lg:px-6">
        <SidebarTrigger className="-ml-1" />
        <Separator
          orientation="vertical"
          className="mx-2 data-[orientation=vertical]:h-4"
        />
        <h1 className="text-base font-medium tracking-tight">{title}</h1>
        {actions ? (
          <div className="ml-auto flex items-center gap-2">{actions}</div>
        ) : null}
      </div>
      {/* Brand-tinted hairline — leaf → aqua → transparent. Sits on top
       * of the border-b so the header reads as part of the brand surface. */}
      <span
        aria-hidden
        className="pointer-events-none absolute inset-x-0 bottom-0 h-px"
        style={{
          background:
            "linear-gradient(90deg, color-mix(in srgb, var(--brand-leaf) 70%, transparent) 0%, color-mix(in srgb, var(--brand-aqua) 55%, transparent) 35%, transparent 80%)",
        }}
      />
    </motion.header>
  )
}
