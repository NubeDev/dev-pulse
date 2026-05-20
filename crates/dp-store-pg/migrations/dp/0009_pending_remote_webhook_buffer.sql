-- §13.7 reconciler-guard webhook buffer for the projects-issues job.
--
-- Migration-numbering convention (per
-- `.codeless/jobs/projects-issues/STAGE-1-COORDINATION.md` §3):
-- projects-issues owns *odd* slots. `0005_*` landed pins / tags /
-- tag_links (stage 3); `0007_*` landed the §8 write path columns
-- (stage 9); this file claims `0009_*` for the §13.7 webhook
-- replay buffer. Leaderboard's even slots stay open.
--
-- ## What this is for
--
-- SCOPE-PROJECTS §13.7: when a fetcher tick or webhook delivery
-- arrives concerning an `dp_issues` row that is currently in
-- `pending_remote = TRUE` with a fresh `pending_remote_at`, the
-- reconciler **must not** overwrite the row — the user-initiated
-- §8.2 write path is mid-flight and the buffered local state is
-- ahead of authoritative GitHub state by exactly one mutation.
--
-- The §13.7 rule is "buffer and replay", not "drop". Dropping the
-- delivery would lose information whenever the optimistic write
-- ends up *failing* (§8.2 step 8) — the rollback restores the
-- pre-mutation local state and the buffered webhook is then the
-- only record of an intervening GitHub-side edit. So we stash the
-- raw delivery here and replay it through `apply_delivery` after
-- the pending flag clears (§8.2 step 7, step 8, or §8.5 sweeper).
--
-- ## Schema shape
--
--   * One row per (issue × delivery_id). `delivery_id` is unique
--     globally for the same reason `dp_webhook_inbox.delivery_id`
--     is: GitHub re-delivers on failure and we must collapse the
--     replays.
--   * `issue_id` cascade-deletes — if the dp_issues row is gone
--     (pseudonymise / repo purge) the buffered payloads have no
--     replay target.
--   * The payload is stored verbatim (same `event` + `payload`
--     shape as `dp_webhook_inbox`) so the replay path is just
--     `apply_delivery(store, &WebhookDelivery { … })`. No second
--     parser, no schema drift between the inbox and the buffer.
--
-- ## Why a separate table (not a `deferred_at` flag on the inbox)
--
-- The inbox is a queue with `FOR UPDATE SKIP LOCKED` claim
-- semantics (§Stage 5 worker). Adding "deferred" as a third state
-- next to "claimed / processed" muddies the worker's drain loop:
-- it would either keep re-claiming deferred rows (wasted work) or
-- need a partial index to skip them (extra ceremony, easy to
-- forget on a replay tick). A sibling table is cheaper and lets
-- the worker drain its inbox to completion regardless.

CREATE TABLE dp_pending_remote_webhook_buffer (
    -- Caller-assigned UUID. Matches the originating
    -- `dp_webhook_inbox.id` when the buffer is fed from the inbox
    -- drain path, but the buffer accepts standalone inserts (e.g.
    -- a synthesised reconciler delivery that never went through
    -- the inbox), so we keep it as a free UUID rather than an FK.
    id            UUID         PRIMARY KEY,
    -- `dp_issues.id` whose pending_remote flag deflected this
    -- delivery. Cascade-delete: if the issue row is gone the
    -- buffered payload has nowhere to replay to.
    issue_id      UUID         NOT NULL REFERENCES dp_issues(id) ON DELETE CASCADE,
    -- `X-GitHub-Delivery` header. Unique so a GitHub re-delivery
    -- of the same logical event collapses into one buffered row.
    delivery_id   TEXT         NOT NULL UNIQUE,
    -- `X-GitHub-Event` value (e.g. `"issues"`, `"issue_comment"`).
    event         TEXT         NOT NULL,
    -- Payload verbatim. The replay path re-dispatches through the
    -- normal `apply_delivery` so there's no second parser.
    payload       JSONB        NOT NULL,
    -- When the original delivery hit our receiver. Preserved so
    -- the replay can carry the same `received_at` (ordering of
    -- events on an issue stays correct relative to the optimistic
    -- write that deferred it).
    received_at   TIMESTAMPTZ  NOT NULL,
    -- When the deflection happened. Drives replay ordering inside
    -- one drain pass (oldest first).
    buffered_at   TIMESTAMPTZ  NOT NULL DEFAULT now()
);

-- Replay drains "all rows for this issue, oldest first" — index
-- by `(issue_id, buffered_at)` keeps that scan cheap.
CREATE INDEX dp_pending_remote_webhook_buffer_issue_idx
    ON dp_pending_remote_webhook_buffer (issue_id, buffered_at);
