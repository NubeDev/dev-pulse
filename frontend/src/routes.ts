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
 *   #/directory[/...]             — protected: directory
 *   #/admin[/...]                 — protected: admin
 *   #/                            — alias for #/reports
 */

import { useSyncExternalStore } from "react";

export type Section = "reports" | "directory" | "admin" | "login";

/** Sub-route under the reports section — drives the reports sub-nav. */
export type ReportTab = "user" | "team" | "org" | "home-org-split";

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
