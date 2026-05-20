/**
 * App shell — the shadcn `dashboard-01` block layout, verbatim:
 * `SidebarProvider` + `AppSidebar` + `SidebarInset` + `SiteHeader`,
 * with the section's main pane filling the inset body.
 *
 * Routes are mapped onto `NavMain` (Reports → User/Team/Org/Home-org
 * split/Freshness; Directory → Users/Orgs/Teams/Home-org; Admin →
 * Runs/Refresh). The current `/auth/me` user is wired into `NavUser`
 * so the avatar + email + role surface inside the sidebar footer —
 * the same place dashboard-01 puts it. No hand-rolled chrome.
 */

import type { ReactNode } from "react"
import {
  IconChartBar,
  IconShieldLock,
  IconUsersGroup,
} from "@tabler/icons-react"
import { useAuth } from "@nube/starter-ui-core/auth"

import { AppSidebar } from "@/components/app-sidebar"
import { SiteHeader } from "@/components/site-header"
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar"
import type { NavMainItem } from "@/components/nav-main"
import { ThemeToggle } from "../components/theme-toggle.jsx"
import {
  adminTabOf,
  directoryTabOf,
  reportTabOf,
  sectionOf,
  useRoute,
  type Section,
} from "../routes.js"

const NAV_MAIN: NavMainItem[] = [
  {
    title: "Reports",
    url: "#/reports",
    icon: IconChartBar,
    subTestId: "reports-subnav",
    items: [
      { title: "User", url: "#/reports/user" },
      { title: "Team", url: "#/reports/team" },
      { title: "Org", url: "#/reports/org" },
      { title: "Home-org split", url: "#/reports/home-org-split" },
      { title: "Freshness", url: "#/reports/freshness" },
    ],
  },
  {
    title: "Directory",
    url: "#/directory",
    icon: IconUsersGroup,
    subTestId: "directory-subnav",
    items: [
      { title: "Users", url: "#/directory/users" },
      { title: "Orgs", url: "#/directory/orgs" },
      { title: "Teams", url: "#/directory/teams" },
      { title: "Home-org", url: "#/directory/home-org" },
    ],
  },
  {
    title: "Admin",
    url: "#/admin",
    icon: IconShieldLock,
    subTestId: "admin-subnav",
    items: [
      { title: "Runs", url: "#/admin/runs" },
      { title: "Refresh", url: "#/admin/refresh" },
      { title: "Users", url: "#/admin/users" },
    ],
  },
]

const SECTION_TITLE: Record<Section, string> = {
  reports: "Reports",
  directory: "Directory",
  admin: "Admin",
  login: "Login",
}

const REPORT_TITLE: Record<string, string> = {
  user: "User activity",
  team: "Team activity",
  org: "Org activity",
  "home-org-split": "Home-org split",
  freshness: "Freshness",
}

const DIRECTORY_TITLE: Record<string, string> = {
  users: "Users",
  orgs: "Orgs",
  teams: "Teams",
  "home-org": "Home-org assignment",
}

const ADMIN_TITLE: Record<string, string> = {
  runs: "Runs",
  refresh: "Refresh",
  users: "User GDPR",
}

/** Normalise the hash route to the closest `NavMain` url so the
 *  sidebar's active state highlights the right sub-item.
 *  `#/reports/user/abc` → `#/reports/user`, etc. */
function activeUrlFor(route: string): string {
  const path = route.replace(/^#/, "").replace(/^\/+/, "").split("/")
  const section = path[0] ?? ""
  const tab = path[1] ?? ""
  if (!section) return "#/reports"
  if (!tab) return `#/${section}`
  return `#/${section}/${tab}`
}

function titleFor(route: string): string {
  const section = sectionOf(route)
  switch (section) {
    case "reports": {
      const t = reportTabOf(route)
      return `Reports · ${REPORT_TITLE[t] ?? t}`
    }
    case "directory": {
      const t = directoryTabOf(route)
      return `Directory · ${DIRECTORY_TITLE[t] ?? t}`
    }
    case "admin": {
      const t = adminTabOf(route)
      return `Admin · ${ADMIN_TITLE[t] ?? t}`
    }
    case "login":
    default:
      return SECTION_TITLE[section]
  }
}

export interface AppShellProps {
  children: ReactNode
}

export function AppShell({ children }: AppShellProps): JSX.Element {
  const auth = useAuth()
  const route = useRoute()
  const activeUrl = activeUrlFor(route)
  const user = auth.user
    ? {
        name: auth.user.email.split("@")[0] ?? auth.user.email,
        email: auth.user.email,
        role: auth.user.role,
      }
    : { name: "Guest", email: "" }

  return (
    <div data-testid="app-shell" className="min-h-dvh bg-background text-foreground">
      <SidebarProvider>
        <AppSidebar
          navMain={NAV_MAIN}
          activeUrl={activeUrl}
          user={user}
          onLogout={() => {
            void auth.logout()
          }}
        />
        <SidebarInset>
          <SiteHeader title={titleFor(route)} actions={<ThemeToggle />} />
          <div className="flex flex-1 flex-col">
            <div className="@container/main flex flex-1 flex-col gap-2">
              <div className="flex flex-col gap-4 py-4 md:gap-6 md:py-6">
                {children}
              </div>
            </div>
          </div>
        </SidebarInset>
      </SidebarProvider>
    </div>
  )
}
