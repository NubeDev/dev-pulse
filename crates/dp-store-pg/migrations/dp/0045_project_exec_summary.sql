-- 0045_project_exec_summary.sql
--
-- Project Executive Summary — the product-definition surface attached
-- 1-to-1 to every `dp_projects` row. Captures the eight-section form
-- from DOCS/SCOPE-PROJECT-EXECUTIVE-SUMMARY.md (Summary / Scope /
-- Requirements / Hardware / Commercial / Documents / Approval /
-- Change Log) plus the blob-backed image and document attachments.
--
-- Design choices, all from the scope doc §3.1:
--
-- * **One wide table, three children.** Every scalar field is 1-to-1
--   with the project, the form loads/saves them together, and partial
--   PATCH bodies hit a subset of columns — Postgres handles a 40-
--   column row fine. Splitting per-section would force eight joins
--   on every read for no gain.
-- * **`protocols TEXT[]`.** Closed enum of ~12 values, multi-select.
--   A side table would add a join with no upside; the array column
--   is queryable (`'BACnet IP' = ANY(protocols)`) and trivially
--   serdes to/from JSON on the wire.
-- * **`status` CHECK constraint.** The state machine is
--   `draft / in_review / approved`, with `revert` allowed from any
--   state back to `draft`. Schema enforces the closed vocabulary;
--   §3.4 of the scope owns the transition rules.
-- * **`*_cents` for money.** No floats for currency, ever.
--   `target_gp_pct NUMERIC(5,2)` for the gross-profit percent (range
--   `0.00`–`999.99`; the over-100 case is a legitimate cost-recovery
--   model that some hardware lines use).
-- * **`BlobRef` as JSONB.** The starter blob crates serde a `BlobRef`
--   to a small JSON object; storing it as JSONB lets us round-trip
--   it verbatim without inventing a column-per-field encoding.
--   Confirmed shape is opaque per B2 — we never index into it.
-- * **`CASCADE` everywhere.** When a project is deleted, the exec
--   summary, every image / document row, and every change-log entry
--   go with it. There is no standalone exec-summary lifecycle.
--
-- The blob bytes themselves live in whatever `BlobStore` the server
-- picks at boot (fs in dev, garage in prod). Rows here carry the
-- `BlobRef` JSON so the backend swap stays a one-line wiring change
-- per the storage scope's §"Swap test".
--
-- See [crates/dp-rest/src/project_exec_summary.rs] for the handlers
-- and [DOCS/SCOPE-PROJECT-EXECUTIVE-SUMMARY.md] §6 for the per-section
-- completion rules the GET handler computes from this row.

-- ---------- dp_project_exec_summary ---------------------------------

CREATE TABLE dp_project_exec_summary (
    project_id           uuid PRIMARY KEY
                           REFERENCES dp_projects(id) ON DELETE CASCADE,

    -- Summary section (§3.1 / form tab 01)
    product_name         text         NULL,
    part_number          text         NULL,
    target_release_date  date         NULL,
    objective            text         NULL,   -- markdown
    problem              text         NULL,   -- markdown
    value                text         NULL,   -- markdown
    differentiators      text         NULL,   -- markdown
    success_criteria     text         NULL,   -- markdown

    -- Scope section (form tab 02)
    in_scope             text         NULL,
    out_of_scope         text         NULL,
    assumptions          text         NULL,
    dependencies         text         NULL,
    constraints          text         NULL,

    -- Requirements section (form tab 03)
    must_have            text         NULL,
    optional             text         NULL,
    user_interaction     text         NULL,
    architecture         text         NULL,
    protocols            text[]       NOT NULL DEFAULT '{}',
    power                text         NULL,
    mounting             text         NULL,
    certification        text         NULL,

    -- Hardware section (form tab 04). Reference images live in
    -- `dp_project_exec_summary_images`; the textual fields stay here.
    hardware_features    text         NULL,
    physical_notes       text         NULL,
    enclosure            text         NULL,
    mounting_type        text         NULL,
    operating_env        text         NULL,

    -- Commercial section (form tab 05)
    rrp_cents            bigint       NULL CHECK (rrp_cents       IS NULL OR rrp_cents       >= 0),
    oem_price_cents      bigint       NULL CHECK (oem_price_cents IS NULL OR oem_price_cents >= 0),
    target_gp_pct        numeric(5,2) NULL CHECK (target_gp_pct   IS NULL OR target_gp_pct   >= 0),
    revenue_model        text         NULL,
    channel_strategy     text         NULL,
    target_market        text         NULL,
    volume_assumptions   text         NULL,

    -- Approval section (form tab 07). `reviewer` / `approver` are
    -- free-text contact strings in 0.1 — the scope's §8 open question
    -- about multi-sign-off would promote these to a join table.
    status               text         NOT NULL DEFAULT 'draft'
                           CHECK (status IN ('draft', 'in_review', 'approved')),
    reviewer             text         NULL,
    approver             text         NULL,
    review_notes         text         NULL,
    approval_notes       text         NULL,
    submitted_at         timestamptz  NULL,
    approved_at          timestamptz  NULL,

    created_at           timestamptz  NOT NULL DEFAULT now(),
    updated_at           timestamptz  NOT NULL DEFAULT now()
);

-- ---------- dp_project_exec_summary_images -------------------------
--
-- Reference / hero / concept images for the Hardware section. Ordered
-- by `ord` so the UI can drag-reorder without renumbering every row.
CREATE TABLE dp_project_exec_summary_images (
    id            uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id    uuid        NOT NULL REFERENCES dp_projects(id) ON DELETE CASCADE,
    blob_ref      jsonb       NOT NULL,
    filename      text        NOT NULL,
    content_type  text        NOT NULL,
    caption       text        NULL,
    ord           integer     NOT NULL DEFAULT 0,
    created_at    timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX dp_project_exec_summary_images_project_ord_idx
    ON dp_project_exec_summary_images (project_id, ord);

-- ---------- dp_project_exec_summary_documents ----------------------
--
-- Supporting documents for the Documents section. `doc_type` is
-- free-form text in 0.1 (the UI offers a suggestions dropdown:
-- 'brief', 'bom', 'datasheet', 'compliance', 'test_report',
-- 'contract', 'supplier_note', 'other') so we don't have to migrate
-- the schema every time product adds a category.
CREATE TABLE dp_project_exec_summary_documents (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      uuid        NOT NULL REFERENCES dp_projects(id) ON DELETE CASCADE,
    blob_ref        jsonb       NOT NULL,
    title           text        NOT NULL,
    doc_type        text        NULL,
    notes           text        NULL,
    required_action text        NULL,
    uploaded_by     text        NULL,
    created_at      timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX dp_project_exec_summary_documents_project_created_idx
    ON dp_project_exec_summary_documents (project_id, created_at DESC);

-- ---------- dp_project_exec_summary_changelog ----------------------
--
-- Append-only per the scope's E5 rule. `version` is free-form text
-- (semver, calver, internal codes — product picks). `changed_at` is
-- DATE not TIMESTAMPTZ because the UI exposes a date input and we
-- don't want a tz-interpretation surprise the way `dp_milestones`
-- §0030 already documents.
CREATE TABLE dp_project_exec_summary_changelog (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id  uuid        NOT NULL REFERENCES dp_projects(id) ON DELETE CASCADE,
    version     text        NOT NULL,
    changed_at  date        NOT NULL,
    changed_by  text        NOT NULL,
    summary     text        NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX dp_project_exec_summary_changelog_project_date_idx
    ON dp_project_exec_summary_changelog (project_id, changed_at DESC);
