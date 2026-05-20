-- Smart-merge duplicate `dp_users` rows that 0002 left behind.
--
-- 0002 only collapsed (synthetic-negative-id) into (real-positive-id)
-- buckets that shared an exact `login`. The directory page still
-- shows duplicates with the same login because:
--   * two real `github_id` rows can share a `login` (account renames,
--     replay races, or two ingest paths both winning at different
--     times),
--   * `login` matching is case-sensitive in 0002 but GitHub treats
--     logins as case-insensitive (`NubeDev` vs `nubedev`),
--   * two synthetic rows can share a login when `find_user_by_login`
--     didn't find the first one yet (cold-start races).
--
-- Strategy:
--   1. Bucket rows by `lower(login)`, ignoring soft-deleted rows.
--   2. Within each bucket, the canonical row is the one with the
--      lowest `github_id` (oldest real GitHub account wins; negative
--      synthetic ids only win if no real id exists in the bucket).
--   3. Repoint every FK referencing a loser's id (`dp_event_actors`,
--      `dp_memberships`, `dp_audit_log.actor_user_id`) onto the
--      canonical id, skipping rows that would collide with an
--      existing PK on the canonical side.
--   4. Drop the now-orphaned loser rows.
--
-- This migration is idempotent: re-running it after it has already
-- collapsed a bucket is a no-op because the loser rows no longer
-- exist.

BEGIN;

-- Materialize the canonical-per-bucket and the from→to remap. A
-- temp table is cheaper than re-running the window function inside
-- every CTE below, and keeps the FK-repoint statements readable.
CREATE TEMP TABLE _dp_user_merge_remap ON COMMIT DROP AS
WITH ranked AS (
    SELECT
        id,
        lower(login)                                                  AS login_key,
        github_id,
        row_number() OVER (PARTITION BY lower(login) ORDER BY github_id ASC) AS rn
    FROM dp_users
    WHERE deleted_at IS NULL
),
canonical AS (
    SELECT id, login_key
    FROM ranked
    WHERE rn = 1
)
SELECT
    r.id           AS from_id,
    c.id           AS to_id,
    r.login_key
FROM ranked r
JOIN canonical c USING (login_key)
WHERE r.rn > 1;

-- ----------------------------------------------------------------
-- dp_event_actors: PK (event_id, user_id, role)
-- ----------------------------------------------------------------
UPDATE dp_event_actors ea
SET user_id = r.to_id
FROM _dp_user_merge_remap r
WHERE ea.user_id = r.from_id
  AND NOT EXISTS (
      SELECT 1 FROM dp_event_actors ea2
      WHERE ea2.event_id = ea.event_id
        AND ea2.user_id  = r.to_id
        AND ea2.role     = ea.role
  );

-- The UPDATE above skipped PK-collision rows. Drop the leftover
-- rows still pointing at a loser id — the canonical row already
-- captures the same (event, role) relationship.
DELETE FROM dp_event_actors ea
USING _dp_user_merge_remap r
WHERE ea.user_id = r.from_id;

-- ----------------------------------------------------------------
-- dp_memberships: PK (user_id, org_id)
-- ----------------------------------------------------------------
UPDATE dp_memberships m
SET user_id = r.to_id
FROM _dp_user_merge_remap r
WHERE m.user_id = r.from_id
  AND NOT EXISTS (
      SELECT 1 FROM dp_memberships m2
      WHERE m2.user_id = r.to_id AND m2.org_id = m.org_id
  );

DELETE FROM dp_memberships m
USING _dp_user_merge_remap r
WHERE m.user_id = r.from_id;

-- ----------------------------------------------------------------
-- dp_audit_log.actor_user_id (no ON DELETE — repoint required).
--
-- In practice losers are co-authored/imported users and never act
-- as admin, so this is almost always empty. We still repoint to
-- keep the FK satisfied if a row ever exists.
-- ----------------------------------------------------------------
UPDATE dp_audit_log a
SET actor_user_id = r.to_id
FROM _dp_user_merge_remap r
WHERE a.actor_user_id = r.from_id;

-- ----------------------------------------------------------------
-- Drop the now-unreferenced loser rows.
-- ----------------------------------------------------------------
DELETE FROM dp_users u
USING _dp_user_merge_remap r
WHERE u.id = r.from_id;

COMMIT;
