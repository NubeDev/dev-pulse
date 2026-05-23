-- 0044_dp_tags_kv_backfill.sql
--
-- Re-run migration 0031's kv backfill for rows created AFTER 0031
-- shipped. The application's `create_tag` path defaulted `kind` to
-- 'single' (the column default) and never derived `key`/`value`
-- from the name, so kv-shape names like `category:hardware` or
-- `gate:g3-mvp-build` were stored as `kind='single'`. Bucket
-- queries gate on `t.kind = 'kv'`, so those tag links were
-- silently invisible to `Group by: tag:<key>` — issues tagged
-- `category:hardware` landed under "Uncategorised".
--
-- Idempotent: only touches rows that are currently `single` but
-- whose name carries a colon strictly between other chars.

UPDATE dp_tags SET
    kind  = 'kv',
    key   = split_part(name, ':', 1),
    value = substring(name FROM position(':' in name) + 1)
WHERE kind = 'single'
  AND position(':' in name) > 1
  AND position(':' in name) < length(name);
