/**
 * Per-view due-date display preference — extracted from the
 * old `<ViewsTabStrip>` so the wizard / edit dialogs can share
 * the helpers without dragging the whole strip module along.
 *
 * Three modes selectable from the edit dialog:
 *   - "hide"  — no badge on the tab.
 *   - "week"  — "Nth week of <Month>" (default). When the due
 *     year differs from the current year the year is appended.
 *   - "date"  — compact `DD:Mon:YY` (abbreviated month).
 *
 * Persisted in `localStorage` keyed by view id — machine-local
 * but survives reloads; no backend migration required.
 */

const MONTHS = [
  "January", "February", "March", "April", "May", "June",
  "July", "August", "September", "October", "November", "December",
];

const ORDINALS = ["1st", "2nd", "3rd", "4th", "5th"];

const MONTH_ABBR = [
  "Jan", "Feb", "Mar", "Apr", "May", "Jun",
  "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/** Map `YYYY-MM-DD` to a "Nth week of <Month>" label.
 *  Week boundaries: 1-7, 8-14, 15-21, 22-28, 29-end. */
export function weekOfMonthLabel(iso: string): string {
  const m = iso.match(/^(\d{4})-(\d{2})-(\d{2})$/);
  if (!m) return "";
  const month = Number(m[2]);
  const day = Number(m[3]);
  if (month < 1 || month > 12 || day < 1) return "";
  const weekIdx = Math.min(Math.floor((day - 1) / 7), 4);
  return `${ORDINALS[weekIdx]} week of ${MONTHS[month - 1]}`;
}

/** Render a `YYYY-MM-DD` as AU `dd/mm/yyyy`. */
export function formatAu(iso: string): string {
  const m = iso.match(/^(\d{4})-(\d{2})-(\d{2})$/);
  if (!m) return iso;
  return `${m[3]}/${m[2]}/${m[1]}`;
}

export type DateDisplayMode = "hide" | "week" | "date";

const DATE_DISPLAY_DEFAULT: DateDisplayMode = "week";
const DATE_DISPLAY_LS_PREFIX = "dp.projectView.dateDisplay.";

function isDateDisplayMode(v: unknown): v is DateDisplayMode {
  return v === "hide" || v === "week" || v === "date";
}

export function readDateDisplayMode(viewId: string): DateDisplayMode {
  if (typeof window === "undefined") return DATE_DISPLAY_DEFAULT;
  try {
    const raw = window.localStorage.getItem(DATE_DISPLAY_LS_PREFIX + viewId);
    return isDateDisplayMode(raw) ? raw : DATE_DISPLAY_DEFAULT;
  } catch {
    return DATE_DISPLAY_DEFAULT;
  }
}

export function writeDateDisplayMode(
  viewId: string,
  mode: DateDisplayMode,
): void {
  if (typeof window === "undefined") return;
  try {
    if (mode === DATE_DISPLAY_DEFAULT) {
      window.localStorage.removeItem(DATE_DISPLAY_LS_PREFIX + viewId);
    } else {
      window.localStorage.setItem(DATE_DISPLAY_LS_PREFIX + viewId, mode);
    }
  } catch {
    // ignore
  }
}

/** Format `iso` per `mode`. Returns `null` when the badge is
 *  hidden. The "week" mode appends the year whenever the due
 *  year differs from the current calendar year. */
export function formatDateDisplay(
  iso: string,
  mode: DateDisplayMode,
): string | null {
  if (mode === "hide") return null;
  const m = iso.match(/^(\d{4})-(\d{2})-(\d{2})$/);
  if (!m) return null;
  if (mode === "date") {
    const monthIdx = Number(m[2]) - 1;
    const mon =
      monthIdx >= 0 && monthIdx < 12 ? MONTH_ABBR[monthIdx] : m[2];
    return `${m[3]}:${mon}:${m[1]!.slice(2)}`;
  }
  const base = weekOfMonthLabel(iso);
  if (!base) return null;
  const dueYear = Number(m[1]);
  const thisYear = new Date().getFullYear();
  return dueYear === thisYear ? base : `${base} ${dueYear}`;
}

// ---------------------------------------------------------------------------
// Per-view "completed" flag (machine-local, sibling of date-display).
// ---------------------------------------------------------------------------

const COMPLETED_LS_PREFIX = "dp.projectView.completed.";

export function readCompleted(viewId: string): boolean {
  if (typeof window === "undefined") return false;
  try {
    return window.localStorage.getItem(COMPLETED_LS_PREFIX + viewId) === "1";
  } catch {
    return false;
  }
}

export function writeCompleted(viewId: string, completed: boolean): void {
  if (typeof window === "undefined") return;
  try {
    if (completed) {
      window.localStorage.setItem(COMPLETED_LS_PREFIX + viewId, "1");
    } else {
      window.localStorage.removeItem(COMPLETED_LS_PREFIX + viewId);
    }
  } catch {
    // ignore
  }
}
