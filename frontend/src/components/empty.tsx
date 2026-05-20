/**
 * Local Empty primitive — mirrors shadcn's `Empty` family of
 * components from `@/components/ui/empty`.
 *
 * The upstream module relies on the implicit global `React` type
 * (a React-18-vs-19 declaration mismatch — same as `skeleton.tsx`),
 * so we ship thin local wrappers with the same class strings and
 * `data-slot` attributes here. Callers compose
 * `<Empty><EmptyHeader><EmptyTitle/><EmptyDescription/></EmptyHeader><EmptyContent/></Empty>`
 * the same way they would against the upstream component, and the
 * design-token / dashed-border styling is identical.
 */

import type { HTMLAttributes } from "react";
import { cn } from "@/lib/utils";

export function Empty({
  className,
  ...props
}: HTMLAttributes<HTMLDivElement>): JSX.Element {
  return (
    <div
      data-slot="empty"
      className={cn(
        "flex w-full min-w-0 flex-1 flex-col items-center justify-center gap-4 rounded-2xl border border-dashed p-12 text-center text-balance",
        className,
      )}
      {...props}
    />
  );
}

export function EmptyHeader({
  className,
  ...props
}: HTMLAttributes<HTMLDivElement>): JSX.Element {
  return (
    <div
      data-slot="empty-header"
      className={cn("flex max-w-sm flex-col items-center gap-2", className)}
      {...props}
    />
  );
}

export function EmptyTitle({
  className,
  ...props
}: HTMLAttributes<HTMLDivElement>): JSX.Element {
  return (
    <div
      data-slot="empty-title"
      className={cn(
        "font-heading text-lg font-medium tracking-tight",
        className,
      )}
      {...props}
    />
  );
}

export function EmptyDescription({
  className,
  ...props
}: HTMLAttributes<HTMLParagraphElement>): JSX.Element {
  return (
    <div
      data-slot="empty-description"
      className={cn(
        "text-sm/relaxed text-muted-foreground [&>a]:underline [&>a]:underline-offset-4 [&>a:hover]:text-primary",
        className,
      )}
      {...props}
    />
  );
}

export function EmptyContent({
  className,
  ...props
}: HTMLAttributes<HTMLDivElement>): JSX.Element {
  return (
    <div
      data-slot="empty-content"
      className={cn(
        "flex w-full max-w-sm min-w-0 flex-col items-center gap-4 text-sm text-balance",
        className,
      )}
      {...props}
    />
  );
}
