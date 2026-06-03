-- 0050_manufacturing_master_data.sql
--
-- Product & Manufacturing — P1 master data (DOCS/ideas/product-manufacturing.md §5.1).
--
-- Three independent master-data tables: manufacturers, suppliers,
-- customers. Same shape; contact details are deliberately free-text
-- for v1. Suppliers are scaffolded now (table + CRUD) even though
-- nothing consumes them yet — BOM/part-sourcing is deferred (P4, §2).
-- Customers add `account_ref` (external CRM/ERP id).
--
-- House conventions (§4): `id uuid` PK, `org_id` scoping, audit
-- columns, `version bigint` CAS, soft-delete via `archived_at`, and a
-- partial-unique case-insensitive name index that excludes archived
-- rows so a name can be reused after archival.

CREATE TABLE dp_manufacturers (
    id           uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id       uuid        NOT NULL,
    name         text        NOT NULL,
    contact_name text        NULL,
    email        text        NULL,
    phone        text        NULL,
    address      text        NULL,
    website      text        NULL,
    notes        text        NULL,          -- markdown
    archived_at  timestamptz NULL,
    created_by   uuid        NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    version      bigint      NOT NULL DEFAULT 1
);
CREATE UNIQUE INDEX dp_manufacturers_org_name_uniq
    ON dp_manufacturers (org_id, lower(name)) WHERE archived_at IS NULL;
CREATE INDEX dp_manufacturers_org_idx ON dp_manufacturers (org_id);

CREATE TABLE dp_suppliers (
    id           uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id       uuid        NOT NULL,
    name         text        NOT NULL,
    contact_name text        NULL,
    email        text        NULL,
    phone        text        NULL,
    address      text        NULL,
    website      text        NULL,
    notes        text        NULL,          -- markdown
    archived_at  timestamptz NULL,
    created_by   uuid        NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    version      bigint      NOT NULL DEFAULT 1
);
CREATE UNIQUE INDEX dp_suppliers_org_name_uniq
    ON dp_suppliers (org_id, lower(name)) WHERE archived_at IS NULL;
CREATE INDEX dp_suppliers_org_idx ON dp_suppliers (org_id);

CREATE TABLE dp_customers (
    id           uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id       uuid        NOT NULL,
    name         text        NOT NULL,
    contact_name text        NULL,
    email        text        NULL,
    phone        text        NULL,
    address      text        NULL,
    website      text        NULL,
    notes        text        NULL,          -- markdown
    account_ref  text        NULL,          -- external CRM/ERP id
    archived_at  timestamptz NULL,
    created_by   uuid        NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    version      bigint      NOT NULL DEFAULT 1
);
CREATE UNIQUE INDEX dp_customers_org_name_uniq
    ON dp_customers (org_id, lower(name)) WHERE archived_at IS NULL;
CREATE INDEX dp_customers_org_idx ON dp_customers (org_id);
