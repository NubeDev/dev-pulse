/**
 * App shell — proper shadcn sidebar layout matching the codeless-ui
 * reference (h-14 sticky header with backdrop blur, 15rem sidebar on
 * `bg-card/40`, max-w content column).
 *
 * Phase 7 visual overhaul (Stage 2): drops the previous capsule
 * sidebar treatment and the bordered card header for a flatter,
 * shadcn-native chrome. Brand mark + wordmark sit at the far left of
 * the header, the `<Breadcrumb>` follows after a small separator,
 * then a flexible spacer pushes the theme toggle and user menu to the
 * right. The user-menu avatar is a rounded-square initial chip on
 * `bg-muted text-muted-foreground` — same rhythm as the codeless-ui
 * header. Body is a two-column grid (`15rem` sidebar + main); on
 * small viewports the sidebar collapses into a `<Sheet>` triggered
 * from a hamburger button. Sidebar items are anchor-shaped (the hash
 * route stays the source of truth for the active section) but expose
 * a `data-active` attribute that drives the accent state via Tailwind
 * `data-[active=true]:*` utilities — the same pattern shadcn uses for
 * its `SidebarMenuButton`. No inline `style={{}}` in this file.
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
  /** One-glyph mark rendered in a muted square next to the label. */
  readonly glyph: string;
}

const NAV: readonly NavItem[] = [
  { section: "reports", label: "Reports", href: "#/reports", glyph: "▦" },
  { section: "directory", label: "Directory", href: "#/directory", glyph: "◇" },
  { section: "admin", label: "Admin", href: "#/admin", glyph: "✦" },
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

/**
 * Sidebar nav-link classes. Anchor-shaped (hash routing is the source
 * of truth) but drives the active state through `data-active` so the
 * styling lives entirely in Tailwind utilities — matches the shadcn
 * `data-[state=active]` pattern used in their Sidebar primitive.
 */
const NAV_LINK_CLASS = cn(
  "flex items-center gap-2 rounded-md px-3 py-2 text-sm no-underline",
  "text-muted-foreground transition-colors",
  "hover:bg-accent hover:text-foreground",
  "data-[active=true]:bg-accent data-[active=true]:text-foreground",
  "data-[active=true]:font-medium",
);

const NAV_GLYPH_CLASS = cn(
  "inline-flex size-6 items-center justify-center rounded-sm",
  "bg-muted/60 text-[0.7rem] text-muted-foreground",
);

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
            data-active={isActive ? "true" : "false"}
            onClick={onNavigate}
            className={NAV_LINK_CLASS}
          >
            <span aria-hidden className={NAV_GLYPH_CLASS}>
              {item.glyph}
            </span>
            <span>{item.label}</span>
          </a>
        );
      })}
    </>
  );
}

/** Two-letter initials chip for the user-menu trigger. Falls back to
 *  the first letter if the email is single-token. */
function initialsFor(email: string): string {
  const local = email.split("@")[0] ?? email;
  const parts = local.split(/[._\-+]/).filter(Boolean);
  const first = parts[0];
  const second = parts[1];
  if (first && second) {
    return (first.charAt(0) + second.charAt(0)).toUpperCase();
  }
  return (local.charAt(0) || "?").toUpperCase();
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
      className="min-h-dvh bg-background text-foreground"
    >
      <header className="sticky top-0 z-40 flex h-14 items-center gap-3 border-b bg-background/80 px-4 backdrop-blur-xl">
        {/* Mobile-only hamburger that opens the sidebar in a Sheet. */}
        <Sheet open={mobileNavOpen} onOpenChange={setMobileNavOpen}>
          <SheetTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="md:hidden"
              aria-label="Open navigation"
            >
              <span aria-hidden className="text-lg leading-none">≡</span>
            </Button>
          </SheetTrigger>
          <SheetContent side="left" className="w-72 p-0">
            <SheetHeader className="border-b">
              <SheetTitle className="flex items-center gap-2">
                <BrandMark />
                <span className="font-semibold tracking-tight">dev-pulse</span>
              </SheetTitle>
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

        {/* Brand: small coloured mark + wordmark. */}
        <a
          href="#/reports"
          className="flex items-center gap-2 no-underline text-foreground"
        >
          <BrandMark />
          <span className="font-semibold tracking-tight">dev-pulse</span>
        </a>

        <Separator orientation="vertical" className="hidden h-5 md:block" />

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

        {/* Flexible spacer pushes the right-aligned chrome to the edge. */}
        <div className="flex-1" aria-hidden />

        <ThemeToggle />

        {auth.user ? (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                data-testid="user-menu-trigger"
                className="gap-2 pl-2 pr-2"
              >
                <span className="hidden max-w-[16ch] truncate text-sm text-muted-foreground sm:inline">
                  {auth.user.email}
                </span>
                <span
                  aria-hidden
                  className="inline-flex size-7 items-center justify-center rounded-md bg-muted text-xs font-semibold uppercase text-muted-foreground"
                >
                  {initialsFor(auth.user.email)}
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
      </header>

      <div className="grid md:grid-cols-[15rem_minmax(0,1fr)]">
        {/* Desktop sidebar — same density as codeless-ui (15rem wide,
            card-tinted background, anchor-shaped nav rows). */}
        <nav
          aria-label="Primary"
          data-testid="primary-nav"
          className="hidden border-r bg-card/40 p-3 md:flex md:flex-col md:gap-1"
        >
          <NavLinks active={active} />
        </nav>

        <main className="min-w-0 p-6 md:p-8">
          <div className="mx-auto w-full max-w-6xl">{children}</div>
        </main>
      </div>
    </div>
  );
}

/** Small primary-tinted mark that anchors the brand wordmark in the
 *  header. The `rounded-[6px]` matches the radius ladder in
 *  `globals.css` (`--radius-sm`). */
function BrandMark(): JSX.Element {
  return (
    <span
      aria-hidden
      className="inline-flex size-7 items-center justify-center rounded-[6px] bg-primary text-primary-foreground"
    >
      <span className="text-xs font-bold leading-none">dp</span>
    </span>
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
