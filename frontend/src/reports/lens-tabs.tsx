/**
 * Three-lens toggle (SCOPE §8.1):
 *
 *   - Single org           — one row per user, scoped to one org.
 *   - All orgs combined    — one row per user, summed across orgs.
 *   - Per-org split        — one row per (user × org).
 *
 * The user-report page renders the selected lens; the toggle owns
 * the `ScopeMode` value the page maps onto `ReportParams.scope_mode`.
 *
 * Implementation: shadcn `Tabs` in the kit's default horizontal
 * orientation (TabsList sits *above* TabsContent — i.e. tabs read
 * left-to-right). The kit's Tabs root carries `flex` + `data-horizontal:flex-col`,
 * so wrap it in a `grid gap-3` block to give the list and its hint
 * paragraph room to breathe without the triggers wrapping vertically.
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
    <Tabs
      value={value}
      onValueChange={(v) => onChange(v as ScopeMode)}
      orientation="horizontal"
      className="gap-3"
    >
      <TabsList className="self-start">
        {LENSES.map((l) => (
          <TabsTrigger key={l.value} value={l.value}>{l.label}</TabsTrigger>
        ))}
      </TabsList>
      {LENSES.map((l) => (
        <TabsContent key={l.value} value={l.value} className="grid gap-3">
          <p className="text-[0.8125rem] text-muted-foreground">{l.hint}</p>
          {children}
        </TabsContent>
      ))}
    </Tabs>
  );
}
