// Project saved-views regression suite (Slice 4 / §5.4).
//
// Locks in the fix for "save a new view → tab doesn't appear":
//
//   1. `useCreateProjectView`'s hook `onSuccess` invalidates the
//      views query → starts a refetch (`isFetching=true`, but
//      `isPending=false` because the cache still holds the OLD
//      list).
//   2. The call-site `onSuccess` does `patchUrl({ view: v.id })`.
//   3. React re-renders before the refetch returns. `activeView`
//      is `null` because the new id isn't in the cached list yet.
//   4. Pre-fix, the stale-`?view=` recovery branch in
//      [`project-workbench.tsx`] checked only `!isPending` and
//      so fired during the refetch, bouncing the URL back to
//      `view=null` (the pinned "All" tab). The user perceived
//      the saved view as missing and clicked Save again — every
//      retry hit the server's `view_name_taken` 409 because the
//      first POST actually had landed.
//
// The fix adds `!isFetching` to the recovery guard. This test
// drives the wizard end-to-end against stubbed `/projects/**`
// routes and asserts that the new tab appears + is the active
// tab after a single Save click.
//
// All `/projects/**` + `/tags` routes are stubbed in-test with a
// stateful in-memory store; auth is `stubAuth({ preAuthenticated
// : true })` so the workbench mounts immediately.

import { expect, test, type Page, type Route } from "@playwright/test";
import { stubAuth } from "./helpers";

// The repo-wide `playwright.config.ts` targets :5173, which on a
// dev box is often already taken by another project's Vite (then
// `reuseExistingServer` silently lands the suite on the wrong
// app). Pin every test in this file to dev-pulse's strict-port
// dev server (:8732, launched by `make start`) — all network is
// stubbed via `page.route` below so we're not coupled to the
// live backend on :8731.
test.use({ baseURL: process.env.DP_FRONTEND_URL ?? "http://localhost:8732" });

const ORG_ID = "11111111-1111-1111-1111-111111111111";
const USER_ID = "22222222-2222-2222-2222-222222222222";
const PROJECT_ID = "33333333-3333-3333-3333-333333333333";

interface StoredView {
  id: string;
  project_id: string;
  owner_user_id: string;
  name: string;
  group_by: string | null;
  filter_clauses: unknown[];
  sort: string;
  position: number;
  visibility: string;
  start_date: string | null;
  due_date: string | null;
  categories: string[];
  created_at: string;
  updated_at: string;
  open_issue_count: number;
  total_issue_count: number;
}

interface MockStore {
  views: StoredView[];
}

function newId(seed: number): string {
  const hex = seed.toString(16).padStart(8, "0");
  return `${hex}-aaaa-aaaa-aaaa-aaaaaaaaaaaa`;
}

async function installProjectStubs(page: Page, store: MockStore): Promise<void> {
  const now = "2026-05-22T00:00:00Z";

  // ── App-shell sidecar endpoints ─────────────────────────────────
  // The sidebar/header fires these on every page; if they 401 the
  // AuthProvider can flip the user back to /login. Return empty
  // payloads so the shell renders and our subject-under-test (the
  // workbench) gets to mount.
  await page.route("**/me/pins", async (route: Route) => {
    if (route.request().method() !== "GET") return route.fallback();
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: "[]",
    });
  });
  await page.route("**/me/app-install-banner", async (route: Route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ dismissed: true }),
    });
  });
  await page.route("**/me/queue*", async (route: Route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ rows: [], total: 0, limit: 1, offset: 0 }),
    });
  });

  // GET /tags — empty by default; the wizard's category step is
  // optional and we never exercise it here. Match the bare path
  // (with optional query) and NOT Vite module URLs like
  // `/src/.../tags-page.tsx?t=…`.
  await page.route(/\/tags(\?[^/]*)?$/, async (route: Route) => {
    if (route.request().method() !== "GET") return route.fallback();
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: "[]",
    });
  });

  // GET /projects (list) — sidebar / breadcrumb counters fire with
  // `?status=…&count_only=1`. Match both shapes.
  await page.route(/\/projects(\?[^/]*)?$/, async (route: Route) => {
    if (route.request().method() !== "GET") return route.fallback();
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        rows: [],
        total: 0,
        limit: 50,
        offset: 0,
      }),
    });
  });

  // GET /projects/{id}
  await page.route(`**/projects/${PROJECT_ID}`, async (route: Route) => {
    if (route.request().method() !== "GET") return route.fallback();
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        id: PROJECT_ID,
        org_id: ORG_ID,
        name: "Repro",
        description: null,
        lead_user_id: null,
        status: "active",
        start_at: null,
        due_at: null,
        issue_count: 0,
        closed_issue_count: 0,
        board_link_count: 0,
        version: 1,
        created_by: USER_ID,
        created_at: now,
        updated_at: now,
        primary_milestone_id: null,
      }),
    });
  });

  // Per-project sub-collections we don't exercise — return empty
  // payloads with the shape each hook expects so its zod parse
  // doesn't throw and trap the page in a loading state.
  await page.route(
    `**/projects/${PROJECT_ID}/issues*`,
    async (route: Route) => {
      if (route.request().method() !== "GET") return route.fallback();
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ rows: [], total: 0, limit: 100, offset: 0 }),
      });
    },
  );
  await page.route(
    `**/projects/${PROJECT_ID}/group-by-options`,
    async (route: Route) => {
      if (route.request().method() !== "GET") return route.fallback();
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ dims: [] }),
      });
    },
  );
  await page.route(
    `**/projects/${PROJECT_ID}/milestones*`,
    async (route: Route) => {
      if (route.request().method() !== "GET") return route.fallback();
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: "[]",
      });
    },
  );
  await page.route(
    `**/projects/${PROJECT_ID}/repos*`,
    async (route: Route) => {
      if (route.request().method() !== "GET") return route.fallback();
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: "[]",
      });
    },
  );
  await page.route(
    `**/projects/${PROJECT_ID}/board-links*`,
    async (route: Route) => {
      if (route.request().method() !== "GET") return route.fallback();
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: "[]",
      });
    },
  );

  // Views CRUD — the actual subject under test. Single handler
  // dispatches on (method, sub-path).
  await page.route(
    new RegExp(`/projects/${PROJECT_ID}/views(/[^?]+)?(\\?.*)?$`),
    async (route: Route) => {
      const req = route.request();
      const url = new URL(req.url());
      // Sub-path after `/views`. Empty = collection, `/<id>` = item.
      const subpath = url.pathname.replace(
        new RegExp(`.*/projects/${PROJECT_ID}/views`),
        "",
      );

      if (req.method() === "GET" && subpath === "") {
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify(
            [...store.views]
              .sort((a, b) => a.position - b.position)
              .map((v, i) => ({ ...v, position: i })),
          ),
        });
        return;
      }

      if (req.method() === "POST" && subpath === "") {
        const body = JSON.parse(req.postData() ?? "{}") as {
          name: string;
          group_by: string | null;
          filter_clauses: unknown[];
          sort: string;
          start_date?: string | null;
          due_date?: string | null;
          categories?: string[];
        };
        const dup = store.views.find((v) => v.name === body.name);
        if (dup) {
          // Match the real server error shape so the SPA's
          // `DpRestError` extraction lights up correctly.
          await route.fulfill({
            status: 409,
            contentType: "application/json",
            body: JSON.stringify({
              error: "a view with that name already exists",
              code: "view_name_taken",
            }),
          });
          return;
        }
        const id = newId(store.views.length + 1);
        const view: StoredView = {
          id,
          project_id: PROJECT_ID,
          owner_user_id: USER_ID,
          name: body.name,
          group_by: body.group_by ?? null,
          filter_clauses: body.filter_clauses ?? [],
          sort: body.sort,
          position: store.views.length,
          visibility: "private",
          start_date: body.start_date ?? null,
          due_date: body.due_date ?? null,
          categories: body.categories ?? [],
          created_at: now,
          updated_at: now,
          open_issue_count: 0,
          total_issue_count: 0,
        };
        store.views.push(view);
        await route.fulfill({
          status: 201,
          contentType: "application/json",
          body: JSON.stringify(view),
        });
        return;
      }

      await route.fallback();
    },
  );
}

test.describe("project saved views", () => {
  test.beforeEach(async ({ page }) => {
    await stubAuth(page, { preAuthenticated: true });
  });

  test("saving a new view immediately shows + activates its tab", async ({
    page,
  }) => {
    const store: MockStore = { views: [] };
    await installProjectStubs(page, store);

    // Track every POST /views — pre-fix the user clicked Save many
    // times because nothing visible happened, producing a cascade
    // of `view_name_taken` 409s. Post-fix exactly one POST should
    // fire for a single Save click.
    const createPosts: string[] = [];
    page.on("request", (req) => {
      if (
        req.method() === "POST" &&
        req.url().endsWith(`/projects/${PROJECT_ID}/views`)
      ) {
        createPosts.push(req.url());
      }
    });

    await page.goto(`/#/projects/${PROJECT_ID}`);

    // Strip mounts even with zero saved views (pinned "All" + "+").
    await expect(page.getByTestId("project-views-tab-strip")).toBeVisible();
    await expect(page.getByTestId("project-view-new")).toBeVisible();

    // Open the wizard → custom template → name → submit.
    await page.getByTestId("project-view-new").click();
    await expect(page.getByTestId("project-view-wizard")).toBeVisible();
    await page.getByTestId("project-view-template-custom").click();
    await page.getByTestId("project-view-wizard-next").click();
    await page.getByTestId("project-view-name-input").fill("aaa");
    await page.getByTestId("project-view-wizard-submit").click();

    // The new tab must render — this is the regression. Pre-fix
    // the URL-stale-recovery branch fired during the post-create
    // refetch and the tab never appeared.
    const newId = `${(1).toString(16).padStart(8, "0")}-aaaa-aaaa-aaaa-aaaaaaaaaaaa`;
    const newTab = page.getByTestId(`project-view-tab-${newId}`);
    await expect(newTab).toBeVisible();
    await expect(newTab).toContainText("aaa");

    // …and it must be the active tab, not bounced back to "All".
    await expect(newTab).toHaveAttribute("data-active", "true");

    // Exactly one POST — no retry storm.
    expect(createPosts).toHaveLength(1);
    expect(store.views.map((v) => v.name)).toEqual(["aaa"]);
  });

  test("server 409 surfaces without losing earlier saved views", async ({
    page,
  }) => {
    // Pre-seed one view so the GET response is non-empty; then try
    // to create a second with the same name and assert the 409
    // doesn't wipe the existing tab from the strip.
    const seededId = newId(1);
    const store: MockStore = {
      views: [
        {
          id: seededId,
          project_id: PROJECT_ID,
          owner_user_id: USER_ID,
          name: "aaa",
          group_by: null,
          filter_clauses: [],
          sort: "updated_desc",
          position: 0,
          visibility: "private",
          start_date: null,
          due_date: null,
          categories: [],
          created_at: "2026-05-22T00:00:00Z",
          updated_at: "2026-05-22T00:00:00Z",
          open_issue_count: 0,
          total_issue_count: 0,
        },
      ],
    };
    await installProjectStubs(page, store);

    await page.goto(`/#/projects/${PROJECT_ID}`);

    // The seeded tab is visible from first paint.
    await expect(
      page.getByTestId(`project-view-tab-${seededId}`),
    ).toBeVisible();

    // Drive the wizard with the colliding name.
    await page.getByTestId("project-view-new").click();
    await page.getByTestId("project-view-template-custom").click();
    await page.getByTestId("project-view-wizard-next").click();
    await page.getByTestId("project-view-name-input").fill("aaa");
    await page.getByTestId("project-view-wizard-submit").click();

    // The seeded tab survives the failed POST.
    await expect(
      page.getByTestId(`project-view-tab-${seededId}`),
    ).toBeVisible();
    // Server state is unchanged — exactly one view, the seed.
    expect(store.views).toHaveLength(1);
  });
});
