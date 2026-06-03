-- Per-product software / firmware release history (major.minor).
-- Editable rows → version CAS; derived child of a product → ON DELETE CASCADE.
CREATE TABLE dp_product_releases (
    id            uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id        uuid        NOT NULL,
    product_id    uuid        NOT NULL REFERENCES dp_products(id) ON DELETE CASCADE,
    kind          text        NOT NULL CHECK (kind IN ('software','firmware')),
    major         integer     NOT NULL CHECK (major >= 0),
    minor         integer     NOT NULL CHECK (minor >= 0),
    release_notes text        NULL,
    released_at   timestamptz NULL,
    archived_at   timestamptz NULL,
    created_by    uuid        NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    version       bigint      NOT NULL DEFAULT 1
);
CREATE INDEX dp_product_releases_product_idx
    ON dp_product_releases (product_id, kind, major DESC, minor DESC);
CREATE UNIQUE INDEX dp_product_releases_version_uniq
    ON dp_product_releases (product_id, kind, major, minor)
    WHERE archived_at IS NULL;
