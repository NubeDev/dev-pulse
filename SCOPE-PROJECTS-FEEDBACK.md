# Project Management — User Feedback Triage

> **Status: triage / scope.** Captures a round of free-form user
> feedback on the Projects + Executive Summary (ES) surface, maps each
> idea to what already exists in the codebase, and proposes the
> concrete change for the items we're building now. Email alerts are
> explicitly deferred. PL1/PL2/PL3 is held pending a clarifying
> question back to the user.
>
> Normative project scope: [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md) →
> [SCOPE.md](SCOPE.md). ES scope:
> `SCOPE-PROJECT-EXECUTIVE-SUMMARY.md`. When this doc and those
> disagree, those are normative — this is a working triage.

---

## Raw feedback (verbatim)

> Ideas for Project management:
>
> - Colour code the product by OEM / Nube iO Product
> - Email alerts for when a task is added
> - Executive summary: needs LoRa and a WiFi, free text field
> - Put multi products in commercial. Commercial add PL1 / PL2 / PL3
> - Want to see images uploaded, image upload maybe for a project and/or ES
> - Update dates updates the ES as well (this is like the project due date)
> - Can't add assigned person later, this is maybe for ES as well

---

## Summary table

| # | Feedback item | State today | Verdict |
|---|---|---|---|
| 1 | Colour-code product by OEM / Nube iO | Products have `manufacturer_id` but **no kind/type flag** | **Build** — small |
| 2 | Email alerts on task add | No notification infra on this surface | **Defer** (user agreed) |
| 3 | LoRa / WiFi / free-text on ES | `LoRaWAN` exists only as a protocol checkbox | **Build** — small |
| 4 | Multi-products in commercial | Project↔product link already exists | **Mostly done** — surface it |
| 4b | Commercial PL1 / PL2 / PL3 | Unknown meaning | **Ask user** |
| 5 | See / upload images for project &/or ES | ES Hardware already has image upload | **Mostly done** — surface / extend |
| 6 | Project date updates the ES | Both fields exist, **not synced** | **Build** — small wiring |
| 7 | Can't add assignee later (+ ES) | `lead_user_id` is PATCH-able | **Bug** — investigate |

---

## Item-by-item

### 1. Colour-code product by OEM vs Nube iO — **build**

Today a product carries `manufacturer_id`
([products.ts `ProductDtoSchema`](frontend/src/api/schemas/products.ts))
but there's no field distinguishing an in-house **Nube iO** product
from an **OEM** product.

**Proposed change**

- Add a `kind` enum to the product domain + store + REST DTO:
  `nube_io | oem` (default `nube_io`).
- Settable in [new-product-dialog.tsx](frontend/src/products/new-product-dialog.tsx)
  and via CAS PATCH on the product Overview tab.
- Colour-code in [product-list.tsx](frontend/src/products/product-list.tsx)
  and on the project's products panel
  ([project-products-panel.tsx](frontend/src/products/project-products-panel.tsx))
  — a small badge/dot, two semantic tokens.

Migration: new nullable column with a backfill default; existing rows
become `nube_io`.

### 2. Email alerts on task add — **defer**

No SMTP / queue / notification plumbing exists on the projects
surface. The user already flagged *"maybe we can't do email now."*
Agreed — out of this round. If revived, it's its own scope (delivery,
opt-in/preferences, dedupe, retry) — not a quick add.

### 3. LoRa / WiFi / free-text on ES — **build**

`LoRaWAN` currently appears only inside the **Protocols** checkbox
group on the ES Requirements section
([requirements-section.tsx](frontend/src/projects/exec-summary/sections/requirements-section.tsx),
`PROTOCOL_OPTIONS` in
[exec-summary.ts](frontend/src/api/schemas/exec-summary.ts#L46)).
The user wants first-class fields.

**Proposed change** — extend `ExecSummaryRequirementsSchema` (domain +
REST + store + zod) with:

- `lora` — free text (markdown or short text — TBD with user; default
  short text, "e.g. AU915, SF7–SF12").
- `wifi` — free text ("e.g. 2.4 GHz b/g/n, WPA2").
- `notes` / `free_text` — a general free-text field on the section.

Render as fields in the Requirements section. All optional on the
wire so partial autosave keeps working (§4.3 of the ES scope).

### 4. Multi-products in commercial — **mostly done**

A project **already** links to multiple products: link / unlink /
picker in
[project-products-panel.tsx](frontend/src/products/project-products-panel.tsx),
backed by `useProjectProducts` / `useLinkProductProject`. OEM price is
already a commercial field
([exec-summary.ts `oem_price_cents`](frontend/src/api/schemas/exec-summary.ts#L84)).

Likely the user hasn't found the existing panel, **or** wants the
linked products surfaced *inside the ES Commercial section* (a
read-through list / per-product pricing) rather than only on the
project page.

**Action**: confirm which. If "show linked products in Commercial," it's
a render-only addition reusing `useProjectProducts`.

### 4b. Commercial "PL1 / PL2 / PL3" — **ask user**

Meaning is ambiguous. Candidate readings:

1. **Product-line tiers** — a product belongs to PL1/PL2/PL3 (enum/tag).
2. **Price levels** — three volume price bands on the commercial section.
3. **Part-list slots** — three product/part-number slots.

→ **Open question, blocking this sub-item only.** Hold until answered.

### 5. See / upload images for project &/or ES — **mostly done**

The ES **Hardware** section already has full image upload — drag-drop,
browse, delete, captions —
([hardware-section.tsx](frontend/src/projects/exec-summary/sections/hardware-section.tsx),
`ExecSummaryImageDto`). Backed by the blob-storage work in
[SCOPE-STORAGE-FEEDBACK.md](SCOPE-STORAGE-FEEDBACK.md).

So "image upload for the ES" is **done**; the gap is likely
discoverability, or wanting a **project-level gallery** distinct from
the ES Hardware images.

**Action**: confirm whether a separate project-level image store is
wanted. If yes, it reuses the same blob plumbing
(`dp_project_files`-style table) — straightforward, not new infra.

### 6. Project due date updates the ES — **build (small)**

Project carries `start_at` / `due_at`
([projects.ts](frontend/src/api/schemas/projects.ts#L23)); the ES
Summary section carries `target_release_date`
([summary-section.tsx](frontend/src/projects/exec-summary/sections/summary-section.tsx#L40)).
They are independent today.

**Proposed change** — when a project's `due_at` is set/changed,
reflect it into the ES `target_release_date` (one source of truth).
Decision needed: **one-way mirror** (project → ES, ES field becomes
read-only/derived) vs **default-on-create only** (seed once, then
independent). Recommend **one-way mirror** with an ES override flag, so
the ES can diverge deliberately but tracks by default.

### 7. "Can't add assigned person later" — **bug, investigate**

The project lead is `lead_user_id`, and it **is** in `PatchProjectRequest`
([projects.ts](frontend/src/api/schemas/projects.ts)) — so the backend
*does* support setting it after creation. The "can't add later"
complaint therefore points at a **UI gap or bug**, not a missing
capability:

- Reproduce: open an existing project → try to set/change the lead.
- Check the assignee control on
  [project-detail-page.tsx](frontend/src/projects/project-detail-page.tsx)
  (the `lead_user_id` picker around the Overview card) is wired to the
  PATCH path and not disabled/hidden after create.
- The ES has its own approval `reviewer` / `approver` fields
  ([exec-summary.ts `ExecSummaryApprovalSchema`](frontend/src/api/schemas/exec-summary.ts)).
  Confirm whether "also for ES" means an **ES owner/assignee** is
  wanted distinct from those — if so, that's a small new field.

---

## Proposed order of work (this round)

1. **#7 assignee bug** — investigate first; may be a one-line UI fix.
2. **#3 LoRa/WiFi/free-text** — additive schema fields, low risk.
3. **#1 OEM/Nube colour-code** — new product field + badge.
4. **#6 date sync** — after the one-way-vs-default decision.
5. **#4 / #5** — confirm "already exists vs wants more" with user; likely render-only.
6. **#4b PL1/2/3** — blocked on clarification.
7. **#2 email** — deferred.

## Open questions for the user

- **PL1 / PL2 / PL3**: what does PL mean? (product-line tier / price
  level / part slot)
- **#6 date sync**: hard mirror (ES derived from project) or seed-once?
- **#4 / #5**: does the existing project↔product link + ES Hardware
  image upload already cover it, or do you want them surfaced
  elsewhere (products inside Commercial; a project-level image gallery)?
- **#3**: should LoRa/WiFi be free text or structured (band / region)?
- **#7 ES**: is an ES-specific owner wanted, separate from the
  reviewer/approver fields?
