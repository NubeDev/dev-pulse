-- 0043_project_view_categories.sql
--
-- PROJECT-VIEW.md — categories as ordered sections inside a view.
--
-- A categorised view is just a view with a non-empty `categories`
-- array. The frontend forces `group_by = 'tag:category'` for these
-- views, then layers `categories` over the server-returned buckets
-- to render empty sections and a stable order.
--
-- Storage shape: JSONB array of lowercase slugs that match the
-- tagging.md §3 grammar (`[a-z0-9_-]{1,50}`). The DB-level CHECK
-- enforces only the array invariant — slug grammar is validated by
-- the REST writer (`dp_rest::project_views::validate_categories`)
-- so the error path mirrors `filter_clauses`.

ALTER TABLE dp_project_views
    ADD COLUMN categories JSONB NOT NULL DEFAULT '[]'::jsonb
        CHECK (jsonb_typeof(categories) = 'array');
