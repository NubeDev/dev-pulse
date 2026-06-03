-- 0051_products.sql
--
-- Product & Manufacturing — P1 product definition + project links +
-- document uploads (DOCS/ideas/product-manufacturing.md §5.2).
--
-- `dp_products` is the model/SKU definition (model number unique per
-- org). `dp_product_project_links` is a dedicated N—N join (NOT
-- dp_tag_links — §5.2 prose): a first-class structural relationship
-- with its own audit columns. `dp_product_documents` mirrors
-- `dp_project_exec_summary_documents` (0045) verbatim for blob upload.

CREATE TABLE dp_products (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id          uuid        NOT NULL,
    name            text        NOT NULL,
    model_number    text        NOT NULL,
    description     text        NULL,        -- markdown
    manufacturer_id uuid        NULL REFERENCES dp_manufacturers(id) ON DELETE SET NULL,
    status          text        NOT NULL DEFAULT 'active'
        CHECK (status IN ('draft','active','eol','archived')),
    -- Serial-number generation config (§6).
    serial_prefix   text        NULL,        -- e.g. 'NB'
    serial_format   text        NULL,        -- template, e.g. '{prefix}-{run_code}-{seq:05}'
    archived_at     timestamptz NULL,
    created_by      uuid        NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    version         bigint      NOT NULL DEFAULT 1
);
CREATE UNIQUE INDEX dp_products_org_model_uniq
    ON dp_products (org_id, lower(model_number)) WHERE archived_at IS NULL;
CREATE INDEX dp_products_org_status_idx ON dp_products (org_id, status);

-- N—N product ↔ project. Dedicated join table with its own audit
-- columns. Both sides cascade: these are derived link rows (§4 delete
-- policy), not history-bearing children.
CREATE TABLE dp_product_project_links (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    product_id  uuid        NOT NULL REFERENCES dp_products(id)  ON DELETE CASCADE,
    project_id  uuid        NOT NULL REFERENCES dp_projects(id)  ON DELETE CASCADE,
    linked_by   uuid        NULL,
    linked_at   timestamptz NOT NULL DEFAULT now(),
    UNIQUE (product_id, project_id)
);
CREATE INDEX dp_product_project_links_project_idx
    ON dp_product_project_links (project_id);

-- Document uploads — mirrors dp_project_exec_summary_documents (0045).
-- Derived child → ON DELETE CASCADE.
CREATE TABLE dp_product_documents (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    product_id  uuid        NOT NULL REFERENCES dp_products(id) ON DELETE CASCADE,
    blob_ref    jsonb       NOT NULL,         -- opaque BlobRef
    title       text        NOT NULL,
    doc_type    text        NULL,             -- 'datasheet','bom','cert',...
    notes       text        NULL,
    uploaded_by text        NULL,             -- free-text uploader label (§7.1), not an app-user uuid
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX dp_product_documents_product_created_idx
    ON dp_product_documents (product_id, created_at DESC);
