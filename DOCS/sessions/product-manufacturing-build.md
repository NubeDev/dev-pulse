# Session: Product & Manufacturing Build

- **Date:** 2026-06-03
- **Branch:** `feat/product-manufacturing` (off `main`)
- **Status:** 🟢 P1 + P2 + P3 COMPLETE (backend + frontend, green). P4 excluded by scope.
- **Spec:** `DOCS/ideas/product-manufacturing.md` (authoritative)

## Objective & scope

Implement the Product & Manufacturing feature end-to-end (phases P1–P3 of spec §9),
as vertical slices (migration → domain → store → REST → UI) per phase.

- **P1 — Master data & product:** products, customers, manufacturers, suppliers
  (scaffold-only CRUD), product↔project linking, document upload, manuals + revisions.
- **P2 — Manufacturing & serialisation:** runs, serialised units, serial allocation,
  QR (on-screen + `qr.svg`), EOL test reports + run counters.
- **P3 — Returns:** RMA / warranty returns workflow keyed on unit + customer.
- **P4 — EXCLUDED:** no supplier↔part BOM (suppliers stay table + CRUD only),
  no analytics dashboards.

## LOCKED DECISIONS (final)

1. **Serial numbers — per RUN.** `next_serial_seq` on `dp_manufacturing_runs`; template
   embeds `run_code` (`NB-R2026-014-00042`). Allocate via the atomic single-statement
   reservation (spec §6 `UPDATE ... SET next_serial_seq = next_serial_seq + $N ... RETURNING`),
   NEVER via the run's `version` CAS.
2. **QR `/u/{id}` — TOKEN-GATED PUBLIC.** QR encodes `{base_url}/u/{unit_id}?t=<token>`,
   `<token> = HMAC-SHA256(server_secret, unit_id)` (no expiry). Server secret via existing
   config/env, else add `MANUFACTURING_QR_SECRET`. `/u/{id}` route PUBLIC (no login) but
   requires valid token; returns lean read-only view ONLY (serial, model number, status,
   published-manual links). Missing/invalid token → 404. Full internal view stays at
   authenticated `/units/{id}`.
3. **EOL — per-unit reports PLUS run-level summary.** Keep `dp_eol_test_reports` (one
   pass/fail report per unit; re-tests allowed; current = latest). ADD `dp_run_eol_summary`
   (one row per run: built/pass/fail snapshot, `signed_by`/`signed_at` sign-off, markdown
   notes). Per-unit reports are source of truth; summary is a sign-off snapshot. Run counters
   per spec §5.4 (current-state, re-test-safe — adjust on a unit's latest-outcome transition).
4. **Authz — one `manufacturing` resource** (read/write) gating ALL new routes via
   `with_permission(..., "manufacturing", ...)`, EXCEPT the token-gated public `/u/{id}`.
5. **Other defaults:** customers internal-only (no portal); manual revisions free-form
   strings; migrations next free numbers from `0050` up (one logical group per file);
   history-bearing children (`runs`, `units`, `rma_returns`) `ON DELETE RESTRICT`.
6. **Conventions:** spec §4/§7/§10 + existing `dp_projects` / `dp_project_exec_summary*`.
   No Postgres ENUM (TEXT + CHECK + `encode.rs`); `version bigint` CAS on mutable top-level
   rows; blob upload reuses exec-summary pattern; frontend reuses shadcn/ui + react-query
   keys factory + `markdown.tsx` + `documents-section.tsx`.

## Task breakdown

### P1 — Master data & product  ✅ COMPLETE
- [x] Migration `0050` master data (manufacturers/suppliers/customers)
- [x] Migration `0051` products + project links + documents
- [x] Migration `0052` manuals + revisions
- [x] Domain: `product.rs`, `party.rs`, `product_manual.rs`, `product_doc.rs` + Store trait
- [x] Store: `parties.rs`, `products.rs`, `product_manuals.rs` + rows + encode + mod
- [x] REST: `parties.rs`, `products.rs`, `product_manuals.rs` + lib + openapi + authz
- [x] UI: products hub, product detail (Overview/Projects/Docs/Manuals), parties admin, manual editor, project-page panel
- [x] Tests: enum round-trips (`encode.rs`, run under `cargo test`). NOTE: store-method integration
      tests deferred — see "Deferred" (need Docker; harness has no DB). Enum round-trips DONE.
- [x] Adversarial review run; 3 parent-child validation gaps found & fixed (see progress log).

### P2 — Manufacturing & serialisation  ✅ COMPLETE
- [x] Migration `0053` runs + units + EOL reports + run EOL summary
- [x] Domain: `manufacturing.rs`, `eol.rs` + Store trait
- [x] Store: `manufacturing.rs` (alloc + counters) + rows + encode + mod
- [x] REST: `manufacturing.rs` + qr.svg + qr token + lib + openapi + authz
- [x] UI: Runs/Units tabs, run detail + add-units, unit detail + QR, unit landing (backend HTML), EOL dialog
- [x] Tests: serial allocation, counter transitions (2 store integration tests pass); enum round-trips
- [x] Adversarial backend review run; no confirmed bugs (all 6 locked decisions verified correct)

### P3 — Returns  ✅ COMPLETE
- [x] Migration `0054` rma_returns
- [x] Domain: `rma.rs` + Store trait
- [x] Store: `rma.rs` + rows + encode + mod
- [x] REST: `rma.rs` + lib + openapi + authz + audit (RMA_CREATE/RMA_UPDATE)
- [x] UI: Returns tab, RMA list + detail/status-workflow, create-RMA serial→unit, customer rollups
- [x] Tests: `rma_crud_and_filters` integration test passes; `rma_status` enum round-trip

## Progress log

- **2026-06-03** — Created branch `feat/product-manufacturing`, read spec in full,
  created this session doc. Next free migrations confirmed: 0050–0054 (last existing 0049).
  Launching parallel Explore research agents.
- **2026-06-03 — Research batch.** Ran 4 parallel `Explore` sub-agents: (a) domain+store+migrations
  patterns, (b) REST routing+openapi+authz+blob+config, (c) frontend projects/exec-summary patterns,
  (d) dp-server router assembly + permission registration + secret resolution. Verified key
  integration points myself directly: policy TOML (wildcard `org-gate-allow` rule means a registered
  resource is auto-granted to in-org users — no policy edit needed), Caddyfile catch-all routing
  (→ public unit landing is backend-served), integration test layout.
- **2026-06-03 — P1 backend COMPLETE & compiling.** Built migrations 0050–0052; domain modules
  `product.rs`, `party.rs`, `product_manual.rs`, `product_doc.rs` + Store trait methods + lib exports;
  encode helpers (`product_status`, `revision_status`) + round-trip unit tests; row mappers in
  `rows.rs`; store impls `parties.rs`, `products.rs`, `product_manuals.rs` + mod delegations; REST
  modules `parties.rs`, `products.rs`, `product_manuals.rs` (DTOs, handlers, routers,
  `(manufacturing, read|write)` authz) + openapi registration + audit verbs; AppState fields
  (`public_base_url`, `manufacturing_qr_secret`) + builders; dp-server router wiring + `manufacturing`
  permission resource; main.rs `[manufacturing] qr_secret_ref` config + secret resolution.
  `cargo build` green; `cargo test --workspace` green EXCEPT one PRE-EXISTING failure
  (`dp-reports::no_means_anywhere`, introduced in main commit 56cbd40, unrelated — I never touched
  dp-reports). Next: P1 frontend.
- **2026-06-03 — P1 frontend COMPLETE (1 sub-agent).** Ran a `general-purpose` sub-agent to build the
  `products/` feature area against the exact API client + Zod schemas I'd written. It created
  `use-products-data.ts` (keys + hooks), `product-list.tsx` (hub), `product-detail-page.tsx`
  (Overview/Projects/Manuals/Documents tabs), `new-product-dialog.tsx`,
  `product-projects-section.tsx`, `product-manuals-section.tsx` (manual editor w/ split-pane preview +
  publish), `product-documents-section.tsx`, `project-products-panel.tsx`, `parties/parties-admin.tsx`
  (shared customers/manufacturers/suppliers, suppliers flagged scaffold), `parties/customer-detail.tsx`;
  wired `routes.ts` (products/manufacturing/customers sections + parse helpers), `app.tsx`
  (SectionPane + SECTION_MIN_ROLE reader), `app-shell.tsx` (Products sidebar entry), and embedded a
  Products panel in `project-detail-page.tsx`. Frontend `typecheck` + `build` GREEN (verified
  independently). Note: product types imported directly from `api/schemas/products.ts` (the schemas
  barrel omits products) — documented by the agent.
- **2026-06-03 — P1 adversarial review + fixes.** Ran an `Explore` reviewer over the P1 backend diff.
  Verdict: CAS, publish-tx, CHECK consistency, serial-format validator, link idempotency, row-mapper↔SQL
  agreement, and authz wrapping all CORRECT. It found 3 parent-child validation gaps (spec §8 "never
  trust a child id in isolation"): `DELETE /products/{id}/documents/{doc_id}`,
  `POST …/revisions`, and `…/revisions/{rev}/publish` ignored the path `product_id`/`manual_id` link.
  FIXED all 3 (verify the child belongs to the parent before acting). dp-rest rebuilds green.
- **2026-06-03 — P1 store integration tests ADDED & PASSING.** Added 3 `#[ignore]` integration tests
  to `crates/dp-store-pg/tests/integration.rs` (`parties_crud_and_archive`,
  `products_crud_links_and_documents`, `manuals_publish_supersedes_prior`) matching existing style.
  Docker IS available in this harness — ran them: **all 3 pass** against real Postgres (validates
  migrations 0050–0052 + store SQL end-to-end). **P1 COMPLETE.**

- **2026-06-03 — P2 frontend COMPLETE + reconciled.** Wired the P2 manufacturing UI end-to-end and
  brought it to green (`typecheck` + `build` both pass). Orchestrator-owned integration: `app.tsx`
  (SectionPane `runs`/`units` cases + `SECTION_MIN_ROLE` reader entries + section-union), `app-shell.tsx`
  (`SECTION_TITLE` runs/units), `routes.ts` (`ProductDetailTab` gains `runs`/`units`; runs/units section
  helpers already present), `product-detail-page.tsx` (Runs + Units tabs added).
  **Reconciliation note:** a prior turn had already built the bulk of the P2 frontend into subdirectories
  (`products/runs/{run-detail,product-runs-section,run-shared}`, `products/units/product-units-section`,
  `products/eol/eol-dialog`) backed by a dedicated `use-manufacturing-data.ts` hooks file — the handover
  doc didn't record this. I had initially added duplicate P2 hooks to `use-products-data.ts` per the
  (stale) handover; on discovering the pre-existing `use-manufacturing-data.ts` I **reverted those
  duplicate hooks** so there is a single source of truth (`use-manufacturing-data.ts`, keys under
  `["manufacturing", …]`). A build sub-agent created the missing flat `unit-detail-page.tsx` (full impl,
  client QR via `qrcode.react`, EOL timeline, status/ship control) and initially added thin shim files to
  bridge naming; I then **collapsed the redundant shims** — `app.tsx` imports `RunDetail` directly,
  `product-detail-page.tsx` imports the runs/units sections directly (passing the full `ProductDto`), and
  `unit-detail-page.tsx` imports `EolDialog` directly. Deleted the 4 shim files.
  **Units tab decision (logged):** there is no product-level units endpoint (units are run-scoped), so the
  Units tab lists the product's runs and unions `listRunUnits` across them client-side, with a run filter
  + serial search. N+1 is acceptable (small per-run build quantities, cached per run).
- **2026-06-03 — P2 adversarial backend review.** Ran an `Explore` reviewer over the P2 backend diff
  against the 6 locked decisions. **No confirmed bugs.** Verified correct: serial allocation is a single
  atomic `UPDATE … next_serial_seq` (no run-version CAS), reserved range `[first_seq, first_seq+count)`;
  `/u/{id}` mounted outside `with_principal`, HMAC token compared constant-time, missing/invalid/wrong-unit
  token → uniform 404 (both JSON + HTML branches); EOL counters re-test-safe (adjust only on a unit's
  latest-outcome transition); authz read/write split correct; parent-child handlers lock the path parent
  `FOR UPDATE`. Two non-issues flagged & dismissed: `render_serial`'s `seq:NN` `unwrap_or(0)` is
  unreachable because `validate_serial_format` (products.rs:297) already requires all-ASCII-digit widths;
  i32 `next_serial_seq` overflow is theoretical only (≤1000/alloc, ~2.1B ceiling/run). No code changes made.

- **2026-06-03 — P3 (Returns / RMA) COMPLETE, backend + frontend green.**
  **Backend** (vertical slice mirroring P2): migration `0054_rma_returns.sql` (verbatim spec §5.5 —
  `dp_rma_returns`, `product_id` ON DELETE RESTRICT, `unit_id`/`customer_id` ON DELETE SET NULL, status
  TEXT+CHECK, `version` CAS, unique `(org_id, lower(rma_number))`); domain `rma.rs` (`RmaStatus`, `Rma`,
  `RmaCreate`, `RmaUpdate`, `RmaFilter`) + Store-trait block (default impls) + lib exports; store `rma.rs`
  (`list_rma` dynamic filters newest-first, `get_rma`, `create_rma` with §8 parent-child validation —
  product must exist, unit must belong to product, unique→Conflict via `map_sqlx`; `update_rma` CAS with
  miss→Conflict/NotFound disambiguation + unit re-validation) + `row_to_rma` + `rma_status` encode
  helpers/round-trip test + `mod.rs` delegations; REST `rma.rs` (DTO+From, `list/get/create/patch`
  handlers, `(manufacturing, read|write)` router identical to `manufacturing_router`, audit
  `RMA_CREATE`/`RMA_UPDATE`) + `lib.rs`/`openapi.rs` registration; `dp-server` mounts `rma_router` inside
  `with_principal`. `cargo build --workspace` GREEN; `rma_crud_and_filters` `#[ignore]` integration test
  **passes** against real Postgres (create, get, status-filtered list, CAS update + version bump,
  stale-CAS→Conflict, duplicate rma_number→Conflict, cross-product unit→Invalid). Reviewed the REST +
  store directly — parent-child validation, CAS, authz split, audit, unique mapping all correct.
  **Frontend:** appended RMA Zod schemas + `dev-pulse-api.ts` client (`listRma/getRma/createRma/patchRma`)
  + RMA hooks in `use-manufacturing-data.ts` (keys under `["manufacturing","rma",…]`); new
  `products/rma/{rma-shared.ts, rma-list, rma-detail, new-rma-dialog, product-returns-section}.tsx`;
  shared wiring (orchestrator-owned): `routes.ts` (`rma` section + `rmaListRoute`/`rmaDetailIdOf`/
  `rmaStatusOf` + `returns` product tab), `app.tsx` (rma SectionPane + min-role reader), `app-shell.tsx`
  (Returns sidebar entry + SECTION_TITLE), `product-detail-page.tsx` (Returns tab), `customer-detail.tsx`
  (open-RMA rollup table, screen 11). RMA detail = status-workflow stepper + Select (auto-stamps
  `received_at`/`resolved_at` on first transition), warranty toggle, markdown diagnosis/resolution, CAS
  full-upsert PATCH + 409 banner, lazy product/unit/customer resolution. Create-RMA resolves a serial→unit
  via a unit picker fanned out over the product's runs (no serial-lookup endpoint added). `typecheck` +
  `build` GREEN. **All of P1–P3 now complete; P4 out of scope.**

## Decisions & assumptions

- **Public `/u/{id}` landing is BACKEND-served HTML** (not a SPA/React page). Rationale: the
  production Caddyfile catch-all routes any non-SPA path (the SPA is hash-routed) to the backend,
  so `/u/{id}` naturally lands on the Rust server. Serving a small self-contained, token-gated
  HTML page server-side (no login, no SPA bootstrap) is the most robust way to satisfy "lean
  read-only view" + "works on a phone scanning a QR" without special-casing the hash router.
  The route also returns lean JSON when `Accept: application/json` (for tests). This route is
  mounted OUTSIDE `with_principal` (like the webhook router), via `ServerBuilder::merge_router`.
  This fulfils §7.4 screen 5.
- **QR base URL reuses `server.base_url`** (already in config, used for OAuth callbacks) rather
  than adding a new config value. QR payload = `{server.base_url}/u/{unit_id}?t=<token>`.
- **QR secret:** added `MANUFACTURING_QR_SECRET` via the existing `secret://` mechanism
  (config `[manufacturing] qr_secret_ref`), threaded into `AppState`. If absent, the unit landing
  + qr endpoints behave as if no valid token can be produced (token routes 404 / qr disabled).
- **Permission resource `manufacturing`** registered with actions `["read","write"]` in
  `register_dev_pulse_resources`; the wildcard `org-gate-allow-in-org-everything` policy rule
  already grants in-org users any registered resource, so no policy-file edit is needed.
- Store integration tests use `#[ignore]` + `fixture()` per existing style; enum round-trip
  tests in `encode.rs` are plain unit tests that run under `cargo test`.

## Files created/changed

**P1 — backend (created):** `crates/dp-store-pg/migrations/dp/{0050_manufacturing_master_data,
0051_products,0052_product_manuals}.sql`; `crates/dp-domain/src/{product,party,product_manual,
product_doc}.rs`; `crates/dp-store-pg/src/store/{parties,products,product_manuals}.rs`;
`crates/dp-rest/src/{parties,products,product_manuals}.rs`.
**P1 — backend (modified):** `crates/dp-domain/src/{lib,store}.rs`;
`crates/dp-store-pg/src/encode.rs`; `crates/dp-store-pg/src/store/{mod,rows}.rs`;
`crates/dp-rest/src/{lib,openapi,state,audit,project_exec_summary}.rs`;
`crates/dp-server/src/lib.rs`; `crates/dp-server/src/auth/policy.rs`;
`crates/dp-server/tests/phase4_smoke.rs`; `crates/dev-pulse/src/main.rs`;
`crates/dp-store-pg/tests/integration.rs` (+3 tests).
**P1 — frontend (created):** `frontend/src/api/schemas/products.ts`;
`frontend/src/products/{use-products-data,product-list,new-product-dialog,product-detail-page,
product-projects-section,product-manuals-section,product-documents-section,project-products-panel}.tsx`;
`frontend/src/products/parties/{parties-admin,customer-detail}.tsx`.
**P1 — frontend (modified):** `frontend/src/api/dev-pulse-api.ts`; `frontend/src/{routes.ts,app.tsx}`;
`frontend/src/layout/app-shell.tsx`; `frontend/src/projects/project-detail-page.tsx`.

**P2 — backend (created):** `crates/dp-store-pg/migrations/dp/0053_manufacturing_runs.sql`;
`crates/dp-domain/src/{manufacturing,eol}.rs`; `crates/dp-store-pg/src/store/manufacturing.rs`;
`crates/dp-rest/src/manufacturing.rs`.
**P2 — backend (modified):** `crates/dp-domain/src/{lib,store}.rs`; `crates/dp-store-pg/src/encode.rs`;
`crates/dp-store-pg/src/store/{mod,rows}.rs`; `crates/dp-rest/src/{lib,openapi,state}.rs`;
`crates/dp-server/src/lib.rs` (authenticated router in `with_principal`; public `/u/{id}` router merged
outside); `crates/dev-pulse/src/main.rs` (`MANUFACTURING_QR_SECRET`); `Cargo.lock`/`dp-rest/Cargo.toml`
(`qrcode` dep); `crates/dp-store-pg/tests/integration.rs` (+2 P2 tests).
**P2 — frontend (created):** `frontend/src/products/use-manufacturing-data.ts`;
`frontend/src/products/runs/{run-detail,product-runs-section}.tsx` + `runs/run-shared.ts`;
`frontend/src/products/units/product-units-section.tsx`; `frontend/src/products/eol/eol-dialog.tsx`;
`frontend/src/products/unit-detail-page.tsx`.
**P2 — frontend (modified):** `frontend/src/api/{dev-pulse-api.ts,schemas/products.ts}` (P2 client +
schemas); `frontend/src/{app.tsx,routes.ts}`; `frontend/src/layout/app-shell.tsx`;
`frontend/src/products/product-detail-page.tsx` (Runs + Units tabs); `frontend/package.json` /
`pnpm-lock.yaml` (`qrcode.react`).

**P3 — backend (created):** `crates/dp-store-pg/migrations/dp/0054_rma_returns.sql`;
`crates/dp-domain/src/rma.rs`; `crates/dp-store-pg/src/store/rma.rs`; `crates/dp-rest/src/rma.rs`.
**P3 — backend (modified):** `crates/dp-domain/src/{lib,store}.rs`; `crates/dp-store-pg/src/encode.rs`;
`crates/dp-store-pg/src/store/{mod,rows}.rs`; `crates/dp-rest/src/{lib,openapi}.rs`;
`crates/dp-server/src/lib.rs`; `crates/dp-store-pg/tests/integration.rs` (+`rma_crud_and_filters`).
**P3 — frontend (created):** `frontend/src/products/rma/{rma-shared.ts, rma-list, rma-detail,
new-rma-dialog, product-returns-section}.tsx`.
**P3 — frontend (modified):** `frontend/src/api/{dev-pulse-api.ts, schemas/products.ts}`;
`frontend/src/products/use-manufacturing-data.ts`; `frontend/src/products/parties/customer-detail.tsx`;
`frontend/src/{app.tsx,routes.ts}`; `frontend/src/layout/app-shell.tsx`;
`frontend/src/products/product-detail-page.tsx` (Returns tab).

## Migrations added

- **0050** `manufacturing_master_data` — `dp_manufacturers`, `dp_suppliers`, `dp_customers`.
- **0051** `products` — `dp_products`, `dp_product_project_links`, `dp_product_documents`.
- **0052** `product_manuals` — `dp_product_manuals`, `dp_product_manual_revisions`.
- **0053** `manufacturing_runs` — `dp_manufacturing_runs`, `dp_product_units`,
  `dp_eol_test_reports`, `dp_run_eol_summary`.
- **0054** `rma_returns` — `dp_rma_returns`.

## How to verify

Discovered from `Makefile`, `Cargo.toml`, `frontend/package.json`:

- **Backend build:** `cargo build` (workspace) — must compile.
- **Backend unit tests (Docker-free):** `cargo test --workspace` — runs `encode.rs` enum
  round-trip tests etc. Integration tests in `crates/dp-store-pg/tests/integration.rs` are
  marked `#[ignore = "requires Docker or DP_TEST_DATABASE_URL"]`.
- **Backend integration tests (needs Docker/PG):** `cargo test -p dp-store-pg -- --ignored`
  (or set `DP_TEST_DATABASE_URL` to an empty PG and run the same).
- **Frontend typecheck:** `pnpm --filter dev-pulse-frontend typecheck` (= `tsc -p tsconfig.json --noEmit`).
- **Frontend build:** `pnpm --filter dev-pulse-frontend build`.

Results are recorded in the progress log per phase.

## Deferred / follow-ups

- P4: supplier↔part BOM, analytics dashboards (yield %, RMA rate), printable label PDF sheets.
