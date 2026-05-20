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
  | "account"
  | "login";

/** Sub-route under the account section
 *  (`linear-projects-idea.md` §10 multi-identity). Defaults to
 *  `identities` — the link / unlink / transfer / set-primary
 *  surface. */
export type AccountTab = "identities";

/** Parse `#/account/...` → the active sub-tab. */
export function accountTabOf(_route: string): AccountTab {
  return "identities";
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
} = {}): string {
  const params = new URLSearchParams();
  if (opts.view && opts.view !== "mine") params.set("view", opts.view);
  if (opts.repoId) params.set("repo_id", opts.repoId);
  if (opts.issueId) params.set("issue", opts.issueId);
  const qs = params.toString();
  return qs ? `#/workflow/triage?${qs}` : "#/workflow/triage";
}

/** Sub-route under the reports section — drives the reports sub-nav. */
export type ReportTab = "user" | "team" | "org" | "home-org-split" | "leaderboard" | "repo-activity" | "freshness";

/** Sub-route under the admin section — drives the admin sub-nav.
 *  - `runs` (default): paginated fetch_runs log.
 *  - `refresh`       : operator-triggered reconciler tick + org scope.
 *  - `users`         : GDPR controls (anonymise + export). */
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
  const path = route.replace(/^#/, "").replace(/^\/+/, "").split("/");
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
    case "user":
    case undefined:
    case "":
    default:
      return "user";
  }
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
 *  -> `"reports"`. Used by the layout to pick the active sidebar item. */
export function sectionOf(route: string): Section {
  const path = route.replace(/^#/, "").replace(/^\/+/, "");
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
  const path = route.replace(/^#/, "").replace(/^\/+/, "");
  const head = path.split("/")[0] ?? "";
  return (
    head === "" ||
    head === "login" ||
    head === "reports" ||
    head === "directory" ||
    head === "admin" ||
    head === "workflow" ||
    head === "account"
  );
}
