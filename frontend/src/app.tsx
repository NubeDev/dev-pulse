/**
 * dev-pulse SPA root.
 *
 * The dashboard-01 shell (AppShell) renders the sidebar + section
 * sub-nav + header; pages render in the inset body. No hand-rolled
 * subnav strip — the sidebar's `NavMain` sub-items carry the
 * section testids the smoke suite asserts on.
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
  const protectedSection = section === "login" ? "reports" : section;
  return (
    <ProtectedRoute>
      <AppShell>
        <ErrorBoundary scope={protectedSection} resetKey={route}>
          <SectionPane section={protectedSection} route={route} />
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
      return <ReportsPane tab={reportTabOf(route)} />;
    case "directory":
      return <DirectoryPane tab={directoryTabOf(route)} />;
    case "admin":
      return <AdminPane tab={adminTabOf(route)} />;
  }
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
