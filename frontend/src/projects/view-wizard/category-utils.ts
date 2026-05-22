/**
 * Category helpers — categories are collapsible sections rendered
 * INSIDE a single view (see `templates.ts` `categorised` kind and
 * `ProjectViewDto.categories`). Each category maps to one DP tag
 * named `category:<slug>` at the project's org scope; when an
 * issue isn't local that tag mirrors to a GitHub label (tagging
 * .md §2 / §5), giving users a single mental model:
 * "tag = category = label".
 *
 * Centralised so the wizard, the edit dialog, the workbench, and
 * the add-issue dialog all agree on key normalisation, tag
 * naming, and tag lookup.
 */

import { api, type TagDto } from "../../api/client.js";

/** The kv tag KEY used for category sections. The VALUE is the
 *  category slug (`category:hardware`, `category:firmware`, …). */
export const CATEGORY_TAG_KEY = "category";

/** Lowercase a free-form category label into the §3 tagging
 *  grammar — `[a-z0-9_-]`, no leading/trailing dash, max 50
 *  chars (the GitHub topic ceiling so it stays portable). */
export function slugifyCategoryKey(input: string): string {
  return input
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/-+/g, "-")
    .replace(/^[-_]+|[-_]+$/g, "")
    .slice(0, 50);
}

/** Canonical tag name for a category key (`category:<key>`). */
export function categoryTagName(key: string): string {
  return `${CATEGORY_TAG_KEY}:${key}`;
}

/** Find an existing org-scoped tag matching the given category
 *  key. Returns `null` when not present; callers should fall
 *  back to [`ensureCategoryTag`]. */
export function findCategoryTag(
  tags: TagDto[],
  orgId: string,
  key: string,
): TagDto | null {
  const needle = categoryTagName(key).toLowerCase();
  return (
    tags.find(
      (t) =>
        t.scope_kind === "org" &&
        t.scope_id === orgId &&
        t.name.toLowerCase() === needle &&
        !t.archived_at,
    ) ?? null
  );
}

/** Idempotently create the org-scoped `category:<key>` tag. If
 *  the server returns `409 already_exists` we swallow it and
 *  return `null` so the caller falls back to a refetch — the
 *  uniqueness index on `(scope_kind, scope_id, lower(name))`
 *  guarantees there's at most one. */
export async function ensureCategoryTag(
  orgId: string,
  key: string,
): Promise<TagDto | null> {
  try {
    return await api.createTag({
      scope_kind: "org",
      scope_id: orgId,
      name: categoryTagName(key),
      color: pickCategoryColor(key),
    });
  } catch (err) {
    // 409 = tag already exists; any other error bubbles so the
    // wizard's error banner surfaces it.
    if (err && typeof err === "object" && "status" in err && (err as { status: number }).status === 409) {
      return null;
    }
    throw err;
  }
}

/** Stable hash → palette so the same category key always
 *  picks the same colour across users. The colour set
 *  matches the tag chip palette used elsewhere. */
const CATEGORY_PALETTE = [
  "#6366f1", // indigo
  "#ef4444", // red
  "#f97316", // orange
  "#eab308", // amber
  "#22c55e", // green
  "#14b8a6", // teal
  "#0ea5e9", // sky
  "#8b5cf6", // violet
  "#ec4899", // pink
];

function pickCategoryColor(key: string): string {
  let h = 0;
  for (let i = 0; i < key.length; i++) {
    h = (h * 31 + key.charCodeAt(i)) >>> 0;
  }
  return CATEGORY_PALETTE[h % CATEGORY_PALETTE.length]!;
}

/** Ensure every category in `keys` has a backing org-scoped tag.
 *  Sequenced (not parallel) so back-to-back 409 collisions are
 *  easy to reason about. Errors short-circuit so the caller can
 *  surface them before persisting the view. */
export async function ensureCategoryTags(
  orgId: string,
  keys: readonly string[],
): Promise<void> {
  for (const key of keys) {
    if (key.length === 0) continue;
    await ensureCategoryTag(orgId, key);
  }
}
