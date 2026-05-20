/**
 * Three-lens toggle (SCOPE §8.1):
 *
 *   - Single org           — one row per user, scoped to one org.
 *   - All orgs combined    — one row per user, summed across orgs.
 *   - Per-org split        — one row per (user × org).
 *
 * Implementation: shadcn `Tabs` in the kit's default segmented style
 * (TabsList renders the muted pill row with the active trigger
 * elevated to `bg-background`). The triggers read left-to-right as
 * the kit's horizontal-orientation default; the page composes the
 * tab content underneath.
 */

import {
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/tabs";

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
  const active = LENSES.find((l) => l.value === value) ?? LENSES[0];
  return (
    <Tabs
      value={value}
      onValueChange={(v) => onChange(v as ScopeMode)}
      orientation="horizontal"
      className="gap-4"
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <TabsList>
          {LENSES.map((l) => (
            <TabsTrigger key={l.value} value={l.value}>{l.label}</TabsTrigger>
          ))}
        </TabsList>
        <p className="text-xs text-muted-foreground">{active!.hint}</p>
      </div>
      {LENSES.map((l) => (
        <TabsContent key={l.value} value={l.value} className="grid gap-4">
          {children}
        </TabsContent>
      ))}
    </Tabs>
  );
}
