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
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@nube/starter-ui-kit/components/card";

import { api } from "./api/client.js";
import { authStrategy } from "./auth/strategy.js";
import { LoginPage } from "./auth/login-page.jsx";
import { ProtectedRoute } from "./auth/protected-route.jsx";
import { AppShell } from "./layout/app-shell.jsx";
import { UserReportPage } from "./reports/user-report-page.jsx";
import { isLoginRoute, sectionOf, useRoute } from "./routes.js";

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
            <SectionPane section="reports" />
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
        <SectionPane section={proteced} />
      </AppShell>
    </ProtectedRoute>
  );
}

function SectionPane({ section }: { section: "reports" | "directory" | "admin" }): JSX.Element {
  switch (section) {
    case "reports":
      return <UserReportPage />;
    case "directory":
      return <DirectoryHome />;
    case "admin":
      return <AdminHome />;
  }
}

function DirectoryHome(): JSX.Element {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Directory</CardTitle>
        <CardDescription>Users, orgs, teams. Stage 5+ wires the listings.</CardDescription>
      </CardHeader>
      <CardContent>
        <p style={{ color: "var(--muted-foreground)" }}>Directory placeholder.</p>
      </CardContent>
    </Card>
  );
}

function AdminHome(): JSX.Element {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Admin</CardTitle>
        <CardDescription>Refresh, run log, GDPR export / anonymise. Stage 5+.</CardDescription>
      </CardHeader>
      <CardContent>
        <p style={{ color: "var(--muted-foreground)" }}>Admin placeholder.</p>
      </CardContent>
    </Card>
  );
}
