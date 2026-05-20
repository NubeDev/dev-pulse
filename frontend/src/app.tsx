/**
 * dev-pulse SPA root.
 *
 * Stage 3 wires:
 *   - `<AuthProvider>` from `@nube/starter-ui-core/auth` with the
 *     session strategy (cookie login at `POST /auth/login`).
 *   - A hash-based route switch: `#/login` shows the login form,
 *     anything else falls through to the `<AppShell>` behind the
 *     `<ProtectedRoute>` gate.
 *   - Per-section placeholder panes (Reports / Directory / Admin); the
 *     real `§11.5` report pages land in stage 4+ and replace the
 *     `ReportsHome` body.
 */

import { useAuth, AuthProvider } from "@nube/starter-ui-core/auth";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@nube/starter-ui-kit/components/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@nube/starter-ui-kit/components/tabs";

import { api } from "./api/client.js";
import { authStrategy } from "./auth/strategy.js";
import { LoginPage } from "./auth/login-page.jsx";
import { ProtectedRoute } from "./auth/protected-route.jsx";
import { AppShell } from "./layout/app-shell.jsx";
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
      return <ReportsHome />;
    case "directory":
      return <DirectoryHome />;
    case "admin":
      return <AdminHome />;
  }
}

function ReportsHome(): JSX.Element {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Reports</CardTitle>
        <CardDescription>
          Headline + table + trend per SCOPE §11.5, with the three-lens toggle (§8.1).
          Real pages land in stage 4+.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <Tabs defaultValue="single">
          <TabsList>
            <TabsTrigger value="single">Single org</TabsTrigger>
            <TabsTrigger value="combined">All orgs combined</TabsTrigger>
            <TabsTrigger value="split">Per-org split</TabsTrigger>
          </TabsList>
          <TabsContent value="single">
            <p style={{ color: "var(--muted-foreground)" }}>Single-org lens placeholder.</p>
          </TabsContent>
          <TabsContent value="combined">
            <p style={{ color: "var(--muted-foreground)" }}>All-orgs-combined lens placeholder.</p>
          </TabsContent>
          <TabsContent value="split">
            <p style={{ color: "var(--muted-foreground)" }}>Per-org-split lens placeholder.</p>
          </TabsContent>
        </Tabs>
      </CardContent>
    </Card>
  );
}

function DirectoryHome(): JSX.Element {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Directory</CardTitle>
        <CardDescription>Users, orgs, teams. Stage 4+ wires the listings.</CardDescription>
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
        <CardDescription>Refresh, run log, GDPR export / anonymise. Stage 4+.</CardDescription>
      </CardHeader>
      <CardContent>
        <p style={{ color: "var(--muted-foreground)" }}>Admin placeholder.</p>
      </CardContent>
    </Card>
  );
}
