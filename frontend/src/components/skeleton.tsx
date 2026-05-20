/**
 * Local Skeleton primitive.
 *
 * The shadcn-shipped `@nube/starter-ui-kit/components/skeleton`
 * uses the implicit global `React` type, which trips a
 * React-18-vs-19 declaration mismatch when imported into this
 * frontend (React 18). To keep dev-pulse's typecheck clean
 * without forking the upstream package we ship a thin local
 * Skeleton that mirrors the same shape (a pulsing rounded
 * `<div>`) and reads the same design tokens (`var(--muted)`).
 *
 * No animation class is used — instead we inline a keyframe via
 * the `globals.css` `@keyframes dp-pulse` so this primitive has
 * no Tailwind dependency beyond the design tokens.
 */

import type { CSSProperties, HTMLAttributes } from "react";

export interface SkeletonProps extends HTMLAttributes<HTMLDivElement> {}

export function Skeleton({ style, ...props }: SkeletonProps): JSX.Element {
  const merged: CSSProperties = {
    display: "inline-block",
    background: "var(--muted)",
    borderRadius: "0.5rem",
    animation: "dp-pulse 1.6s ease-in-out infinite",
    ...style,
  };
  return <div data-slot="skeleton" style={merged} {...props} />;
}
