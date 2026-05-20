-- Issue write-path columns + audit table for SCOPE-PROJECTS.md §8.2 +
-- §8.5 + §13.4 + §13.7. Stage 9 of the projects-issues job.
--
-- Migration-numbering convention (per
-- `.codeless/jobs/projects-issues/STAGE-1-COORDINATION.md` §3):
-- projects-issues owns *odd* slots. `0005_*` landed pins / tags /
-- tag_links in stage 3; this file claims `0007_*` for the §8 write
-- path. Leaderboard's `0004_*` / `0006_*` slots stay open.
--
-- Two things land here:
--
--   * Four columns on `dp_issues` — `version`, `pending_remote`,
--     `pending_remote_at`, `pending_remote_actor`. These together
--     are the optimistic-CAS token (§8.2 step 5) and the
--     reconciler-guard signal (§13.7).
--   * `dp_issue_mutations` audit table — one row per
--     user-initiated GitHub issue write, in lockstep with the
--     dp-domain `IssueMutation` shape (`crates/dp-domain/src/
--     issue_mutation.rs`). The four §8.5 lifecycle states
--     (`pending` / `committed` / `failed` / `pending_remote_timeout`)
--     live on the row, not on a sibling table.
--
-- Why the columns live on `dp_issues` and not on a sidecar table:
-- the §8.2 CAS clause is `WHERE id = ? AND version = ?` — the cheap
-- form only when `version` is a column on the row itself. Splitting
-- it off forces a join on every write *and* on every reconciler
-- decision (§13.7), neither of which is acceptable hot-path cost.
--
-- Why `pending_remote_actor` is denormalised onto the row: the
-- timeout sweeper (§8.5) needs to know who to blame in the
-- `pending_remote_timeout` audit row *without* a JOIN through
-- `dp_issue_mutations` — when the sync handler crashed between §8.2
-- step 5 and step 7, the audit row may or may not have been
-- written, but the row-level flag is the authoritative signal.
-- Keeping the actor on the row keeps the sweeper's query a single
-- table scan.

-- ---------- dp_issues — CAS / pending-remote columns ---------------

-- `version`: monotonically increasing, bumped on every fetched
-- update *and* every optimistic local write (§8.2 step 5).
-- Starting all existing rows at `1` is correct — the first
-- subsequent fetcher tick will bump them naturally, and the first
-- §8 write that lands against an existing row supplies the
-- `expected_version` it reads from the GET. BIGINT (not INT)
-- because the bumps are unbounded over a repo's lifetime.
ALTER TABLE dp_issues
    ADD COLUMN version              BIGINT       NOT NULL DEFAULT 1;

-- `pending_remote`: set `true` by the CAS clause in §8.2 step 5,
-- cleared on §8.2 step 7 (success), §8.2 step 8 (rollback) or by
-- the timeout sweeper (§8.5). Together with `pending_remote_at`
-- this is the §13.7 reconciler guard — a webhook / fetcher tick
-- must *not* clobber a row where this flag is set and
-- `pending_remote_at` is younger than
-- `issues.pending_remote_timeout_secs`.
ALTER TABLE dp_issues
    ADD COLUMN pending_remote       BOOLEAN      NOT NULL DEFAULT FALSE;

-- `pending_remote_at`: when the CAS in §8.2 step 5 set
-- `pending_remote = true`. Drives the sweeper's "older than
-- timeout" cutoff and the §13.7 reconciler-defer rule. NULL
-- whenever `pending_remote = false`; CHECK constraint below.
ALTER TABLE dp_issues
    ADD COLUMN pending_remote_at    TIMESTAMPTZ  NULL;

-- `pending_remote_actor`: the dp-pulse user who initiated the
-- inflight write. Denormalised here so the timeout sweeper can
-- emit a `pending_remote_timeout` audit row without joining
-- through `dp_issue_mutations` (whose row may or may not have
-- been written before the crash). NULL when `pending_remote =
-- false`.
ALTER TABLE dp_issues
    ADD COLUMN pending_remote_actor UUID         NULL
        REFERENCES dp_users(id);

-- Three-column invariant: `pending_remote` and the two
-- denormalised fields are populated together or not at all. The
-- CAS / rollback / sweeper paths all preserve this; the CHECK
-- catches accidental partial updates from future hands.
ALTER TABLE dp_issues
    ADD CONSTRAINT dp_issues_pending_remote_consistent CHECK (
        (pending_remote = FALSE
            AND pending_remote_at    IS NULL
            AND pending_remote_actor IS NULL)
     OR (pending_remote = TRUE
            AND pending_remote_at    IS NOT NULL
            AND pending_remote_actor IS NOT NULL)
    );

-- Sweeper query is `SELECT … WHERE pending_remote = true AND
-- pending_remote_at < ?`. Partial index keeps it tiny — the
-- steady-state population of this index is empty / near-empty.
CREATE INDEX dp_issues_pending_remote_idx
    ON dp_issues (pending_remote_at)
    WHERE pending_remote = TRUE;

-- ---------- dp_issue_mutations — §8.5 audit table ------------------

-- One row per user-initiated GitHub issue write. The verbs are the
-- five locked-vocabulary values from §8.5 (`issue.create`,
-- `issue.update`, `issue.close`, `issue.reopen`, `issue.comment`)
-- and the four lifecycle states match
-- `dp_domain::issue_mutation::IssueMutationResult`. Rows are
-- written in `pending` state from §8.2 step 5 and updated in place
-- (no new row) on §8.2 step 7 / step 8 / sweeper completion — that
-- way `(actor, issue, ts)` joins do not multiply.
CREATE TABLE dp_issue_mutations (
    -- Caller-assigned UUID so the dp-rest handler can correlate the
    -- inflight GitHub call without a round-trip; see
    -- `IssueMutation::id` docstring.
    id                  UUID         PRIMARY KEY,
    actor_user_id       UUID         NOT NULL REFERENCES dp_users(id),
    issue_id            UUID         NOT NULL REFERENCES dp_issues(id) ON DELETE CASCADE,
    -- Denormalised so the audit log answers `(actor, repo)` queries
    -- without joining through `dp_issues` (which may have been
    -- purged by a GDPR pseudonymise run by the time the question is
    -- asked).
    repo_id             UUID         NOT NULL REFERENCES dp_repos(id) ON DELETE CASCADE,
    op                  TEXT         NOT NULL,
    -- Optimistic-CAS transition (§13.4). `version_before` is the
    -- `expected_version` from §8.2 step 1; `version_after` is
    -- `version_before + 1` on the happy path. The rollback in
    -- §8.2 step 8 bumps `version` *again* on `dp_issues` — that
    -- second bump is **not** reflected here; this column is the
    -- initial optimistic bump only.
    version_before      BIGINT       NOT NULL,
    version_after       BIGINT       NOT NULL,
    -- `{"before": …, "after": …}`. `before` omitted on
    -- `issue.create`; on `issue.comment` the diff carries the new
    -- comment body with no `before`.
    diff                JSONB        NOT NULL,
    -- One of `pending` / `committed` / `failed` /
    -- `pending_remote_timeout`. Locked to match
    -- `IssueMutationResult` (snake_case serde).
    result              TEXT         NOT NULL,
    -- `X-GitHub-Delivery` id when GitHub returned one — lets the
    -- reconciler match the inbound webhook to the mutation that
    -- caused it (§13.7).
    github_delivery_id  TEXT         NULL,
    -- Verbatim GitHub error on `failed` rows; NULL otherwise.
    error               TEXT         NULL,
    created_at          TIMESTAMPTZ  NOT NULL DEFAULT now(),
    -- Stamped when the row leaves `pending` (success, failure, or
    -- sweeper rollback). NULL while still in flight.
    finished_at         TIMESTAMPTZ  NULL,
    CHECK (op IN (
        'create', 'update', 'close', 'reopen', 'comment'
    )),
    CHECK (result IN (
        'pending', 'committed', 'failed', 'pending_remote_timeout'
    )),
    -- `pending` rows never carry an error or a finished_at; all
    -- other states must carry a finished_at.
    CHECK (
        (result = 'pending'
            AND finished_at IS NULL
            AND error       IS NULL)
     OR (result <> 'pending'
            AND finished_at IS NOT NULL)
    )
);

-- "Mutations performed by this user, recent first" — §11 success
-- criterion ("a user can request a full export of mutations they
-- performed").
CREATE INDEX dp_issue_mutations_actor_created_idx
    ON dp_issue_mutations (actor_user_id, created_at DESC);

-- "Mutations against this issue" — also a §11 export.
CREATE INDEX dp_issue_mutations_issue_created_idx
    ON dp_issue_mutations (issue_id, created_at DESC);

-- Sweeper enumeration: "find rows still in `pending` older than
-- the timeout". Partial keeps it small.
CREATE INDEX dp_issue_mutations_pending_idx
    ON dp_issue_mutations (created_at)
    WHERE result = 'pending';
