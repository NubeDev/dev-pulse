-- 0054_rma_returns.sql
--
-- Product & Manufacturing — P3 returns / RMA
-- (DOCS/ideas/product-manufacturing.md §5.5).
--
-- One row per return authorisation. The optional unit links a
-- serialised instance (ON DELETE SET NULL — the RMA history outlives
-- a scrapped unit); product_id is required and ON DELETE RESTRICT
-- because the RMA is product-scoped history. Customers are
-- internal-only references (§7) → ON DELETE SET NULL. Status is the
-- usual TEXT+CHECK closed enum (no PG enums); version is the §8.2 CAS
-- counter on this mutable top-level row.

CREATE TABLE dp_rma_returns (
    id           uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id       uuid        NOT NULL,
    unit_id      uuid        NULL REFERENCES dp_product_units(id) ON DELETE SET NULL,
    product_id   uuid        NOT NULL REFERENCES dp_products(id)  ON DELETE RESTRICT,
    customer_id  uuid        NULL REFERENCES dp_customers(id)     ON DELETE SET NULL,
    rma_number   text        NOT NULL,
    under_warranty boolean   NOT NULL DEFAULT false,
    status       text        NOT NULL DEFAULT 'open'
        CHECK (status IN ('open','received','diagnosed','repaired',
                          'replaced','rejected','closed')),
    reason       text        NULL,
    diagnosis    text        NULL,
    resolution   text        NULL,
    received_at  timestamptz NULL,
    resolved_at  timestamptz NULL,
    created_by   uuid        NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    version      bigint      NOT NULL DEFAULT 1
);
CREATE UNIQUE INDEX dp_rma_returns_org_number_uniq
    ON dp_rma_returns (org_id, lower(rma_number));
CREATE INDEX dp_rma_returns_unit_idx     ON dp_rma_returns (unit_id);
CREATE INDEX dp_rma_returns_customer_idx ON dp_rma_returns (customer_id);
CREATE INDEX dp_rma_returns_status_idx   ON dp_rma_returns (org_id, status);
