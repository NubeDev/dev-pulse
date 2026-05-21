-- Per-saved-view issue membership (PROJECT-VIEW.md §5.4 amendment).
--
-- A row here marks an issue as "manually placed" on a saved view's
-- tab. The view's own filter/group/sort still apply on top as
-- presentation refinements; this table is what makes tabs feel like
-- containers ("issue added on tab X stays on tab X") rather than
-- pure saved searches.
--
-- The implicit `All` tab (no `?view=` in the URL) keeps the
-- pre-existing project-level semantics and does NOT use this table.
--
-- Cascades:
--   * Delete the view  → drop its memberships (the view is gone, the
--     memberships are meaningless).
--   * Delete the issue → drop the membership rows that reference it
--     (issues can be removed from a project; cascading here keeps
--     the (view, issue) pair from dangling).
--
-- We deliberately do NOT add a CHECK or FK that enforces "the issue
-- is also a member of the parent project" — the API path always
-- adds to the project first, then to the view, in the same
-- transaction. A trigger would duplicate that invariant for no
-- additional safety since direct SQL writers don't exist.
CREATE TABLE IF NOT EXISTS dp_project_view_issues (
    view_id  UUID        NOT NULL REFERENCES dp_project_views(id) ON DELETE CASCADE,
    issue_id UUID        NOT NULL REFERENCES dp_issues(id)         ON DELETE CASCADE,
    added_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (view_id, issue_id)
);

-- Reverse lookup: "which views contain this issue?" — used when an
-- issue is removed from the project to surface a list of affected
-- tabs, and by the future per-issue detail panel.
CREATE INDEX IF NOT EXISTS dp_project_view_issues_issue_idx
    ON dp_project_view_issues (issue_id);
