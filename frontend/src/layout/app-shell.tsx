/**
 * App shell — sidebar nav + top bar + main content slot.
 *
 * Stage 2 (phase-7-frontend-polish): rebuilt on shadcn primitives +
 * Tailwind utilities. Desktop renders a fixed sidebar; mobile collapses
 * the same nav into a `<Sheet>` triggered from a hamburger button in
 * the header. The user menu is a `<DropdownMenu>` (email + role +
 * logout); the header gains a `<Breadcrumb>` derived from the current
 * hash route. No inline `style={{}}` remain in this file.
 */

import { useState, type ReactNode } from "react";
import { useAuth } from "@nube/starter-ui-core/auth";
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from "@nube/starter-ui-kit/components/breadcrumb";
import { Button } from "@nube/starter-ui-kit/components/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@nube/starter-ui-kit/components/dropdown-menu";
import { Separator } from "@nube/starter-ui-kit/components/separator";
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@nube/starter-ui-kit/components/sheet";
import { cn } from "@nube/starter-ui-kit/lib/utils";

import { ThemeToggle } from "../components/theme-toggle.jsx";
import {
  adminTabOf,
  directoryTabOf,
  reportTabOf,
  sectionOf,
  useRoute,
  type AdminTab,
  type DirectoryTab,
  type ReportTab,
  type Section,
} from "../routes.js";

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

const SECTION_LABEL: Record<Section, string> = {
  reports: "Reports",
  directory: "Directory",
  admin: "Admin",
  login: "Login",
};

const REPORT_TAB_LABEL: Record<ReportTab, string> = {
  user: "User",
  team: "Team",
  org: "Org",
  "home-org-split": "Home-org split",
  freshness: "Freshness",
};

const DIRECTORY_TAB_LABEL: Record<DirectoryTab, string> = {
  users: "Users",
  orgs: "Orgs",
  teams: "Teams",
  "home-org": "Home-org",
};

const ADMIN_TAB_LABEL: Record<AdminTab, string> = {
  runs: "Runs",
  refresh: "Refresh",
  users: "Users",
};

interface Crumb {
  readonly label: string;
  readonly href?: string;
}

function crumbsFor(route: string): readonly Crumb[] {
  const section = sectionOf(route);
  const sectionHref = `#/${section}`;
  const sectionCrumb: Crumb = { label: SECTION_LABEL[section] };
  switch (section) {
    case "reports": {
      return [
        { ...sectionCrumb, href: sectionHref },
        { label: REPORT_TAB_LABEL[reportTabOf(route)] },
      ];
    }
    case "directory": {
      return [
        { ...sectionCrumb, href: sectionHref },
        { label: DIRECTORY_TAB_LABEL[directoryTabOf(route)] },
      ];
    }
    case "admin": {
      return [
        { ...sectionCrumb, href: sectionHref },
        { label: ADMIN_TAB_LABEL[adminTabOf(route)] },
      ];
    }
    case "login":
    default:
      return [sectionCrumb];
  }
}

function navLinkClass(isActive: boolean): string {
  return cn(
    "block whitespace-nowrap rounded-md px-3 py-2 text-sm font-medium no-underline transition-colors",
    isActive
      ? "bg-primary text-primary-foreground hover:bg-primary/90"
      : "text-foreground hover:bg-muted hover:text-foreground",
  );
}

interface NavLinksProps {
  readonly active: Section;
  readonly onNavigate?: () => void;
}

function NavLinks({ active, onNavigate }: NavLinksProps): JSX.Element {
  return (
    <>
      {NAV.map((item) => {
        const isActive = item.section === active;
        return (
          <a
            key={item.section}
            href={item.href}
            aria-current={isActive ? "page" : undefined}
            onClick={onNavigate}
            className={navLinkClass(isActive)}
          >
            {item.label}
          </a>
        );
      })}
    </>
  );
}

export interface AppShellProps {
  children: ReactNode;
}

export function AppShell({ children }: AppShellProps): JSX.Element {
  const auth = useAuth();
  const route = useRoute();
  const active = sectionOf(route);
  const crumbs = crumbsFor(route);
  const [mobileNavOpen, setMobileNavOpen] = useState(false);

  return (
    <div
      data-testid="app-shell"
      className="grid min-h-dvh grid-rows-[auto_1fr] bg-background text-foreground md:grid-cols-[16rem_1fr]"
    >
      <header className="col-span-full flex items-center justify-between gap-2 border-b border-border bg-card px-4 py-3">
        <div className="flex items-center gap-3">
          {/* Mobile-only Sheet trigger for the primary nav. */}
          <Sheet open={mobileNavOpen} onOpenChange={setMobileNavOpen}>
            <SheetTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                className="md:hidden"
                aria-label="Open navigation"
              >
                <span aria-hidden className="text-lg leading-none">≡</span>
              </Button>
            </SheetTrigger>
            <SheetContent side="left" className="w-72 p-0">
              <SheetHeader className="border-b border-border">
                <SheetTitle>dev-pulse</SheetTitle>
              </SheetHeader>
              <nav
                aria-label="Primary"
                data-testid="primary-nav-mobile"
                className="flex flex-col gap-1 p-3"
              >
                <NavLinks
                  active={active}
                  onNavigate={() => setMobileNavOpen(false)}
                />
              </nav>
            </SheetContent>
          </Sheet>

          <strong className="text-sm font-semibold">dev-pulse</strong>

          <Separator
            orientation="vertical"
            className="hidden h-5 md:block"
          />

          <Breadcrumb className="hidden md:block">
            <BreadcrumbList>
              {crumbs.map((crumb, i) => {
                const isLast = i === crumbs.length - 1;
                return (
                  <BreadcrumbFragment
                    key={`${crumb.label}-${i}`}
                    crumb={crumb}
                    isLast={isLast}
                    showSeparator={i > 0}
                  />
                );
              })}
            </BreadcrumbList>
          </Breadcrumb>
        </div>

        <div className="flex flex-wrap items-center justify-end gap-2">
          <ThemeToggle />
          {auth.user ? (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="ghost"
                  size="sm"
                  data-testid="user-menu-trigger"
                  className="gap-2"
                >
                  <span className="hidden max-w-[16ch] truncate sm:inline">
                    {auth.user.email}
                  </span>
                  <span
                    aria-hidden
                    className="inline-flex size-7 items-center justify-center rounded-full bg-muted text-xs font-semibold uppercase text-muted-foreground"
                  >
                    {auth.user.email.slice(0, 1)}
                  </span>
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="min-w-56">
                <DropdownMenuLabel className="flex flex-col gap-0.5">
                  <span className="truncate text-sm font-medium text-foreground">
                    {auth.user.email}
                  </span>
                  <span className="text-xs font-normal text-muted-foreground">
                    Role: {auth.user.role}
                  </span>
                </DropdownMenuLabel>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  variant="destructive"
                  onSelect={() => {
                    void auth.logout();
                  }}
                >
                  Logout
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          ) : (
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                void auth.logout();
              }}
            >
              Logout
            </Button>
          )}
        </div>
      </header>

      {/* Desktop sidebar. */}
      <nav
        aria-label="Primary"
        data-testid="primary-nav"
        className="hidden border-r border-border bg-card md:flex md:flex-col md:gap-1 md:px-3 md:py-4"
      >
        <NavLinks active={active} />
        <Separator className="my-3" />
        <span className="px-3 text-xs text-muted-foreground">
          Phase 7 frontend
        </span>
      </nav>

      <main className="min-w-0 overflow-auto p-4 md:p-6">{children}</main>
    </div>
  );
}

interface BreadcrumbFragmentProps {
  readonly crumb: Crumb;
  readonly isLast: boolean;
  readonly showSeparator: boolean;
}

function BreadcrumbFragment({
  crumb,
  isLast,
  showSeparator,
}: BreadcrumbFragmentProps): JSX.Element {
  return (
    <>
      {showSeparator && <BreadcrumbSeparator />}
      <BreadcrumbItem>
        {isLast || !crumb.href ? (
          <BreadcrumbPage>{crumb.label}</BreadcrumbPage>
        ) : (
          <BreadcrumbLink href={crumb.href}>{crumb.label}</BreadcrumbLink>
        )}
      </BreadcrumbItem>
    </>
  );
}
