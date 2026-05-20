/**
 * dev-pulse SPA root.
 *
 * Stage 4 promotes the reports section: `#/reports` (and the
 * deep-linked `#/reports/user/:user_id`) now renders the SCOPE §11.5
 * user-activity report — headline + sortable table + per-kind
 * sparkline trend, with the three-lens toggle and "Data as of"
 * banner. Directory / Admin remain placeholders until later stages.
 */

import { useAuth, AuthProvider } from "@nube/starter-ui-core/auth";

import { api } from "./api/client.js";
import { authStrategy } from "./auth/strategy.js";
import { LoginPage } from "./auth/login-page.jsx";
import { ProtectedRoute } from "./auth/protected-route.jsx";
import { AppShell } from "./layout/app-shell.jsx";
import { AdminUsersPage } from "./admin/users-page.jsx";
import { RefreshPage } from "./admin/refresh-page.jsx";
import { RunsPage } from "./admin/runs-page.jsx";
import { HomeOrgPage } from "./directory/home-org-page.jsx";
import { OrgsPage } from "./directory/orgs-page.jsx";
import { TeamsPage } from "./directory/teams-page.jsx";
import { UsersPage } from "./directory/users-page.jsx";
import { FreshnessPage } from "./reports/freshness-page.jsx";
import { HomeOrgSplitReportPage } from "./reports/home-org-split-report-page.jsx";
import { OrgReportPage } from "./reports/org-report-page.jsx";
import { TeamReportPage } from "./reports/team-report-page.jsx";
import { UserReportPage } from "./reports/user-report-page.jsx";
import {
  adminTabOf,
  directoryTabOf,
  isLoginRoute,
  reportTabOf,
  sectionOf,
  useRoute,
  type AdminTab,
  type DirectoryTab,
  type ReportTab,
} from "./routes.js";

export function App(): JSX.Element {
  return (
    <AuthProvider client={api.client} strategy={authStrategy}>
      <Router />
    </AuthProvider>
  );
}

function Router(): JSX.Element {
  const route = useRoute();
  const auth = useAuth();

  // If we already have a session and the user lands on /login, bounce
  // them into the app. This covers the "back-button to /login after
  // signing in" case.
  if (isLoginRoute(route)) {
    if (auth.status === "authenticated") {
      return (
        <ProtectedRoute>
          <AppShell>
            <SectionPane section="reports" route={route} />
          </AppShell>
        </ProtectedRoute>
      );
    }
    return <LoginPage />;
  }

  const section = sectionOf(route);
  // `sectionOf` may return "login" (defensive — the `isLoginRoute`
  // branch above already absorbed that case for unauthenticated users),
  // so narrow to the three protected sections here.
  const proteced = section === "login" ? "reports" : section;
  return (
    <ProtectedRoute>
      <AppShell>
        <SectionPane section={proteced} route={route} />
      </AppShell>
    </ProtectedRoute>
  );
}

function SectionPane({
  section,
  route,
}: {
  section: "reports" | "directory" | "admin";
  route: string;
}): JSX.Element {
  switch (section) {
    case "reports":
      return <ReportsSection tab={reportTabOf(route)} />;
    case "directory":
      return <DirectorySection tab={directoryTabOf(route)} />;
    case "admin":
      return <AdminSection tab={adminTabOf(route)} />;
  }
}

interface ReportNavItem {
  readonly tab: ReportTab;
  readonly label: string;
  readonly href: string;
}

const REPORT_TABS: readonly ReportNavItem[] = [
  { tab: "user", label: "User", href: "#/reports/user" },
  { tab: "team", label: "Team", href: "#/reports/team" },
  { tab: "org", label: "Org", href: "#/reports/org" },
  { tab: "home-org-split", label: "Home-org split", href: "#/reports/home-org-split" },
  { tab: "freshness", label: "Freshness", href: "#/reports/freshness" },
];

function ReportsSection({ tab }: { tab: ReportTab }): JSX.Element {
  // The reports sub-nav is rendered above the active report pane.
  // Plain anchors (not Tabs) keep the route the source of truth so
  // deep links + back-button work without an extra controlled state.
  // No leaderboard affordance anywhere — by design (§4).
  return (
    <div style={{ display: "grid", gap: "1rem" }}>
      <nav
        aria-label="Reports"
        data-testid="reports-subnav"
        style={{
          display: "flex",
          gap: "0.25rem",
          padding: "0.25rem",
          background: "var(--muted)",
          borderRadius: "var(--radius-md, 0.5rem)",
          alignSelf: "flex-start",
        }}
      >
        {REPORT_TABS.map((item) => {
          const isActive = item.tab === tab;
          return (
            <a
              key={item.tab}
              href={item.href}
              aria-current={isActive ? "page" : undefined}
              style={{
                padding: "0.375rem 0.75rem",
                borderRadius: "var(--radius-sm, 0.375rem)",
                fontSize: "0.875rem",
                textDecoration: "none",
                color: isActive ? "var(--primary-foreground)" : "var(--foreground)",
                background: isActive ? "var(--primary)" : "transparent",
              }}
            >
              {item.label}
            </a>
          );
        })}
      </nav>
      <ReportsPane tab={tab} />
    </div>
  );
}

function ReportsPane({ tab }: { tab: ReportTab }): JSX.Element {
  switch (tab) {
    case "user":
      return <UserReportPage />;
    case "team":
      return <TeamReportPage />;
    case "org":
      return <OrgReportPage />;
    case "home-org-split":
      return <HomeOrgSplitReportPage />;
    case "freshness":
      return <FreshnessPage />;
  }
}

interface DirectoryNavItem {
  readonly tab: DirectoryTab;
  readonly label: string;
  readonly href: string;
}

const DIRECTORY_TABS: readonly DirectoryNavItem[] = [
  { tab: "users", label: "Users", href: "#/directory/users" },
  { tab: "orgs", label: "Orgs", href: "#/directory/orgs" },
  { tab: "teams", label: "Teams", href: "#/directory/teams" },
  { tab: "home-org", label: "Home-org assignment", href: "#/directory/home-org" },
];

function DirectorySection({ tab }: { tab: DirectoryTab }): JSX.Element {
  // Same sub-nav pattern as Reports: plain anchors so the hash
  // route is the source of truth for the active tab.
  return (
    <div style={{ display: "grid", gap: "1rem" }}>
      <nav
        aria-label="Directory"
        data-testid="directory-subnav"
        style={{
          display: "flex",
          gap: "0.25rem",
          padding: "0.25rem",
          background: "var(--muted)",
          borderRadius: "var(--radius-md, 0.5rem)",
          alignSelf: "flex-start",
        }}
      >
        {DIRECTORY_TABS.map((item) => {
          const isActive = item.tab === tab;
          return (
            <a
              key={item.tab}
              href={item.href}
              aria-current={isActive ? "page" : undefined}
              style={{
                padding: "0.375rem 0.75rem",
                borderRadius: "var(--radius-sm, 0.375rem)",
                fontSize: "0.875rem",
                textDecoration: "none",
                color: isActive ? "var(--primary-foreground)" : "var(--foreground)",
                background: isActive ? "var(--primary)" : "transparent",
              }}
            >
              {item.label}
            </a>
          );
        })}
      </nav>
      <DirectoryPane tab={tab} />
    </div>
  );
}

function DirectoryPane({ tab }: { tab: DirectoryTab }): JSX.Element {
  switch (tab) {
    case "users":
      return <UsersPage />;
    case "orgs":
      return <OrgsPage />;
    case "teams":
      return <TeamsPage />;
    case "home-org":
      return <HomeOrgPage />;
  }
}

interface AdminNavItem {
  readonly tab: AdminTab;
  readonly label: string;
  readonly href: string;
}

const ADMIN_TABS: readonly AdminNavItem[] = [
  { tab: "runs", label: "Runs", href: "#/admin/runs" },
  { tab: "refresh", label: "Refresh", href: "#/admin/refresh" },
  { tab: "users", label: "User GDPR", href: "#/admin/users" },
];

function AdminSection({ tab }: { tab: AdminTab }): JSX.Element {
  // Same plain-anchor sub-nav pattern as Reports / Directory — the
  // hash route stays the source of truth so deep links + back-button
  // work without an extra controlled state.
  return (
    <div style={{ display: "grid", gap: "1rem" }}>
      <nav
        aria-label="Admin"
        data-testid="admin-subnav"
        style={{
          display: "flex",
          gap: "0.25rem",
          padding: "0.25rem",
          background: "var(--muted)",
          borderRadius: "var(--radius-md, 0.5rem)",
          alignSelf: "flex-start",
        }}
      >
        {ADMIN_TABS.map((item) => {
          const isActive = item.tab === tab;
          return (
            <a
              key={item.tab}
              href={item.href}
              aria-current={isActive ? "page" : undefined}
              style={{
                padding: "0.375rem 0.75rem",
                borderRadius: "var(--radius-sm, 0.375rem)",
                fontSize: "0.875rem",
                textDecoration: "none",
                color: isActive ? "var(--primary-foreground)" : "var(--foreground)",
                background: isActive ? "var(--primary)" : "transparent",
              }}
            >
              {item.label}
            </a>
          );
        })}
      </nav>
      <AdminPane tab={tab} />
    </div>
  );
}

function AdminPane({ tab }: { tab: AdminTab }): JSX.Element {
  switch (tab) {
    case "runs":
      return <RunsPage />;
    case "refresh":
      return <RefreshPage />;
    case "users":
      return <AdminUsersPage />;
  }
}
