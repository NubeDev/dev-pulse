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
import { ErrorBoundary } from "./components/error-boundary.jsx";
import { NotFoundPage } from "./components/not-found.jsx";
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
  isKnownRoute,
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
            <ErrorBoundary scope="reports" resetKey={route}>
              <SectionPane section="reports" route={route} />
            </ErrorBoundary>
          </AppShell>
        </ProtectedRoute>
      );
    }
    return <LoginPage />;
  }

  // Truly unknown root segment (e.g. `#/foo/bar`) -> dedicated 404.
  // Sub-path typos still fall back to the section default per
  // `*TabOf` parsers; that's intentional.
  if (!isKnownRoute(route)) {
    return (
      <ProtectedRoute>
        <AppShell>
          <ErrorBoundary scope="this page" resetKey={route}>
            <NotFoundPage />
          </ErrorBoundary>
        </AppShell>
      </ProtectedRoute>
    );
  }

  const section = sectionOf(route);
  // `sectionOf` may return "login" (defensive — the `isLoginRoute`
  // branch above already absorbed that case for unauthenticated users),
  // so narrow to the three protected sections here.
  const proteced = section === "login" ? "reports" : section;
  return (
    <ProtectedRoute>
      <AppShell>
        <ErrorBoundary scope={proteced} resetKey={route}>
          <SectionPane section={proteced} route={route} />
        </ErrorBoundary>
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

/**
 * Shared sub-nav strip for Reports / Directory / Admin. Anchor-shaped
 * (the hash route stays the source of truth — we don't want a
 * controlled `Tabs` here because deep links + back-button must work
 * without extra state) but visually a segmented control: a muted
 * pill background with an active-state primary swatch on the selected
 * anchor. Pure Tailwind utilities — no inline styles.
 */
interface SubNavItem<T extends string> {
  readonly tab: T;
  readonly label: string;
  readonly href: string;
}

function SubNav<T extends string>({
  label,
  testId,
  items,
  active,
}: {
  label: string;
  testId: string;
  items: readonly SubNavItem<T>[];
  active: T;
}): JSX.Element {
  return (
    <nav
      aria-label={label}
      data-testid={testId}
      className="flex gap-1 self-start rounded-md bg-muted p-1"
    >
      {items.map((item) => {
        const isActive = item.tab === active;
        return (
          <a
            key={item.tab}
            href={item.href}
            aria-current={isActive ? "page" : undefined}
            className={
              "rounded-sm px-3 py-1.5 text-sm no-underline transition-colors " +
              (isActive
                ? "bg-primary text-primary-foreground"
                : "text-foreground hover:bg-background/60")
            }
          >
            {item.label}
          </a>
        );
      })}
    </nav>
  );
}

function ReportsSection({ tab }: { tab: ReportTab }): JSX.Element {
  // The reports sub-nav is rendered above the active report pane.
  // Plain anchors (not Tabs) keep the route the source of truth so
  // deep links + back-button work without an extra controlled state.
  // No leaderboard affordance anywhere — by design (§4).
  return (
    <div className="grid gap-4">
      <SubNav label="Reports" testId="reports-subnav" items={REPORT_TABS} active={tab} />
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
    <div className="grid gap-4">
      <SubNav label="Directory" testId="directory-subnav" items={DIRECTORY_TABS} active={tab} />
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
    <div className="grid gap-4">
      <SubNav label="Admin" testId="admin-subnav" items={ADMIN_TABS} active={tab} />
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
