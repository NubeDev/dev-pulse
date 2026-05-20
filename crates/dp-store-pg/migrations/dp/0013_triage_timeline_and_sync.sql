-- Triage slice 2 — read-side endpoints support.
-- See `linear-projects-idea.md` §5.6 (timeline), §5.4 keyset
-- pagination (covering index on dp_issues.updated_at), and §6
-- (the guarded expression index on dp_activity_events that lets
-- `GET /issues/{id}/timeline` join across issue events without
-- a payload-shape crash on malformed rows).

-- §5.6 + §6 — issue timeline filter. The cast in the expression
-- is guarded by the `payload ? 'number' AND payload->>'number' ~
-- '^[0-9]+$'` predicate so PostgreSQL can build (and refresh)
-- this index safely even when the fetcher has historically
-- written events whose payload omits `number` (older non-issue
-- kinds, malformed webhook replays).
CREATE INDEX IF NOT EXISTS dp_activity_events_issue_idx
    ON dp_activity_events
       (repo_id, ((payload->>'number')::int), ts DESC)
    WHERE kind IN ('issue_opened', 'issue_closed', 'issue_comment')
      AND payload ? 'number'
      AND payload->>'number' ~ '^[0-9]+$';

-- §5.4 — covering index for the keyset paginated `/me/queue`
-- order key `(updated_at DESC, id DESC)`. The store layer
-- emits `ORDER BY i.updated_at DESC, i.id DESC LIMIT $cap` for
-- every arm, so a single ordered-descending btree serves the
-- planner cheaply for any tenant.
CREATE INDEX IF NOT EXISTS dp_issues_updated_at_idx
    ON dp_issues (updated_at DESC, id DESC);
