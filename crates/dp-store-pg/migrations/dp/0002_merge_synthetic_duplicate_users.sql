-- Merge duplicate `dp_users` rows created by the co-author /
-- noreply-login trailer path (negative synthetic `github_id`) into
-- the canonical row produced by the reconciler (positive real
-- `github_id`) when both share the same `login`.
--
-- Future ingests can't recreate these duplicates: the worker now
-- calls `Store::find_user_by_login` and reuses an existing row
-- before minting a synthetic one (see
-- `crates/dp-fetcher/src/worker/handlers.rs::upsert_user_by_login`).
-- This migration is the one-shot backfill for already-stored rows.
--
-- Strategy: for every (login) bucket that has at least one positive
-- `github_id` row and at least one negative `github_id` row,
-- repoint the FK rows in `dp_event_actors` and `dp_memberships`
-- from the synthetic id to the canonical id (skipping conflicts
-- the PK would reject), then delete the now-orphaned synthetic
-- rows.

WITH canonical AS (
    SELECT DISTINCT ON (login) id, login, github_id
    FROM dp_users
    WHERE github_id >= 0 AND deleted_at IS NULL
    ORDER BY login, github_id DESC
),
synthetic AS (
    SELECT u.id, u.login
    FROM dp_users u
    JOIN canonical c ON c.login = u.login
    WHERE u.github_id < 0
),
remap AS (
    SELECT s.id AS from_id, c.id AS to_id
    FROM synthetic s
    JOIN canonical c ON c.login = s.login
)
-- Re-point event_actor rows. Skip the row when an
-- (event_id, to_id, role) row already exists — the PK would
-- collide and a co-author who is also the author of the same
-- event keeps the canonical row.
, ea_update AS (
    UPDATE dp_event_actors ea
    SET user_id = r.to_id
    FROM remap r
    WHERE ea.user_id = r.from_id
      AND NOT EXISTS (
          SELECT 1 FROM dp_event_actors ea2
          WHERE ea2.event_id = ea.event_id
            AND ea2.user_id  = r.to_id
            AND ea2.role     = ea.role
      )
    RETURNING 1
)
SELECT count(*) FROM ea_update;

-- Drop any leftover event_actor rows that would still point at a
-- synthetic id because the (event_id, to_id, role) row already
-- existed and the UPDATE above skipped them.
DELETE FROM dp_event_actors ea
USING dp_users u
WHERE ea.user_id = u.id
  AND u.github_id < 0
  AND EXISTS (
      SELECT 1 FROM dp_users c
      WHERE c.login = u.login AND c.github_id >= 0 AND c.deleted_at IS NULL
  );

-- Same dance for memberships: PK is (user_id, org_id). Move the
-- synthetic's memberships onto the canonical row when no row exists
-- for the canonical user yet; otherwise drop the synthetic one.
WITH canonical AS (
    SELECT DISTINCT ON (login) id, login
    FROM dp_users
    WHERE github_id >= 0 AND deleted_at IS NULL
    ORDER BY login, github_id DESC
),
remap AS (
    SELECT u.id AS from_id, c.id AS to_id
    FROM dp_users u
    JOIN canonical c ON c.login = u.login
    WHERE u.github_id < 0
)
UPDATE dp_memberships m
SET user_id = r.to_id
FROM remap r
WHERE m.user_id = r.from_id
  AND NOT EXISTS (
      SELECT 1 FROM dp_memberships m2
      WHERE m2.user_id = r.to_id AND m2.org_id = m.org_id
  );

DELETE FROM dp_memberships m
USING dp_users u
WHERE m.user_id = u.id
  AND u.github_id < 0
  AND EXISTS (
      SELECT 1 FROM dp_users c
      WHERE c.login = u.login AND c.github_id >= 0 AND c.deleted_at IS NULL
  );

-- Finally, drop the now-unreferenced synthetic rows.
DELETE FROM dp_users u
WHERE u.github_id < 0
  AND EXISTS (
      SELECT 1 FROM dp_users c
      WHERE c.login = u.login AND c.github_id >= 0 AND c.deleted_at IS NULL
  );
