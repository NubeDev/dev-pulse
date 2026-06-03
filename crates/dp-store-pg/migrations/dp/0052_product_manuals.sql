-- 0052_product_manuals.sql
--
-- Product & Manufacturing — P1 user manuals (markdown + revisions)
-- (DOCS/ideas/product-manufacturing.md §5.3).
--
-- A manual is a named container; each save creates an immutable
-- revision. The product page shows the *published* revision; editors
-- work on a *draft*. Revision strings are free-form ('A','B','1.0').
-- At most one published revision per manual is enforced by a partial
-- unique index, backed by a store tx that flips the prior published
-- revision to 'superseded' when a new one is published.

CREATE TABLE dp_product_manuals (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    product_id  uuid        NOT NULL REFERENCES dp_products(id) ON DELETE CASCADE,
    title       text        NOT NULL,         -- e.g. 'Installation Guide'
    created_by  uuid        NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    version     bigint      NOT NULL DEFAULT 1
);
CREATE INDEX dp_product_manuals_product_idx
    ON dp_product_manuals (product_id, created_at DESC);

CREATE TABLE dp_product_manual_revisions (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    manual_id   uuid        NOT NULL REFERENCES dp_product_manuals(id) ON DELETE CASCADE,
    revision    text        NOT NULL,         -- free-form: 'A','B','1.0','2026-06'
    status      text        NOT NULL DEFAULT 'draft'
        CHECK (status IN ('draft','published','superseded')),
    body_md     text        NOT NULL,         -- the manual content, markdown
    change_note text        NULL,             -- "what changed" for this revision
    authored_by uuid        NULL,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX dp_product_manual_revisions_manual_idx
    ON dp_product_manual_revisions (manual_id, created_at DESC);
CREATE UNIQUE INDEX dp_product_manual_revisions_manual_rev_uniq
    ON dp_product_manual_revisions (manual_id, lower(revision));
-- At most one published revision per manual.
CREATE UNIQUE INDEX dp_product_manual_revisions_one_published
    ON dp_product_manual_revisions (manual_id) WHERE status = 'published';
