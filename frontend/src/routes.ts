/**
 * Minimal hash-based router for the dev-pulse SPA.
 *
 * No react-router dependency — the app has a flat route table (login +
 * three protected sections, each with its own page index) and a hash
 * router fits in 30 lines + survives a static-asset deploy (no server
 * rewrite required).
 *
 * Route shape:
 *   #/login                       — login page
 *   #/reports                     — protected: reports landing (= user report)
 *   #/reports/user[/:user_id]     — user activity report
 *   #/reports/team[/:team_id]     — team activity report
 *   #/reports/org[/:org_id]       — org activity report
 *   #/reports/home-org-split      — cross-company exec view
 *   #/reports/freshness           — per-org data freshness dashboard
 *   #/directory                   — protected: directory landing (= users)
 *   #/directory/users             — users list (search + org filter)
 *   #/directory/orgs              — orgs list with member count
 *   #/directory/teams             — teams list (filtered by org)
 *   #/directory/home-org          — home-org assignment UI
 *   #/admin[/...]                 — protected: admin
 *   #/                            — alias for #/reports
 */

import { useSyncExternalStore } from "react";

export type Section =
  | "reports"
  | "directory"
  | "admin"
  | "workflow"
  | "projects"
  | "products"
  | "manufacturing"
  | "customers"
  | "runs"
  | "units"
  | "rma"
  | "account"
  | "login";

/** §6.1 sidebar status filter for `#/projects?status=…`. The value
 *  round-trips through copy-paste so a deep link to "Backlog" lands
 *  the right §6.2 grouping. `null` ⇒ render every status (the §6.2
 *  default landing view). */
export type ProjectStatusRoute =
  | "active"
  | "backlog"
  | "done"
  | "archived";

export function projectsStatusOf(route: string): ProjectStatusRoute | null {
  const q = route.indexOf("?");
  if (q < 0) return null;
  const v = new URLSearchParams(route.slice(q + 1)).get("status");
  switch (v) {
    case "active":
    case "backlog":
    case "done":
    case "archived":
      return v;
    default:
      return null;
  }
}

/** Build a `#/projects` URL with an optional status filter. */
export function projectsRoute(status?: ProjectStatusRoute | null): string {
  if (!status) return "#/projects";
  return `#/projects?status=${status}`;
}

/** Parse `#/projects/{id}` → the project id (UUID), or `null` for
 *  the list landing (`#/projects` / `#/projects?status=…`). The
 *  detail page is opened by §6.3 once a project exists; slice B
 *  uses it as the host for the Link-a-board dialog and the
 *  per-link mirror status rows. */
export function projectDetailIdOf(route: string): string | null {
  const q = route.indexOf("?");
  const pathPart = q < 0 ? route : route.slice(0, q);
  const parts = pathPart.replace(/^#/, "").replace(/^\/+/, "").split("/");
  if (parts[0] !== "projects") return null;
  const id = parts[1];
  if (!id) return null;
  // Crude UUID guard — anything else falls back to the list page
  // so a typo doesn't land in detail-with-no-data.
  if (!/^[0-9a-fA-F-]{36}$/.test(id)) return null;
  return id;
}

/** Build a `#/projects/{id}` URL, optionally with `?issue=<uuid>`. */
export function projectDetailRoute(id: string, issueId?: string | null): string {
  if (issueId) return `#/projects/${id}?issue=${issueId}`;
  return `#/projects/${id}`;
}

/** Parse `?issue=<uuid>` from a `#/projects/{id}?issue=…` route. */
export function projectSelectedIssue(route: string): string | null {
  const q = route.indexOf("?");
  if (q < 0) return null;
  const params = new URLSearchParams(route.slice(q + 1));
  const id = params.get("issue");
  return id && id.length > 0 ? id : null;
}

/** Parse `?group=<dim>` from a `#/projects/{id}?group=…` route
 *  (PROJECT-VIEW.md §5.4). Accepts `status` or `tag:<key>`; any
 *  other value returns `null` so a bad hash falls back to the
 *  flat list rather than 400-ing the server. */
export function projectGroupBy(route: string): string | null {
  const q = route.indexOf("?");
  if (q < 0) return null;
  const params = new URLSearchParams(route.slice(q + 1));
  const raw = params.get("group");
  if (!raw) return null;
  if (raw === "status") return raw;
  if (/^tag:[a-z0-9][a-z0-9-]{0,49}$/.test(raw)) return raw;
  return null;
}

/** Sentinel value for an *explicit empty* filter override while a
 *  saved view is active. `URLSearchParams` collapses empty strings,
 *  so `?filter=` would round-trip to "no override" and let the
 *  view's stored filter resurface — which is exactly the bug that
 *  used to make removing the last chip on a view tab undo-able.
 *  `__none__` is reserved here because it can't collide with any
 *  legal chip string (which always contains a `:`). */
export const FILTER_EMPTY_OVERRIDE = "__none__";

/** Parse `?filter=<chips>` from a project detail route
 *  (PROJECT-VIEW.md §5.4). Returns the raw wire string verbatim —
 *  the workbench is responsible for splitting on `;` and rendering
 *  chips. `null` when absent; **empty string** when the URL carries
 *  the explicit-empty sentinel (see [`FILTER_EMPTY_OVERRIDE`]). */
export function projectFilter(route: string): string | null {
  const q = route.indexOf("?");
  if (q < 0) return null;
  const params = new URLSearchParams(route.slice(q + 1));
  const raw = params.get("filter");
  if (raw === null) return null;
  if (raw === "" || raw === FILTER_EMPTY_OVERRIDE) return "";
  return raw;
}

/** Parse `?sort=<order>` from a project detail route
 *  (PROJECT-VIEW.md §5.4 / §5.3). */
export function projectSort(route: string): string | null {
  const q = route.indexOf("?");
  if (q < 0) return null;
  const params = new URLSearchParams(route.slice(q + 1));
  const raw = params.get("sort");
  if (!raw) return null;
  return raw;
}

/** Parse `?view=<uuid>` from a project detail route
 *  (PROJECT-VIEW.md §5.4). When present the workbench treats the
 *  saved view as the source of truth and only treats explicit
 *  `group`/`filter`/`sort` overrides as a "dirty" delta. Returns
 *  the raw value; the caller verifies it against the loaded list
 *  and shows a "view no longer exists" toast on mismatch. */
export function projectViewId(route: string): string | null {
  const q = route.indexOf("?");
  if (q < 0) return null;
  const params = new URLSearchParams(route.slice(q + 1));
  const raw = params.get("view");
  if (!raw) return null;
  // Loose UUID shape — server is the authority on existence.
  if (
    !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(
      raw,
    )
  ) {
    return null;
  }
  return raw;
}

/** Build a `#/projects/{id}` URL while preserving the existing
 *  query string and replacing the `group` param with `group`.
 *  Pass `null` to clear. Other params (issue, filter, sort, view)
 *  are passed through untouched so the toolbar's URL persistence
 *  composes with the issue-detail deep link. */
export function projectDetailRouteWithParams(
  id: string,
  patch: {
    issueId?: string | null;
    group?: string | null;
    filter?: string | null;
    sort?: string | null;
    view?: string | null;
  },
): string {
  const params = new URLSearchParams();
  if (patch.issueId) params.set("issue", patch.issueId);
  if (patch.view) params.set("view", patch.view);
  if (patch.group) params.set("group", patch.group);
  if (patch.filter === "") params.set("filter", FILTER_EMPTY_OVERRIDE);
  else if (patch.filter) params.set("filter", patch.filter);
  if (patch.sort) params.set("sort", patch.sort);
  const qs = params.toString();
  return `#/projects/${id}${qs ? `?${qs}` : ""}`;
}

/** Sub-tab on `#/projects/{id}` — `workbench` (default issue list /
 *  KPIs / milestones) or `exec-summary` (the §4 Executive Summary
 *  surface). Persisted via `?tab=…` so deep links land on the right
 *  surface. */
export type ProjectDetailTab = "workbench" | "exec-summary";

export function projectDetailTab(route: string): ProjectDetailTab {
  const q = route.indexOf("?");
  if (q < 0) return "workbench";
  const params = new URLSearchParams(route.slice(q + 1));
  return params.get("tab") === "exec-summary" ? "exec-summary" : "workbench";
}

export function projectDetailTabRoute(
  id: string,
  tab: ProjectDetailTab,
): string {
  if (tab === "workbench") return `#/projects/${id}`;
  return `#/projects/${id}?tab=${tab}`;
}

/** Sub-route under the account section. Defaults to
 *  `identities` — the link / unlink / transfer / set-primary
 *  surface (`linear-projects-idea.md` §10 multi-identity).
 *  `settings` is the per-user K/V settings page (GitHub PAT,
 *  future preferences). `tags` is the cross-org grouping
 *  primitive CRUD surface (SCOPE-PROJECTS §7). */
export type AccountTab = "identities" | "settings" | "tags";

/** Parse `#/account/...` → the active sub-tab. */
export function accountTabOf(route: string): AccountTab {
  const pathPart = route.split("?")[0] ?? route;
  const parts = pathPart.replace(/^#/, "").replace(/^\/+/, "").split("/");
  if (parts[0] !== "account") return "identities";
  switch (parts[1]) {
    case "settings":
      return "settings";
    case "tags":
      return "tags";
    default:
      return "identities";
  }
}

/** §14.1 deep-link selection — `#/workflow?issue=<uuid>` carries the
 *  detail-pane focus across copy-paste / back-forward. `null` ⇒ no
 *  selection (detail pane shows the empty state). */
export function workflowSelectedIssue(route: string): string | null {
  const q = route.indexOf("?");
  if (q < 0) return null;
  const params = new URLSearchParams(route.slice(q + 1));
  const id = params.get("issue");
  return id && id.length > 0 ? id : null;
}

/** Repo drill-down filter for `#/workflow/issues?repo_id=<uuid>`. */
export function workflowSelectedRepoId(route: string): string | null {
  const q = route.indexOf("?");
  if (q < 0) return null;
  const params = new URLSearchParams(route.slice(q + 1));
  const id = params.get("repo_id");
  return id && id.length > 0 ? id : null;
}

/** Active label filter on the triage / issues route, parsed from
 *  `?labels=bug,p1`. AND-semantics — every named label must be
 *  carried by the row. Names are case-preserved (the backend
 *  compares `dp_issues.labels` JSONB membership directly). */
export function workflowSelectedLabels(route: string): string[] {
  const q = route.indexOf("?");
  if (q < 0) return [];
  const raw = new URLSearchParams(route.slice(q + 1)).get("labels");
  if (!raw) return [];
  return raw
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

/** Build the `#/workflow/issues` route with optional repo / issue
 *  selection. Both round-trip through copy-paste so the workflow
 *  surface is fully shareable. */
export function workflowIssuesRoute(opts: { repoId?: string | null; issueId?: string | null } = {}): string {
  const params = new URLSearchParams();
  if (opts.repoId) params.set("repo_id", opts.repoId);
  if (opts.issueId) params.set("issue", opts.issueId);
  const qs = params.toString();
  return qs ? `#/workflow/issues?${qs}` : "#/workflow/issues";
}

/** Sub-route under the workflow section.
 *  - `triage` (default): Linear-style three-pane triage surface
 *                        (`linear-projects-idea.md` §3) — smart views
 *                        rail + dense issue list + peek panel.
 *  - `repos`           : legacy repos master.
 *  - `issues`          : legacy paginated issues table. */
export type WorkflowTab = "triage" | "repos" | "issues";

/** Parse `#/workflow/...` → the active sub-tab. Defaults to `triage`
 *  so `#/workflow` lands on the Linear-style triage page
 *  (`linear-projects-idea.md` §3). */
export function workflowTabOf(route: string): WorkflowTab {
  const path = route.replace(/^#/, "").replace(/^\/+/, "").split("/");
  if (path[0] !== "workflow") return "triage";
  switch (path[1]) {
    case "issues":
      return "issues";
    case "repos":
      return "repos";
    case "triage":
    case undefined:
    case "":
    default:
      return "triage";
  }
}

/** Smart-view identifier inside the triage page
 *  (`linear-projects-idea.md` §3.5). Lives in the URL query as
 *  `?view=...` so a deep-linked rail selection round-trips. */
/** Smart-view identifier. The four built-ins plus two date-driven
 *  views (`due_week` / `overdue`, §3.10) and a saved-view escape
 *  hatch (`tag:<uuid>`, §14.6) that maps onto a single tag-backed
 *  list. The `tag:<uuid>` form is opaque to the router — it falls
 *  through to the runtime check in `triage-page.tsx`. */
export type TriageView =
  | "mine"
  | "untriaged"
  | "all"
  | "snoozed"
  | "due_week"
  | "overdue"
  | `tag:${string}`;

/** Parse `#/workflow/triage?view=...` → the active smart view. */
export function triageView(route: string): TriageView {
  const q = route.indexOf("?");
  if (q < 0) return "mine";
  const v = new URLSearchParams(route.slice(q + 1)).get("view");
  if (!v) return "mine";
  if (v.startsWith("tag:")) return v as TriageView;
  switch (v) {
    case "untriaged":
    case "all":
    case "snoozed":
    case "due_week":
    case "overdue":
      return v;
    case "mine":
    default:
      return "mine";
  }
}

/** Build a `#/workflow/triage` URL with optional view / repo / issue
 *  selection. All three round-trip through copy-paste. */
export function workflowTriageRoute(opts: {
  view?: TriageView;
  repoId?: string | null;
  issueId?: string | null;
  labels?: ReadonlyArray<string> | null;
} = {}): string {
  const params = new URLSearchParams();
  if (opts.view && opts.view !== "mine") params.set("view", opts.view);
  if (opts.repoId) params.set("repo_id", opts.repoId);
  if (opts.issueId) params.set("issue", opts.issueId);
  if (opts.labels && opts.labels.length > 0) {
    params.set("labels", opts.labels.join(","));
  }
  const qs = params.toString();
  return qs ? `#/workflow/triage?${qs}` : "#/workflow/triage";
}

/** Sub-route under the reports section — drives the reports sub-nav. */
export type ReportTab = "user" | "team" | "org" | "home-org-split" | "leaderboard" | "repo-activity" | "freshness" | "projects";

/** Sub-route under the admin section — drives the admin sub-nav.
 *  - `runs` (default): paginated fetch_runs log.
 *  - `refresh`       : operator-triggered reconciler tick + org scope.
 *  - `users`         : GDPR controls (anonymise + export).
 *
 *  The legacy SCOPE-PROJECTS §3.10 per-repo board linker
 *  (`project-sync` / `projects` admin sub-tabs) is retired in stage
 *  11 of `linear-projects-v2.md`: the primary Link-a-board surface
 *  is the §6.4 dialog on each project detail page. */
export type AdminTab = "runs" | "refresh" | "users";

/** Parse `#/admin/...` → the active sub-tab. Defaults to `runs`. */
export function adminTabOf(route: string): AdminTab {
  const path = route.replace(/^#/, "").replace(/^\/+/, "").split("/");
  if (path[0] !== "admin") return "runs";
  switch (path[1]) {
    case "refresh":
      return "refresh";
    case "users":
      return "users";
    case "runs":
    case undefined:
    case "":
    default:
      return "runs";
  }
}

/** Sub-route under the directory section — drives the directory sub-nav.
 *  - `users` (default): people list with search + org filter + memberships + home-org badge.
 *  - `orgs`            : org list with member counts.
 *  - `teams`           : team list filtered by org.
 *  - `home-org`        : assignment UI (select user, select org, POST /home-org). */
export type DirectoryTab = "users" | "orgs" | "teams" | "home-org";

/** Parse `#/directory/...` → the active sub-tab. Defaults to `users`. */
export function directoryTabOf(route: string): DirectoryTab {
  const path = route.replace(/^#/, "").replace(/^\/+/, "").split("/");
  if (path[0] !== "directory") return "users";
  switch (path[1]) {
    case "orgs":
      return "orgs";
    case "teams":
      return "teams";
    case "home-org":
      return "home-org";
    case "users":
    case undefined:
    case "":
    default:
      return "users";
  }
}

/** Parse `#/reports/...` → the active sub-tab. Defaults to `user`
 *  (the SCOPE §11.5 landing report). */
export function reportTabOf(route: string): ReportTab {
  const path = route.replace(/^#/, "").replace(/^\/+/, "").split("?")[0]!.split("/");
  if (path[0] !== "reports") return "user";
  switch (path[1]) {
    case "team":
      return "team";
    case "org":
      return "org";
    case "home-org-split":
      return "home-org-split";
    case "leaderboard":
      return "leaderboard";
    case "repo-activity":
      return "repo-activity";
    case "freshness":
      return "freshness";
    case "projects":
      return "projects";
    case "user":
    case undefined:
    case "":
    default:
      return "user";
  }
}

// ---------------------------------------------------------------------------
// Product & Manufacturing — §7.4 routes
//   #/products                         — products hub
//   #/products?status=…                — hub filtered to a status
//   #/products/{id}                    — product detail (+ ?tab=)
//   #/manufacturing/parties            — parties admin (+ ?kind=)
//   #/customers/{id}                   — customer detail
// ---------------------------------------------------------------------------

/** §7.4 status filter for `#/products?status=…`. `null` ⇒ show every
 *  status (the hub default landing). */
export type ProductStatusRoute = "draft" | "active" | "eol" | "archived";

export function productsStatusOf(route: string): ProductStatusRoute | null {
  const q = route.indexOf("?");
  if (q < 0) return null;
  const v = new URLSearchParams(route.slice(q + 1)).get("status");
  switch (v) {
    case "draft":
    case "active":
    case "eol":
    case "archived":
      return v;
    default:
      return null;
  }
}

/** Build a `#/products` URL with an optional status filter. */
export function productsRoute(status?: ProductStatusRoute | null): string {
  if (!status) return "#/products";
  return `#/products?status=${status}`;
}

/** Parse `#/products/{id}` → the product id (UUID), or `null` for the
 *  hub landing (`#/products` / `#/products?status=…`). */
export function productDetailIdOf(route: string): string | null {
  const q = route.indexOf("?");
  const pathPart = q < 0 ? route : route.slice(0, q);
  const parts = pathPart.replace(/^#/, "").replace(/^\/+/, "").split("/");
  if (parts[0] !== "products") return null;
  const id = parts[1];
  if (!id) return null;
  if (!/^[0-9a-fA-F-]{36}$/.test(id)) return null;
  return id;
}

/** Build a `#/products/{id}` URL. */
export function productDetailRoute(id: string): string {
  return `#/products/${id}`;
}

/** Sub-tab on `#/products/{id}` — persisted via `?tab=…`. */
export type ProductDetailTab =
  | "overview"
  | "projects"
  | "runs"
  | "units"
  | "manuals"
  | "documents"
  | "returns";

export function productDetailTab(route: string): ProductDetailTab {
  const q = route.indexOf("?");
  if (q < 0) return "overview";
  const v = new URLSearchParams(route.slice(q + 1)).get("tab");
  switch (v) {
    case "projects":
    case "runs":
    case "units":
    case "manuals":
    case "documents":
    case "returns":
      return v;
    default:
      return "overview";
  }
}

export function productDetailTabRoute(
  id: string,
  tab: ProductDetailTab,
): string {
  if (tab === "overview") return `#/products/${id}`;
  return `#/products/${id}?tab=${tab}`;
}

/** Active manual id inside the Manuals tab — `?manual=<uuid>` opens
 *  the in-tab manual editor. `null` ⇒ the manual list. */
export function productManualId(route: string): string | null {
  const q = route.indexOf("?");
  if (q < 0) return null;
  const raw = new URLSearchParams(route.slice(q + 1)).get("manual");
  if (!raw) return null;
  if (!/^[0-9a-fA-F-]{36}$/.test(raw)) return null;
  return raw;
}

/** Build `#/products/{id}?tab=manuals[&manual=<uuid>]`. */
export function productManualRoute(
  id: string,
  manualId: string | null,
): string {
  const params = new URLSearchParams();
  params.set("tab", "manuals");
  if (manualId) params.set("manual", manualId);
  return `#/products/${id}?${params.toString()}`;
}

/** Parties admin sub-nav kind, parsed from `#/manufacturing/parties?kind=…`.
 *  Defaults to `customers`. */
export type PartiesKindRoute = "customers" | "manufacturers" | "suppliers";

export function partiesKindOf(route: string): PartiesKindRoute {
  const q = route.indexOf("?");
  if (q < 0) return "customers";
  const v = new URLSearchParams(route.slice(q + 1)).get("kind");
  switch (v) {
    case "manufacturers":
      return "manufacturers";
    case "suppliers":
      return "suppliers";
    default:
      return "customers";
  }
}

export function partiesRoute(kind?: PartiesKindRoute | null): string {
  if (!kind || kind === "customers") return "#/manufacturing/parties";
  return `#/manufacturing/parties?kind=${kind}`;
}

/** Parse `#/customers/{id}` → the customer id (UUID), or `null`. */
export function customerDetailIdOf(route: string): string | null {
  const q = route.indexOf("?");
  const pathPart = q < 0 ? route : route.slice(0, q);
  const parts = pathPart.replace(/^#/, "").replace(/^\/+/, "").split("/");
  if (parts[0] !== "customers") return null;
  const id = parts[1];
  if (!id) return null;
  if (!/^[0-9a-fA-F-]{36}$/.test(id)) return null;
  return id;
}

export function customerDetailRoute(id: string): string {
  return `#/customers/${id}`;
}

// ---------------------------------------------------------------------------
// P2 — manufacturing runs + serialised units (§7.4)
//   #/runs/{id}    — run detail
//   #/units/{id}   — unit detail
// ---------------------------------------------------------------------------

/** Parse `#/runs/{id}` → the run id (UUID), or `null`. */
export function runDetailIdOf(route: string): string | null {
  const q = route.indexOf("?");
  const pathPart = q < 0 ? route : route.slice(0, q);
  const parts = pathPart.replace(/^#/, "").replace(/^\/+/, "").split("/");
  if (parts[0] !== "runs") return null;
  const id = parts[1];
  if (!id) return null;
  if (!/^[0-9a-fA-F-]{36}$/.test(id)) return null;
  return id;
}

export function runDetailRoute(id: string): string {
  return `#/runs/${id}`;
}

/** Parse `#/units/{id}` → the unit id (UUID), or `null`. */
export function unitDetailIdOf(route: string): string | null {
  const q = route.indexOf("?");
  const pathPart = q < 0 ? route : route.slice(0, q);
  const parts = pathPart.replace(/^#/, "").replace(/^\/+/, "").split("/");
  if (parts[0] !== "units") return null;
  const id = parts[1];
  if (!id) return null;
  if (!/^[0-9a-fA-F-]{36}$/.test(id)) return null;
  return id;
}

export function unitDetailRoute(id: string): string {
  return `#/units/${id}`;
}

// ---------------------------------------------------------------------------
// P3 — Returns / RMA (§7.4)
//   #/rma          — RMA list (filter by status/customer/product)
//   #/rma/{id}     — RMA detail / status workflow
// ---------------------------------------------------------------------------

/** §7.4 status filter for `#/rma?status=…`. `null` ⇒ all statuses. */
export type RmaStatusRoute =
  | "open"
  | "received"
  | "diagnosed"
  | "repaired"
  | "replaced"
  | "rejected"
  | "closed";

export function rmaStatusOf(route: string): RmaStatusRoute | null {
  const q = route.indexOf("?");
  if (q < 0) return null;
  const v = new URLSearchParams(route.slice(q + 1)).get("status");
  switch (v) {
    case "open":
    case "received":
    case "diagnosed":
    case "repaired":
    case "replaced":
    case "rejected":
    case "closed":
      return v;
    default:
      return null;
  }
}

export function rmaListRoute(status?: RmaStatusRoute | null): string {
  if (!status) return "#/rma";
  return `#/rma?status=${status}`;
}

/** Parse `#/rma/{id}` → the RMA id (UUID), or `null` for the list. */
export function rmaDetailIdOf(route: string): string | null {
  const q = route.indexOf("?");
  const pathPart = q < 0 ? route : route.slice(0, q);
  const parts = pathPart.replace(/^#/, "").replace(/^\/+/, "").split("/");
  if (parts[0] !== "rma") return null;
  const id = parts[1];
  if (!id) return null;
  if (!/^[0-9a-fA-F-]{36}$/.test(id)) return null;
  return id;
}

export function rmaDetailRoute(id: string): string {
  return `#/rma/${id}`;
}

function subscribe(cb: () => void): () => void {
  window.addEventListener("hashchange", cb);
  return () => window.removeEventListener("hashchange", cb);
}

function snapshot(): string {
  return window.location.hash || "#/";
}

/** Current hash route, reactive via `useSyncExternalStore`. */
export function useRoute(): string {
  return useSyncExternalStore(subscribe, snapshot, () => "#/");
}

/** Imperative navigation. Use `<a href="#/...">` for normal links;
 *  this helper is for redirects (the protected-route gate uses it). */
export function navigate(to: string): void {
  const next = to.startsWith("#") ? to : `#${to.startsWith("/") ? to : `/${to}`}`;
  if (window.location.hash !== next) {
    window.location.hash = next;
  }
}

/** Strip the leading `#`, return the first path segment. `#/reports/user/42`
 *  -> `"reports"`. Used by the layout to pick the active sidebar item.
 *
 *  Query strings are stripped before splitting on `/` so
 *  `#/projects?status=active` resolves cleanly to the `projects`
 *  section (the §6.1 sidebar filter rides in the query string).
 *  Without this strip the head would be `"projects?status=active"`
 *  and the §6.1 deep link would fall through `isKnownRoute` into
 *  the NotFound page. */
export function sectionOf(route: string): Section {
  const path = route
    .replace(/^#/, "")
    .replace(/^\/+/, "")
    .split("?")[0]!;
  const head = path.split("/")[0] ?? "";
  switch (head) {
    case "login":
      return "login";
    case "directory":
      return "directory";
    case "admin":
      return "admin";
    case "workflow":
      return "workflow";
    case "projects":
      return "projects";
    case "products":
      return "products";
    case "manufacturing":
      return "manufacturing";
    case "customers":
      return "customers";
    case "runs":
      return "runs";
    case "units":
      return "units";
    case "rma":
      return "rma";
    case "account":
      return "account";
    case "reports":
    case "":
    default:
      return "reports";
  }
}

/** True if the route lands on the login page. */
export function isLoginRoute(route: string): boolean {
  return sectionOf(route) === "login";
}

/**
 * True if the leading hash segment names a real section. Used by the
 * 404 gate in `app.tsx` — `sectionOf` deliberately defaults unknown
 * heads to "reports" so a typo still lands somewhere useful, but a
 * completely unknown root segment (e.g. `#/foo/bar`) should render
 * NotFound instead of silently rewriting.
 */
export function isKnownRoute(route: string): boolean {
  const path = route
    .replace(/^#/, "")
    .replace(/^\/+/, "")
    .split("?")[0]!;
  const head = path.split("/")[0] ?? "";
  return (
    head === "" ||
    head === "login" ||
    head === "reports" ||
    head === "directory" ||
    head === "admin" ||
    head === "workflow" ||
    head === "projects" ||
    head === "products" ||
    head === "manufacturing" ||
    head === "customers" ||
    head === "runs" ||
    head === "units" ||
    head === "rma" ||
    head === "account"
  );
}
