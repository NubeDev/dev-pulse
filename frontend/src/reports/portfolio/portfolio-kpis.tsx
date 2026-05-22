import type { ReactNode } from "react";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { cn } from "@/lib/utils";

export function relativeDue(
  due: string | null | undefined,
  nowMs: number,
): { label: string; tone: "ok" | "soon" | "overdue" } | null {
  if (!due) return null;
  const target = new Date(due).getTime();
  if (Number.isNaN(target)) return null;
  const oneDay = 86_400_000;
  const days = Math.round((target - nowMs) / oneDay);
  if (days < 0) return { label: `${Math.abs(days)}d overdue`, tone: "overdue" };
  if (days === 0) return { label: "due today", tone: "soon" };
  if (days <= 7) return { label: `due in ${days}d`, tone: "soon" };
  return { label: `due in ${days}d`, tone: "ok" };
}

export function KpiTile({
  label,
  value,
  hint,
  tone = "neutral",
  icon: Icon,
}: {
  label: string;
  value: string | number;
  hint?: string;
  tone?: "neutral" | "good" | "warn" | "bad";
  icon?: React.ComponentType<{ className?: string }>;
}): JSX.Element {
  const toneRing: Record<"neutral" | "good" | "warn" | "bad", string> = {
    neutral:
      "bg-slate-100 text-slate-700 ring-slate-500/20 dark:bg-slate-500/15 dark:text-slate-300 dark:ring-slate-400/20",
    good:
      "bg-emerald-100 text-emerald-700 ring-emerald-500/20 dark:bg-emerald-500/15 dark:text-emerald-300 dark:ring-emerald-400/20",
    warn:
      "bg-amber-100 text-amber-700 ring-amber-500/20 dark:bg-amber-500/15 dark:text-amber-300 dark:ring-amber-400/20",
    bad:
      "bg-rose-100 text-rose-700 ring-rose-500/20 dark:bg-rose-500/15 dark:text-rose-300 dark:ring-rose-400/20",
  };
  return (
    <Card className="gap-2 py-4">
      <CardHeader className="flex flex-row items-center justify-between px-4">
        <CardTitle className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
          {label}
        </CardTitle>
        {Icon ? (
          <span
            className={cn(
              "inline-flex size-7 items-center justify-center rounded-md ring-1 ring-inset",
              toneRing[tone],
            )}
          >
            <Icon className="size-3.5" />
          </span>
        ) : null}
      </CardHeader>
      <CardContent className="px-4">
        <div className="text-2xl font-semibold tabular-nums">{value}</div>
        {hint ? (
          <div className="text-xs text-muted-foreground">{hint}</div>
        ) : null}
      </CardContent>
    </Card>
  );
}

export function StackedStrip({
  segments,
}: {
  segments: { key: string; value: number; color: string }[];
}): JSX.Element {
  const total = segments.reduce((a, s) => a + s.value, 0);
  if (total === 0) return <div className="h-6 rounded bg-muted" />;
  return (
    <div className="flex h-6 overflow-hidden rounded bg-muted">
      {segments.map((s) =>
        s.value > 0 ? (
          <div
            key={s.key}
            style={{
              width: `${(s.value / total) * 100}%`,
              background: s.color,
            }}
            aria-label={`${s.key}: ${s.value}`}
          />
        ) : null,
      )}
    </div>
  );
}

export function Legend({
  color,
  label,
  children,
}: {
  color: string;
  label: string;
  children: ReactNode;
}): JSX.Element {
  return (
    <span className="flex items-center gap-1.5">
      <span aria-hidden className="size-2 rounded-sm" style={{ background: color }} />
      <span className="text-muted-foreground">{label}</span>
      <span className="tabular-nums">{children}</span>
    </span>
  );
}
