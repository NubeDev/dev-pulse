// dev-pulse SPA smoke walkthrough (stage 11).
//
// Drives the seven things the stage-11 brief calls out:
//   1. login flow
//   2. navigate to user report
//   3. toggle lens
//   4. change window
//   5. verify data_as_of displays
//   6. navigate to admin
//   7. trigger refresh + freshness updates
//
// Data is served from the in-process mock fixtures
// (VITE_USE_MOCK_REPORTS=1, set in playwright.config.ts). Auth is
// stubbed via page.route() — see helpers.ts.

import { expect, test } from "@playwright/test";
import { signIn, stubAuth } from "./helpers";

test.describe("dev-pulse smoke", () => {
  test.beforeEach(async ({ page }) => {
    await stubAuth(page);
  });

  test("login flow lands on the reports section", async ({ page }) => {
    await page.goto("/#/login");
    // The login card title renders the product name; assert on its
    // text rather than a heading role (CardTitle is a styled div).
    await expect(page.getByText("dev-pulse", { exact: true })).toBeVisible();

    await page.getByLabel("Email").fill("operator@example.com");
    await page.getByLabel("Password").fill("password-stub");
    await page.getByRole("button", { name: /sign in/i }).click();

    // Lands on reports → app-shell + the reports sub-nav both render.
    await expect(page.getByTestId("app-shell")).toBeVisible();
    await expect(page.getByTestId("reports-subnav")).toBeVisible();
  });

  test("user report: lens toggle + window picker + data-as-of", async ({ page }) => {
    await signIn(page);

    // Wait for the user report to be the active pane.
    await expect(page.getByTestId("user-select")).toBeVisible();

    // The "Data as of" banner must render per SCOPE §0.3 — the mock
    // fixture seeds a headline so the banner is non-loading.
    const banner = page.getByTestId("data-as-of");
    await expect(banner).toBeVisible();
    await expect(banner).toContainText(/Data as of/i);

    // Headline summarises the table totals.
    await expect(page.getByTestId("headline").first()).toBeVisible();
    await expect(page.getByTestId("activity-table").first()).toBeVisible();

    // ---- Toggle the three-lens tabs (§8.1).
    // The default tab is "Single org"; flipping picks "All orgs combined".
    const allOrgsTab = page.getByRole("tab", { name: /All orgs combined/i });
    await allOrgsTab.click();
    await expect(allOrgsTab).toHaveAttribute("data-state", "active");

    const perOrgTab = page.getByRole("tab", { name: /Per-org split/i });
    await perOrgTab.click();
    await expect(perOrgTab).toHaveAttribute("data-state", "active");

    // ---- Change the window.
    // The picker uses Radix Select triggers; click the "Window" combobox
    // (the first one in the picker) and pick "Last 30 days".
    const windowTrigger = page.getByRole("combobox").filter({ hasText: /Last/i }).first();
    await windowTrigger.click();
    await page.getByRole("option", { name: "Last 30 days" }).click();
    await expect(windowTrigger).toContainText(/Last 30 days/);

    // The banner is still there after re-render.
    await expect(page.getByTestId("data-as-of")).toBeVisible();
  });

  test("admin refresh triggers a result", async ({ page }) => {
    await signIn(page);

    // Navigate via the primary nav to /admin.
    await page.locator('a[href="#/admin"]').first().click();
    await expect(page.getByTestId("admin-subnav")).toBeVisible();

    // The default admin tab is Runs; jump to Refresh.
    await page.locator('a[href="#/admin/refresh"]').click();
    await expect(page.getByTestId("refresh-trigger")).toBeVisible();

    // Trigger a refresh — the mock resolves with `ran: true`.
    await page.getByTestId("refresh-trigger").click();
    const result = page.getByTestId("refresh-result");
    await expect(result).toBeVisible();
    await expect(result).toHaveAttribute("data-ran", "true");
    await expect(page.getByTestId("refresh-items")).toBeVisible();
  });

  test("freshness dashboard renders headline + cards with mixed bands", async ({ page }) => {
    // Fresh sign-in (no other tab has populated the shared `["orgs"]`
    // query cache yet) so the freshness page's mockOrgs() drives the
    // org list and the seeded per-band timestamps line up.
    await signIn(page);

    // Navigate via the in-app reports sub-nav link rather than
    // page.goto — Playwright's goto won't re-fire when only the hash
    // changes, but the anchor click triggers a real hashchange.
    await page.locator('a[href="#/reports/freshness"]').click();
    await expect(page.getByTestId("freshness-headline")).toBeVisible();
    await expect(page.getByTestId("freshness-grid")).toBeVisible();
    const cards = page.getByTestId("freshness-card");
    expect(await cards.count()).toBeGreaterThanOrEqual(3);
    // The fixture seeds one card per band — at least one fresh + one
    // stale must be present.
    await expect(page.locator('[data-testid="freshness-card"][data-band="fresh"]')).toHaveCount(1);
    await expect(page.locator('[data-testid="freshness-card"][data-band="stale"]')).toHaveCount(1);
  });

  // Stage 7 visual-regression smoke. Every report page must render
  // through the shadcn primitives that the polish refactor introduced
  // — at least one `<Card>` (data-slot="card") and at least one
  // shadcn `<Tabs>` (data-slot="tabs-list"). If a future regression
  // strips a CardHeader or hand-rolls the lens toggle again, this
  // test trips immediately.
  //
  // Freshness is a single-view status dashboard (no lens toggle), so
  // it's checked for Card only — the Tabs requirement applies to the
  // four lens-bearing report pages (user, team, org, home-org-split).
  test("every report page renders with shadcn Card + Tabs primitives", async ({ page }) => {
    await signIn(page);

    const reportsWithTabs = [
      { href: "#/reports/user", label: "user" },
      { href: "#/reports/team", label: "team" },
      { href: "#/reports/org", label: "org" },
      { href: "#/reports/home-org-split", label: "home-org-split" },
    ] as const;

    for (const r of reportsWithTabs) {
      await page.locator(`a[href="${r.href}"]`).first().click();
      await expect(page.getByTestId("reports-subnav")).toBeVisible();
      const cards = page.locator('[data-slot="card"]');
      const tabsLists = page.locator('[data-slot="tabs-list"]');
      expect(await cards.count(), `${r.label}: <Card> count`).toBeGreaterThanOrEqual(1);
      expect(await tabsLists.count(), `${r.label}: <Tabs> count`).toBeGreaterThanOrEqual(1);
    }

    // Freshness — single-view dashboard, Card only.
    await page.locator('a[href="#/reports/freshness"]').click();
    await expect(page.getByTestId("freshness-headline")).toBeVisible();
    expect(
      await page.locator('[data-slot="card"]').count(),
      "freshness: <Card> count",
    ).toBeGreaterThanOrEqual(1);
  });

  test("directory pages render via the sub-nav", async ({ page }) => {
    await signIn(page);
    await page.locator('a[href="#/directory"]').first().click();
    await expect(page.getByTestId("directory-subnav")).toBeVisible();
    await page.locator('a[href="#/directory/orgs"]').click();
    // Orgs page renders a table-ish list — assert the sub-nav still
    // shows orgs as the active tab.
    await expect(page.locator('a[href="#/directory/orgs"][aria-current="page"]')).toBeVisible();
  });
});
