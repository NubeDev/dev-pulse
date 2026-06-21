-- 0059_product_kind.sql
--
-- Product feedback #1: colour-code a product by who makes it — an
-- in-house Nube iO product vs a re-badged OEM product. Adds a `kind`
-- discriminator distinct from `manufacturer_id` (which records *which*
-- manufacturer, not the in-house/OEM split).
--
-- Backfilled to 'nube_io' for every existing row — the catalogue
-- predates the OEM concept, so existing products are treated as
-- in-house until reclassified.

ALTER TABLE dp_products
    ADD COLUMN kind text NOT NULL DEFAULT 'nube_io'
        CHECK (kind IN ('nube_io', 'oem'));
