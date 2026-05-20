// Helpers for the dev-pulse Playwright smoke suite (stage 11).
//
// The Vite dev server is booted with VITE_USE_MOCK_REPORTS=1 so every
// report / directory / admin query reads from in-memory fixtures.
// Auth still flows through the real `sessionStrategy` (POST /auth/login
// + GET /auth/me) — we intercept those two paths here so the tests
// don't need a Rust backend.

import type { Page, Route } from "@playwright/test";

interface AuthOptions {
  /** Start signed-in: `/auth/me` returns a MeResponse before the
   *  AuthProvider's first probe. Default: false (unauthenticated). */
  readonly preAuthenticated?: boolean;
}

interface SessionState {
  authenticated: boolean;
}

/** Install per-test stubs for `/auth/login` and `/auth/me`. The login
 *  handler flips an in-test cookie + `/auth/me` returns 200 thereafter. */
export async function stubAuth(page: Page, opts: AuthOptions = {}): Promise<SessionState> {
  const state: SessionState = { authenticated: !!opts.preAuthenticated };

  await page.route("**/auth/me", async (route: Route) => {
    if (state.authenticated) {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          subject: "operator-1",
          email: "operator@example.com",
          role: "admin",
        }),
      });
    } else {
      // Match dp-rest's "no session" response — sessionStrategy treats
      // a 401 from /auth/me as `unauthenticated`.
      await route.fulfill({
        status: 401,
        contentType: "application/json",
        body: JSON.stringify({ type: "unauthorized", detail: "no session" }),
      });
    }
  });

  await page.route("**/auth/login", async (route: Route) => {
    state.authenticated = true;
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      headers: {
        // session cookie shape the dp-server emits — value is opaque
        // because the stubbed /auth/me reads in-memory state, not the
        // cookie itself, but we set it so the SPA's CSRF reader is happy.
        "set-cookie": "sas_session=stub; Path=/; HttpOnly; SameSite=Lax",
      },
      body: JSON.stringify({ csrf_token: "stub-csrf-token" }),
    });
  });

  await page.route("**/auth/logout", async (route: Route) => {
    state.authenticated = false;
    await route.fulfill({ status: 200, contentType: "application/json", body: "{}" });
  });

  return state;
}

/** Drive the login form to completion. Assumes `stubAuth(page)` has
 *  already been called. */
export async function signIn(page: Page): Promise<void> {
  await page.goto("/#/login");
  await page.getByLabel("Email").fill("operator@example.com");
  await page.getByLabel("Password").fill("password-stub");
  await page.getByRole("button", { name: /sign in/i }).click();
}
