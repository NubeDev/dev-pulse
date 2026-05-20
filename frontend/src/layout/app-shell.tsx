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
  IconBuilding,
  IconBuildingSkyscraper,
  IconBug,
  IconChartBar,
  IconClockHour4,
  IconColumns,
  IconHome,
  IconHistory,
  IconPinned,
  IconRefresh,
  IconShieldLock,
  IconTags,
  IconTrophy,
  IconUser,
  IconUserCog,
  IconUsers,
  IconUsersGroup,
  IconBriefcase,
} from "@tabler/icons-react"
import { useAuth } from "@nube/starter-ui-core/auth"

import { AppSidebar } from "@/components/app-sidebar"
import { SiteHeader } from "@/components/site-header"
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar"
import type { NavMainItem } from "@/components/nav-main"
import { ThemeToggle } from "../components/theme-toggle.jsx"
import { PinSidebar } from "../workflow/pin-sidebar.jsx"
import { WritesBanner } from "../workflow/writes-banner.jsx"
import {
  adminTabOf,
  directoryTabOf,
  reportTabOf,
  sectionOf,
  useRoute,
  workflowTabOf,
  type Section,
} from "../routes.js"
const NAV_MAIN: NavMainItem[] = [
  {
    title: "Reports",
    url: "#/reports",
    icon: IconChartBar,
    accent: "var(--accent-reports)",
    subTestId: "reports-subnav",
    items: [
      { title: "User", url: "#/reports/user", icon: IconUser },
      { title: "Team", url: "#/reports/team", icon: IconUsers },
      { title: "Org", url: "#/reports/org", icon: IconBuilding },
      { title: "Leaderboard", url: "#/reports/leaderboard", icon: IconTrophy },
      { title: "Home-org split", url: "#/reports/home-org-split", icon: IconColumns },
      { title: "Freshness", url: "#/reports/freshness", icon: IconClockHour4 },
    ],
  },
  {
    title: "Workflow",
    url: "#/workflow",
    icon: IconBriefcase,
    accent: "var(--accent-reports)",
    subTestId: "workflow-subnav",
    items: [
      { title: "Repos", url: "#/workflow/repos", icon: IconBuilding },
      { title: "Issues", url: "#/workflow/issues", icon: IconBriefcase },
    ],
  },
  {
    title: "Directory",
    url: "#/directory",
    icon: IconUsersGroup,
    accent: "var(--accent-directory)",
    subTestId: "directory-subnav",
    items: [
      { title: "Users", url: "#/directory/users", icon: IconUser },
      { title: "Orgs", url: "#/directory/orgs", icon: IconBuildingSkyscraper },
      { title: "Teams", url: "#/directory/teams", icon: IconUsers },
      { title: "Home-org", url: "#/directory/home-org", icon: IconHome },
    ],
  },
  {
    title: "Admin",
    url: "#/admin",
    icon: IconShieldLock,
    accent: "var(--accent-admin)",
    subTestId: "admin-subnav",
    items: [
      { title: "Runs", url: "#/admin/runs", icon: IconHistory },
      { title: "Refresh", url: "#/admin/refresh", icon: IconRefresh },
      { title: "Users", url: "#/admin/users", icon: IconUserCog },
    ],
  },
]

const SECTION_TITLE: Record<Section, string> = {
  reports: "Reports",
  directory: "Directory",
  admin: "Admin",
  workflow: "Workflow",
  login: "Login",
}

const WORKFLOW_TITLE: Record<string, string> = {
  repos: "Repos",
  issues: "Issues",
}

const REPORT_TITLE: Record<string, string> = {
  user: "User activity",
  team: "Team activity",
  org: "Org activity",
  "home-org-split": "Home-org split",
  leaderboard: "Leaderboard",
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
    case "workflow": {
      const t = workflowTabOf(route)
      return `Workflow · ${WORKFLOW_TITLE[t] ?? t}`
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
          extraContent={<PinSidebar />}
        />
        <SidebarInset>
          <SiteHeader title={titleFor(route)} actions={<ThemeToggle />} />
          <div className="flex flex-1 flex-col">
            <div className="@container/main flex flex-1 flex-col gap-2">
              <div className="flex flex-col gap-4 py-4 md:gap-6 md:py-6">
                <div className="px-4 lg:px-6">
                  <WritesBanner />
                </div>
                {children}
              </div>
            </div>
          </div>
        </SidebarInset>
      </SidebarProvider>
    </div>
  )
}
