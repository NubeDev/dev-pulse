-- 0022_projects.sql  (linear-projects-v2.md §5, §8 — slice A)
--
-- First-class Projects surface: a dev-pulse-owned planning object
-- the team plans against. Two tables land here; the GitHub-side
-- mirror plumbing (`dp_project_board_links`, `dp_project_board_items`,
-- and the rename of the legacy `dp_repo_project_link`) is slice B
-- and ships under `0023_*` / `0024_*` once these foundations are
-- exercised by the API + UI.
--
-- Migration-numbering convention: projects-issues owns *even* slots
-- this slice; `0021_*` (issue github node id) was the last even slot
-- before this one. `0023_*` and `0024_*` stay reserved for the
-- board-link tables and the legacy-table rename respectively.
--
--   * `dp_projects` — owns name / description / lead / dates /
--     status / CAS version / denormalised issue counts. Counts are
--     maintained transactionally by the §7.2 add / remove paths and
--     by the issue-close webhook; v1 trades a strict normal form for
--     a < 200ms p95 on the §6.2 list page.
--   * `dp_project_issues` — join table. The v1 `UNIQUE (issue_id)`
--     constraint (§4) hard-codes "one project per issue" so the
--     mirror semantics stay defined (which board owns the date?)
--     and the §6.5 detail-pane chip stays singular. The constraint
--     is a single `ALTER TABLE … DROP CONSTRAINT` away when v2
--     wants to relax it; no destructive migration required.

-- ---------- dp_projects --------------------------------------------

-- `status` is a closed enum guarded by CHECK so the application
-- cannot widen the vocabulary without a migration. Mirrors the §5
-- entity diagram.
--
-- `version` is the §8.2 CAS column — every PATCH / archive carries
-- `expected_version` and the SQL `WHERE id = ? AND version = ?`
-- clause emits 0 rows on a stale write. BIGINT (not INT) so a
-- decade of hot-row churn cannot wrap.
--
-- `issue_count` / `closed_issue_count` are denormalised counters
-- maintained by the application (§7.1 contract). Schema does not
-- enforce them — the §7.2 add / remove paths and the issue-close
-- webhook are responsible. We keep them on the row so the list-
-- page render is a single round-trip with no per-row aggregate
-- subqueries.
CREATE TABLE dp_projects (
    id                   UUID         NOT NULL PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id               UUID         NOT NULL REFERENCES dp_orgs(id)  ON DELETE CASCADE,
    name                 TEXT         NOT NULL,
    description          TEXT         NULL,
    lead_user_id         UUID         NULL REFERENCES dp_users(id) ON DELETE SET NULL,
    status               TEXT         NOT NULL DEFAULT 'active',
    start_at             TIMESTAMPTZ  NULL,
    due_at               TIMESTAMPTZ  NULL,
    issue_count          INTEGER      NOT NULL DEFAULT 0,
    closed_issue_count   INTEGER      NOT NULL DEFAULT 0,
    created_by           UUID         NULL REFERENCES dp_users(id) ON DELETE SET NULL,
    created_at           TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ  NOT NULL DEFAULT now(),
    version              BIGINT       NOT NULL DEFAULT 1,
    CHECK (status IN ('active', 'backlog', 'done', 'archived')),
    CHECK (start_at IS NULL OR due_at IS NULL OR start_at <= due_at),
    CHECK (issue_count >= 0),
    CHECK (closed_issue_count >= 0),
    CHECK (closed_issue_count <= issue_count)
);

-- List-page sort key: §6.2 default is `status` then
-- `due_at ASC NULLS LAST`. Org-scoped first so the §15 access gate
-- predicate is index-prefix-covered.
CREATE INDEX dp_projects_org_status_due_idx
    ON dp_projects (org_id, status, due_at);

-- Partial-unique name index (§5 / §8 indexes). Case-insensitive so
-- "Rubix v2 launch" and "rubix v2 launch" collide. Archived rows
-- are excluded so users can recycle the name of a project they
-- archived — the §6.1 sidebar hides archived by default and the
-- name collision is the whole reason archive is not a soft-delete.
CREATE UNIQUE INDEX dp_projects_org_name_unique
    ON dp_projects (org_id, lower(name))
    WHERE status <> 'archived';

-- ---------- dp_project_issues --------------------------------------

-- Join table. `UNIQUE (issue_id)` is the v1 "one project per issue"
-- guarantee (§4) — the application surfaces a collision as
-- `BulkAddResult.skipped[*].reason = "already_in_project"` so the
-- UI can offer a one-click `Move here?`.
--
-- `ON DELETE CASCADE` on both FKs so dropping a project or an
-- issue cleans up the membership rows without orphans.
CREATE TABLE dp_project_issues (
    project_id   UUID         NOT NULL REFERENCES dp_projects(id) ON DELETE CASCADE,
    issue_id     UUID         NOT NULL REFERENCES dp_issues(id)   ON DELETE CASCADE,
    added_by     UUID         NULL REFERENCES dp_users(id) ON DELETE SET NULL,
    added_at     TIMESTAMPTZ  NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, issue_id),
    UNIQUE (issue_id)
);

-- Reverse-lookup: "which project is this issue in?" is the §6.5
-- detail-pane chip and the §7.2 `GET /issues/{id}/project` query.
-- Strictly speaking the `UNIQUE (issue_id)` constraint above
-- already implies a btree index on `issue_id`; we name it
-- explicitly here for documentation and so a future v2 that drops
-- the unique constraint keeps the read path indexed.
CREATE INDEX dp_project_issues_issue_idx
    ON dp_project_issues (issue_id);
