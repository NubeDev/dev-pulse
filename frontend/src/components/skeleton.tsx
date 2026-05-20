/**
 * Local Skeleton primitive.
 *
 * The shadcn-shipped `@/components/ui/skeleton`
 * uses the implicit global `React` type, which trips a
 * React-18-vs-19 declaration mismatch when imported into this
 * frontend (React 18). To keep dev-pulse's typecheck clean
 * without forking the upstream package we ship a thin local
 * Skeleton that mirrors the same shape (a pulsing rounded
 * `<div>`) and reads the same design tokens (`bg-muted`).
 *
 * Implementation matches shadcn's: a `bg-muted` rounded div with
 * Tailwind's built-in `animate-pulse` keyframe. Callers pass
 * `className` to size each shimmer (`h-3.5 w-10` for a numeric
 * cell, `h-5 w-full` for a sparkline strip, etc.) so loading
 * placeholders match the real content shape, not just a square.
 */

import type { HTMLAttributes } from "react";
import { cn } from "@/lib/utils";

export interface SkeletonProps extends HTMLAttributes<HTMLDivElement> {}

export function Skeleton({ className, ...props }: SkeletonProps): JSX.Element {
  return (
    <div
      data-slot="skeleton"
      className={cn("animate-pulse rounded-md bg-muted", className)}
      {...props}
    />
  );
}
