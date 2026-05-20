/**
 * Block-derived `NavMain` — verbatim shadcn dashboard-01 component
 * with the canned "Quick Create" affordance dropped and support for
 * a single level of sub-items added (matches the dev-pulse routes:
 * each section has a small set of children). Active state is driven
 * by `activeUrl` so the hash router stays the source of truth.
 */

import type * as React from "react"
import { type Icon } from "@tabler/icons-react"

import {
  SidebarGroup,
  SidebarGroupContent,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
} from "@/components/ui/sidebar"

export interface NavMainSubItem {
  title: string
  url: string
  icon?: Icon
  /** Optional small badge rendered at the right edge of the sub-button.
   *  Used to surface live counts (e.g. the inbox unread / queue size
   *  on `#/workflow/issues` — `linear-projects-idea.md` §3.8). */
  badge?: React.ReactNode
}

export interface NavMainItem {
  title: string
  url: string
  icon?: Icon
  /** CSS color value for the section icon (e.g. `var(--accent-reports)`). */
  accent?: string
  items?: NavMainSubItem[]
  /** Optional test id stamped on the SidebarMenuSub wrapper (used
   *  by the Playwright smoke suite to pin section sub-navs). */
  subTestId?: string
}

export function NavMain({
  items,
  activeUrl,
}: {
  items: NavMainItem[]
  activeUrl?: string
}) {
  return (
    <SidebarGroup>
      <SidebarGroupContent className="flex flex-col gap-2">
        <SidebarMenu>
          {items.map((item) => {
            const isSectionActive =
              !!activeUrl &&
              (activeUrl === item.url || activeUrl.startsWith(`${item.url}/`))
            return (
              <SidebarMenuItem key={item.title}>
                <SidebarMenuButton
                  asChild
                  tooltip={item.title}
                  isActive={isSectionActive && !item.items}
                >
                  <a
                    href={item.url}
                    aria-current={isSectionActive && !item.items ? "page" : undefined}
                    style={item.accent ? ({ "--nav-accent": item.accent } as React.CSSProperties) : undefined}
                  >
                    {item.icon && (
                      <item.icon className={item.accent ? "text-(--nav-accent)" : undefined} />
                    )}
                    <span>{item.title}</span>
                  </a>
                </SidebarMenuButton>
                {item.items && item.items.length > 0 ? (
                  <SidebarMenuSub data-testid={item.subTestId}>
                    {item.items.map((sub) => {
                      const isSubActive = activeUrl === sub.url
                      return (
                        <SidebarMenuSubItem key={sub.title}>
                          <SidebarMenuSubButton asChild isActive={isSubActive}>
                            <a
                              href={sub.url}
                              aria-current={isSubActive ? "page" : undefined}
                              style={item.accent ? ({ "--nav-accent": item.accent } as React.CSSProperties) : undefined}
                            >
                              {sub.icon && (
                                <sub.icon
                                  className={item.accent ? "text-(--nav-accent) opacity-80" : "text-muted-foreground"}
                                />
                              )}
                              <span>{sub.title}</span>
                              {sub.badge != null && (
                                <span className="ml-auto" data-testid="nav-sub-badge">
                                  {sub.badge}
                                </span>
                              )}
                            </a>
                          </SidebarMenuSubButton>
                        </SidebarMenuSubItem>
                      )
                    })}
                  </SidebarMenuSub>
                ) : null}
              </SidebarMenuItem>
            )
          })}
        </SidebarMenu>
      </SidebarGroupContent>
    </SidebarGroup>
  )
}
