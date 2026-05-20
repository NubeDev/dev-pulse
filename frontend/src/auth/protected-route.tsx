/**
 * Protected-route gate.
 *
 * Wraps a subtree and:
 *   - shows a loading shim while `<AuthProvider>` is probing `/auth/me`;
 *   - redirects to `#/login` if the probe came back unauthenticated;
 *   - otherwise renders children.
 *
 * The gate is purely client-side — dp-rest's `with_principal` layer
 * still enforces auth on every protected route, so a user that bypasses
 * the redirect (e.g. by typing `#/reports` after the cookie expired)
 * will see 401s from the API but no leaked data.
 */

import { useEffect, type ReactNode } from "react";
import { useAuth } from "@nube/starter-ui-core/auth";

import { navigate } from "../routes.js";

export interface ProtectedRouteProps {
  children: ReactNode;
}

export function ProtectedRoute({ children }: ProtectedRouteProps): JSX.Element {
  const auth = useAuth();

  useEffect(() => {
    if (auth.status === "unauthenticated") {
      navigate("/login");
    }
  }, [auth.status]);

  if (auth.status === "loading") {
    return (
      <main
        style={{
          minHeight: "100dvh",
          display: "grid",
          placeItems: "center",
          color: "var(--muted-foreground)",
        }}
      >
        <p>Checking session…</p>
      </main>
    );
  }

  if (auth.status === "unauthenticated") {
    // The effect has already kicked off navigation; render nothing
    // for a tick so children with required-user assumptions don't
    // explode on a null `me`.
    return <></>;
  }

  return <>{children}</>;
}
