-- 0030_milestones.sql
--
-- GitHub repo milestones, fetched and refreshed per-repo by the
-- fetcher worker. Tracks the GitHub-side `(title, description,
-- state, due_on, open_issues, closed_issues)` payload so dev-pulse
-- can render the §9 classification chip on issue rows and the §9.5
-- "Adopt as primary milestone" affordance on project detail pages
-- without round-tripping GitHub on every render.
--
-- See `tagging.md` §9.3 for the design rationale. This migration
-- ships the **storage only** — the fetcher worker integration
-- and the `dp_issues.milestone_id` FK arrive in a follow-up slice.
-- Until then, the existing `dp_issues.milestone` TEXT column keeps
-- driving the existing `IssueDto.milestone` field; this table is
-- additive and can be empty without breaking any read path.
--
-- Schema choices, all from `tagging.md` §9.3:
--
-- * `github_number INTEGER NOT NULL` — repo-scoped milestone
--   number. Composite UNIQUE with `repo_id` is the natural key
--   for fetcher upserts (the GraphQL `node_id` is preferred for
--   joins but isn't always present on REST payloads from
--   pre-2020 milestones).
-- * `github_node_id TEXT NOT NULL` — opaque GraphQL node id, used
--   for Projects v2 / GraphQL joins. Same precedent as
--   `dp_issues.github_node_id` (migration 0021).
-- * `due_on DATE NULL` — GitHub's `due_on` is a calendar date,
--   not a timestamp. Storing as TIMESTAMPTZ would force a tz
--   interpretation ("UTC midnight") that displays as the previous
--   day west of UTC. DATE keeps it tz-agnostic; the §9.5 follow-
--   the-milestone path doesn't need finer precision.
-- * `open_issues` / `closed_issues` — denormalised counters
--   GitHub already maintains; lets the §9.2 triage rail render
--   `closed/total` progress without a per-row join.
-- * `remote_missing_streak` — N=3 quarantine counter (§5.1 / §9.4).
--   Increments only when the fetcher confirms a complete page set
--   on `list_milestones` and this row's `github_number` was absent.
--   Resets to 0 on any pull that re-observes the row. The
--   migration ships the column; the fetcher slice will populate
--   it via store helpers that don't exist yet.
--
-- Indexes:
--
-- * `(repo_id, github_number)` UNIQUE — natural-key lookup for
--   the fetcher upsert path and for resolving `dp_issues.milestone`
--   text to a row.
-- * `(repo_id, state)` — the triage rail's "active milestones"
--   query filters by `state='open'`; this serves both lookups.
-- * Partial `(due_on)` WHERE state='open' — the §9.5 "Due in
--   current milestone" smart view sorts by due date and only
--   cares about open ones.

CREATE TABLE dp_milestones (
    id                    uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    repo_id               uuid        NOT NULL REFERENCES dp_repos(id) ON DELETE CASCADE,
    github_number         integer     NOT NULL,
    github_node_id        text        NOT NULL,
    title                 text        NOT NULL,
    description           text        NULL,
    state                 text        NOT NULL CHECK (state IN ('open', 'closed')),
    due_on                date        NULL,
    open_issues           integer     NOT NULL DEFAULT 0,
    closed_issues         integer     NOT NULL DEFAULT 0,
    created_at            timestamptz NOT NULL,
    updated_at            timestamptz NOT NULL,
    closed_at             timestamptz NULL,
    fetched_at            timestamptz NOT NULL DEFAULT now(),
    remote_missing_streak integer     NOT NULL DEFAULT 0,
    UNIQUE (repo_id, github_number)
);

CREATE INDEX dp_milestones_repo_state_idx ON dp_milestones (repo_id, state);

CREATE INDEX dp_milestones_due_idx
    ON dp_milestones (due_on)
    WHERE state = 'open';
