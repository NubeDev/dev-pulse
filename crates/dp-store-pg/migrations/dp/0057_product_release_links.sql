-- Build / download links on a release (firmware/software).
-- A JSON array of {label, url} objects, edited together with the row.
-- Additive column; existing rows default to an empty list.
ALTER TABLE dp_product_releases
    ADD COLUMN links jsonb NOT NULL DEFAULT '[]'::jsonb;
