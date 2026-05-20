/**
 * App shell — sidebar nav + top bar + main content slot.
 *
 * Stage 9 widens the polish surface:
 *   - responsive layout: at <= 768px the sidebar collapses into a
 *     horizontal scroll-strip below the header so reports + directory
 *     remain reachable on a phone (Lighthouse Mobile a11y signal),
 *   - the header gains a theme-toggle button driven by the
 *     `@nube/starter-ui-kit` `<ThemeProvider>` (light / dark / system),
 *   - active section is still derived from `useRoute()` via `sectionOf`
 *     so deep links + back-button keep working without a controlled
 *     state in the shell.
 */

import { useSyncExternalStore, type ReactNode } from "react";
import { useAuth } from "@nube/starter-ui-core/auth";
import { Button } from "@nube/starter-ui-kit/components/button";
import { Separator } from "@nube/starter-ui-kit/components/separator";

import { ThemeToggle } from "../components/theme-toggle.jsx";
import { sectionOf, useRoute, type Section } from "../routes.js";

interface NavItem {
  readonly section: Section;
  readonly label: string;
  readonly href: string;
}

const NAV: readonly NavItem[] = [
  { section: "reports", label: "Reports", href: "#/reports" },
  { section: "directory", label: "Directory", href: "#/directory" },
  { section: "admin", label: "Admin", href: "#/admin" },
];

/** Mobile breakpoint — below this width we collapse the sidebar into a
 *  horizontal nav strip under the header. Matches the Tailwind `md` token. */
const MOBILE_MAX_PX = 768;

function subscribeViewport(cb: () => void): () => void {
  window.addEventListener("resize", cb);
  return () => window.removeEventListener("resize", cb);
}

function snapshotIsMobile(): boolean {
  if (typeof window === "undefined") return false;
  return window.innerWidth <= MOBILE_MAX_PX;
}

/** Reactive `isMobile` flag driven by viewport width. SSR returns `false`. */
function useIsMobile(): boolean {
  return useSyncExternalStore(subscribeViewport, snapshotIsMobile, () => false);
}

export interface AppShellProps {
  children: ReactNode;
}

export function AppShell({ children }: AppShellProps): JSX.Element {
  const auth = useAuth();
  const route = useRoute();
  const active = sectionOf(route);
  const isMobile = useIsMobile();

  return (
    <div
      data-testid="app-shell"
      data-layout={isMobile ? "mobile" : "desktop"}
      style={{
        minHeight: "100dvh",
        display: "grid",
        gridTemplateColumns: isMobile ? "1fr" : "16rem 1fr",
        gridTemplateRows: isMobile ? "auto auto 1fr" : "auto 1fr",
        background: "var(--background)",
        color: "var(--foreground)",
      }}
    >
      <header
        style={{
          gridColumn: "1 / -1",
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          padding: "0.75rem 1rem",
          borderBottom: "1px solid var(--border)",
          background: "var(--card)",
          gap: "0.5rem",
        }}
      >
        <strong style={{ fontSize: "0.95rem" }}>dev-pulse</strong>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: "0.5rem",
            flexWrap: "wrap",
            justifyContent: "flex-end",
          }}
        >
          {auth.user && !isMobile && (
            <span style={{ color: "var(--muted-foreground)", fontSize: "0.875rem" }}>
              {auth.user.email}
              <span
                style={{
                  marginLeft: "0.5rem",
                  padding: "0.125rem 0.5rem",
                  borderRadius: "var(--radius-sm, 0.25rem)",
                  background: "var(--muted)",
                  fontSize: "0.75rem",
                }}
              >
                {auth.user.role}
              </span>
            </span>
          )}
          <ThemeToggle />
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              void auth.logout();
            }}
          >
            Logout
          </Button>
        </div>
      </header>

      <nav
        aria-label="Primary"
        data-testid="primary-nav"
        data-layout={isMobile ? "horizontal" : "vertical"}
        style={
          isMobile
            ? {
                borderBottom: "1px solid var(--border)",
                padding: "0.5rem 0.75rem",
                background: "var(--card)",
                display: "flex",
                flexDirection: "row",
                gap: "0.25rem",
                overflowX: "auto",
                WebkitOverflowScrolling: "touch",
              }
            : {
                borderRight: "1px solid var(--border)",
                padding: "1rem 0.75rem",
                background: "var(--card)",
                display: "flex",
                flexDirection: "column",
                gap: "0.25rem",
              }
        }
      >
        {NAV.map((item) => {
          const isActive = item.section === active;
          return (
            <a
              key={item.section}
              href={item.href}
              aria-current={isActive ? "page" : undefined}
              style={{
                display: "block",
                padding: "0.5rem 0.75rem",
                borderRadius: "var(--radius-sm, 0.375rem)",
                fontSize: "0.9rem",
                textDecoration: "none",
                whiteSpace: "nowrap",
                color: isActive ? "var(--primary-foreground)" : "var(--foreground)",
                background: isActive ? "var(--primary)" : "transparent",
              }}
            >
              {item.label}
            </a>
          );
        })}
        {!isMobile && (
          <>
            <Separator style={{ margin: "0.75rem 0" }} />
            <span
              style={{
                fontSize: "0.75rem",
                color: "var(--muted-foreground)",
                padding: "0 0.75rem",
              }}
            >
              Phase 7 frontend
            </span>
          </>
        )}
      </nav>

      <main
        style={{
          padding: isMobile ? "1rem" : "1.5rem",
          overflow: "auto",
          minWidth: 0,
        }}
      >
        {children}
      </main>
    </div>
  );
}
