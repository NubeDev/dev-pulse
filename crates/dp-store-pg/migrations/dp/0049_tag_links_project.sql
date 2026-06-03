-- Add `project` as a fifth tag-link target kind (dev-pulse "tags for
-- projects" surface). The polymorphic edge in `dp_tag_links`
-- (migration 0005) covered repo|issue|user|team; projects are the
-- cross-org grouping concept the portfolio surface needs to tag.
--
-- Convention: projects-issues track owns odd migration numbers, so
-- this lands at `0049` (next free odd after `0047_user_role.sql`).
--
-- Three structural changes, in order:
--
--   1. Add the nullable `target_project_id` column with the same
--      `ON DELETE CASCADE` semantics as the other four target FKs —
--      deleting a project drops its tag links, never the tag.
--   2. Replace the two inline CHECK constraints from 0005 (the
--      `kind IN (...)` check and the exactly-one-target polymorphism
--      check). They were created unnamed, so we drop every CHECK on
--      the table by introspection and re-add both as named
--      constraints that now admit `project`.
--   3. Rebuild the uniqueness index (the COALESCE must include the
--      new target column) and add the per-kind reverse-lookup index.

ALTER TABLE dp_tag_links
    ADD COLUMN target_project_id UUID NULL REFERENCES dp_projects(id) ON DELETE CASCADE;

-- Drop the unnamed inline CHECKs from 0005 so we can widen them.
DO $$
DECLARE c text;
BEGIN
    FOR c IN
        SELECT conname FROM pg_constraint
         WHERE conrelid = 'dp_tag_links'::regclass AND contype = 'c'
    LOOP
        EXECUTE format('ALTER TABLE dp_tag_links DROP CONSTRAINT %I', c);
    END LOOP;
END $$;

ALTER TABLE dp_tag_links
    ADD CONSTRAINT dp_tag_links_kind_check
        CHECK (kind IN ('repo', 'issue', 'user', 'team', 'project'));

ALTER TABLE dp_tag_links
    ADD CONSTRAINT dp_tag_links_target_check CHECK (
        (kind = 'repo'
            AND target_repo_id    IS NOT NULL
            AND target_issue_id   IS NULL
            AND target_user_id    IS NULL
            AND target_team_id    IS NULL
            AND target_project_id IS NULL)
     OR (kind = 'issue'
            AND target_issue_id   IS NOT NULL
            AND target_repo_id    IS NULL
            AND target_user_id    IS NULL
            AND target_team_id    IS NULL
            AND target_project_id IS NULL)
     OR (kind = 'user'
            AND target_user_id    IS NOT NULL
            AND target_repo_id    IS NULL
            AND target_issue_id   IS NULL
            AND target_team_id    IS NULL
            AND target_project_id IS NULL)
     OR (kind = 'team'
            AND target_team_id    IS NOT NULL
            AND target_repo_id    IS NULL
            AND target_issue_id   IS NULL
            AND target_user_id    IS NULL
            AND target_project_id IS NULL)
     OR (kind = 'project'
            AND target_project_id IS NOT NULL
            AND target_repo_id    IS NULL
            AND target_issue_id   IS NULL
            AND target_user_id    IS NULL
            AND target_team_id    IS NULL)
    );

-- (tag_id, kind, target) uniqueness — the COALESCE must now also
-- collapse target_project_id, else two project links on one tag would
-- be allowed.
DROP INDEX IF EXISTS dp_tag_links_tag_target_uniq;
CREATE UNIQUE INDEX dp_tag_links_tag_target_uniq
    ON dp_tag_links (
        tag_id,
        kind,
        COALESCE(target_repo_id, target_issue_id, target_user_id,
                 target_team_id, target_project_id)
    );

-- Reverse lookup: "what tags link this project?" (portfolio column +
-- the `GET /projects/{id}/tags` route).
CREATE INDEX dp_tag_links_project_idx
    ON dp_tag_links (target_project_id) WHERE kind = 'project';
