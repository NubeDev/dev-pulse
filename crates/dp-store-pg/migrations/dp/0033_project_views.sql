-- 0033_project_views.sql
--
-- Saved views (PROJECT-VIEW.md §6.1, Slice 4) — per-(project, user)
-- named bundles of (group_by, filter, sort). v1 ships **private**
-- only; the `visibility` enum reserves the `'project'` slot so the
-- future shared-views slice lands without a migration.
--
-- Schema mirrors the design block verbatim except where noted:
--
--   * No trigger fn for `filter_json` shape — the REST validator in
--     `dp_rest::project_views::validate_filter_clauses` is the only
--     write path and rejects malformed clauses. A future trigger can
--     be bolted on without data migration; the CHECK still rejects
--     non-array roots in the meantime.
--
--   * `position` is per `(project_id, owner_user_id)`. Reorder is a
--     client-driven `POST /projects/{id}/views/reorder` that rewrites
--     positions in one tx (§7.1). No `position >= 0` CHECK so the
--     rewrite can use temporary negative values inside the tx if
--     needed without splitting it.

CREATE TABLE dp_project_views (
    id            UUID PRIMARY KEY,
    project_id    UUID NOT NULL
                  REFERENCES dp_projects(id) ON DELETE CASCADE,
    owner_user_id UUID NOT NULL
                  REFERENCES dp_users(id) ON DELETE CASCADE,
    name          TEXT NOT NULL
                  CHECK (length(name) BETWEEN 1 AND 60),
    -- group_by: NULL | 'status' | 'tag:<key>' (v1).
    -- Future: 'milestone' | 'issue_type' (after Slices 1 / 5+).
    group_by      TEXT,
    -- Canonical filter clauses; see
    -- dp_rest::project_views::FilterClause / validate_filter_clauses
    -- for the per-dim shape.
    filter_json   JSONB NOT NULL DEFAULT '[]'::jsonb
                  CHECK (jsonb_typeof(filter_json) = 'array'),
    sort          TEXT NOT NULL DEFAULT 'updated_desc',
    position      INT  NOT NULL,
    visibility    TEXT NOT NULL DEFAULT 'private'
                  CHECK (visibility IN ('private', 'project')),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, owner_user_id, name)
);

CREATE INDEX dp_project_views_project_idx
    ON dp_project_views (project_id, owner_user_id, position);
