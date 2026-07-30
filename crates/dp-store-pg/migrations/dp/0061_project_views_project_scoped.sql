-- 0061_project_views_project_scoped.sql
--
-- Saved views become **project-scoped**, not per-user.
--
-- Until now every row in `dp_project_views` was owned by its creator and
-- the list query filtered on `owner_user_id = $caller`, with `visibility`
-- ('private' | 'project') reserved for a shared-views slice that never
-- shipped — the REST create-body defaulted to 'private' and the frontend
-- never sent the field, so *every* view in production is 'private'. The
-- practical effect: a view created by user A was invisible to user B on
-- the same project, which reads as "this user can't see the views".
--
-- Views are a property of the project, not of whoever happened to click
-- "+". This migration makes that the schema-level truth:
--
--   (1) Every existing row is flipped to visibility='project'.
--   (2) The per-owner UNIQUE (project_id, owner_user_id, name) is
--       replaced by UNIQUE (project_id, name). Two users may each hold a
--       view named 'All' on one project today, so duplicates are
--       auto-renamed (oldest keeps the bare name; later ones get a
--       ' (2)', ' (3)', … suffix) before the constraint is applied.
--   (3) The per-owner UNIQUE (project_id, owner_user_id, position) is
--       replaced by UNIQUE (project_id, position), re-packing positions
--       to a gapless 0..N-1 sequence per project.
--
-- `owner_user_id` is deliberately KEPT — it degrades from an access-control
-- key to a plain "created by" record, still useful for audit/attribution.
-- The column stays NOT NULL; nothing writes it differently.

-- (1) All existing views become shared. `visibility` is retained (rather
--     than dropped) so the column can still express a future per-user
--     scratch view without another schema change; nothing sets 'private'
--     today.
UPDATE dp_project_views
   SET visibility = 'project'
 WHERE visibility IS DISTINCT FROM 'project';

-- (2) Resolve name collisions before the project-wide UNIQUE lands.
--     Ordering by (created_at, id) makes the winner deterministic and
--     stable across re-runs: the oldest view keeps its exact name.
--     `id` breaks ties on identical timestamps so the result never
--     depends on physical row order.
--
--     The suffix is applied in a loop because a rename can itself collide
--     with an unrelated pre-existing view (e.g. rows named 'All', 'All'
--     and 'All (2)' — the second 'All' wants 'All (2)', which is taken).
--     Each pass renames only rows that are still duplicated and probes
--     upward for a free suffix, so it converges.
DO $$
DECLARE
    dup RECORD;
    candidate TEXT;
    suffix INT;
BEGIN
    LOOP
        SELECT * INTO dup
          FROM (
            SELECT id, project_id, name,
                   row_number() OVER (
                     PARTITION BY project_id, name
                     ORDER BY created_at ASC, id ASC
                   ) AS rn
              FROM dp_project_views
          ) ranked
         WHERE ranked.rn > 1
         LIMIT 1;

        EXIT WHEN NOT FOUND;

        -- Probe upward from ' (2)' for the first free name on this
        -- project. Truncate to the 60-char CHECK on `name` if the
        -- suffixed value would overflow it.
        suffix := 2;
        LOOP
            candidate := dup.name || ' (' || suffix || ')';
            IF length(candidate) > 60 THEN
                candidate := left(dup.name, 60 - length(' (' || suffix || ')'))
                             || ' (' || suffix || ')';
            END IF;
            EXIT WHEN NOT EXISTS (
                SELECT 1 FROM dp_project_views
                 WHERE project_id = dup.project_id
                   AND name = candidate
            );
            suffix := suffix + 1;
        END LOOP;

        UPDATE dp_project_views
           SET name = candidate
         WHERE id = dup.id;
    END LOOP;
END $$;

ALTER TABLE dp_project_views
    DROP CONSTRAINT IF EXISTS dp_project_views_project_id_owner_user_id_name_key;

ALTER TABLE dp_project_views
    ADD CONSTRAINT dp_project_views_project_name_key
    UNIQUE (project_id, name);

-- (3) Re-pack positions per project (they were only unique per owner, so
--     two users' strips both start at 0 and would collide immediately
--     under a project-wide UNIQUE). Existing relative order is preserved
--     via the same ORDER BY the list query used.
--     The old per-owner UNIQUE index (0056: project_id, owner_user_id,
--     position) MUST be dropped BEFORE the re-pack, not after. Renumbering
--     project-wide moves each owner's rows onto values that another of
--     *that same owner's* rows still holds mid-statement: with two owners
--     holding positions 0..7 each, owner B's block becomes 8..15, and
--     owner A's row moving 3->3 is fine but B's 3->11 passes through
--     values B already occupies. Postgres checks the unique index per
--     row-update, so the permutation trips the stale constraint and the
--     whole migration aborts.
DROP INDEX IF EXISTS dp_project_views_project_idx;

WITH ranked AS (
  SELECT id,
         row_number() OVER (
           PARTITION BY project_id
           ORDER BY position ASC, created_at ASC, id ASC
         ) - 1 AS new_pos
    FROM dp_project_views
)
UPDATE dp_project_views v
   SET position = r.new_pos
  FROM ranked r
 WHERE v.id = r.id
   AND v.position IS DISTINCT FROM r.new_pos;

CREATE UNIQUE INDEX dp_project_views_project_idx
    ON dp_project_views (project_id, position);
