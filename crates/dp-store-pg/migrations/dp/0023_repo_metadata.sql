-- 0023_repo_metadata.sql
--
-- Per-repo snapshot of mutable GitHub metadata — the fields that
-- describe what a repo *is* (stars, primary language, default
-- branch, archival state, …) rather than its activity timeline.
--
-- Kept in a sibling table — not as columns on `dp_repos` — for two
-- reasons:
--
--   * `dp_repos` is identity: `(org_id, github_id) -> repo_id`.
--     Stars / forks / pushed_at change with every webhook delivery
--     and pollute `upsert_repo`'s diff if folded into the same row.
--   * Surfaces that only care about identity (issue write path,
--     reconciler scopes) keep a small SELECT; the repo-activity
--     dashboard joins this table explicitly when it needs the
--     snapshot.
--
-- All metric columns default to 0 / false so a snapshot row can be
-- inserted before the fetcher has filled them in (the row appears
-- on first webhook touch; numeric fields land as GitHub returns
-- them). `metadata_updated_at` is the wall clock of the last
-- mutation by the fetcher / handler, distinct from `pushed_at`
-- (GitHub's own last-push timestamp).
--
-- SCOPE §4 fit: every column describes the repo, not a user.
-- No attribution, no LOC-per-author. Safe by construction for
-- the repo-activity surface.

CREATE TABLE dp_repo_metadata (
    repo_id              UUID         PRIMARY KEY REFERENCES dp_repos(id) ON DELETE CASCADE,
    -- Snapshot counters from the GitHub repo object.
    stars                BIGINT       NOT NULL DEFAULT 0,
    forks                BIGINT       NOT NULL DEFAULT 0,
    watchers             BIGINT       NOT NULL DEFAULT 0,
    open_issues_remote   BIGINT       NOT NULL DEFAULT 0,
    -- Descriptive fields. NULL = GitHub returned null / absent.
    primary_language     TEXT         NULL,
    default_branch       TEXT         NULL,
    description          TEXT         NULL,
    homepage             TEXT         NULL,
    -- Boolean flags. Default false so a row that lands before a
    -- payload carrying the flag reads "active, not a fork" — the
    -- common case for repos we track.
    is_archived          BOOLEAN      NOT NULL DEFAULT FALSE,
    is_fork              BOOLEAN      NOT NULL DEFAULT FALSE,
    is_private           BOOLEAN      NOT NULL DEFAULT FALSE,
    -- GitHub's own "last push to any branch" timestamp.
    pushed_at            TIMESTAMPTZ  NULL,
    -- Wall-clock the dev-pulse fetcher last wrote this row.
    metadata_updated_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

-- Index for "most active repos first" sorts on the dashboard.
CREATE INDEX dp_repo_metadata_pushed_at_idx
    ON dp_repo_metadata (pushed_at DESC NULLS LAST);
