-- 0053_manufacturing_runs.sql
--
-- Product & Manufacturing — P2 manufacturing runs, serialised units,
-- EOL test reports, and the run-level EOL sign-off summary
-- (DOCS/ideas/product-manufacturing.md §5.4 + LOCKED DECISION #3).
--
-- Counter semantics (§5.4, re-test-safe): qty_built counts distinct
-- units; qty_passed/qty_failed count units by their LATEST EOL
-- outcome (a unit moves buckets on re-test, never double-counts), so
-- the CHECK (qty_passed + qty_failed <= qty_built) stays true.
--
-- Serial allocation (§6): the store reserves a contiguous block via a
-- single atomic UPDATE on next_serial_seq — it NEVER rides the
-- user-facing `version` CAS counter.
--
-- Delete policy (§4): runs + units are history-bearing → ON DELETE
-- RESTRICT to the product. EOL reports + the run summary are derived
-- children → ON DELETE CASCADE.

CREATE TABLE dp_manufacturing_runs (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id          uuid        NOT NULL,
    product_id      uuid        NOT NULL REFERENCES dp_products(id)       ON DELETE RESTRICT,
    manufacturer_id uuid        NULL     REFERENCES dp_manufacturers(id)  ON DELETE SET NULL,
    run_code        text        NOT NULL,     -- batch/lot code, e.g. 'R2026-014'
    status          text        NOT NULL DEFAULT 'planned'
        CHECK (status IN ('planned','in_progress','completed','cancelled')),
    qty_planned     integer     NOT NULL DEFAULT 0 CHECK (qty_planned >= 0),
    qty_built       integer     NOT NULL DEFAULT 0 CHECK (qty_built   >= 0),
    qty_passed      integer     NOT NULL DEFAULT 0 CHECK (qty_passed  >= 0),
    qty_failed      integer     NOT NULL DEFAULT 0 CHECK (qty_failed  >= 0),
    next_serial_seq integer     NOT NULL DEFAULT 1,   -- serial allocator (§6); atomic reservation, NOT version CAS
    started_at      timestamptz NULL,
    completed_at    timestamptz NULL,
    notes           text        NULL,
    created_by      uuid        NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    version         bigint      NOT NULL DEFAULT 1,
    CHECK (qty_passed + qty_failed <= qty_built)
);
CREATE UNIQUE INDEX dp_manufacturing_runs_org_code_uniq
    ON dp_manufacturing_runs (org_id, lower(run_code));
CREATE INDEX dp_manufacturing_runs_product_idx
    ON dp_manufacturing_runs (product_id, created_at DESC);

CREATE TABLE dp_product_units (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id          uuid        NOT NULL,
    product_id      uuid        NOT NULL REFERENCES dp_products(id)            ON DELETE RESTRICT,
    run_id          uuid        NULL     REFERENCES dp_manufacturing_runs(id)  ON DELETE RESTRICT,
    serial_number   text        NOT NULL,     -- unique within org (§6)
    -- No stored QR URL: the unit id IS the stable payload; the absolute
    -- URL `{base_url}/u/{id}?t=<token>` is composed at render/SVG time (§6).
    status          text        NOT NULL DEFAULT 'built'
        CHECK (status IN ('built','tested','shipped','returned','scrapped')),
    customer_id     uuid        NULL REFERENCES dp_customers(id) ON DELETE SET NULL,
    built_at        timestamptz NULL,
    shipped_at      timestamptz NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    version         bigint      NOT NULL DEFAULT 1
);
CREATE UNIQUE INDEX dp_product_units_org_serial_uniq
    ON dp_product_units (org_id, serial_number);
CREATE INDEX dp_product_units_run_idx     ON dp_product_units (run_id);
CREATE INDEX dp_product_units_product_idx ON dp_product_units (product_id, created_at DESC);

CREATE TABLE dp_eol_test_reports (
    id            uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    unit_id       uuid        NOT NULL REFERENCES dp_product_units(id) ON DELETE CASCADE,
    result        text        NOT NULL CHECK (result IN ('pass','fail')),
    station       text        NULL,           -- test rig / bench id
    firmware      text        NULL,           -- fw version under test
    measurements  jsonb       NOT NULL DEFAULT '{}'::jsonb,  -- structured results
    log_blob_ref  jsonb       NULL,           -- optional raw-log upload (BlobRef)
    notes         text        NULL,
    tested_by     text        NULL,           -- free-text station operator (§7.1), not an app-user uuid
    tested_at     timestamptz NOT NULL DEFAULT now(),
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX dp_eol_test_reports_unit_idx
    ON dp_eol_test_reports (unit_id, tested_at DESC);

-- Run-level EOL sign-off summary (LOCKED DECISION #3). One row per run:
-- a built/pass/fail snapshot at sign-off time plus operator sign-off and
-- markdown notes. Per-unit reports above stay the source of truth; this
-- is a point-in-time sign-off snapshot. Derived child → ON DELETE CASCADE.
CREATE TABLE dp_run_eol_summary (
    run_id        uuid        PRIMARY KEY REFERENCES dp_manufacturing_runs(id) ON DELETE CASCADE,
    built_count   integer     NOT NULL DEFAULT 0 CHECK (built_count  >= 0),
    pass_count    integer     NOT NULL DEFAULT 0 CHECK (pass_count   >= 0),
    fail_count    integer     NOT NULL DEFAULT 0 CHECK (fail_count   >= 0),
    notes_md      text        NULL,
    signed_by     uuid        NULL,
    signed_at     timestamptz NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    version       bigint      NOT NULL DEFAULT 1
);
