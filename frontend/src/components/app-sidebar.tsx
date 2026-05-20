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
} from "@/components/ui/sidebar"

export interface AppSidebarProps extends React.ComponentProps<typeof Sidebar> {
  navMain: NavMainItem[]
  activeUrl?: string
  user: NavUserProps["user"]
  onLogout?: () => void
  brand?: { title: string; url: string }
}

export function AppSidebar({
  navMain,
  activeUrl,
  user,
  onLogout,
  brand = { title: "dev-pulse", url: "#/reports" },
  ...props
}: AppSidebarProps) {
  return (
    <Sidebar collapsible="offcanvas" {...props}>
      <SidebarHeader>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              asChild
              className="data-[slot=sidebar-menu-button]:p-1.5!"
            >
              <a href={brand.url}>
                <IconActivity className="size-5!" />
                <span className="text-base font-semibold">{brand.title}</span>
              </a>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>
      <SidebarContent>
        <NavMain items={navMain} activeUrl={activeUrl} />
      </SidebarContent>
      <SidebarFooter>
        <NavUser user={user} onLogout={onLogout} />
      </SidebarFooter>
    </Sidebar>
  )
}
