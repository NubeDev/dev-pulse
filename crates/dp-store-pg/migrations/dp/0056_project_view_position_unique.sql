-- 0056_project_view_position_unique.sql
--
-- Saved-view ordering integrity (PROJECT-VIEW.md §7.1).
--
-- The original `(project_id, owner_user_id, position)` index was a plain
-- (non-UNIQUE) btree, and `create_project_view` derived the next position
-- from `COUNT(*)`. After a delete the positions are no longer contiguous
-- (deleting position 2 from [0,1,2,3] leaves [0,1,3]), so the next
-- `COUNT(*) = 3` collides with the surviving position 3. The result is
-- multiple views sharing a single position and a non-deterministic
-- tab-strip / Gantt row order.
--
-- This migration (1) compacts existing positions to a gapless 0..N-1
-- sequence per (project, owner) — preserving the current display order —
-- and (2) makes the index UNIQUE so a collision can never be persisted
-- again. The insert path is switched to `MAX(position) + 1` in the same
-- change set (create_project_view_impl), which is gap-tolerant under the
-- new constraint.

-- (1) Re-pack to a gapless 0..N-1 sequence per (project, owner),
--     preserving the existing `ORDER BY position ASC, created_at ASC`
--     display order so nobody's tab strip visibly reshuffles.
WITH ranked AS (
  SELECT id,
         row_number() OVER (
           PARTITION BY project_id, owner_user_id
           ORDER BY position ASC, created_at ASC
         ) - 1 AS new_pos
    FROM dp_project_views
)
UPDATE dp_project_views v
   SET position = r.new_pos
  FROM ranked r
 WHERE v.id = r.id
   AND v.position IS DISTINCT FROM r.new_pos;

-- (2) Replace the non-unique ordering index with a UNIQUE one. It still
--     serves the per-owner list query's WHERE + ORDER BY, and now also
--     rejects any future duplicate position at the database layer.
DROP INDEX IF EXISTS dp_project_views_project_idx;
CREATE UNIQUE INDEX dp_project_views_project_idx
    ON dp_project_views (project_id, owner_user_id, position);
