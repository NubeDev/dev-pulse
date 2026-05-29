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

import { useMemo, type ReactNode } from "react"
import { AnimatePresence, motion, useScroll, useSpring } from "motion/react"
import {
  IconBuilding,
  IconBuildingSkyscraper,
  IconBug,
  IconChartBar,
  IconClockHour4,
  IconColumns,
  IconHome,
  IconHistory,
  IconInbox,
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
  IconArchive,
  IconLayoutKanban,
  IconCircleDashed,
  IconCircleCheck,
  IconClipboardList,
  IconSettings,
  IconUserCircle,
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
  accountTabOf,
  adminTabOf,
  directoryTabOf,
  projectsStatusOf,
  reportTabOf,
  sectionOf,
  useRoute,
  workflowTabOf,
  type Section,
} from "../routes.js"
import { useMyQueue } from "../workflow/use-workflow-data.js"
import { useProjectCount } from "../projects/use-projects-data.js"
import { Badge } from "@/components/ui/badge"
const NAV_MAIN: NavMainItem[] = [
  {
    // `linear-projects-v2.md` §6.1 — top-level Projects section
    // between Workflow and Directory. The four sub-items mirror the
    // §6.1 mock: Active / Backlog / Done / Archived. Counts on the
    // first three are wired live from `useProjectCount` (which fires
    // `GET /projects?status=…&count_only=1`); Archived is collapsed
    // and intentionally uncounted to keep the eye off it.
    title: "Projects",
    url: "#/projects",
    icon: IconLayoutKanban,
    accent: "var(--accent-reports)",
    subTestId: "projects-subnav",
    items: [
      {
        title: "Portfolio",
        url: "#/reports/projects",
        icon: IconChartBar,
        description:
          "Cross-org portfolio report: which projects are on track, slipping, or done. Table + Gantt.",
      },
      {
        title: "Active",
        url: "#/reports/projects?status=active",
        icon: IconCircleDashed,
        description:
          "Portfolio filtered to projects currently being worked on.",
      },
      {
        title: "Backlog",
        url: "#/reports/projects?status=backlog",
        icon: IconClipboardList,
        description:
          "Portfolio filtered to planned projects not yet started.",
      },
      {
        title: "Done",
        url: "#/reports/projects?status=done",
        icon: IconCircleCheck,
        description:
          "Portfolio filtered to completed projects.",
      },
      {
        title: "Archived",
        url: "#/reports/projects?status=archived",
        icon: IconArchive,
        description:
          "Portfolio filtered to archived projects (hidden by default; data preserved).",
      },
    ],
  },
  {
    title: "Workflow",
    minRole: "writer",
    url: "#/workflow",
    icon: IconBriefcase,
    accent: "var(--accent-reports)",
    subTestId: "workflow-subnav",
    items: [
      {
        title: "Triage",
        url: "#/workflow/triage",
        icon: IconInbox,
        description:
          "Your inbox of issues / PRs needing your attention: review requests, mentions, assigned work.",
      },
      {
        title: "Repos",
        url: "#/workflow/repos",
        icon: IconBuilding,
        description:
          "Pick which repos dev-pulse tracks. Toggle visibility and refresh schedule per repo.",
      },
      {
        title: "Issues",
        url: "#/workflow/issues",
        icon: IconBriefcase,
        description:
          "Cross-repo issue search. Filter by state, labels, assignee, and date — independent of projects.",
      },
    ],
  },
  {
    title: "Reports",
    url: "#/reports",
    icon: IconChartBar,
    accent: "var(--accent-reports)",
    subTestId: "reports-subnav",
    items: [
      {
        title: "User",
        url: "#/reports/user",
        icon: IconUser,
        description:
          "One person's activity over a window — PRs opened, reviews given, comments, commits.",
      },
      {
        title: "Team",
        url: "#/reports/team",
        icon: IconUsers,
        description:
          "Roll-up of a team's activity over a window. Same metrics as User, summed across members.",
      },
      {
        title: "Org",
        url: "#/reports/org",
        icon: IconBuilding,
        description:
          "Whole-org activity over a window. Use for monthly or quarterly stakeholder snapshots.",
      },
      {
        title: "Leaderboard",
        url: "#/reports/leaderboard",
        icon: IconTrophy,
        description:
          "Ranked list of contributors by activity volume in a window. Filter by org or team.",
      },
      {
        title: "Repo activity",
        url: "#/reports/repo-activity",
        icon: IconBuilding,
        description:
          "Per-repo heatmap, review velocity, contributor diversity, and focus score for a window.",
      },
      {
        title: "Home-org split",
        url: "#/reports/home-org-split",
        icon: IconColumns,
        description:
          "How much work each user did inside vs. outside their home org. Surfaces cross-org collaboration.",
      },
      {
        title: "Freshness",
        url: "#/reports/freshness",
        icon: IconClockHour4,
        description:
          "When was each repo / user / org last refreshed from GitHub? Find stale data before reporting on it.",
      },
    ],
  },
  {
    title: "Directory",
    url: "#/directory",
    icon: IconUsersGroup,
    accent: "var(--accent-directory)",
    subTestId: "directory-subnav",
    items: [
      {
        title: "Users",
        url: "#/directory/users",
        icon: IconUser,
        description:
          "Every GitHub user dev-pulse has seen. Search, view profile, jump to their User report.",
      },
      {
        title: "Orgs",
        url: "#/directory/orgs",
        icon: IconBuildingSkyscraper,
        description:
          "All tracked GitHub orgs. Visibility, member counts, and links into per-org reports.",
      },
      {
        title: "Teams",
        url: "#/directory/teams",
        icon: IconUsers,
        description:
          "Teams within tracked orgs. Manage membership and run team-scoped reports.",
      },
      {
        title: "Home-org",
        url: "#/directory/home-org",
        icon: IconHome,
        description:
          "Assign each user a primary 'home' org. Drives the Home-org split report and identity coalescing.",
      },
    ],
  },
  {
    title: "Admin",
    minRole: "admin",
    url: "#/admin",
    icon: IconShieldLock,
    accent: "var(--accent-admin)",
    subTestId: "admin-subnav",
    items: [
      {
        title: "Runs",
        url: "#/admin/runs",
        icon: IconHistory,
        description:
          "History of fetch + report runs. Inspect logs, retry failed runs, see cost and duration.",
      },
      {
        title: "Refresh",
        url: "#/admin/refresh",
        icon: IconRefresh,
        description:
          "Trigger an immediate GitHub refresh for a repo, org, or user. Bypasses the scheduled cadence.",
      },
      {
        title: "Users",
        url: "#/admin/users",
        icon: IconUserCog,
        description:
          "User administration: roles, GDPR export / delete, and identity merge.",
      },
    ],
  },
  {
    // Account section — per-user identity + settings surface.
    // Distinct from `Admin` (operator-only) and `Directory`
    // (operator-facing user listings); this is the "my account"
    // self-service area.
    title: "Account",
    url: "#/account",
    icon: IconUserCircle,
    accent: "var(--accent-admin)",
    subTestId: "account-subnav",
    items: [
      {
        title: "Identities",
        url: "#/account/identities",
        icon: IconUser,
        description:
          "Link multiple GitHub accounts to one dev-pulse identity so your activity rolls up correctly.",
      },
      {
        title: "Tags",
        url: "#/account/tags",
        icon: IconTags,
        description:
          "Personal tags you can attach to issues, PRs, or projects for your own filtering.",
      },
      {
        title: "Settings",
        url: "#/account/settings",
        icon: IconSettings,
        description:
          "Notification preferences, default window, theme, and other per-account options.",
      },
    ],
  },
]

const SECTION_TITLE: Record<Section, string> = {
  reports: "Reports",
  directory: "Directory",
  admin: "Admin",
  workflow: "Workflow",
  projects: "Projects",
  account: "Account",
  login: "Login",
}

const PROJECT_STATUS_TITLE: Record<string, string> = {
  active: "Active",
  backlog: "Backlog",
  done: "Done",
  archived: "Archived",
}

const WORKFLOW_TITLE: Record<string, string> = {
  triage: "Triage",
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
  users: "Users",
}

/** Normalise the hash route to the closest `NavMain` url so the
 *  sidebar's active state highlights the right sub-item.
 *  `#/reports/user/abc` → `#/reports/user`, etc.
 *
 *  Projects is the one section whose sub-items are query-string-
 *  scoped (`#/projects?status=active`) rather than path-scoped, so
 *  we preserve the `status` filter here — otherwise every Projects
 *  sub-item would always read "Active" as the active one.
 *
 *  The portfolio report (`#/reports/projects?status=…`) is also
 *  query-string-scoped — same reason, same fix. */
function activeUrlFor(route: string): string {
  const q = route.indexOf("?")
  const pathPart = q < 0 ? route : route.slice(0, q)
  const path = pathPart.replace(/^#/, "").replace(/^\/+/, "").split("/")
  const section = path[0] ?? ""
  const tab = path[1] ?? ""
  if (!section) return "#/reports"
  if (section === "projects") {
    const status = projectsStatusOf(route)
    return status ? `#/projects?status=${status}` : "#/projects"
  }
  if (section === "reports" && tab === "projects") {
    // Portfolio: preserve the `?status=` so the matching sub-item
    // (Active / Backlog / Done / Archived) lights up; plain
    // `#/reports/projects` (no filter) highlights "Portfolio".
    const params = new URLSearchParams(q >= 0 ? route.slice(q + 1) : "")
    const status = params.get("status")
    const valid = new Set(["active", "backlog", "done", "archived"])
    return status && valid.has(status)
      ? `#/reports/projects?status=${status}`
      : "#/reports/projects"
  }
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
    case "projects": {
      // §6.1 — `#/projects?status=active` → "Projects · Active",
      // `#/projects` (no filter) → "Projects".
      const status = projectsStatusOf(route)
      if (!status) return "Projects"
      return `Projects · ${PROJECT_STATUS_TITLE[status] ?? status}`
    }
    case "account":
      return accountTabOf(route) === "settings"
        ? "Account · Settings"
        : "Account · Identities"
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

  // Inbox badge for the Issues sub-nav (`linear-projects-idea.md`
  // §3.8). One-row query so we get the `total` without rendering
  // any rows; the queue endpoint already excludes done/snoozed
  // rows server-side, so `total` is the live "unhandled" count.
  // Authed gate: `useMyQueue` enables unconditionally, but the
  // request fails closed when there's no session — we still need
  // to render the sidebar in that case, so we read `total` defensively.
  const queueProbe = useMyQueue({ limit: 1, offset: 0 })
  const inboxCount = queueProbe.data?.total ?? 0

  // `linear-projects-v2.md` §6.1 — live per-status counts on the
  // Projects sidebar entry. Three count-only probes
  // (`GET /projects?status=…&count_only=1`) — Archived is
  // intentionally uncounted per spec. Each hook caches under its
  // own key so a future create / archive mutation can selectively
  // invalidate.
  const activeProjects = useProjectCount("active")
  const backlogProjects = useProjectCount("backlog")
  const doneProjects = useProjectCount("done")

  // Role tier comparison: section is visible iff
  // `auth.user.role >= item.minRole`. Unauthenticated users see the
  // reader-tier sections only (the unauthed shell never actually
  // renders nav past the login flow, but the guard is cheap).
  // DOCS/SCOPE-AUTHZ-USERS.md §4.1.
  const userRole = (auth.user?.role ?? "reader") as
    | "reader"
    | "writer"
    | "admin"
  const ROLE_RANK: Record<"reader" | "writer" | "admin", number> = {
    reader: 0,
    writer: 1,
    admin: 2,
  }
  const roleAtLeast = (
    have: "reader" | "writer" | "admin",
    need?: "reader" | "writer" | "admin",
  ): boolean => ROLE_RANK[have] >= ROLE_RANK[need ?? "reader"]

  const navMain = useMemo<NavMainItem[]>(
    () =>
      NAV_MAIN.filter((item) => roleAtLeast(userRole, item.minRole)).map((item) => {
        if (item.title === "Workflow") {
          return {
            ...item,
            items: item.items?.map((sub) =>
              sub.url === "#/workflow/triage" && inboxCount > 0
                ? {
                    ...sub,
                    badge: (
                      <Badge
                        variant="secondary"
                        className="h-5 min-w-5 justify-center px-1.5 text-[10px] font-semibold"
                        data-testid="workflow-triage-inbox-badge"
                      >
                        {inboxCount > 99 ? "99+" : inboxCount}
                      </Badge>
                    ),
                  }
                : sub,
            ),
          }
        }
        if (item.title === "Projects") {
          // Map status -> live count. Archived stays uncounted per
          // §6.1 — the eye should not be drawn to the archive bin.
          const counts: Record<string, { value: number; testId: string }> = {
            "#/reports/projects?status=active": {
              value: activeProjects.count,
              testId: "projects-count-active",
            },
            "#/reports/projects?status=backlog": {
              value: backlogProjects.count,
              testId: "projects-count-backlog",
            },
            "#/reports/projects?status=done": {
              value: doneProjects.count,
              testId: "projects-count-done",
            },
          }
          return {
            ...item,
            items: item.items?.map((sub) => {
              const c = counts[sub.url]
              if (!c || c.value <= 0) return sub
              return {
                ...sub,
                badge: (
                  <Badge
                    variant="secondary"
                    className="h-5 min-w-5 justify-center px-1.5 text-[10px] font-semibold"
                    data-testid={c.testId}
                  >
                    {c.value > 99 ? "99+" : c.value}
                  </Badge>
                ),
              }
            }),
          }
        }
        return item
      }),
    [
      inboxCount,
      activeProjects.count,
      backlogProjects.count,
      doneProjects.count,
      userRole,
    ],
  )

  return (
    <div data-testid="app-shell" className="min-h-dvh bg-background text-foreground">
      <ScrollProgress />
      <SidebarProvider>
        <AppSidebar
          navMain={navMain}
          activeUrl={activeUrl}
          user={user}
          onLogout={() => {
            void auth.logout()
          }}
          extraContent={<PinSidebar />}
        />
        <SidebarInset>
          <SiteHeader
            title={titleFor(route)}
            actions={
              <>
                <a
                  href="#/account/identities"
                  data-testid="user-menu-identity-badge"
                  title="Linked identities (slice 2 §10)"
                  className="hidden items-center gap-1 rounded-md border border-border bg-background px-2 py-1 text-xs font-medium text-muted-foreground hover:bg-accent sm:inline-flex"
                >
                  <span className="size-1.5 rounded-full bg-primary" />
                  <span>1 identity</span>
                </a>
                <ThemeToggle />
              </>
            }
          />
          <AnimatePresence mode="wait">
            <motion.div
              // Key on the *path* portion of the hash route so the fade
              // only fires for real page navigation. Without this strip,
              // query-string state — open issue drawer, active tab, status
              // filter — would re-key on every click and replay the
              // intro animation, which felt jittery on busy pages.
              key={route.split("?")[0]}
              initial={{ opacity: 0, y: 8 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -4 }}
              transition={{ duration: 0.35, ease: [0.22, 1, 0.36, 1] }}
              className="flex flex-1 flex-col"
            >
              <div className="@container/main flex flex-1 flex-col gap-2">
                <div className="flex flex-col gap-4 py-4 md:gap-6 md:py-6">
                  <div className="px-4 lg:px-6">
                    <WritesBanner />
                  </div>
                  {children}
                </div>
              </div>
            </motion.div>
          </AnimatePresence>
        </SidebarInset>
      </SidebarProvider>
    </div>
  )
}

/**
 * Gradient scroll-progress bar pinned to the top of the viewport.
 * Tracks `useScroll()` and springs the scaleX. Hue arc:
 * leaf → aqua → sun so the brand reads at every scroll depth.
 */
function ScrollProgress() {
  const { scrollYProgress } = useScroll()
  const scaleX = useSpring(scrollYProgress, { stiffness: 100, damping: 30 })
  return (
    <motion.div
      aria-hidden
      style={{
        scaleX,
        transformOrigin: "0% 50%",
        background:
          "linear-gradient(90deg, var(--brand-leaf), var(--brand-aqua), var(--brand-sun))",
      }}
      className="pointer-events-none fixed inset-x-0 top-0 z-[60] h-[2px]"
    />
  )
}
