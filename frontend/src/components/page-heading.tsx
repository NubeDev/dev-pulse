/**
 * Page heading lockup — the `<h1>` + supporting muted paragraph that
 * sits above the filter Card on every report page. Establishes the
 * vertical rhythm so each page opens with the same typographic
 * cadence as the codeless-ui reference.
 */

import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export interface PageHeadingProps {
  title: ReactNode;
  description?: ReactNode;
  /** Right-aligned slot for badges / banners / quick actions. */
  trailing?: ReactNode;
  className?: string;
}

export function PageHeading({
  title,
  description,
  trailing,
  className,
}: PageHeadingProps): JSX.Element {
  return (
    <div
      className={cn(
        "flex flex-wrap items-start justify-between gap-4",
        className,
      )}
    >
      <div className="grid gap-1.5">
        <h1 className="text-2xl font-semibold tracking-tight">{title}</h1>
        {description ? (
          <p className="text-muted-foreground text-sm">{description}</p>
        ) : null}
      </div>
      {trailing ? <div className="flex shrink-0 items-center gap-2">{trailing}</div> : null}
    </div>
  );
}
