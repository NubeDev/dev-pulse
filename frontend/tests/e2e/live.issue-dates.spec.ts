/**
 * Live e2e: §3.10 issue start/due date editor against the real
 * backend.
 *
 * Pre-conditions (asserted by the test up-front so failures point at
 * the missing setup rather than at the UI):
 *   - `make start` is up (backend on :8731, frontend on :8732).
 *   - The seeded dev admin (`dev@dev.com` / `dev123456789`) exists.
 *   - The `NubeDev/ai-ui` repo has been mirrored into `dp_repos` —
 *     this happens automatically on the first reconciler tick after
 *     the org install (see Makefile `make start` → migrate, plus the
 *     one-off `POST /admin/refresh?org_id=...&repo_id=...` the test
 *     fires below if no issue exists yet).
 *
 * What this verifies (top-down):
 *   1. Real auth: the dev account can sign in via the same login
 *      form the user uses (no `page.route()` stubs).
 *   2. The deep link `#/workflow/issues?repo_id=…&issue=…` opens
 *      the Sheet detail panel with the IssueDatesEditor mounted.
 *   3. Dates set via `PATCH /issues/{id}/dates` survive a full page
 *      reload (server is the source of truth, not the cache).
 *   4. Editing in the UI (`issue-dates-start` / `issue-dates-due` +
 *      "Save dates") round-trips through the live REST handler and
 *      the new value sticks after reload.
 *   5. The §3.10 mirror status microcopy renders in the expected
 *      lane: with no `dp_project_board_links` row for the test project,
 *      neither `issue-dates-mirror-synced` nor
 *      `issue-dates-mirror-error` should appear (server returns
 *      both `mirror_synced_at` and `mirror_error` null, by design).
 */

import { expect, request, test, type APIRequestContext } from "@playwright/test";

const BACKEND = process.env.DP_BACKEND_URL ?? "http://localhost:8731";
const FRONTEND = process.env.DP_FRONTEND_URL ?? "http://localhost:8732";
const EMAIL = process.env.DP_EMAIL ?? "dev@dev.com";
const PASSWORD = process.env.DP_PASSWORD ?? "dev123456789";

// The test rig hard-codes the `NubeDev/ai-ui` repo because the user
// nominated it as the date-mirror playground. If you need to swap
// repos, override via env vars.
const REPO_SLUG = process.env.DP_TEST_REPO ?? "NubeDev/ai-ui";

interface Ctx {
  api: APIRequestContext;
  csrf: string;
  repoId: string;
  orgId: string;
  issueId: string;
  issueNumber: number;
}

/** Format a `Date` as a YYYY-MM-DD string in UTC (matches the
 *  `<input type="date">` value format the editor reads/writes). */
function isoDate(d: Date): string {
  return d.toISOString().slice(0, 10);
}

/** Authenticated API client + repo / issue bootstrap. Creates an
 *  issue in `NubeDev/ai-ui` if none exists; idempotent across runs. */
async function bootstrap(): Promise<Ctx> {
  const api = await request.newContext({
    baseURL: BACKEND,
    extraHTTPHeaders: { "content-type": "application/json" },
  });

  // 1. Login — pulls back the CSRF token + Set-Cookie for the
  //    session. The session cookie lives in the APIRequestContext
  //    cookie jar from here on.
  const loginRes = await api.post("/auth/login", {
    data: { email: EMAIL, password: PASSWORD },
  });
  expect(loginRes.status(), `login failed: ${await loginRes.text()}`).toBe(200);
  const { csrf_token: csrf } = await loginRes.json();
  expect(csrf, "no csrf_token in /auth/login response").toBeTruthy();

  // 2. Resolve the repo by slug.
  const [orgLogin, repoName] = REPO_SLUG.split("/");
  const reposRes = await api.get(`/repos?org_login=${orgLogin}&limit=200`);
  expect(reposRes.ok(), `GET /repos failed`).toBeTruthy();
  const repos = (await reposRes.json()).rows as Array<{
    id: string;
    org_id: string;
    name: string;
    slug: string;
  }>;
  const repo = repos.find((r) => r.slug === REPO_SLUG || r.name === repoName);
  expect(
    repo,
    `repo ${REPO_SLUG} not mirrored into dp_repos — run \`make start\` + \`POST /admin/refresh\` first`,
  ).toBeTruthy();

  // 3. Find or create an issue we own.
  const issuesRes = await api.get(`/issues?repo_id=${repo!.id}&limit=20`);
  expect(issuesRes.ok()).toBeTruthy();
  let issues = (await issuesRes.json()).rows as Array<{
    id: string;
    number: number;
    title: string;
  }>;

  // Prefer an existing issue with our e2e marker in the title so we
  // don't create one per run.
  const marker = "E2E test: start/due date mirror";
  let issue = issues.find((i) => i.title.startsWith(marker));
  if (!issue) {
    const createRes = await api.post("/issues", {
      headers: { "x-csrf-token": csrf },
      data: {
        repo_id: repo!.id,
        title: marker,
        body: "Created by the live Playwright spec to verify the §3.10 IssueDatesEditor.",
      },
    });
    expect(
      createRes.ok(),
      `POST /issues failed: ${createRes.status()} ${await createRes.text()}`,
    ).toBeTruthy();
    // Force a reconcile so the new row materialises in dp_issues.
    const refRes = await api.post(
      `/admin/refresh?org_id=${repo!.org_id}&repo_id=${repo!.id}`,
      { headers: { "x-csrf-token": csrf } },
    );
    expect(refRes.ok()).toBeTruthy();
    const issuesRes2 = await api.get(`/issues?repo_id=${repo!.id}&limit=20`);
    issues = (await issuesRes2.json()).rows;
    issue = issues.find((i) => i.title.startsWith(marker));
    expect(
      issue,
      "issue did not materialise locally after admin/refresh",
    ).toBeTruthy();
  }

  return {
    api,
    csrf,
    repoId: repo!.id,
    orgId: repo!.org_id,
    issueId: issue!.id,
    issueNumber: issue!.number,
  };
}

test.describe("live · issue start/due dates · NubeDev/ai-ui", () => {
  let ctx: Ctx;

  test.beforeAll(async () => {
    ctx = await bootstrap();
    // Seed a known baseline so the UI has something to render the
    // first time we open the editor — independent of any prior run.
    const start = new Date();
    const due = new Date(Date.now() + 25 * 24 * 60 * 60 * 1000);
    const res = await ctx.api.patch(`/issues/${ctx.issueId}/dates`, {
      headers: { "x-csrf-token": ctx.csrf },
      data: {
        start_at: `${isoDate(start)}T00:00:00Z`,
        due_at: `${isoDate(due)}T23:59:59Z`,
      },
    });
    expect(res.ok(), `seed PATCH failed: ${await res.text()}`).toBeTruthy();
  });

  test.afterAll(async () => {
    await ctx?.api?.dispose();
  });

  test("reads back the seeded dates after login + deep link", async ({ page }) => {
    // ---- Real login (no stubs) ------------------------------------
    await page.goto(`${FRONTEND}/#/login`);
    await page.getByLabel("Email").fill(EMAIL);
    await page.getByLabel("Password").fill(PASSWORD);
    await page.getByRole("button", { name: /sign in/i }).click();
    await expect(page.getByTestId("app-shell")).toBeVisible();

    // ---- Deep-link straight into the Sheet ------------------------
    const deep = `${FRONTEND}/#/workflow/issues?repo_id=${ctx.repoId}&issue=${ctx.issueId}`;
    await page.goto(deep);

    // Sheet + editor must mount.
    const editor = page.getByTestId("issue-dates-editor");
    await expect(editor).toBeVisible();

    // ---- Seeded values land in the inputs -------------------------
    const start = page.getByTestId("issue-dates-start");
    const due = page.getByTestId("issue-dates-due");
    // Wait for the GET /issues/{id}/dates response to seed the inputs.
    await expect(start).not.toHaveValue("");
    await expect(due).not.toHaveValue("");

    // Sanity: the due date must be strictly later than the start.
    const startVal = await start.inputValue();
    const dueVal = await due.inputValue();
    expect(startVal < dueVal).toBeTruthy();

    // ---- Mirror lane — no project link, so neither synced nor
    // error footnote should render.
    await expect(page.getByTestId("issue-dates-mirror-synced")).toHaveCount(0);
    await expect(page.getByTestId("issue-dates-mirror-error")).toHaveCount(0);

    // Visual artefact for the human reviewer.
    await page.screenshot({
      path: "tests/e2e/.artefacts/live-dates-seeded.png",
      fullPage: true,
    });
  });

  test("editing dates in the UI round-trips through PATCH /issues/{id}/dates", async ({
    page,
  }) => {
    await page.goto(`${FRONTEND}/#/login`);
    await page.getByLabel("Email").fill(EMAIL);
    await page.getByLabel("Password").fill(PASSWORD);
    await page.getByRole("button", { name: /sign in/i }).click();
    await expect(page.getByTestId("app-shell")).toBeVisible();

    const deep = `${FRONTEND}/#/workflow/issues?repo_id=${ctx.repoId}&issue=${ctx.issueId}`;
    await page.goto(deep);

    const start = page.getByTestId("issue-dates-start");
    const due = page.getByTestId("issue-dates-due");
    await expect(start).toBeVisible();
    await expect(due).toBeVisible();

    // Pick a new due date 50 days out — distinguishable from the
    // 25-day seed so a no-op handler would fail the post-reload check.
    const newDue = isoDate(new Date(Date.now() + 50 * 24 * 60 * 60 * 1000));
    await due.fill(newDue);
    await page.getByTestId("issue-dates-save").click();

    // The button toggles to "Saving…" mid-flight and back. Wait for
    // it to settle before we reload.
    await expect(page.getByTestId("issue-dates-save")).toHaveText(
      /save dates/i,
    );

    // ---- Reload to drop the react-query cache and re-fetch from
    // the live backend. The new value must persist.
    await page.reload();
    await page.goto(deep);
    await expect(page.getByTestId("issue-dates-due")).toHaveValue(newDue);

    // Confirm the server agrees.
    const res = await ctx.api.get(`/issues/${ctx.issueId}/dates`);
    const dto = await res.json();
    expect(dto.due_at.slice(0, 10)).toBe(newDue);

    await page.screenshot({
      path: "tests/e2e/.artefacts/live-dates-edited.png",
      fullPage: true,
    });
  });
});
