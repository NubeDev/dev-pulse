-- 0041_local_issues.sql
--
-- Adds the "local-only issue" lane (SCOPE.md §4.1 amendment): an
-- issue row that lives in dev-pulse but has *not* been pushed to
-- GitHub. The user-visible entry point is the "Create" button on
-- the project's Add-issue → Create new dialog (the sibling of
-- "Create and sync to GitHub").
--
-- Design notes:
--   * `is_local` is a single boolean on `dp_issues`; the existing
--     queries that don't care (read-side filters, project membership
--     joins, milestones) continue to work unchanged.
--   * `repo_id` stays NOT NULL — every local issue is still scoped
--     to one of the project's linked repos (the dialog's "Repo"
--     picker), so org-scope / repo-scope filters keep working.
--   * `github_id` and `number` stay NOT NULL too. We allocate
--     synthetic *negative* values per-repo via
--     `dp_repos.local_issue_counter`, mirroring the same trick
--     `dp_users` uses for synthetic ids (negative = "not really a
--     github id"). The existing UNIQUE (repo_id, github_id) and
--     UNIQUE (repo_id, number) constraints keep their teeth — two
--     local issues in the same repo get distinct negative numbers.
--   * If/when a local issue is later promoted ("Sync to GitHub"),
--     its row is UPDATEd in place: `is_local = FALSE`,
--     `github_id` / `number` / `github_node_id` rewritten from the
--     GitHub POST response. The row keeps its `id` (and therefore
--     every project-membership / tag link / inbox-state row that
--     points at it).

ALTER TABLE dp_issues
    ADD COLUMN is_local BOOLEAN NOT NULL DEFAULT FALSE;

-- Per-repo counter for synthetic local issue numbers. Starts at 0
-- and is decremented on every local-issue insert, so the first
-- local issue gets number -1, the second -2, and so on. Kept on
-- `dp_repos` rather than a global sequence so each repo's negative
-- space is independent (mirrors GitHub's per-repo number space).
ALTER TABLE dp_repos
    ADD COLUMN local_issue_counter BIGINT NOT NULL DEFAULT 0;
