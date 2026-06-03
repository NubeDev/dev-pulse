# Product & Manufacturing — Scope

> Status: **Draft / idea** · Owner: TBD · Last updated: 2026-06-03
>
> A new domain area for tracking **products** through their lifecycle:
> definition → manufacturing runs → serialised units → end-of-line
> testing → shipping → RMA / warranty returns. Plus the supporting
> master data (customers, manufacturers, suppliers), user manuals,
> document uploads, and links back into `dp_projects`.

This document is a scope, not an implementation plan. It describes the
data model, the layer-by-layer surface area, and a suggested phasing.
It deliberately mirrors existing dev-pulse conventions so the work
slots into the codebase rather than fighting it.

> **⚑ Peer review (2026-06-03) incorporated.** Four runtime-correctness
> fixes plus hardening notes are folded in and marked inline with ⚑:
> (1) serial allocation uses an atomic `next_serial_seq` reservation,
> never the user-facing `version` CAS; (2) no absolute URL is stored —
> the unit id is the QR payload and the URL is composed at the edge;
> (3) run counters are defined as current-state and re-test-safe;
> (4) history-bearing children (`runs`, `units`, `rma_returns`) use
> `ON DELETE RESTRICT`, not `CASCADE`. Smaller hardening: partial-unique
> published revision, `under_warranty` boolean (not a `return_type`
> enum), bulk-create cap, `serial_format` validation, and an explicit
> org-scoping/IDOR note.

---

## 1. Goals

- A first-class **Product** object (model number, status, owning
  manufacturer) that can be linked to **many projects**.
- **Master data** tables: `customers`, `manufacturers`, and
  `suppliers`. Suppliers are scaffolded now even though nothing
  consumes them yet — the table and CRUD exist; BOM/part-sourcing is a
  later phase.
- **Manufacturing runs** (production batches) for a product, with
  planned/built/pass/fail counters.
- **Serialised units** — one row per physical unit, carrying a unique
  **serial number**, the product's **model number**, and a **QR code**
  payload that resolves back to the unit page.
- **End-Of-Line (EOL) test reports** — one report per unit (pass/fail +
  measurements + optional raw-log upload).
- **RMA returns and warranty returns** — a return workflow keyed on a
  unit + customer.
- **User manuals** authored in **markdown**, with **revision numbers**
  and draft/published status.
- **Document uploads** per product, reusing the existing blob-store
  pattern already shipped for project exec-summaries.

## 2. Non-goals (for the first phases)

- Bill-of-materials / parts inventory / supplier part catalogues
  (supplier table exists; relationships are future work — §11).
- Stock levels, purchasing, or accounting integration.
- Label-printer hardware integration (we generate the QR/label
  artefact; physically printing it is out of scope).
- Per-unit firmware OTA or device telemetry.
- Multi-tenant changes beyond the existing `org_id` scoping.

## 3. Glossary

| Term | Meaning |
|---|---|
| **Product** | A model/SKU the org designs or sells. Has a model number. |
| **Manufacturing run** | A production batch of one product (a.k.a. build/lot). |
| **Unit** | One physical, serialised instance of a product. |
| **EOL test** | End-Of-Line test performed on a unit before it ships. |
| **RMA** | Return Merchandise Authorization — a customer-initiated return. |
| **Warranty return** | A return with `under_warranty = true`. An orthogonal flag on the return, not a separate kind. |
| **Manufacturer** | The party that builds the product (may be the org itself or a CM). |
| **Supplier** | A party that supplies parts/components. Scaffolded, not yet wired. |
| **Customer** | The party a unit ships to / that raises an RMA. |

---

## 4. Domain model

```
dp_manufacturers ─┐
                  ├─< dp_products >─┬─< dp_product_project_links >── dp_projects
dp_suppliers      │   (model #)    │
 (future BOM)     │                ├─< dp_product_documents        (blob uploads)
                  │                ├─< dp_product_manuals >─< dp_product_manual_revisions (markdown)
                  │                └─< dp_manufacturing_runs >─< dp_product_units >─┬─< dp_eol_test_reports
dp_customers ─────┴──────────────────────(ships-to / raises)──────────────┘        └─< dp_rma_returns >── dp_customers
```

Cardinalities:

- **Manufacturer 1—N Product** — a product has one owning manufacturer
  (nullable; can also be overridden per run).
- **Product N—N Project** — via `dp_product_project_links`. A product
  can appear in many projects; a project can reference many products.
- **Product 1—N Manufacturing run.**
- **Manufacturing run 1—N Unit.**
- **Unit 1—N EOL test report** (re-test allowed; latest = current).
- **Unit 1—N RMA return** (a unit can come back more than once).
- **Product 1—N Manual; Manual 1—N Revision.**
- **Product 1—N Document.**
- **Customer 1—N Unit** (shipped-to, nullable) and **1—N RMA.**

All tables follow the house conventions confirmed from `dp_projects`
and `dp_project_exec_summary*`:

- `id uuid PRIMARY KEY DEFAULT gen_random_uuid()`
- `org_id uuid NOT NULL` scoping on every top-level entity (FKs are raw
  `Uuid`, **no newtype wrappers** — matches `project.rs`).
- `created_at` / `updated_at timestamptz NOT NULL DEFAULT now()`.
- `created_by uuid` with `ON DELETE SET NULL` where we want history to
  survive user pseudonymisation.
- `version bigint NOT NULL DEFAULT 1` CAS counter on every **mutable**
  top-level row (PATCH/archive send `expected_version`; `WHERE id = ?
  AND version = ?`). Append-only child tables (revisions, test reports)
  do **not** need a version.
- Closed enums stored as `TEXT` + `CHECK (col IN (...))`, with a Rust
  enum exposing `as_str()` / `from_str()` and an `encode.rs`
  `*_from_text` helper — never a Postgres `ENUM` type.
- Soft-delete via `archived_at timestamptz NULL` where archival is
  wanted (products, customers, manufacturers, suppliers). Operational
  rows (runs, units, returns) use lifecycle status instead.
- **⚑ Delete policy.** A product is *archived*, never hard-deleted, so
  its history must not be cascaded away. History-bearing children
  (`dp_manufacturing_runs`, `dp_product_units`, `dp_rma_returns`) use
  `ON DELETE RESTRICT` — a hard `DELETE` is forced to fail rather than
  silently destroy shipped-unit / warranty / RMA records you may be
  legally required to retain. `ON DELETE CASCADE` is reserved for
  genuinely derived rows: `dp_product_project_links`,
  `dp_product_documents`, `dp_product_manuals` + revisions.
- Opaque blob handles stored as `jsonb` `blob_ref` columns.

---

## 5. Tables / migrations

Migrations currently run to `0049`. The block below is suggested as
`0050`–`0054`; **renumber to the next free slots at implementation
time**. Grouping mirrors `0045_project_exec_summary.sql` (one wide
parent + its children per file).

### 5.1 `0050_manufacturing_master_data.sql`

Three independent master-data tables. Same shape; contact details kept
deliberately simple (free-text) for v1.

```sql
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

-- dp_suppliers: identical columns (scaffold; no consumers yet).
-- dp_customers: identical columns + an optional account_ref text.
```

`dp_suppliers` and `dp_customers` are column-for-column the same as
`dp_manufacturers` (with `dp_customers` adding `account_ref text NULL`
for an external CRM/ERP id). Kept as three tables rather than one
polymorphic `parties` table because their lifecycles and access
patterns diverge later (customers gain RMAs; suppliers gain a BOM).

### 5.2 `0051_products.sql`

The product/model definition, the project link table, and the document
table (document upload reuses the exec-summary precedent verbatim).

```sql
CREATE TABLE dp_products (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id          uuid        NOT NULL,
    name            text        NOT NULL,
    model_number    text        NOT NULL,
    description     text        NULL,        -- markdown
    manufacturer_id uuid        NULL REFERENCES dp_manufacturers(id) ON DELETE SET NULL,
    status          text        NOT NULL DEFAULT 'active'
        CHECK (status IN ('draft','active','eol','archived')),
    -- Serial-number generation config (see §6).
    serial_prefix   text        NULL,        -- e.g. 'NB'
    serial_format   text        NULL,        -- template, e.g. '{prefix}-{run}-{seq:05}'
    archived_at     timestamptz NULL,
    created_by      uuid        NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    version         bigint      NOT NULL DEFAULT 1
);
CREATE UNIQUE INDEX dp_products_org_model_uniq
    ON dp_products (org_id, lower(model_number)) WHERE archived_at IS NULL;
CREATE INDEX dp_products_org_status_idx ON dp_products (org_id, status);

-- N—N product ↔ project. Dedicated join table (NOT dp_tag_links):
-- this is a first-class relationship the product page renders, with
-- its own audit columns.
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

-- Document uploads — mirrors dp_project_exec_summary_documents.
CREATE TABLE dp_product_documents (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    product_id  uuid        NOT NULL REFERENCES dp_products(id) ON DELETE CASCADE,
    blob_ref    jsonb       NOT NULL,         -- opaque BlobRef
    title       text        NOT NULL,
    doc_type    text        NULL,             -- 'datasheet','bom','cert',...
    notes       text        NULL,
    uploaded_by text        NULL,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX dp_product_documents_product_created_idx
    ON dp_product_documents (product_id, created_at DESC);
```

> **Link mechanism decision.** A dedicated join table is preferred over
> extending the polymorphic `dp_tag_links` (which now has a `'project'`
> kind via `0049`). `dp_tag_links` models *tagging*; product↔project is
> a structural relationship surfaced on both the product and project
> pages, so it gets its own table with `linked_by`/`linked_at`.

### 5.3 `0052_product_manuals.sql`

User manuals as markdown with revision numbers. A manual is a named
container; each save creates an immutable revision. The product page
shows the **published** revision; editors work on a **draft**.

```sql
CREATE TABLE dp_product_manuals (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    product_id  uuid        NOT NULL REFERENCES dp_products(id) ON DELETE CASCADE,
    title       text        NOT NULL,         -- e.g. 'Installation Guide'
    created_by  uuid        NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    version     bigint      NOT NULL DEFAULT 1
);

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
-- At most one published revision per manual — enforced by a PARTIAL
-- UNIQUE INDEX (unbreakable even under a concurrent or buggy publish),
-- backed by the store tx that flips the prior published revision to
-- 'superseded' when a new one is published.
CREATE INDEX dp_product_manual_revisions_manual_idx
    ON dp_product_manual_revisions (manual_id, created_at DESC);
CREATE UNIQUE INDEX dp_product_manual_revisions_manual_rev_uniq
    ON dp_product_manual_revisions (manual_id, lower(revision));
CREATE UNIQUE INDEX dp_product_manual_revisions_one_published
    ON dp_product_manual_revisions (manual_id) WHERE status = 'published';
```

Markdown is rendered with the existing `frontend/src/components/markdown.tsx`
(`react-markdown` + `remark-gfm`), the same component used for
exec-summary long-text fields.

### 5.4 `0053_manufacturing_runs.sql`

Runs and the serialised units they produce, plus EOL test reports.

```sql
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
    next_serial_seq integer     NOT NULL DEFAULT 1,   -- serial allocator; bumped by an atomic reservation, NOT the version CAS (§6)
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
    -- URL is composed at render/SVG time from a configured base (§6).
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
    tested_by     text        NULL,           -- free-text station operator, not an app-user uuid (§7.1)
    tested_at     timestamptz NOT NULL DEFAULT now(),
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX dp_eol_test_reports_unit_idx
    ON dp_eol_test_reports (unit_id, tested_at DESC);
```

**⚑ Counter semantics (re-test-safe).** `qty_built` counts distinct
units in the run. `qty_passed` / `qty_failed` count **units by their
latest EOL outcome**, *not* test events — a unit that fails and then
passes on re-test moves from the failed bucket to the passed bucket
rather than incrementing both. The store maintains them on the
*transition of a unit's latest outcome* (decrement old bucket, increment
new), keyed on `dp_product_units`; untested units sit in neither bucket,
keeping the `qty_passed + qty_failed <= qty_built` CHECK true.
`dp_projects.issue_count` is a clean precedent only because project
membership is monotonic — EOL re-tests are not, so the maintenance rule
deliberately differs (do **not** bump per report insert).

### 5.5 `0054_rma_returns.sql`

RMA / warranty returns. One table; warranty is the orthogonal
`under_warranty` boolean (see §5.5 prose), not a separate table or a
return "kind".

```sql
CREATE TABLE dp_rma_returns (
    id           uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id       uuid        NOT NULL,
    unit_id      uuid        NULL REFERENCES dp_product_units(id) ON DELETE SET NULL,
    product_id   uuid        NOT NULL REFERENCES dp_products(id)  ON DELETE RESTRICT,
    customer_id  uuid        NULL REFERENCES dp_customers(id)     ON DELETE SET NULL,
    rma_number   text        NOT NULL,        -- human-facing RMA id
    under_warranty boolean   NOT NULL DEFAULT false,  -- orthogonal flag, not a return "kind"
    status       text        NOT NULL DEFAULT 'open'
        CHECK (status IN ('open','received','diagnosed','repaired',
                          'replaced','rejected','closed')),
    reason       text        NULL,            -- customer-reported fault
    diagnosis    text        NULL,            -- markdown
    resolution   text        NULL,            -- markdown
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
```

`unit_id` is nullable so a return can be opened before the unit is
matched (customer quotes a serial we then resolve). `product_id` is
mandatory (and `ON DELETE RESTRICT`) so the return always rolls up to a
product and a product can't be hard-deleted out from under its return
history. **⚑ Warranty is the orthogonal `under_warranty` boolean**, not
a `return_type` value: per the glossary a warranty return *is* an RMA,
just one covered by warranty, so a sibling `'rma'`/`'warranty'` enum
would force `'rma'` to mean "non-warranty". If a disposition taxonomy
(repair / replace / refund) is later needed it is a separate orthogonal
column, not this flag.

---

## 6. Serial number, model number & QR code

- **Model number** lives on the **product** (`dp_products.model_number`,
  unique per org). It is shared by every unit of that product.
- **Serial number** is allocated **per unit** at creation. A serial is
  unique within the org (`dp_product_units` unique index).

**⚑ Allocation — atomic reservation, not CAS.** Serial allocation must
*not* ride the run's user-facing `version` counter. Bumping `version` on
every unit insert would (a) hand a spurious 409 to anyone who loaded the
run and then PATCHes its status/notes (units were added underneath
them), and (b) turn concurrent bulk allocations into CAS retry storms.
Instead reserve a contiguous block of sequence numbers in one statement
that touches only `next_serial_seq` and leaves `version` alone:

```sql
UPDATE dp_manufacturing_runs
   SET next_serial_seq = next_serial_seq + $N
 WHERE id = $run
RETURNING next_serial_seq - $N AS first_seq;   -- reserves [first_seq, first_seq + N)
```

The handler then formats `$N` serials from the reserved block and bulk
inserts the units. `version` stays reserved for genuine user edits.
Default template if `serial_format` is null:

```
{prefix}-{run_code}-{seq:05}        e.g.  NB-R2026-014-00042
```

**Bulk cap.** Cap `N` per request (e.g. 1000) and chunk anything larger
— "add 50" and "add 50,000" are otherwise the same single-transaction
code path. The reservation above keeps counter contention to one short
UPDATE regardless of `N`.

**`serial_format` is operator-supplied** and parsed app-side, so
validate it on save: a strict token whitelist (`{prefix}`, `{run_code}`,
`{seq[:NN]}`) and a **mandatory `{seq}` token**. Without `{seq}`, every
unit after the first collides on the unique index while still consuming
sequence numbers.

**⚑ QR code — store the id, compose the URL at the edge.** Do **not**
store an absolute `https://<host>/u/<id>` in a column: it bakes the host
and environment into row data, breaks across staging/prod and custom
domains, and would need a full-table rewrite to change. There is no
stored `qr_payload`. The unit **id** is the stable payload; both
renderers compose the absolute URL from a configured base URL at output
time:

```
{configured_base_url}/u/{unit_id}
```

Encoding the opaque `unit_id` (not the serial) keeps the link stable if
a serial is ever corrected, while the serial stays the human-readable
label printed beside the code. Rendering:

- **On-screen** (unit page, run table): render client-side from
  `unit_id` + the app's base URL with a small React QR component (e.g.
  `qrcode.react` — a **new dependency to vet**; only `react-markdown` /
  `remark-gfm` exist today). No backend round-trip.
- **For labels / PDF export**: `GET /products/units/{id}/qr.svg` returns
  a crisp SVG generated with the Rust [`qrcode`] crate, composing the
  same URL from server config. This plugs into the existing PDF export
  path (cf. recent "improved pdf export" work) for printable label
  sheets.

If a non-URL payload (GS1, DataMatrix, serial-only) is ever needed,
store it **host-less** and still compose any environment-specific prefix
at the edge.

No QR/serial code exists in the repo today — this is greenfield.

[`qrcode`]: https://crates.io/crates/qrcode

---

## 7. Layer-by-layer surface area

Conventions below are taken directly from `project.rs`, `store/projects.rs`,
`reports.rs`, and `project_exec_summary.rs`.

### 7.1 Domain (`dp-domain`)

New modules, each following the `Project` / `ProjectUpsert` read-shape +
upsert-shape split, with `#[serde(rename_all = "lowercase")]` enums
carrying `as_str()` / `from_str()`:

- `product.rs` — `Product`, `ProductUpsert`, `ProductStatus`.
- `party.rs` (or three files) — `Customer`, `Manufacturer`, `Supplier`
  + their upserts. Shared shape; one file with three structs is fine.
- `manufacturing.rs` — `ManufacturingRun`, `RunUpsert`, `RunStatus`,
  `ProductUnit`, `UnitUpsert`, `UnitStatus`.
- `eol.rs` — `EolTestReport`, `EolTestUpsert`, `EolResult`.
- `rma.rs` — `RmaReturn`, `RmaUpsert`, `RmaStatus` (warranty is a
  `bool under_warranty` field, not an enum — §5.5).
- `product_manual.rs` — `ProductManual`, `ManualRevision`, `RevisionStatus`.
- `product_doc.rs` — `ProductDocument` (mirrors `ExecSummaryDocument`).

Extend the `Store` trait in `dp-domain/src/store.rs` with the new
methods (list/get/upsert/archive per entity; allocate-units;
record-eol; link/unlink-project).

**Actor columns.** `created_by` / `authored_by` are app-user `Uuid`s
(`ON DELETE SET NULL`). `uploaded_by` / `tested_by` are free-text — the
station operator or uploader label inherited from the `0045` precedent,
**not** an app user. Keep them `text` deliberately and comment why; if
they ever need to be real app users, standardise on `Uuid`.

### 7.2 Store (`dp-store-pg`)

- One `store/<entity>.rs` per area (`products.rs`, `parties.rs`,
  `manufacturing.rs`, `rma.rs`, `product_manuals.rs`) with `_impl`
  methods, raw `sqlx::query`, and `.map_err(map_sqlx)?`.
- Row mappers `row_to_product`, `row_to_unit`, … added to
  `store/rows.rs`.
- Enum `*_from_text` / `*_to_text` helpers added to `encode.rs`, each
  with a round-trip unit test.
- Delegations wired in `store/mod.rs`.
- Serial allocation uses the atomic `next_serial_seq` reservation (§6),
  never the run's `version` CAS. Run counter maintenance follows the
  re-test-safe rule (§5.4): adjust `qty_passed` / `qty_failed` only when
  a new EOL report changes a unit's *latest* outcome, not on every
  insert.
- Bulk unit creation caps and chunks `N` (§6) so one request never holds
  a pathologically long transaction.

### 7.3 REST (`dp-rest`)

New modules `products.rs`, `parties.rs`, `manufacturing.rs`, `rma.rs`,
`product_manuals.rs`, each an axum router of `#[utoipa::path]` handlers
returning `Result<Json<T>, ApiError>`, registered in `lib.rs` and the
OpenAPI doc in `openapi.rs`, wrapped with `with_permission(resource,
action)`. Indicative routes:

```
# master data (same shape ×3)
GET/POST            /customers            /manufacturers          /suppliers
GET/PATCH/DELETE    /customers/{id}       /manufacturers/{id}     /suppliers/{id}

# products
GET  /products                 POST /products
GET/PATCH/DELETE /products/{id}
POST/DELETE      /products/{id}/projects/{project_id}     # link / unlink
GET  /projects/{id}/products                              # reverse view

# manuals (markdown + revisions)
GET/POST   /products/{id}/manuals
GET/POST   /products/{id}/manuals/{manual_id}/revisions   # POST = new revision
POST       /products/{id}/manuals/{manual_id}/revisions/{rev_id}/publish

# documents (multipart upload — reuse exec-summary read_upload + BlobStore)
POST   /products/{id}/documents
DELETE /products/{id}/documents/{doc_id}
GET    /blobs/product/{kind}/{row_id}                      # proxy download

# manufacturing
GET/POST          /products/{id}/runs
GET/PATCH         /runs/{run_id}
POST              /runs/{run_id}/units            # allocate N serialised units
GET               /runs/{run_id}/units
GET/PATCH         /units/{unit_id}
GET               /units/{unit_id}/qr.svg
POST/GET          /units/{unit_id}/eol            # record / list EOL reports

# returns
GET/POST          /rma            (filter by status/customer/unit/product)
GET/PATCH         /rma/{id}
```

Document upload reuses the exact exec-summary precedent: `read_upload`
multipart parser, 25 MiB cap, `BlobStore::put_bytes`, `blob_ref` JSONB
persisted, proxy `GET /blobs/...` streams it back with
`Content-Disposition`.

### 7.4 Frontend / UI (`frontend/src/`)

UI is a first-class deliverable, not an afterthought — every backend
phase ships its screens (see the phasing table, §9). A new `products/`
feature area mirrors `projects/` and reuses the same stack with **zero
new UI frameworks**: shadcn/ui + Tailwind, `lucide-react` icons,
`@tanstack/react-query` (a `productsKeys` cache-key factory +
`staleTime` like `projectsKeys`), the `Markdown` component
(`react-markdown` + `remark-gfm`), and Zod DTO schemas under
`frontend/src/api/schemas/` (cf. `exec-summary.ts`). Concretely reuse
the exec-summary building blocks: `documents-section.tsx` (upload UI),
`form-fields.tsx` (labelled inputs), and the `view-wizard` dialog
pattern for create/edit modals. The only genuinely new dependency is the
client QR renderer (`qrcode.react`, §6).

#### 7.4.1 Information architecture / navigation

- New **Products** entry in the left sidebar, peer to **Projects**
  (same sidebar component, `icon-for-name.ts` gets a product icon).
- **Master data** (customers · manufacturers · suppliers) lives under a
  **Manufacturing ▸ Parties** settings area — low-traffic admin lists,
  kept out of the main product flow.
- Deep links: `/products`, `/products/{id}` (tabs as query/sub-route),
  `/runs/{id}`, `/units/{id}`, `/rma`, `/rma/{id}`, plus the public-ish
  `/u/{id}` unit landing page a QR scan resolves to.

#### 7.4.2 Screen inventory

| # | Screen | Route | Purpose |
|---|---|---|---|
| 1 | Products hub | `/products` | List/grid, status filter, search by name/model #, create. |
| 2 | Product detail | `/products/{id}` | Tabbed: **Overview · Projects · Runs · Units · Manuals · Documents · Returns**. |
| 3 | Run detail | `/runs/{id}` | Counters, units table, *add units*, status transitions. |
| 4 | Unit detail | `/units/{id}` | Serial, model #, QR, EOL history, RMA history, ship/customer. |
| 5 | Unit landing | `/u/{id}` | Lean scan-target page (QR resolves here): serial, model, status, manuals. |
| 6 | EOL record | dialog | Record pass/fail + measurements + optional log upload. |
| 7 | RMA list | `/rma` | Filter by status/customer/product; create. |
| 8 | RMA detail | `/rma/{id}` | Status workflow, diagnosis/resolution (markdown), warranty flag. |
| 9 | Manual editor | tab/route | Markdown + live preview, revision list, publish. |
| 10 | Parties admin | `/manufacturing/parties` | Customers/manufacturers/suppliers lists + edit dialogs. |
| 11 | Customer detail | `/customers/{id}` | Their shipped units + open RMAs. |

#### 7.4.3 Per-screen detail

- **Products hub** (`products/product-list.tsx`) — card/table grid;
  status chips (`draft/active/eol/archived`); search box; `Create
  product` dialog (name, model #, manufacturer select, status). Empty
  state with a primary CTA.
- **Product detail** (`products/product-detail-page.tsx`) — tab
  container modeled on `project-detail-page.tsx`:
  - *Overview* — model #, manufacturer, status, markdown description
    (rendered via `Markdown`, edited in a textarea), and the
    serial-format config (`serial_prefix`, `serial_format` with a live
    "example serial" preview validated per §6).
  - *Projects* — linked projects with link/unlink (reverse of the
    project-page panel, §7.4.6); add via a project picker dialog.
  - *Runs* — table of manufacturing runs (code, status, qty
    planned/built/pass/fail), `New run` dialog, row → run detail.
  - *Units* — paginated/filterable serial table (filter by run +
    status), serial/model columns, QR thumbnail, row → unit detail.
  - *Manuals* — list of manuals; open → manual editor (§7.4.5).
  - *Documents* — **reuse `documents-section.tsx` verbatim** (multipart
    upload, list, download, delete) against `/products/{id}/documents`.
  - *Returns* — this product's RMAs (subset of screen 7), `New RMA`.
- **Run detail** (`products/runs/run-detail.tsx`) — header with the four
  counters as stat cards and a yield % derived from them; status control
  (`planned → in_progress → completed/cancelled`); units table; an
  **Add units** dialog that takes a quantity `N` (capped per §6), calls
  the allocation endpoint, and shows the freshly reserved serial range.
- **Unit detail** (`products/units/unit-detail.tsx`) — serial + model #,
  a **QR rendered client-side** from `unit_id` + base URL (§6) with a
  *Download SVG* / *Print label* action hitting `qr.svg`; status; ship
  / assign-customer control; EOL report timeline; RMA history.
- **Unit landing** (`/u/{id}`) — deliberately lean read-only page a
  scanned QR opens (may be viewed on a phone): serial, model, current
  status, and links to published manuals. Honors the same authz.
- **EOL record** (`products/eol/eol-dialog.tsx`) — pass/fail toggle,
  station + firmware fields, a key/value measurements editor (writes the
  `measurements` JSONB), optional raw-log file upload (blob), notes,
  tester. A run-level **bulk EOL** entry mode for batch testing.
- **RMA list / detail** (`products/rma/*`) — list with status + customer
  filters; detail drives the status workflow
  (`open → received → diagnosed → repaired/replaced/rejected → closed`)
  as a stepper, an `under_warranty` toggle, and markdown
  diagnosis/resolution fields. **Create-RMA flow**: enter a serial →
  resolve to a unit (or leave unmatched with product chosen), pick
  customer, reason.
- **Manual editor** (`products/manuals/manual-editor.tsx`) — split-pane
  markdown textarea + live `Markdown` preview; revision sidebar (revision
  string, status badge); `Save draft` and `Publish` (publish flips prior
  published → superseded, §5.3); read-only view of older revisions with a
  "what changed" note.
- **Parties admin** (`products/parties/*`) — three near-identical
  list+edit screens (customers, manufacturers, suppliers) built from one
  shared component; create/edit dialogs from `form-fields.tsx`. Supplier
  screen is present but visibly "scaffold / not yet wired" (§2).
- **Customer detail** — that customer's shipped units and open RMAs;
  read-only rollups for support.

#### 7.4.4 Cross-cutting UI concerns

- **States** — every list/detail handles loading (skeletons), empty
  (CTA), and error (the `ApiError.code` → message map) explicitly, as
  the projects surfaces do.
- **CAS 409 handling** — mutating screens (product/run/unit/RMA edits,
  publish) send `expected_version`; on a 409 the UI refetches and shows
  a "changed underneath you" banner rather than silently clobbering.
  Note: per §6 *adding units does not bump the run's version*, so the
  run editor won't spuriously 409 while a build is in progress.
- **Permission-gated actions** — write actions (create/edit/publish/
  upload/record-EOL) are hidden or disabled by the `manufacturing`
  permission (§8); read views stay visible.
- **QR / labels** — on-screen QR is client-rendered; print/PDF label
  sheets use the `qr.svg` endpoint and the existing PDF export path.

#### 7.4.5 Project-page integration

On the existing **project** detail page, a small **Products** panel
lists linked products with link/unlink (the reverse of
`/products/{id}/projects`), so the relationship is reachable and
editable from both sides.

---

## 8. Permissions

The repo gained authz recently (`with_permission(resource, action)`).
**Start with one umbrella `manufacturing` resource** (read/write) for
v1 — splitting it later into finer resources is a non-breaking, additive
change, so don't over-invest now:

- `manufacturing` — products, parties, runs, units, EOL, returns,
  manuals, documents.

Natural split if/when needed: `products` (products, manuals, documents,
links) · `manufacturing` (runs, units, EOL) · `returns` (RMA) ·
`parties` (customers, manufacturers, suppliers).

**⚑ Org-scoping / IDOR.** Only top-level rows carry `org_id` (products,
runs, units, rma); children (EOL reports, manual revisions, documents,
project links) inherit org via their FK chain. Every list/get handler
and `with_permission` resolution **must** resolve and check `org_id` by
joining up to the org-scoped parent — never trust a child id in
isolation. A valid id belonging to *another* org is otherwise an IDOR
surface.

---

## 9. Suggested phasing

| Phase | Scope (data + API) | UI screens shipped (§7.4) |
|---|---|---|
| **P1 — Master data & product** | products, customers, manufacturers, suppliers (scaffold), project linking, document upload, manuals+revisions | Products hub (1) + detail Overview/Projects/Documents/Manuals tabs (2), Parties admin (10), manual editor (9), project-page Products panel (§7.4.5). |
| **P2 — Manufacturing & serialisation** | runs, units, serial allocation, QR payload, EOL test reports, run counters | Product *Runs*/*Units* tabs, run detail + add-units (3), unit detail + on-screen QR (4), unit landing (5), EOL record dialog (6). |
| **P3 — Returns** | RMA / warranty workflow keyed on unit+customer | Product *Returns* tab, RMA list + detail/status workflow (7,8), create-RMA serial→unit match, customer detail (11). |
| **P4 — Polish / future** | QR SVG endpoint + label PDF, supplier↔part BOM, analytics (yield %, RMA rate) | Printable unit-label / QR sheet, yield % + RMA-rate dashboard widgets. |

Each phase is a vertical slice (migration → domain → store → REST → **UI**)
shippable on its own — no phase lands backend without its screens.

---

## 10. Reused precedents (so we don't reinvent)

- **Document upload / blob storage:** `dp_project_exec_summary_documents`
  + `project_exec_summary.rs` (`read_upload`, `BlobStore`, proxy GET).
  Copy wholesale for `dp_product_documents`.
- **Markdown render:** `frontend/src/components/markdown.tsx`.
- **Revision/changelog table:** `dp_project_exec_summary_changelog`
  (append-only, free-form version string) — the manual-revision and
  RMA models follow it.
- **CAS + upsert split:** `Project` / `ProjectUpsert` in `project.rs`.
- **Polymorphic-link awareness:** `dp_tag_links` (`0049`) — considered
  and deliberately *not* reused for product↔project (§5.2).
- **Enum-as-TEXT+CHECK:** `ProjectStatus` + `encode.rs` helpers.

---

## 11. Open questions

1. **Serial scope** — *Recommended:* a per-**run** sequence with
   `run_code` in the template, giving readable per-batch serials
   (`NB-R2026-014-00042`). Per-product monotonic is a one-line schema
   swap (`next_serial_seq` on `dp_products` instead of the run). Confirm
   the intended UX.
2. **QR payload** — *Resolved (§6):* store the host-less unit id and
   compose `{base_url}/u/{id}` at render/SVG time. Revisit only if a
   GS1 / DataMatrix payload is required.
3. **Permissions granularity** — *Resolved (§8):* one umbrella
   `manufacturing` resource for v1; split later (additive).
4. **EOL granularity** — report per **unit** (assumed) or also a
   run-level summary report? Per-unit rolls up to a run yield either way.
5. **Customer scope** — are customers org-internal records only, or do
   they ever need a portal/login? (Out of scope now; affects the table
   if yes.)
6. **Suppliers** — confirm we only want the table + CRUD now, with
   BOM/part-sourcing explicitly deferred to P4.
7. **Manual numbering** — free-form revision strings (`A`,`B`,`1.0`) as
   modelled, or an enforced auto-increment scheme?
8. **Migration ownership/numbering** — confirm the next free slots and
   which team's range these fall in.
