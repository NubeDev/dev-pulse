import { test } from "@playwright/test";
test.use({ baseURL: "http://127.0.0.1:5179" });
test("zoom", async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 2200 });
  await page.goto("/_pdf-preview.html");
  await page.waitForLoadState("networkidle");
  await page.waitForTimeout(500);
  // crop to summary + scope + requirements regions
  const top = page.locator("section").first().locator("..");
  await top.screenshot({ path: "test-results/pdf-zoom.png" });
});
