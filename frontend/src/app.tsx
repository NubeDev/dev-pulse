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
import { IdentitiesPage } from "./account/identities-page.jsx";
import { AdminUsersPage } from "./admin/users-page.jsx";
import { ProjectsPage as AdminProjectsPage } from "./admin/projects-page.jsx";
import { RefreshPage } from "./admin/refresh-page.jsx";
import { RunsPage } from "./admin/runs-page.jsx";
import { HomeOrgPage } from "./directory/home-org-page.jsx";
import { OrgsPage } from "./directory/orgs-page.jsx";
import { TeamsPage } from "./directory/teams-page.jsx";
import { UsersPage } from "./directory/users-page.jsx";
import { FreshnessPage } from "./reports/freshness-page.jsx";
import { HomeOrgSplitReportPage } from "./reports/home-org-split-report-page.jsx";
import { LeaderboardPage } from "./reports/leaderboard-page.jsx";
import { OrgReportPage } from "./reports/org-report-page.jsx";
import { RepoActivityPage } from "./reports/repo-activity-page.jsx";
import { TeamReportPage } from "./reports/team-report-page.jsx";
import { UserReportPage } from "./reports/user-report-page.jsx";
import { IssuesPage } from "./workflow/issues-page.jsx";
import { ReposPage } from "./workflow/repos-page.jsx";
import { TriagePage } from "./workflow/triage-page.jsx";
import { ProjectsPage } from "./projects/projects-page.jsx";
import { ProjectDetailPage } from "./projects/project-detail-page.jsx";
import {
  accountTabOf,
  adminTabOf,
  directoryTabOf,
  isKnownRoute,
  isLoginRoute,
  projectDetailIdOf,
  reportTabOf,
  sectionOf,
  useRoute,
  workflowTabOf,
  type AccountTab,
  type AdminTab,
  type DirectoryTab,
  type ReportTab,
  type WorkflowTab,
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
  section: "reports" | "directory" | "admin" | "workflow" | "projects" | "account";
  route: string;
}): JSX.Element {
  switch (section) {
    case "reports":
      return <ReportsPane tab={reportTabOf(route)} />;
    case "directory":
      return <DirectoryPane tab={directoryTabOf(route)} />;
    case "admin":
      return <AdminPane tab={adminTabOf(route)} />;
    case "workflow":
      return <WorkflowPane tab={workflowTabOf(route)} />;
    case "projects": {
      const id = projectDetailIdOf(route);
      return id ? <ProjectDetailPage projectId={id} /> : <ProjectsPage />;
    }
    case "account":
      return <AccountPane tab={accountTabOf(route)} />;
  }
}

function AccountPane({ tab }: { tab: AccountTab }): JSX.Element {
  switch (tab) {
    case "identities":
      return <IdentitiesPage />;
  }
}

function WorkflowPane({ tab }: { tab: WorkflowTab }): JSX.Element {
  switch (tab) {
    case "triage":
      return <TriagePage />;
    case "repos":
      return <ReposPage />;
    case "issues":
      return <IssuesPage />;
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
    case "leaderboard":
      return <LeaderboardPage />;
    case "repo-activity":
      return <RepoActivityPage />;
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
    case "project-sync":
      return <AdminProjectsPage />;
  }
}
