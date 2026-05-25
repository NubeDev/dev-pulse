/**
 * Block-derived `AppSidebar` — verbatim shadcn dashboard-01 component
 * with the canned data + secondary nav dropped. Sections + user are
 * passed in by the app shell, so the component is a pure layout
 * primitive. The brand row keeps the block's small icon + bold title
 * pattern.
 */

import * as React from "react"
import { IconActivity } from "@tabler/icons-react"

import { NavMain, type NavMainItem } from "@/components/nav-main"
import { NavUser, type NavUserProps } from "@/components/nav-user"
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarRail,
} from "@/components/ui/sidebar"

export interface AppSidebarProps extends React.ComponentProps<typeof Sidebar> {
  navMain: NavMainItem[]
  activeUrl?: string
  user: NavUserProps["user"]
  onLogout?: () => void
  brand?: { title: string; url: string; tagline?: string }
  /** Extra content rendered below the main nav (e.g. the
   *  SCOPE-PROJECTS §6 pin sidebar widget). */
  extraContent?: React.ReactNode
}

export function AppSidebar({
  navMain,
  activeUrl,
  user,
  onLogout,
  brand = { title: "dev-pulse", url: "#/reports", tagline: "operator UI" },
  extraContent,
  ...props
}: AppSidebarProps) {
  return (
    <Sidebar collapsible="icon" variant="floating" {...props}>
      <SidebarHeader>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              asChild
              className="data-[slot=sidebar-menu-button]:p-1.5! h-auto"
            >
              <a href={brand.url} className="flex items-center gap-2.5">
                {/* Leaf-colored tile with sparkle, matching the rubix brand
                 * mark. Uses --brand-leaf so it tracks the active palette. */}
                <span
                  aria-hidden
                  className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-[var(--brand-leaf)] text-[var(--primary-foreground)] shadow-sm"
                >
                  <IconActivity className="size-4" strokeWidth={2.25} />
                </span>
                <span className="flex flex-col leading-tight">
                  <span className="text-sm font-semibold tracking-tight">
                    {brand.title}
                  </span>
                  {brand.tagline ? (
                    <span className="text-[10px] uppercase tracking-[0.18em] text-[var(--subtle)]">
                      {brand.tagline}
                    </span>
                  ) : null}
                </span>
              </a>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>
      <SidebarContent>
        <NavMain items={navMain} activeUrl={activeUrl} />
        {extraContent}
      </SidebarContent>
      <SidebarFooter>
        <NavUser user={user} onLogout={onLogout} />
      </SidebarFooter>
      <SidebarRail />
    </Sidebar>
  )
}
