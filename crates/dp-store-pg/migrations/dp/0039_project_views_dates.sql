-- 0039_project_views_dates.sql
--
-- Adds optional start_date / due_date to dp_project_views so a saved
-- view can carry its own timeline (independent of the parent
-- project's dates). Both columns are nullable DATE — tz-agnostic to
-- match `dp_milestones.due_on` (PROJECT-VIEW.md §5.5) and rendered
-- in AU dd/mm/yyyy by the workbench.
--
-- No CHECK on (start_date <= due_date); the UI keeps the picker
-- pair consistent and we don't want to reject legacy rows that may
-- have been seeded out of order during the rollout.

ALTER TABLE dp_project_views
    ADD COLUMN start_date DATE,
    ADD COLUMN due_date   DATE;
