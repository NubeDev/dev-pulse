-- Triage slice 2 — issue start / due dates (§3.10).
--
-- Migration-numbering convention: projects-issues owns *odd* slots.
-- `0013_*` was the last (timeline + sync-status indexes); this
-- claims `0015_*` for the dates surface. `0017_*` stays open for the
-- next slice-2 stage.
--
-- Three things land here:
--
--   * `dp_issue_dates` — 1:1 sidecar to `dp_issues` carrying the
--     local-first `start_at` / `due_at` plus the GraphQL mirror
--     provenance (`mirror_node_id`, `mirror_synced_at`,
--     `mirror_error`). Sidecar (not extra columns on `dp_issues`)
--     because dates are sparse — most issues never carry them —
--     and the mirror columns are write-side-only churn we don't
--     want to drag through every read of the hot row.
--   * `dp_repo_project_link` — optional 1:1 mapping from a repo to
--     the GitHub Projects v2 project the mirror task should
--     `addProjectV2ItemById` into, plus the start / due field
--     node ids `updateProjectV2ItemFieldValue` needs. Optional:
--     repos without a link are local-only; the mirror task is a
--     no-op for them.
--   * `dp_projectv2_mirror_tasks` — best-effort outbox the
--     `PATCH /issues/{id}/dates` handler enqueues a row into after
--     the local upsert succeeds. A worker (slice 3) drains the
--     queue and writes any failure back to
--     `dp_issue_dates.mirror_error`. The stub task type
--     `pull_back` is reserved here (zero rows produced this slice)
--     so the slice-3 Projects v2 pull-back can land without a
--     follow-on migration.

-- ---------- dp_issue_dates -----------------------------------------

-- Sidecar so dates land local-first and the mirror provenance
-- never bloats the hot `dp_issues` row. PK on `issue_id` so the
-- handler's UPSERT semantics are a one-row collision: the
-- application always upserts the full pair.
--
-- The CHECK ensures we never persist an inverted window. Both
-- bounds are nullable so the user can clear either independently
-- — the constraint short-circuits whenever either side is NULL.
--
-- `mirror_node_id` is the Projects v2 *item* node id GitHub
-- returns from `addProjectV2ItemById`. We persist it so subsequent
-- date edits update the existing item via
-- `updateProjectV2ItemFieldValue` rather than creating a duplicate
-- card on every edit.
--
-- `mirror_synced_at` is the wall-clock the mirror task last
-- *successfully* wrote both fields. `mirror_error` is the verbatim
-- GraphQL error text from the most recent *failed* attempt;
-- success clears it. Both columns are advisory — the local
-- start_at / due_at remain authoritative.
CREATE TABLE dp_issue_dates (
    issue_id         UUID         NOT NULL PRIMARY KEY REFERENCES dp_issues(id) ON DELETE CASCADE,
    start_at         TIMESTAMPTZ  NULL,
    due_at           TIMESTAMPTZ  NULL,
    mirror_node_id   TEXT         NULL,
    mirror_synced_at TIMESTAMPTZ  NULL,
    mirror_error     TEXT         NULL,
    updated_at       TIMESTAMPTZ  NOT NULL DEFAULT now(),
    CHECK (start_at IS NULL OR due_at IS NULL OR start_at <= due_at)
);

-- ---------- dp_repo_project_link -----------------------------------

-- Optional 1:1 mapping. Absence means "no Projects v2 mirroring
-- for this repo" — the §3.10 mirror task is a no-op and the local
-- date upsert is the entire story.
--
-- The three node id columns are everything the GraphQL mirror
-- needs to issue `addProjectV2ItemById(projectId,
-- contentId=issueNodeId)` followed by two
-- `updateProjectV2ItemFieldValue(projectId, itemId, fieldId, ...)`
-- calls. They live here (not on `dp_issue_dates`) because they're
-- the same across every issue in the repo.
CREATE TABLE dp_repo_project_link (
    repo_id              UUID         NOT NULL PRIMARY KEY REFERENCES dp_repos(id) ON DELETE CASCADE,
    project_node_id      TEXT         NOT NULL,
    start_field_node_id  TEXT         NULL,
    due_field_node_id    TEXT         NULL,
    created_at           TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ  NOT NULL DEFAULT now()
);

-- ---------- dp_projectv2_mirror_tasks ------------------------------

-- Best-effort outbox. The PATCH handler enqueues `mirror_dates`
-- rows after the local upsert; a worker drains and on failure
-- writes the error text back to `dp_issue_dates.mirror_error`.
-- Success deletes the row (or marks `processed_at`); the handler
-- itself never blocks on this.
--
-- `kind` is a closed enum guarded by CHECK so the application
-- cannot widen the vocabulary without a migration. `pull_back`
-- is reserved for the slice-3 Projects v2 pull-back (currently
-- a stub — zero rows produced this slice).
CREATE TABLE dp_projectv2_mirror_tasks (
    id           UUID         NOT NULL PRIMARY KEY DEFAULT gen_random_uuid(),
    issue_id     UUID         NOT NULL REFERENCES dp_issues(id) ON DELETE CASCADE,
    repo_id      UUID         NOT NULL REFERENCES dp_repos(id)  ON DELETE CASCADE,
    kind         TEXT         NOT NULL,
    payload      JSONB        NOT NULL DEFAULT '{}'::jsonb,
    attempts     INTEGER      NOT NULL DEFAULT 0,
    last_error   TEXT         NULL,
    enqueued_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    processed_at TIMESTAMPTZ  NULL,
    CHECK (kind IN ('mirror_dates', 'pull_back'))
);

-- Worker drain order: oldest unprocessed first. Partial index so
-- the index footprint stays proportional to the *unprocessed*
-- set, not the lifetime row count.
CREATE INDEX dp_projectv2_mirror_tasks_pending_idx
    ON dp_projectv2_mirror_tasks (enqueued_at)
    WHERE processed_at IS NULL;
