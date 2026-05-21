-- 0035_project_primary_milestone.sql
--
-- PROJECT-VIEW.md §5.5 / §9.5 — adopt a milestone as the project's
-- primary planning anchor. The pointer enables:
--
--   * `★ primary` chip on the §5.5 milestones strip card;
--   * "Due in primary milestone" smart view on the §6.5 detail-pane;
--   * Eventual `?filter=milestone:primary` shorthand (Slice 5.x).
--
-- Nullable + ON DELETE SET NULL so dropping a repo (which cascades
-- to its milestones, see migration 0030) automatically detaches the
-- project's primary pointer instead of leaving a dangling row. Set
-- / cleared by `POST /projects/{id}/adopt-milestone` (Slice 5).
--
-- No unique constraint: many projects can share the same primary
-- milestone (e.g. cross-team push to a shared release date), and a
-- project can have at most one (column-level singleton — the
-- pointer's whole point).

ALTER TABLE dp_projects
    ADD COLUMN primary_milestone_id uuid NULL
        REFERENCES dp_milestones(id) ON DELETE SET NULL;
