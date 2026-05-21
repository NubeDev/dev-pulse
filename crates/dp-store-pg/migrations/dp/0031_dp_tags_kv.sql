-- 0031_dp_tags_kv.sql
--
-- Add kv-tag columns to `dp_tags` (`kind`, `key`, `value`) and
-- backfill from existing `name`s. This is the subset of
-- `tagging.md` §4.1 that PROJECT-VIEW.md depends on for
-- `Group by: tag:<key>` (§5.1, §6.1). The `sync_mode` column
-- and tag-push/pull pipeline land in a separate slice.
--
-- Grammar (tagging.md §3):
--
--   * Names containing **no** ':' are `kind='single'` (`iot`,
--     `dashboard`); `key` and `value` are NULL.
--   * Names containing **at least one** ':' are `kind='kv'`
--     (`gate:g3-mvp-build`, `team:backend:v2`). The split is on the
--     **first** ':' — `key='team'`, `value='backend:v2'`. Names
--     starting or ending with ':' are treated as `single` so the
--     invariant doesn't reject historical data; the §6.4 validator
--     in `tags.rs` rejects them on write.
--
-- Indexes:
--
--   * `dp_tags_key_idx (scope_kind, key) WHERE archived_at IS NULL`
--     — backs `Group by: tag:<key>` and `GET /tags?key=...`.

-- 1. Add columns. `kind` defaults to 'single' so the existing rows
--    are valid before the backfill upgrades the kv ones.
ALTER TABLE dp_tags
    ADD COLUMN kind  TEXT NOT NULL DEFAULT 'single'
        CHECK (kind IN ('single', 'kv')),
    ADD COLUMN key   TEXT NULL,
    ADD COLUMN value TEXT NULL;

-- 2. Backfill kind/key/value from existing names. Only rows where
--    ':' appears **strictly between** other chars become 'kv'; names
--    like `:foo` or `foo:` stay 'single' so the invariant CHECK below
--    doesn't reject historical data (the §6.4 write-path validator
--    will refuse new tags with those shapes).
UPDATE dp_tags SET
    kind  = CASE
              WHEN position(':' in name) > 1
                AND position(':' in name) < length(name)
              THEN 'kv'
              ELSE 'single'
            END,
    key   = CASE
              WHEN position(':' in name) > 1
                AND position(':' in name) < length(name)
              THEN split_part(name, ':', 1)
              ELSE NULL
            END,
    value = CASE
              WHEN position(':' in name) > 1
                AND position(':' in name) < length(name)
              THEN substring(name FROM position(':' in name) + 1)
              ELSE NULL
            END;

-- 3. Lock in the invariant. Future writes with `kind='kv' AND key
--    IS NULL` (or `kind='single' AND key IS NOT NULL`) are rejected
--    at the database — the REST validator in `tags.rs` is the first
--    line of defence, this is the belt-and-braces guarantee.
ALTER TABLE dp_tags
    ADD CONSTRAINT dp_tags_kind_kv_invariant CHECK (
        (kind = 'kv'     AND key IS NOT NULL AND value IS NOT NULL)
     OR (kind = 'single' AND key IS NULL     AND value IS NULL)
    );

-- 4. Cheap lookup for `Group by: tag:<key>` and `GET /tags?key=...`.
--    Partial on `archived_at IS NULL` so soft-deleted tags don't
--    bloat the index — the workbench dropdown only surfaces live
--    keys (PROJECT-VIEW.md §5.1).
CREATE INDEX dp_tags_key_idx
    ON dp_tags (scope_kind, key)
    WHERE archived_at IS NULL;
