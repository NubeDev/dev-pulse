/**
 * NavMain — rubix-style sidebar nav.
 *
 * Each top-level `NavMainItem` becomes a `SidebarGroup` with a
 * `SidebarGroupLabel` (the section title). Sub-items are rendered as
 * a collapsible flat menu in the expanded state, and as a hover
 * dropdown when the sidebar collapses to icons — matching rubix's
 * `NavGroup` component verbatim, adapted to dev-pulse's existing
 * `NavMainItem` data shape so `app-shell.tsx` is untouched.
 *
 * The `description` prop on a sub-item still surfaces as a hover
 * tooltip so the dense sidebar labels stay self-documenting.
 */

import type * as React from "react"
import { useCallback, useEffect, useState } from "react"
import { IconChevronRight, type Icon } from "@tabler/icons-react"

import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  SidebarGroup,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
  useSidebar,
} from "@/components/ui/sidebar"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"

export interface NavMainSubItem {
  title: string
  url: string
  icon?: Icon
  /** Optional small badge rendered at the right edge of the sub-button. */
  badge?: React.ReactNode
  /** One-line plain-English explanation surfaced as a hover tooltip. */
  description?: string
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
  const { state, isMobile } = useSidebar()
  const collapsedIcons = state === "collapsed" && !isMobile
  return (
    <>
      {items.map((item) => (
        <SidebarGroup key={item.title}>
          <SidebarGroupLabel>{item.title}</SidebarGroupLabel>
          <SidebarMenu>
            {item.items && item.items.length > 0 ? (
              collapsedIcons ? (
                <SectionDropdown item={item} activeUrl={activeUrl} />
              ) : (
                <SectionCollapsible item={item} activeUrl={activeUrl} />
              )
            ) : (
              <SectionLink item={item} activeUrl={activeUrl} />
            )}
          </SidebarMenu>
        </SidebarGroup>
      ))}
    </>
  )
}

/** Section with no children — render as a single link. */
function SectionLink({
  item,
  activeUrl,
}: {
  item: NavMainItem
  activeUrl?: string
}) {
  const isActive = sectionContainsActive(item, activeUrl)
  return (
    <SidebarMenuItem>
      <SidebarMenuButton asChild tooltip={item.title} isActive={isActive}>
        <a
          href={item.url}
          aria-current={isActive ? "page" : undefined}
          style={item.accent ? ({ "--nav-accent": item.accent } as React.CSSProperties) : undefined}
        >
          {item.icon && (
            <item.icon className={item.accent ? "text-(--nav-accent)" : undefined} />
          )}
          <span>{item.title}</span>
        </a>
      </SidebarMenuButton>
    </SidebarMenuItem>
  )
}

/** Section with children — render as a Collapsible. Top-level button
 *  toggles open/close (matches rubix); sub-items navigate.
 *
 *  Open/closed state is persisted to `localStorage` so the section
 *  the user expanded stays expanded across refreshes. When the user
 *  navigates *into* a section, that section auto-opens (overriding
 *  the stored state) so the active page is always visible — same
 *  behaviour as the previous `defaultOpen={isSectionActive}` rule. */
function SectionCollapsible({
  item,
  activeUrl,
}: {
  item: NavMainItem
  activeUrl?: string
}) {
  const isSectionActive = sectionContainsActive(item, activeUrl)
  const [open, setOpen] = usePersistentOpen(item.title, isSectionActive)
  // When the user navigates into this section, force-open it (even if
  // the user had previously collapsed it). The effect runs only when
  // `isSectionActive` flips true so manual collapses of the active
  // section are still respected within the same browsing turn.
  useEffect(() => {
    if (isSectionActive) setOpen(true)
  }, [isSectionActive, setOpen])
  return (
    <Collapsible
      asChild
      open={open}
      onOpenChange={setOpen}
      className="group/collapsible"
    >
      <SidebarMenuItem>
        <CollapsibleTrigger asChild>
          <SidebarMenuButton
            tooltip={item.title}
            style={item.accent ? ({ "--nav-accent": item.accent } as React.CSSProperties) : undefined}
          >
            {item.icon && (
              <item.icon className={item.accent ? "text-(--nav-accent)" : undefined} />
            )}
            <span>{item.title}</span>
            <IconChevronRight className="ms-auto transition-transform duration-200 group-data-[state=open]/collapsible:rotate-90" />
          </SidebarMenuButton>
        </CollapsibleTrigger>
        <CollapsibleContent className="CollapsibleContent">
          <SidebarMenuSub data-testid={item.subTestId}>
            {item.items!.map((sub) => (
              <NavSubButton key={sub.title} sub={sub} accent={item.accent} activeUrl={activeUrl} />
            ))}
          </SidebarMenuSub>
        </CollapsibleContent>
      </SidebarMenuItem>
    </Collapsible>
  )
}

/** True when this section "owns" `activeUrl`.
 *
 *  Critical asymmetry: sections with children only count their *children's*
 *  URLs. Without this rule, Reports (`#/reports`) would prefix-match
 *  `#/reports/projects?status=backlog` — the Projects portfolio URL — and
 *  spuriously open the Reports section. By making child URLs authoritative
 *  for grouped sections, each URL is owned by exactly one section.
 *
 *  Sections without children fall back to the obvious prefix match on
 *  their own URL. */
function sectionContainsActive(item: NavMainItem, activeUrl?: string): boolean {
  if (!activeUrl) return false
  if (item.items && item.items.length > 0) {
    return item.items.some(
      (sub) => activeUrl === sub.url || activeUrl.startsWith(`${sub.url}/`),
    )
  }
  return activeUrl === item.url || activeUrl.startsWith(`${item.url}/`)
}

const STORAGE_KEY = "dp-sidebar-open"

/** Persist a per-section open boolean to localStorage so the user's
 *  expand/collapse choices survive refreshes. `fallback` (the initial
 *  is-active check) is used when nothing is stored yet, so on first
 *  visit the active section reads as open. Storage is namespaced by
 *  section title — short, stable, doesn't include URL state. */
function usePersistentOpen(
  key: string,
  fallback: boolean,
): [boolean, (next: boolean) => void] {
  // Lazy initializer reads localStorage synchronously on mount, which
  // means the first paint already has the right state — no flicker.
  const [open, setOpen] = useState<boolean>(() => {
    if (typeof window === "undefined") return fallback
    try {
      const raw = window.localStorage.getItem(STORAGE_KEY)
      if (!raw) return fallback
      const stored = JSON.parse(raw) as Record<string, boolean>
      return key in stored ? stored[key]! : fallback
    } catch {
      return fallback
    }
  })
  const setAndStore = useCallback(
    (next: boolean) => {
      setOpen(next)
      if (typeof window === "undefined") return
      try {
        const raw = window.localStorage.getItem(STORAGE_KEY)
        const stored = raw ? (JSON.parse(raw) as Record<string, boolean>) : {}
        stored[key] = next
        window.localStorage.setItem(STORAGE_KEY, JSON.stringify(stored))
      } catch {
        // localStorage can throw in private-browsing / quota cases —
        // in-memory state is still updated, so this is a best-effort
        // persistence layer and the UI keeps working.
      }
    },
    [key],
  )
  return [open, setAndStore]
}

/** Collapsed (icon-only) sidebar: render the section as a dropdown so
 *  sub-items stay reachable. Mirrors rubix's `SidebarMenuCollapsedDropdown`. */
function SectionDropdown({
  item,
  activeUrl,
}: {
  item: NavMainItem
  activeUrl?: string
}) {
  const isSectionActive = sectionContainsActive(item, activeUrl)
  return (
    <SidebarMenuItem>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <SidebarMenuButton
            tooltip={item.title}
            isActive={isSectionActive}
            style={item.accent ? ({ "--nav-accent": item.accent } as React.CSSProperties) : undefined}
          >
            {item.icon && (
              <item.icon className={item.accent ? "text-(--nav-accent)" : undefined} />
            )}
            <span>{item.title}</span>
            <IconChevronRight className="ms-auto" />
          </SidebarMenuButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent side="right" align="start" sideOffset={4} className="min-w-56">
          <DropdownMenuLabel>{item.title}</DropdownMenuLabel>
          <DropdownMenuSeparator />
          {item.items!.map((sub) => {
            const isSubActive = activeUrl === sub.url
            return (
              <DropdownMenuItem key={sub.title} asChild>
                <a
                  href={sub.url}
                  aria-current={isSubActive ? "page" : undefined}
                  className={isSubActive ? "bg-accent" : undefined}
                  style={item.accent ? ({ "--nav-accent": item.accent } as React.CSSProperties) : undefined}
                >
                  {sub.icon && (
                    <sub.icon
                      className={item.accent ? "text-(--nav-accent) opacity-80" : "text-muted-foreground"}
                    />
                  )}
                  <span className="max-w-52 text-wrap">{sub.title}</span>
                  {sub.badge != null && <span className="ms-auto text-xs">{sub.badge}</span>}
                </a>
              </DropdownMenuItem>
            )
          })}
        </DropdownMenuContent>
      </DropdownMenu>
    </SidebarMenuItem>
  )
}

function NavSubButton({
  sub,
  accent,
  activeUrl,
}: {
  sub: NavMainSubItem
  accent?: string
  activeUrl?: string
}) {
  const isSubActive = activeUrl === sub.url
  const button = (
    <SidebarMenuSubButton asChild isActive={isSubActive}>
      <a
        href={sub.url}
        aria-current={isSubActive ? "page" : undefined}
        style={accent ? ({ "--nav-accent": accent } as React.CSSProperties) : undefined}
      >
        {sub.icon && (
          <sub.icon
            className={accent ? "text-(--nav-accent) opacity-80" : "text-muted-foreground"}
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
  )
  return (
    <SidebarMenuSubItem>
      {sub.description ? (
        <Tooltip>
          <TooltipTrigger asChild>{button}</TooltipTrigger>
          <TooltipContent side="right" align="center" className="max-w-xs">
            {sub.description}
          </TooltipContent>
        </Tooltip>
      ) : (
        button
      )}
    </SidebarMenuSubItem>
  )
}
