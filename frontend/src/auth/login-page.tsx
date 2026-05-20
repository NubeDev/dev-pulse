/**
 * Login page for dev-pulse operators.
 *
 * Stage 3: session login via `POST /auth/login` (email + password). The
 * matching local user row is seeded out-of-band by the Phase 6 CLI's
 * `claim` command. GitHub OAuth is a separate Phase 6 follow-up; once
 * wired the page will gain a "Continue with GitHub" button that hits
 * `GET /auth/oauth/github/login`.
 *
 * The form lives behind `useAuth().login`, so the AuthProvider owns
 * the state transition (status -> "authenticated") and the
 * `ProtectedRoute` gate flips automatically once login resolves.
 *
 * Stage 2 (phase-7-frontend-polish): rebuilt on shadcn `Card` +
 * Tailwind utilities. No inline `style={{}}` remain.
 */

import { useState, type FormEvent } from "react";
import { useAuth } from "@nube/starter-ui-core/auth";
import {
  Alert,
  AlertDescription,
} from "@nube/starter-ui-kit/components/alert";
import { Button } from "@nube/starter-ui-kit/components/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@nube/starter-ui-kit/components/card";
import { Input } from "@nube/starter-ui-kit/components/input";
import { Label } from "@nube/starter-ui-kit/components/label";
import { Separator } from "@nube/starter-ui-kit/components/separator";

import { navigate } from "../routes.js";

export function LoginPage(): JSX.Element {
  const auth = useAuth();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  async function onSubmit(e: FormEvent<HTMLFormElement>): Promise<void> {
    e.preventDefault();
    setError(null);
    setSubmitting(true);
    try {
      await auth.login({ kind: "credentials", email, password });
      // After a successful login, drop into the default protected
      // section. The ProtectedRoute gate will pass because the
      // AuthProvider has flipped `status` to "authenticated".
      navigate("/reports");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <main className="grid min-h-dvh place-items-center bg-background p-8">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle>dev-pulse</CardTitle>
          <CardDescription>
            Sign in with your operator credentials.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={onSubmit} className="grid gap-4">
            <div className="grid gap-2">
              <Label htmlFor="login-email">Email</Label>
              <Input
                id="login-email"
                type="email"
                autoComplete="username"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                required
                disabled={submitting}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="login-password">Password</Label>
              <Input
                id="login-password"
                type="password"
                autoComplete="current-password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
                disabled={submitting}
              />
            </div>
            {error && (
              <Alert variant="destructive" role="alert">
                <AlertDescription>{error}</AlertDescription>
              </Alert>
            )}
            <Separator />
            <Button type="submit" disabled={submitting || !email || !password}>
              {submitting ? "Signing in…" : "Sign in"}
            </Button>
          </form>
        </CardContent>
      </Card>
    </main>
  );
}
