/**
 * App shell — sidebar nav + top bar + main content slot.
 *
 * Stage 3 ships the chrome only; the section panes are placeholders so
 * the navigation + logout flow can be exercised end-to-end before the
 * SCOPE §11.5 report pages land in stage 4+.
 *
 * Nav sections (per the stage description): Reports, Directory, Admin.
 * Active section is derived from `useRoute()` via `sectionOf` so the
 * sidebar highlight survives a manual hash edit / back-button.
 */

import type { ReactNode } from "react";
import { useAuth } from "@nube/starter-ui-core/auth";
import { Button } from "@nube/starter-ui-kit/components/button";
import { Separator } from "@nube/starter-ui-kit/components/separator";

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

export interface AppShellProps {
  children: ReactNode;
}

export function AppShell({ children }: AppShellProps): JSX.Element {
  const auth = useAuth();
  const route = useRoute();
  const active = sectionOf(route);

  return (
    <div
      style={{
        minHeight: "100dvh",
        display: "grid",
        gridTemplateColumns: "16rem 1fr",
        gridTemplateRows: "auto 1fr",
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
          padding: "0.75rem 1.25rem",
          borderBottom: "1px solid var(--border)",
          background: "var(--card)",
        }}
      >
        <strong style={{ fontSize: "0.95rem" }}>dev-pulse</strong>
        <div style={{ display: "flex", alignItems: "center", gap: "0.75rem" }}>
          {auth.user && (
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
        style={{
          borderRight: "1px solid var(--border)",
          padding: "1rem 0.75rem",
          background: "var(--card)",
          display: "flex",
          flexDirection: "column",
          gap: "0.25rem",
        }}
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
                color: isActive ? "var(--primary-foreground)" : "var(--foreground)",
                background: isActive ? "var(--primary)" : "transparent",
              }}
            >
              {item.label}
            </a>
          );
        })}
        <Separator style={{ margin: "0.75rem 0" }} />
        <span style={{ fontSize: "0.75rem", color: "var(--muted-foreground)", padding: "0 0.75rem" }}>
          Phase 7 frontend · stage 3
        </span>
      </nav>

      <main style={{ padding: "1.5rem", overflow: "auto" }}>{children}</main>
    </div>
  );
}
