# TODO — Project Executive Summary implementation

Tracking what's landed and what's left for the scope in
[SCOPE-PROJECT-EXECUTIVE-SUMMARY.md](SCOPE-PROJECT-EXECUTIVE-SUMMARY.md).

## Done

- [x] **Migration 0045** —
  [crates/dp-store-pg/migrations/dp/0045_project_exec_summary.sql](../crates/dp-store-pg/migrations/dp/0045_project_exec_summary.sql).
  Four tables: `dp_project_exec_summary` (wide 1-to-1 with
  `dp_projects`), `dp_project_exec_summary_images`,
  `dp_project_exec_summary_documents`,
  `dp_project_exec_summary_changelog`. All `ON DELETE CASCADE` from
  `dp_projects`. Indexes and CHECK constraints in place.

- [x] **dp-domain types** —
  [crates/dp-domain/src/project_exec_summary.rs](../crates/dp-domain/src/project_exec_summary.rs).
  Exposes `ProjectExecSummary`, `ExecSummaryStatus`,
  `ExecSummaryImage`, `ExecSummaryDocument`,
  `ExecSummaryChangelogEntry`, `ExecSummaryChangelogInsert`,
  `ExecSummaryCompletion`, `ProjectExecSummaryPatch`,
  `BlobRefJson` (opaque type alias for `serde_json::Value` so
  dp-domain stays free of `starter_*` imports per §0.6), and the
  `EXEC_SUMMARY_SUBMIT_THRESHOLD_PERCENT = 80` constant.

  `target_gp_pct` is `f64` rather than `rust_decimal::Decimal` —
  decimal precision is enforced by the `NUMERIC(5,2)` column. If
  rust_decimal becomes a workspace dep for other reasons, swap.

- [x] **Store trait methods** added in
  [crates/dp-domain/src/store.rs](../crates/dp-domain/src/store.rs)
  (project-exec-summary section, after milestones). 16 methods cover
  the row + 3 child tables + state transitions. All have safe
  defaults so other store impls / fakes stay compiling.

- [x] **Postgres impl** —
  [crates/dp-store-pg/src/store/project_exec_summary.rs](../crates/dp-store-pg/src/store/project_exec_summary.rs).
  Sparse PATCH uses `CASE WHEN $set THEN $val ELSE col END` per
  nullable column (verbose but keeps the SQL static and the round-trip
  to one statement). Completion booleans are projected alongside the
  row in `get_project_exec_summary_impl` so GET stays one round-trip.
  Status transitions disambiguate "no row" vs "wrong status" with a
  follow-up SELECT, returning `StoreError::Conflict` for the latter.
  Schema decision: `target_gp_bp` (basis points, `BIGINT`) replaced
  the original `NUMERIC(5,2)` plan — keeps the no-floats-for-money
  rule and avoids pulling `rust_decimal` into the workspace.

  `cargo check --workspace` passes clean.

- [x] **REST router** —
  [crates/dp-rest/src/project_exec_summary.rs](../crates/dp-rest/src/project_exec_summary.rs).
  14 routes: GET envelope, PATCH (sparse merge), submit / approve /
  revert state transitions, image/document list+patch+delete,
  changelog list+append+delete. Wire DTOs (`ExecSummaryDto`,
  `ExecSummaryEnvelopeDto`, `ExecSummaryCompletionDto`,
  `SubmitIncompleteBody`, …) carry the `target_gp_pct ↔
  target_gp_bp / 100` conversion at the seam. Lead-only check
  (E2) is enforced per-handler via `require_project_lead`.
  Mounted in [dp-server/src/lib.rs](../crates/dp-server/src/lib.rs)
  on the protected router.

- [x] **Audit verbs** added in
  [crates/dp-rest/src/audit.rs](../crates/dp-rest/src/audit.rs):
  `PROJECT_EXEC_SUMMARY_PATCH/SUBMIT/APPROVE/REVERT` plus
  `_IMAGE_ADD/REMOVE`, `_DOCUMENT_ADD/REMOVE`, `_CHANGELOG_ADD/REMOVE`.

- [x] **DTO reshape to match frontend.** The first REST cut shipped
  flat DTOs; the frontend in
  [frontend/src/api/schemas/exec-summary.ts](../frontend/src/api/schemas/exec-summary.ts)
  is **section-grouped** (`{ summary: {...}, scope: {...}, … }`)
  and was already built around that shape. The DTO block in
  [crates/dp-rest/src/project_exec_summary.rs](../crates/dp-rest/src/project_exec_summary.rs)
  is now section-grouped end-to-end:
  - `ExecSummaryDto` is the full envelope (frontend's
    `ExecSummaryDto` = backend's `ExecSummaryDto`).
  - Per-section dtos (`ExecSummarySummaryDto`,
    `ExecSummaryScopeDto`, `ExecSummaryRequirementsDto`,
    `ExecSummaryHardwareDto`, `ExecSummaryCommercialDto`,
    `ExecSummaryApprovalDto`) and matching `*Patch` payloads.
  - `ExecSummaryCompletionDto` is `{ percent, sections: { summary:
    bool, … } }` (BTreeMap on the wire).
  - Image / document DTOs carry `url` (proxy URL — placeholder
    `/blobs/exec-summary/{kind}/{id}` until the proxy is wired).
  - Mutation handlers (PATCH / submit / approve / revert) all
    return the full envelope so the frontend's react-query cache
    can `setQueryData` without a follow-up GET.
  - `cargo check --workspace` and `tsc --noEmit` both pass.

- [x] **Blob storage wiring** — end-to-end.
  - Workspace adds `starter-blob-memory` and pulls `axum/multipart`.
  - [crates/dp-rest/src/state.rs](../crates/dp-rest/src/state.rs)
    gained `blob_store: Option<Arc<dyn BlobStore>>` plus
    `with_blob_store(...)` builder.
  - **Upload handlers** (`POST /projects/{id}/exec-summary/images`
    and `…/documents`) parse multipart, push bytes to the engine
    via `put_bytes(...)`, persist the opaque `BlobRef` JSON on the
    row alongside `filename` / `content_type`. 25 MiB per-file cap;
    8 KiB text-field cap.
  - **Proxy GET** (`/blobs/exec-summary/{kind}/{row_id}`)
    resolves the row → `BlobRef` → engine `get()` and streams the
    bytes back with `Content-Type` and `Content-Disposition:
    inline; filename="…"`. Mounted under `(projects, read)`.
  - Store gained `get_exec_summary_image / _document` single-row
    lookups so the proxy is O(1).
  - dp-server constructs a `MemoryBlobStore` by default and threads
    it through `RestAppState::with_blob_store`; bin layer swaps to
    `FsBlobStore` / `GarageBlobStore` when shipping.
  - `cargo check --workspace` passes clean.

- [x] **OpenAPI registration + snapshot.** Every new handler and
  DTO is registered in
  [crates/dp-rest/src/openapi.rs](../crates/dp-rest/src/openapi.rs)
  and the snapshot test passes
  (`UPDATE_OPENAPI_SNAPSHOT=1 cargo test -p dp-rest --test
  openapi_snapshot`).

- [x] **Frontend builds and typechecks** against the new
  contract (`tsc --noEmit` clean; `pnpm build` succeeds).

## Verification done

- `cargo check --workspace` clean
- `cargo test -p dp-domain -p dp-store-pg` passes
- `cargo test -p dp-rest --test openapi_snapshot` passes
- `tsc --noEmit` on the frontend clean
- `pnpm build` succeeds

## Follow-ups

### Per-row authz on the blob proxy

Original Store CRUD signature notes (kept for reference; now landed):

- `get_project_exec_summary(&self, project_id: Uuid) -> Result<Option<(ProjectExecSummary, ExecSummaryCompletion)>, StoreError>` —
  one row + computed completion. `None` if the project has no
  summary row yet.
- `upsert_project_exec_summary(&self, project_id: Uuid) -> Result<ProjectExecSummary, StoreError>` —
  lazy-create on first PATCH. Idempotent.
- `patch_project_exec_summary(&self, project_id: Uuid, patch: ProjectExecSummaryPatch) -> Result<ProjectExecSummary, StoreError>` —
  apply sparse patch; bump `updated_at`. Generate SQL dynamically from
  the present fields (the existing `encode.rs` patterns are a guide).
- `submit_project_exec_summary(&self, project_id: Uuid) -> Result<ProjectExecSummary, StoreError>` —
  status `draft → in_review`, sets `submitted_at = now()`. Reject if
  current status is not `draft`. Caller checks the completion threshold.
- `approve_project_exec_summary(&self, project_id: Uuid, approval_notes: Option<&str>) -> Result<ProjectExecSummary, StoreError>` —
  status `in_review → approved`, sets `approved_at = now()`. Reject if
  current status is not `in_review`. The lead-only authz check happens
  in the REST layer, not here.
- `revert_project_exec_summary(&self, project_id: Uuid) -> Result<ProjectExecSummary, StoreError>` —
  status `* → draft`. Always allowed. Preserve `submitted_at` /
  `approved_at` (history).

Child tables:

- `list_exec_summary_images(project_id)` — ORDER BY `ord`, `created_at`.
- `insert_exec_summary_image(project_id, blob_ref, filename, content_type, caption?, ord?)`.
- `update_exec_summary_image(id, caption?, ord?)`.
- `delete_exec_summary_image(id)`.
- `list_exec_summary_documents(project_id)`.
- `insert_exec_summary_document(...)` / `update_..._document(id, title?, doc_type?, notes?, required_action?)` / `delete_..._document(id)`.
- `list_exec_summary_changelog(project_id)` — DESC by `changed_at`.
- `insert_exec_summary_changelog(ExecSummaryChangelogInsert)`.
- `delete_exec_summary_changelog(id)` (admin-only path; UI is
  append-only by default per E5).

**Completion calculation** — implement as a SQL view or as a
post-query computation in `get_project_exec_summary`. The rules
(scope doc §3.5):

| Section      | Rule                                                                 |
| ------------ | -------------------------------------------------------------------- |
| Summary      | `product_name`, `objective`, `success_criteria` all non-empty        |
| Scope        | `in_scope` AND `out_of_scope` non-empty                              |
| Requirements | `must_have` non-empty AND `array_length(protocols, 1) >= 1`          |
| Hardware     | `hardware_features` non-empty OR EXISTS image                        |
| Commercial   | `rrp_cents IS NOT NULL` AND `target_gp_pct IS NOT NULL`              |
| Documents    | EXISTS document                                                      |
| Approval     | `status = 'approved'`                                                |
| Change Log   | EXISTS changelog row                                                 |

### REST router ([crates/dp-rest/src/project_exec_summary.rs](../crates/dp-rest/src/project_exec_summary.rs))

New module, mounted from `lib.rs`. Routes per scope doc §3.2:

```
GET    /projects/{id}/exec-summary
PATCH  /projects/{id}/exec-summary
POST   /projects/{id}/exec-summary/submit
POST   /projects/{id}/exec-summary/approve     body: { approval_notes? }
POST   /projects/{id}/exec-summary/revert

GET    /projects/{id}/exec-summary/images
POST   /projects/{id}/exec-summary/images               → { upload_url, headers, image_id }
POST   /projects/{id}/exec-summary/images/{id}/confirm  body: { blob_ref, etag, size }
PATCH  /projects/{id}/exec-summary/images/{id}          (caption / ord)
DELETE /projects/{id}/exec-summary/images/{id}

GET    /projects/{id}/exec-summary/documents
POST   /projects/{id}/exec-summary/documents            → presign
POST   /projects/{id}/exec-summary/documents/{id}/confirm
PATCH  /projects/{id}/exec-summary/documents/{id}
DELETE /projects/{id}/exec-summary/documents/{id}

GET    /projects/{id}/exec-summary/changelog
POST   /projects/{id}/exec-summary/changelog
DELETE /projects/{id}/exec-summary/changelog/{id}
```

Cross-cutting:

- **Authz**: `(projects, read)` for GETs, `(projects, write)` for
  PATCH / submit / image+document CRUD / changelog. `approve` and
  `revert` additionally require viewer == `dp_projects.lead_user_id`
  (E2 hard rule); 403 otherwise.
- **Submit threshold**: handler reads the freshly-computed
  `ExecSummaryCompletion`; if `percent < EXEC_SUMMARY_SUBMIT_THRESHOLD_PERCENT`
  return 400 with `{ "error": "incomplete", "sections": [list-of-missing] }`.
- **Audit**: pin verbs in `crate::audit` —
  `PROJECT_EXEC_SUMMARY_PATCH`, `_SUBMIT`, `_APPROVE`, `_REVERT`,
  `_IMAGE_ADD`, `_IMAGE_REMOVE`, `_DOCUMENT_ADD`, `_DOCUMENT_REMOVE`,
  `_CHANGELOG_ADD`, `_CHANGELOG_REMOVE`. `target_kind =
  "project_exec_summary"`, `target_id = project_id`. Follow the
  `audit::record` pattern from
  [project_milestones.rs](../crates/dp-rest/src/project_milestones.rs).
- **DTOs**: every wire field needs `ToSchema` + utoipa doc-paths so
  the OpenAPI export stays complete. Pattern follows
  [`MilestoneDto`](../crates/dp-rest/src/project_milestones.rs#L47).
- **Error mapping**: `StoreError::NotFound → 404`,
  `StoreError::InvalidState → 409` for status-transition rejects.

### dp-server wiring ([crates/dp-server/src/lib.rs](../crates/dp-server/src/lib.rs))

Two pieces:

1. **Mount the new router** alongside the existing project routers.
2. **Construct the `BlobStore` and `BlobProxyRouter`**:
   - Add deps: `starter-spi`, `starter-blob-fs` (dev),
     `starter-blob-garage` (prod), `starter-blob-compose`,
     `starter-blob-axum`.
   - At boot: build a root engine from config (fs path in dev, garage
     creds in prod). Wrap in `Namespaced("project-<id>", ...)` lazily
     per request (or use a single `Namespaced::with_prefix_template`
     pattern — verify which the compose crate exposes).
   - Mount `blob_proxy_handler(store, authz)` under `/blobs` with
     authz that resolves `BlobContext.scope` (e.g. `"project-<uuid>"`)
     to a project id and checks `(projects, read)`.
   - The `BlobMeta.filename` reserved key gives free
     `Content-Disposition: attachment; filename="…"` on download.

### Other

- Add `with_quota` once the per-project byte cap lands (storage scope
  Gap 4 in starter is done; per-project policy column lives on
  `dp_projects` and is a separate migration).

## Not done — frontend

All under [frontend/src/projects/exec-summary/](../frontend/src/projects/exec-summary/) (new directory).

Files in scope:

- `project-exec-summary-page.tsx` — top-level; loads via
  `use-exec-summary.ts`.
- `exec-summary-header.tsx` — dark `#071923` header, logo,
  status badge, completion %, Submit/Approve/Revert.
- `exec-summary-nav.tsx` — sticky left tab nav (8 tabs); active /
  completed / idle states match the supplied mock.
- `sections/summary-section.tsx` — product name, part number,
  release date, markdown fields for objective/problem/value/
  differentiators/success criteria.
- `sections/scope-section.tsx` — five markdown textareas.
- `sections/requirements-section.tsx` — markdown textareas +
  12-checkbox protocols grid (closed list in
  [protocols.ts](../frontend/src/projects/exec-summary/protocols.ts),
  to be created) + power/mounting/certification short inputs.
- `sections/hardware-section.tsx` — image dropzone (uses
  `useBlobUpload` from `@nube/starter-ui-core` once available) plus
  hardware features / physical notes / enclosure / mounting type /
  operating environment.
- `sections/commercial-section.tsx` — RRP / OEM / target GP%
  (parse as cents on send), revenue / channel / target market /
  volume.
- `sections/documents-section.tsx` — document dropzone, list view
  with inline title/type/notes/required-action editing, download
  via the `/blobs/{ref}` proxy.
- `sections/approval-section.tsx` — reviewer / approver / notes,
  status cards (Current Status / Submitted At / Approved At),
  Submit / Approve / Revert buttons (Approve disabled when viewer
  is not the project lead).
- `sections/changelog-section.tsx` — append-only list + add form.
- `hooks/use-exec-summary.ts` — react-query: GET + debounced PATCH
  with optimistic update + rollback toast.
- `hooks/use-exec-summary-upload.ts` — thin wrapper around
  `useBlobUpload` bound to the exec-summary image / document
  presign endpoints.

Mount as a new **"Exec Summary"** tab on
[project-detail-page.tsx](../frontend/src/projects/project-detail-page.tsx)
alongside the existing workbench / repos / milestones tabs.

Use the existing shadcn `Input` / `Textarea` / `Badge` / `Card`
primitives in [frontend/src/components/ui/](../frontend/src/components/ui/)
rather than rolling new ones — the mock's styling translates 1:1.

Markdown long-text fields use the existing
[markdown.tsx](../frontend/src/components/markdown.tsx) viewer and
the same `@uiw/react-md-editor` instance the issues surface uses;
pass `onImageUpload` from `useBlobUploadForMarkdown` so paste/drop
inside an editor goes through the same presign+confirm pipeline.

## Cross-cutting

- **OpenAPI** — once the REST module is in,
  [openapi.rs](../crates/dp-rest/src/openapi.rs) needs to list the new
  handlers and schemas. Pattern: append to the existing `paths(...)`
  and `components(schemas(...))` blocks.
- **TypeScript types** — [frontend/src/api/client.ts](../frontend/src/api/client.ts)
  generates from the OpenAPI doc, so once the backend lands the
  client types appear automatically.
- **Tests** — pattern from existing `*_test.rs` integration tests
  next to each REST module. Smoke tests from scope doc §7:
  round-trip, engine swap, permission, auto-save, completion
  threshold.
