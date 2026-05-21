import { defineConfig, devices } from "@playwright/test";

/**
 * "Live" Playwright config — drives the built SPA served by
 * `make start` on http://localhost:8732 against the real Rust
 * backend on :8731 (proxied via the Vite dev server in dev mode,
 * or via nginx in the production frontend container).
 *
 * Unlike `playwright.config.ts`, this config:
 *   - does NOT start a Vite dev server (assumes `make start` already
 *     wired the backend + frontend pair, log to `.run/logs/*.log`).
 *   - does NOT set VITE_USE_MOCK_REPORTS — every request hits the
 *     live REST surface, so the SPA's react-query cache reflects
 *     the on-disk Postgres + auth state.
 *   - logs in with the real seeded dev account (dev@dev.com /
 *     dev123456789, see Makefile + users.md) so the §3.10 issue
 *     date editor's mirror banner is exercised against the real
 *     `dp_issue_dates` + `dp_project_board_links` schema.
 *
 * Run with: `pnpm --filter dev-pulse-frontend exec playwright test \
 *   --config playwright.live.config.ts`
 */
export default defineConfig({
  testDir: "./tests/e2e",
  testMatch: /live\..*\.spec\.ts/,
  timeout: 60_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: process.env.CI ? "github" : "list",
  use: {
    baseURL: process.env.DP_FRONTEND_URL ?? "http://localhost:8732",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "retain-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
