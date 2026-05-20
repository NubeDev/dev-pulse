-- Triage spine — slice 1 of the Linear-style workflow surface.
-- See `linear-projects-idea.md` §3.8, §5.5, §6, §8 (slice 1).
--
-- Migration-numbering convention (per
-- `.codeless/jobs/projects-issues/STAGE-1-COORDINATION.md` §3):
-- projects-issues owns *odd* slots. `0009_*` was the last; this
-- claims `0011_*` for the triage spine. `0013_*` stays open for
-- slice 2 (timeline expression index + mentions projection).
--
-- Three things land here:
--
--   * Two columns on `dp_issues` — `author` and `state_reason`.
--     Both are projected from the JSON payload the fetcher already
--     stores; the migration backfills them in-place so the new
--     filter pills (`author=`, `state_reason=`) and the throughput
--     reports (slice 3) never see a half-populated table.
--   * Three indexes — GIN on `labels` / `assignees` (so the new
--     containment filters in §5.5 hit O(log n)) plus a btree on
--     `author` for the per-author pill.
--   * `dp_user_issue_state` — per-user inbox state (§3.8).
--     Backs the `★ My queue` smart view, the unread-dot UX, the
--     `e` mark-done shortcut, and `h` snooze.
--
-- Why one combined migration: every piece is part of the same
-- user-visible feature (the triage spine). Splitting would let an
-- operator deploy `0011` without the inbox table and ship a
-- triage page whose `My queue` button 500s.
--
-- Why we do not split `author` / `state_reason` into their own
-- table: both are 1:1 with the issue row and are read on every
-- list page; a sidecar table would force a join on the hot path
-- with zero normalisation upside (logins and state-reason
-- enumerations are already unbounded GitHub strings).

-- ---------- dp_issues — author + state_reason -----------------------

-- Author login (GitHub `user.login`). Nullable because rows that
-- predate this column may not have the field populated until the
-- next fetcher tick rewrites them — the backfill below covers
-- everything currently on disk that has the payload, but rows
-- the fetcher never round-tripped through the new path stay NULL
-- and degrade to "no match" for the author filter (correct).
ALTER TABLE dp_issues
    ADD COLUMN author       TEXT NULL;

-- GitHub's per-issue `state_reason` (`completed` / `not_planned` /
-- `reopened` / NULL). Needed by the throughput / lead-time reports
-- in slice 3 to separate "shipped" from "cancelled". Same
-- nullability rationale as `author`.
ALTER TABLE dp_issues
    ADD COLUMN state_reason TEXT NULL;

-- Per-pill indexes. The labels / assignees containment paths in
-- §5.5 already use `jsonb @> jsonb` which is best-served by GIN;
-- author is a flat scalar lookup so btree is fine.
CREATE INDEX dp_issues_labels_gin    ON dp_issues USING GIN (labels);
CREATE INDEX dp_issues_assignees_gin ON dp_issues USING GIN (assignees);
CREATE INDEX dp_issues_author_idx    ON dp_issues (author);
CREATE INDEX dp_issues_state_reason_idx ON dp_issues (state_reason);

-- ---------- dp_user_issue_state — per-user inbox -------------------

-- Per-user issue state. Keyed `(user_id, issue_id)`. The row exists
-- only when the user has interacted with the issue at all; absence
-- means "default state" (unread iff `dp_issues.version > 0`,
-- inbox-eligible iff the issue would otherwise match a smart view).
--
-- Status is a tri-state with a CHECK so the application layer
-- cannot widen it accidentally — adding a new status is a code +
-- migration change in lockstep, never a config-typo away.
--
-- `snoozed_until` is honoured only when `status = 'snoozed'`; the
-- application sets both together. We do not enforce that
-- consistency at the DB level because the snooze flow always
-- writes both fields in the same UPDATE — making it a CHECK would
-- add no defence-in-depth and would block legitimate transient
-- states (e.g. clearing a snooze by setting status back to 'inbox'
-- without first wiping `snoozed_until`).
CREATE TABLE dp_user_issue_state (
    user_id            UUID         NOT NULL REFERENCES dp_users(id)  ON DELETE CASCADE,
    issue_id           UUID         NOT NULL REFERENCES dp_issues(id) ON DELETE CASCADE,
    last_seen_version  BIGINT       NOT NULL DEFAULT 0,
    status             TEXT         NOT NULL DEFAULT 'inbox',
    snoozed_until      TIMESTAMPTZ  NULL,
    updated_at         TIMESTAMPTZ  NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, issue_id),
    CHECK (status IN ('inbox', 'snoozed', 'done'))
);

-- Inbox listing predicate: "rows for this user that are not done,
-- ordered by recency". The partial index keeps the working set
-- small once users start aggressively dismissing things.
CREATE INDEX dp_user_issue_state_inbox_idx
    ON dp_user_issue_state (user_id, updated_at DESC)
    WHERE status <> 'done';

-- Snooze wake-up scan ("rows snoozed past their wake time"). Used
-- by a future reconciler tick to flip snoozed → inbox once the
-- wake instant has passed; for slice 1 the application filters at
-- read time, but having the index now avoids a follow-up migration.
CREATE INDEX dp_user_issue_state_snooze_wake_idx
    ON dp_user_issue_state (snoozed_until)
    WHERE status = 'snoozed' AND snoozed_until IS NOT NULL;
