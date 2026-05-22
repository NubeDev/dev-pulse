/**
 * Deterministic project identity tile.
 *
 * No schema change: we derive a stable palette + 2-letter glyph from
 * `project.id` so every project has visual identity without a new
 * `icon_slug` column. When we add per-project icon picking (planned
 * follow-up) this component grows an optional `icon` prop and the
 * fallback path here stays unchanged.
 *
 * Both the background tint and the foreground glyph carry explicit
 * `dark:` variants so the tile reads in both themes.
 */

import * as React from "react";

import { cn } from "@/lib/utils";

const PALETTE: Array<{ bg: string; fg: string; ring: string }> = [
  {
    bg: "bg-blue-100 dark:bg-blue-500/15",
    fg: "text-blue-700 dark:text-blue-300",
    ring: "ring-blue-500/20 dark:ring-blue-400/20",
  },
  {
    bg: "bg-emerald-100 dark:bg-emerald-500/15",
    fg: "text-emerald-700 dark:text-emerald-300",
    ring: "ring-emerald-500/20 dark:ring-emerald-400/20",
  },
  {
    bg: "bg-violet-100 dark:bg-violet-500/15",
    fg: "text-violet-700 dark:text-violet-300",
    ring: "ring-violet-500/20 dark:ring-violet-400/20",
  },
  {
    bg: "bg-amber-100 dark:bg-amber-500/15",
    fg: "text-amber-700 dark:text-amber-300",
    ring: "ring-amber-500/20 dark:ring-amber-400/20",
  },
  {
    bg: "bg-rose-100 dark:bg-rose-500/15",
    fg: "text-rose-700 dark:text-rose-300",
    ring: "ring-rose-500/20 dark:ring-rose-400/20",
  },
  {
    bg: "bg-cyan-100 dark:bg-cyan-500/15",
    fg: "text-cyan-700 dark:text-cyan-300",
    ring: "ring-cyan-500/20 dark:ring-cyan-400/20",
  },
  {
    bg: "bg-fuchsia-100 dark:bg-fuchsia-500/15",
    fg: "text-fuchsia-700 dark:text-fuchsia-300",
    ring: "ring-fuchsia-500/20 dark:ring-fuchsia-400/20",
  },
  {
    bg: "bg-teal-100 dark:bg-teal-500/15",
    fg: "text-teal-700 dark:text-teal-300",
    ring: "ring-teal-500/20 dark:ring-teal-400/20",
  },
];

function hash(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) {
    h = (h * 31 + s.charCodeAt(i)) | 0;
  }
  return Math.abs(h);
}

function initials(name: string): string {
  const words = name
    .replace(/[^\p{L}\p{N}\s]/gu, " ")
    .split(/\s+/)
    .filter(Boolean);
  if (words.length === 0) return "··";
  const first = words[0]!;
  if (words.length === 1) return first.slice(0, 2).toUpperCase();
  const second = words[1]!;
  return ((first[0] ?? "") + (second[0] ?? "")).toUpperCase();
}

export interface ProjectAvatarProps {
  id: string;
  name: string;
  size?: "sm" | "md";
  className?: string;
}

export function ProjectAvatar({
  id,
  name,
  size = "md",
  className,
}: ProjectAvatarProps): JSX.Element {
  const palette = PALETTE[hash(id) % PALETTE.length]!;
  const dims =
    size === "sm" ? "h-7 w-7 text-[10px]" : "h-9 w-9 text-xs";
  return (
    <div
      aria-hidden="true"
      className={cn(
        "inline-flex shrink-0 items-center justify-center rounded-md font-semibold tracking-wide ring-1 ring-inset",
        dims,
        palette.bg,
        palette.fg,
        palette.ring,
        className,
      )}
    >
      {initials(name)}
    </div>
  );
}
