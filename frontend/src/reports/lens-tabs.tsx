/**
 * Three-lens toggle (SCOPE §8.1):
 *
 *   - Single org           — one row per user, scoped to one org.
 *   - All orgs combined    — one row per user, summed across orgs.
 *   - Per-org split        — one row per (user × org).
 *
 * The user-report page renders the selected lens; the toggle owns
 * the `ScopeMode` value the page maps onto `ReportParams.scope_mode`.
 */

import {
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@nube/starter-ui-kit/components/tabs";

import type { ScopeMode } from "../api/client.js";
import type { ReactNode } from "react";

export const LENSES: ReadonlyArray<{ value: ScopeMode; label: string; hint: string }> = [
  { value: "single_org", label: "Single org", hint: "What happened in this codebase this period." },
  { value: "all_orgs_combined", label: "All orgs combined", hint: "Total across orgs, deduped per user." },
  { value: "per_org_split", label: "Per-org split", hint: "One row per (user × org) — context switching." },
];

export interface LensTabsProps {
  value: ScopeMode;
  onChange: (next: ScopeMode) => void;
  /** Renders inside each lens tab; the page passes the same body for
   *  every lens (the underlying query changes per lens, but the
   *  layout is identical). */
  children: ReactNode;
}

export function LensTabs({ value, onChange, children }: LensTabsProps): JSX.Element {
  return (
    <Tabs value={value} onValueChange={(v) => onChange(v as ScopeMode)}>
      <TabsList>
        {LENSES.map((l) => (
          <TabsTrigger key={l.value} value={l.value}>{l.label}</TabsTrigger>
        ))}
      </TabsList>
      {LENSES.map((l) => (
        <TabsContent key={l.value} value={l.value}>
          <p
            style={{
              color: "var(--muted-foreground)",
              fontSize: "0.8125rem",
              margin: "0.5rem 0 1rem",
            }}
          >
            {l.hint}
          </p>
          {children}
        </TabsContent>
      ))}
    </Tabs>
  );
}
