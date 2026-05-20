import { defineConfig, devices } from "@playwright/test";

/**
 * Stage 11 integration smoke tests.
 *
 * Boots the Vite dev server with `VITE_USE_MOCK_REPORTS=1` so every
 * report / directory / admin page renders from in-memory fixtures
 * without a running Rust backend. Auth is stubbed per-test via
 * `page.route()` against `/auth/login` and `/auth/me` (see
 * `tests/e2e/helpers.ts`), which keeps the test layer aligned with
 * the real fetch surface — the AuthProvider believes it's talking to
 * `dp-server`, just on an injected transport.
 *
 * Single browser (chromium); the smoke suite is deliberately small
 * (one walkthrough per surface + a no-leaderboard grep) and runs
 * locally + in CI without a browser matrix.
 */
export default defineConfig({
  testDir: "./tests/e2e",
  timeout: 30_000,
  expect: { timeout: 5_000 },
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? "github" : "list",
  use: {
    baseURL: "http://localhost:5173",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: {
    command: "pnpm dev --host 127.0.0.1 --port 5173",
    url: "http://localhost:5173",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
    env: {
      VITE_USE_MOCK_REPORTS: "1",
    },
  },
});
