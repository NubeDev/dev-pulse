# Project Executive Summary — Scope (frontend + backend)

> A structured, multi-section "exec summary" attached to every
> `dp_projects` row, captured through a tabbed wizard-style form
> (Summary / Scope / Requirements / Hardware / Commercial / Documents /
> Approval / Change Log), with file uploads backed by the blob storage
> being added in starter (see
> [SCOPE-STORAGE-FEEDBACK.md](../SCOPE-STORAGE-FEEDBACK.md)).
>
> Companion to [SCOPE-PROJECTS.md](../SCOPE-PROJECTS.md) and the
> existing project surfaces in
> [frontend/src/projects/](../frontend/src/projects/).

---

## 1. Vision

Today a `dp_project` carries a name, lead, dates, repos, milestones and
issues — operational metadata. It does not capture the *product
definition*: what the thing is, why we're building it, what's in/out
of scope, what hardware shape it takes, what the commercial model is,
which documents back it, and who approved it.

Product managers currently hold this in a mix of Confluence pages,
Word docs, and Slack threads. The result is that the dev-pulse project
view shows execution state without the **intent** behind it, and
nobody has a single durable record of "what did we agree to ship for
this project, and who signed it off?"

The Executive Summary fixes that. One project, one canonical exec
summary, edited through a clear sectioned form, with reviewer/approver
state and a change log, and with supporting files (briefs, sketches,
datasheets, BOMs) attached via the same blob storage stack the scope
pages will use.

---

## 2. Goals

### Primary

1. **One exec summary per project** — `dp_project_exec_summary`,
   1-to-1 with `dp_projects`. Auto-created lazily on first edit.
2. **Eight typed sections** matching the mock:
   - Summary (product identifiers + objective / problem / value /
     differentiators / success criteria)
   - Scope (in / out / assumptions / dependencies / constraints)
   - Requirements (must-have / optional / UX / architecture / protocols
     multi-select / power / mounting / certification)
   - Hardware (features / physical notes / enclosure / mounting /
     environment + reference images)
   - Commercial (RRP / OEM / GP% / revenue / channel / market / volume)
   - Documents (uploaded files with title / type / notes /
     required-action)
   - Approval (reviewer / approver / notes / status transitions /
     timestamps)
   - Change Log (version / date / author / summary, append-only)
3. **Approval state machine** — `Draft → In Review → Approved`
   (with `→ Draft` revert allowed from any state), with timestamps
   captured server-side and an audit row per transition.
4. **File attachments** — reference images on the Hardware section,
   arbitrary documents on the Documents section, stored via the
   starter `BlobStore` surface, persisted as `BlobRef` JSON.
5. **Completion tracking** — server computes which of the 8 sections
   are "complete" (rules per section, §6 below), surfaces a percentage,
   and the frontend tab nav reflects it (green tick vs number).

### Secondary

- **Markdown long-text fields** — every `Textarea` in the mock is
  actually a markdown field, rendered with the existing
  [markdown.tsx](../frontend/src/components/markdown.tsx) component
  and edited with the same `@uiw/react-md-editor` instance the issues
  surface uses. Inline images paste/drop into the editor and upload
  through the same blob pipeline.
- **Print / export** — read-only printable view that renders the
  whole summary as one page (PDF later, HTML first).

### Non-goals (0.1)

- Per-section permissioning (everyone with project write can edit
  any section; approval gate is the only state lock).
- Templating / cloning summaries across projects.
- Workflow beyond `Draft / In Review / Approved` (no multi-approver,
  no conditional approval, no rejection sub-states).
- Diff view between change-log entries.
- Localisation; UI is English.

---

## 3. Backend

### 3.1 Schema (new migration in [crates/dp-store-pg/migrations/dp/](../crates/dp-store-pg/migrations/dp/))

`00NN_project_exec_summary.sql`:

```sql
CREATE TABLE dp_project_exec_summary (
    project_id      uuid PRIMARY KEY
                      REFERENCES dp_projects(id) ON DELETE CASCADE,

    -- Summary section
    product_name        text,
    part_number         text,
    target_release_date date,
    objective           text,         -- markdown
    problem             text,         -- markdown
    value               text,         -- markdown
    differentiators     text,         -- markdown
    success_criteria    text,         -- markdown

    -- Scope section
    in_scope            text,
    out_of_scope        text,
    assumptions         text,
    dependencies        text,
    constraints         text,

    -- Requirements section
    must_have           text,
    optional            text,
    user_interaction    text,
    architecture        text,
    protocols           text[]  NOT NULL DEFAULT '{}',  -- BACnet MS/TP, …
    power               text,
    mounting            text,
    certification       text,

    -- Hardware section
    hardware_features   text,
    physical_notes      text,
    enclosure           text,
    mounting_type       text,
    operating_env       text,
    -- reference images live in dp_project_exec_summary_images

    -- Commercial section
    rrp_cents           bigint,
    oem_price_cents     bigint,
    target_gp_pct       numeric(5,2),
    revenue_model       text,
    channel_strategy    text,
    target_market       text,
    volume_assumptions  text,

    -- Approval section
    status              text NOT NULL DEFAULT 'draft'
                          CHECK (status IN ('draft','in_review','approved')),
    reviewer            text,
    approver            text,
    review_notes        text,
    approval_notes      text,
    submitted_at        timestamptz,
    approved_at         timestamptz,

    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE dp_project_exec_summary_images (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id  uuid NOT NULL REFERENCES dp_projects(id) ON DELETE CASCADE,
    blob_ref    jsonb NOT NULL,                  -- starter BlobRef
    filename    text  NOT NULL,
    content_type text NOT NULL,
    caption     text,
    ord         int   NOT NULL DEFAULT 0,
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE dp_project_exec_summary_documents (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id  uuid NOT NULL REFERENCES dp_projects(id) ON DELETE CASCADE,
    blob_ref    jsonb NOT NULL,
    title       text  NOT NULL,
    doc_type    text,                            -- 'brief','bom','datasheet',…
    notes       text,
    required_action text,
    uploaded_by text,
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE dp_project_exec_summary_changelog (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id  uuid NOT NULL REFERENCES dp_projects(id) ON DELETE CASCADE,
    version     text  NOT NULL,
    changed_at  date  NOT NULL,
    changed_by  text  NOT NULL,
    summary     text  NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX ON dp_project_exec_summary_images       (project_id, ord);
CREATE INDEX ON dp_project_exec_summary_documents    (project_id, created_at DESC);
CREATE INDEX ON dp_project_exec_summary_changelog    (project_id, changed_at DESC);
```

**Why one wide table vs eight narrow ones**: every field is 1-to-1
with the project; the form loads/saves them together; partial saves
(per-section) hit only a subset of columns, which Postgres handles
fine. Eight tables would force eight joins on every read with no
upside.

**Why arrays for protocols**: closed enum of ~12 values; multi-select.
A separate `dp_project_exec_summary_protocols` table is overkill and
makes the read path slower.

### 3.2 REST surface (new file `crates/dp-rest/src/project_exec_summary.rs`)

```
GET    /projects/{id}/exec-summary
            → { summary, scope, requirements, hardware, commercial,
                approval, images: [...], documents: [...],
                changelog: [...], completion: { percent, sections: {…} } }

PATCH  /projects/{id}/exec-summary
            body: any subset of section payloads; merges and bumps updated_at.

POST   /projects/{id}/exec-summary/submit
            → status='in_review', submitted_at=now(); audit row.

POST   /projects/{id}/exec-summary/approve
            body: { approval_notes? }
            → status='approved', approved_at=now(); audit row.

POST   /projects/{id}/exec-summary/revert
            → status='draft'; audit row. Clears submitted/approved timestamps? NO — keep for history.

POST   /projects/{id}/exec-summary/images
            body: { filename, content_type, size } → { upload_url, blob_ref_placeholder, id }
            (presign via starter BlobStore; client PUTs; then PATCH to confirm)
POST   /projects/{id}/exec-summary/images/{id}/confirm
            body: { blob_ref, etag } → row finalised.
DELETE /projects/{id}/exec-summary/images/{id}

POST   /projects/{id}/exec-summary/documents               (same shape as images)
POST   /projects/{id}/exec-summary/documents/{id}/confirm
PATCH  /projects/{id}/exec-summary/documents/{id}          (title/type/notes/required_action)
DELETE /projects/{id}/exec-summary/documents/{id}

GET    /projects/{id}/exec-summary/changelog
POST   /projects/{id}/exec-summary/changelog
            body: { version, date, changed_by, summary }
PATCH  /projects/{id}/exec-summary/changelog/{id}
DELETE /projects/{id}/exec-summary/changelog/{id}
```

All routes go through the existing project-write authz used by
[project_issues.rs](../crates/dp-rest/src/project_issues.rs).

Files are routed through the **`BlobProxyRouter`** primitive proposed
in [SCOPE-STORAGE-FEEDBACK.md](../SCOPE-STORAGE-FEEDBACK.md) §Gap 1
so that inline-image references in markdown bodies are stable
(non-expiring, auth-checked per request) rather than presigned URLs
with TTL.

### 3.3 Audit

Every state-changing route writes a row to the existing audit table
(see [audit.rs](../crates/dp-rest/src/audit.rs)) with `target_kind =
'project_exec_summary'`, `target_id = project_id`, and a verb of
`patch`, `submit`, `approve`, `revert`, `image_add`, `image_remove`,
`document_add`, `document_remove`, `changelog_add`, `changelog_remove`.

### 3.4 State machine

```
draft ──submit──▶ in_review ──approve──▶ approved
  ▲                   │                       │
  │                   └──revert────┐          │
  └────────revert────────────────┘ │          │
  └──────────revert───────────────────────────┘
```

`submit` is rejected unless `completion.percent ≥ 80%` (rule lives in
the handler, configurable later). `approve` requires `approver` field
set and status currently `in_review`. `revert` is unconditional.

### 3.5 Completion rules

A section counts as complete when:

| Section       | Rule                                                                        |
| ------------- | --------------------------------------------------------------------------- |
| Summary       | `product_name`, `objective`, `success_criteria` all non-empty               |
| Scope         | `in_scope` and `out_of_scope` non-empty                                     |
| Requirements  | `must_have` non-empty AND `protocols` non-empty (≥1)                        |
| Hardware      | `hardware_features` non-empty OR ≥1 reference image                         |
| Commercial    | `rrp_cents` set AND `target_gp_pct` set                                     |
| Documents     | ≥1 document attached                                                        |
| Approval      | `status = 'approved'`                                                       |
| Change Log    | ≥1 changelog row                                                            |

`completion.percent = round(completed / 8 * 100)`. Computed in the
GET handler; cached if it shows up in profiles.

---

## 4. Frontend

### 4.1 Files (new under `frontend/src/projects/exec-summary/`)

```
project-exec-summary-page.tsx        ← top-level page; mounted from project-detail-page.tsx
exec-summary-header.tsx              ← dark header with logo, status badge, completion %, actions
exec-summary-nav.tsx                 ← left tab nav (mock has 8 tabs; reuse navStyle logic)
sections/
  summary-section.tsx
  scope-section.tsx
  requirements-section.tsx
  hardware-section.tsx
  commercial-section.tsx
  documents-section.tsx
  approval-section.tsx
  changelog-section.tsx
hooks/
  use-exec-summary.ts                ← react-query hook: GET + PATCH (debounced auto-save)
  use-exec-summary-upload.ts         ← thin wrapper around `useBlobUpload` (starter-ui-core
                                       hook proposed in storage feedback Gap 2) bound to the
                                       exec-summary image / document presign endpoints
```

### 4.2 Component model (matches the mock)

- **Header** — dark `#071923` slab, logo + title + status badge +
  Submit / Approve / Revert buttons + completion progress bar. Status
  badge styled to status: Draft (slate), In Review (amber), Approved
  (emerald).
- **Left nav** — sticky on `lg+`. 8 tabs; each shows step number or a
  tick if the section is complete. Three visual states from the mock:
  `active` (dark), `completed` (emerald-50), `idle` (slate).
- **Content card** — single `<Card>` swap per `activeTab`, identical
  shell to the mock (rounded-2xl, white, slate-200 border, shadow-sm).
- **Inputs** — wrap shadcn `Input` / `Textarea` so styling matches but
  validation comes from a single schema. Markdown fields use
  `@uiw/react-md-editor` in compact mode (no preview toggle by default,
  toolbar trimmed to bold/italic/link/list/image).
- **Image upload (Hardware)** — drop-zone fed by
  `use-exec-summary-upload`; thumbnails arranged in a 3-column grid
  with caption + drag-reorder.
- **Document upload (Documents)** — list with title / type / notes /
  required-action editable inline; click to download (via the
  `BlobProxyRouter` proxy URL).
- **Approval section** — reviewer / approver / notes inputs plus
  three status cards (Current Status / Submitted At / Approved At)
  driven by server state, with the same Submit / Approve / Revert
  buttons mirrored from the header.
- **Change Log** — table-like list of versions with an "Add entry"
  inline form; entries are append-only from the UI (delete is a
  separate menu action with confirm).

### 4.3 Save model

- **Auto-save per field on blur**, debounced 800 ms. PATCH carries
  only the changed subset.
- **Optimistic** — react-query mutation updates the local cache
  immediately; on failure, rollback + toast.
- **Section navigation never blocks** — switching tabs flushes
  pending edits, but does not wait for the server before moving.

### 4.4 Permissions

- View: anyone who can view the project.
- Edit any section: anyone with project write.
- Submit: anyone with project write.
- Approve / Revert: project lead only (look up `dp_projects.lead`).
- Frontend hides Approve when the viewer is not the lead; backend
  re-checks (do not trust the UI).

### 4.5 Mounting

Add a tab to [project-detail-page.tsx](../frontend/src/projects/project-detail-page.tsx)
called **"Exec Summary"** that renders `project-exec-summary-page.tsx`.
This sits alongside the existing workbench / repos / milestones tabs;
it does not replace any of them.

---

## 5. Dependencies on the storage work

This scope **assumes** the gaps in
[SCOPE-STORAGE-FEEDBACK.md](../SCOPE-STORAGE-FEEDBACK.md) are
landed in starter:

| Storage gap                         | Used by                                                                |
| ----------------------------------- | ---------------------------------------------------------------------- |
| Gap 1 — `BlobProxyRouter`           | Inline images in markdown bodies; document download links              |
| Gap 2 — `useBlobUpload` hook        | Hardware image drop-zone; Documents drop-zone; markdown-editor paste   |
| Gap 3 — `Namespaced("project-<id>") | Wiring in `dp-server` so every exec-summary blob is project-scoped     |
| Gap 5 — reserved `BlobMeta` keys    | `Content-Disposition: filename="…"` on the document download endpoint  |

If starter has not landed these by the time this work begins,
dev-pulse will ship the local versions described in
[SCOPE-STORAGE-FEEDBACK.md](../SCOPE-STORAGE-FEEDBACK.md) §Summary
and migrate to the starter versions once available.

---

## 6. Hard rules

### E1 — One summary per project, no orphans

`ON DELETE CASCADE` from `dp_projects` covers it. There is no
standalone exec-summary entity.

### E2 — Approval is a project-lead decision

Backend enforces; frontend reflects. Anyone *else* clicking Approve
sees a 403, regardless of project-write.

### E3 — Markdown bodies never store engine keys

All embedded image URLs go through the blob proxy
(`/blobs/{ref}`), never raw storage URLs. This keeps the
storage backend swappable per
[SCOPE-STORAGE-FEEDBACK.md](../SCOPE-STORAGE-FEEDBACK.md) B2.

### E4 — Submit threshold is checked server-side

Completion percent is computed server-side and `submit` rejects
under-threshold requests with `400` + a structured list of which
sections are short. The frontend mirrors the calc for the progress
bar only.

### E5 — Change log is append-only from the UI

Editing or deleting a change-log entry requires a separate
confirm flow and writes an audit row. The default UI affords add
only.

---

## 7. Smoke tests (this scope succeeds iff…)

1. **Round-trip test.** A user fills in all 8 sections, uploads one
   image and two documents, adds a change-log entry, submits, and the
   project lead approves. Reloading the page shows the same data,
   status = `approved`, timestamps populated.
2. **Swap test.** The storage engine is swapped from `fs` to `garage`
   in `dp-server` config; existing images and documents continue to
   resolve via the proxy URL with no DB migration.
3. **Permission test.** A non-lead user with project-write clicks
   Approve in the dev console; backend returns 403 and no state
   changes.
4. **Auto-save test.** Typing in a Textarea and switching tabs before
   the debounce fires still persists the edit (flush-on-blur).
5. **Completion test.** With Summary + Scope + Requirements +
   Hardware + Commercial complete (5/8 = 63%), Submit is rejected.
   Adding one document + one change-log entry brings it to 88% and
   Submit succeeds.

---

## 8. Open questions

- **Approval multi-sign-off** — out for 0.1, but the table already
  has `reviewer` and `approver`; if multi-approver lands later we
  promote those to a join table without rewriting the rest.
- **Versioning the whole summary** — change-log captures *what
  changed* in prose, but we don't snapshot the form contents at each
  approval. Worth doing? Defer until someone asks "what did we
  approve last quarter?".
- **PDF export** — out for 0.1; HTML print stylesheet ships, PDF
  pipeline is a follow-up.
- **Template summaries** — cloning a previous project's summary as
  a starter for a new project. Likely valuable; explicitly deferred.

---

## 9. Repo layout (additions)

```
crates/
  dp-store-pg/migrations/dp/
    00NN_project_exec_summary.sql
  dp-rest/src/
    project_exec_summary.rs
  dp-domain/src/
    project_exec_summary.rs            (types shared between rest + store)

frontend/src/projects/exec-summary/
  project-exec-summary-page.tsx
  exec-summary-header.tsx
  exec-summary-nav.tsx
  sections/
    summary-section.tsx
    scope-section.tsx
    requirements-section.tsx
    hardware-section.tsx
    commercial-section.tsx
    documents-section.tsx
    approval-section.tsx
    changelog-section.tsx
  hooks/
    use-exec-summary.ts
    use-exec-summary-upload.ts
```
