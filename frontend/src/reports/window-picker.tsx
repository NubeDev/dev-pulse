/**
 * Window picker — last week / last month / last quarter / custom +
 * TZ-anchor selector. Emits a `WindowState` the user-report page
 * folds straight into the `ReportParams` envelope on `useQuery`.
 *
 * SCOPE §0.4 contract: `{label, tz, anchor}`. Resolution to UTC
 * `[start, end)` is server-side; the picker just hands the labels
 * across the wire.
 *
 * Layout: this component renders its Label+Select pairs as *bare*
 * grid cells (no wrapping Card / border). The parent report page
 * owns the filter Card and the responsive grid; the picker is one
 * row of fields inside it, sitting alongside the entity selector
 * (User/Team/Org). The shared grid keeps the rhythm consistent across
 * every report page.
 */

import { useId } from "react";
import { Input } from "@nube/starter-ui-kit/components/input";
import { Label } from "@nube/starter-ui-kit/components/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@nube/starter-ui-kit/components/select";

import type { WindowAnchor, WindowLabel } from "../api/client.js";

/** Internal preset list — superset of dp-rest's `WindowLabel`. */
export type WindowPreset =
  | "last_7_days"
  | "last_30_days"
  | "last_90_days"
  | "last_week"
  | "last_month"
  | "custom";

const PRESETS: ReadonlyArray<{ value: WindowPreset; label: string }> = [
  { value: "last_7_days", label: "Last 7 days" },
  { value: "last_week", label: "Last week" },
  { value: "last_month", label: "Last month" },
  { value: "last_90_days", label: "Last quarter (90d)" },
  { value: "last_30_days", label: "Last 30 days" },
  { value: "custom", label: "Custom range" },
];

/** Common IANA TZs operators reach for. */
const TZ_OPTIONS: readonly string[] = [
  "UTC",
  "Europe/London",
  "Europe/Berlin",
  "America/New_York",
  "America/Los_Angeles",
  "Asia/Tokyo",
  "Australia/Sydney",
];

export interface WindowState {
  preset: WindowPreset;
  tz: string;
  anchor: WindowAnchor;
  /** RFC3339 UTC; only meaningful when `preset === "custom"`. */
  custom_start?: string;
  /** RFC3339 UTC; only meaningful when `preset === "custom"`. */
  custom_end?: string;
}

export function defaultWindowState(): WindowState {
  return { preset: "last_7_days", tz: "UTC", anchor: "utc" };
}

export function windowStateToParams(
  s: WindowState,
): Pick<
  {
    window_label: WindowLabel;
    tz: string;
    anchor: WindowAnchor;
    custom_start?: string;
    custom_end?: string;
  },
  "window_label" | "tz" | "anchor" | "custom_start" | "custom_end"
> {
  return {
    window_label: s.preset,
    tz: s.tz,
    anchor: s.anchor,
    custom_start: s.preset === "custom" ? s.custom_start : undefined,
    custom_end: s.preset === "custom" ? s.custom_end : undefined,
  };
}

export interface WindowPickerProps {
  value: WindowState;
  onChange: (next: WindowState) => void;
}

/**
 * Render the four picker fields (Window / Time zone / Anchor /
 * optional Custom start+end) as fragment grid cells. The parent grid
 * owns the column flow.
 */
export function WindowPicker({ value, onChange }: WindowPickerProps): JSX.Element {
  const presetId = useId();
  const tzId = useId();
  const anchorId = useId();
  const startId = useId();
  const endId = useId();

  function patch(p: Partial<WindowState>): void {
    onChange({ ...value, ...p });
  }

  function toLocalInput(rfc3339: string | undefined): string {
    if (!rfc3339) return "";
    const m = /^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2})/.exec(rfc3339);
    return m ? (m[1] ?? "") : "";
  }
  function fromLocalInput(local: string): string | undefined {
    if (!local) return undefined;
    return `${local}:00Z`;
  }

  return (
    <>
      <div className="grid gap-1.5">
        <Label htmlFor={presetId}>Window</Label>
        <Select
          value={value.preset}
          onValueChange={(v) => patch({ preset: v as WindowPreset })}
        >
          <SelectTrigger id={presetId}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {PRESETS.map((p) => (
              <SelectItem key={p.value} value={p.value}>{p.label}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="grid gap-1.5">
        <Label htmlFor={tzId}>Time zone</Label>
        <Select value={value.tz} onValueChange={(v) => patch({ tz: v })}>
          <SelectTrigger id={tzId}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {TZ_OPTIONS.map((tz) => (
              <SelectItem key={tz} value={tz}>{tz}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="grid gap-1.5">
        <Label htmlFor={anchorId}>Anchor</Label>
        <Select
          value={value.anchor}
          onValueChange={(v) => patch({ anchor: v as WindowAnchor })}
        >
          <SelectTrigger id={anchorId}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="utc">UTC</SelectItem>
            <SelectItem value="viewer">Viewer TZ</SelectItem>
            <SelectItem value="org">Org TZ</SelectItem>
          </SelectContent>
        </Select>
      </div>

      {value.preset === "custom" && (
        <>
          <div className="grid gap-1.5">
            <Label htmlFor={startId}>Start</Label>
            <Input
              id={startId}
              type="datetime-local"
              value={toLocalInput(value.custom_start)}
              onChange={(e) => patch({ custom_start: fromLocalInput(e.target.value) })}
            />
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor={endId}>End</Label>
            <Input
              id={endId}
              type="datetime-local"
              value={toLocalInput(value.custom_end)}
              onChange={(e) => patch({ custom_end: fromLocalInput(e.target.value) })}
            />
          </div>
        </>
      )}
    </>
  );
}

/**
 * Shared Tailwind class for the parent filter grid. Re-used by every
 * report page so the column flow stays consistent.
 */
export const FILTER_GRID_CLASS =
  "grid grid-cols-[repeat(auto-fit,minmax(12rem,1fr))] gap-3";
