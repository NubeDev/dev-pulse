/**
 * Version numbering for the Executive Summary change log.
 *
 * The user never types a version by hand — they only choose "minor"
 * or "major". We derive the next number from the highest version
 * already in the change log:
 *
 *   (nothing yet)  --any-->  v1
 *   v1             --minor-> v1.1  --minor-> v1.2 …
 *   v1.2           --major-> v2
 *   v2             --minor-> v2.1 …
 *
 * Historical entries may carry free-form version strings (semver,
 * internal codes). We parse what we can and ignore the rest, so a
 * stray "0.1.0-rc" never blocks cutting the next clean version.
 */

import type { ExecSummaryDto } from "../../api/client.js";

export type VersionBump = "minor" | "major";

export interface ParsedVersion {
  major: number;
  minor: number;
}

/** Pull a `{major, minor}` out of a version string like `v1`, `v1.1`,
 *  `1.2.0`. Returns `null` when there's no leading number to read. */
export function parseVersion(raw: string): ParsedVersion | null {
  const m = raw.trim().match(/v?(\d+)(?:\.(\d+))?/i);
  if (!m) return null;
  return { major: Number(m[1]), minor: m[2] ? Number(m[2]) : 0 };
}

/** `v1` when minor is 0, otherwise `v1.1`. */
export function formatVersion({ major, minor }: ParsedVersion): string {
  return minor === 0 ? `v${major}` : `v${major}.${minor}`;
}

/** Highest version across the change log, or `null` if none parse. */
function highestVersion(entries: ExecSummaryDto["changelog"]): ParsedVersion | null {
  let top: ParsedVersion | null = null;
  for (const e of entries) {
    const p = parseVersion(e.version);
    if (!p) continue;
    if (
      !top ||
      p.major > top.major ||
      (p.major === top.major && p.minor > top.minor)
    ) {
      top = p;
    }
  }
  return top;
}

/** Next version label for the requested bump. The first version is
 *  always `v1` regardless of bump (we start the series at v1). */
export function computeNextVersion(
  entries: ExecSummaryDto["changelog"],
  bump: VersionBump,
): string {
  const top = highestVersion(entries);
  if (!top) return "v1";
  if (bump === "major") return formatVersion({ major: top.major + 1, minor: 0 });
  const major = top.major < 1 ? 1 : top.major;
  return formatVersion({ major, minor: top.minor + 1 });
}

/** Timestamp of the most recently cut version, or `null` if the
 *  summary has never been versioned. */
function lastVersionedAt(data: ExecSummaryDto): string | null {
  let latest: string | null = null;
  for (const e of data.changelog) {
    if (!latest || e.created_at > latest) latest = e.created_at;
  }
  return latest;
}

/** True when the summary has been edited since the last version was
 *  cut (or has never been versioned) — drives the "Pending changes"
 *  badge and nudges the user to save a new version. */
export function hasPendingChanges(data: ExecSummaryDto): boolean {
  const latest = lastVersionedAt(data);
  if (!latest) return true;
  return new Date(data.updated_at).getTime() > new Date(latest).getTime();
}
