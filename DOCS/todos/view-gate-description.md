# TODO — Project view "description" is frontend-only

## Resolved (issue #17 — flaky gate creation)

- [x] **Fixed the ~50% silent failure when creating the G1–G8 gate
  tabs.** The batch path fired 8 independent fire-and-forget
  `mutate()` calls in a `for` loop and closed the dialog before any
  POST resolved, so concurrent writes (plus 8 racing
  `invalidateQueries` and 8 racing URL redirects) dropped roughly
  half the tabs. Batch creation is now **sequential and awaited**
  (`mutateAsync`, one gate at a time), with up to 3 attempts +
  linear backoff per view, a `sonner` error toast that keeps the
  dialog open on failure ("Created N of M tabs, then failed…"), and
  a disabled "Creating…" button while in flight. Batch views no
  longer redirect the URL per-success (kills the 8-way redirect
  race); new tabs appear via the parent's query invalidation.

  Changed in
  [frontend/src/projects/view-wizard/wizard-dialog.tsx](../../frontend/src/projects/view-wizard/wizard-dialog.tsx),
  [frontend/src/projects/views-tab-strip.tsx](../../frontend/src/projects/views-tab-strip.tsx),
  and
  [frontend/src/projects/project-workbench.tsx](../../frontend/src/projects/project-workbench.tsx).

- [ ] **Deferred — backend YAML/JSON gate config.** The issue also
  asked for "json or yaml files for the premade views/gates". Left
  out by decision: the reliability fix above resolved the actual
  pain, and moving the `GATES` / `VIEW_TEMPLATES` tables
  ([frontend/src/projects/view-wizard/templates.ts](../../frontend/src/projects/view-wizard/templates.ts))
  into a served config file is a larger change to revisit only if a
  concrete need for operator-editable gate defs comes up.

## Open

- [ ] **Gate descriptions (G1–G8) are not persisted; they're a hardcoded
  frontend mapping.** A saved view has no `description` column/field —
  confirmed against the live API (`POST /projects/{id}/views` silently
  drops any `description` in the body; it never round-trips) and against
  the schema in
  [frontend/src/api/schemas/projects.ts](../../frontend/src/api/schemas/projects.ts)
  (`ProjectViewDtoSchema` / `ProjectViewWriteBody` have `name`,
  `group_by`, `filter_clauses`, `sort`, `start_date`, `due_date`,
  `categories` — no `description`).

  The "description" shown when hovering a `G1`…`G8` tab comes entirely
  from
  [frontend/src/projects/icon-for-name.ts](../../frontend/src/projects/icon-for-name.ts),
  the `GATE_META` table, keyed on the view name being *exactly* the gate
  short-code:

  | View name | Hover tooltip (hardcoded) |
  |---|---|
  | `G1` | Executive Summary |
  | `G2` | Proof of Concept |
  | `G3` | MVP Build |
  | `G4` | Client Acceptance |
  | `G5` | Product Refinement |
  | `G6` | Production Ready |
  | `G7` | Go-To-Market |
  | `G8` | Scale & Support |

  Implications / gotchas:
  - The view **must** be named exactly `G1`…`G8` (case-insensitive,
    trimmed) for the gate icon/colour/tooltip to apply. Putting the
    description *into* the name (e.g. `G1 · Executive Summary`) breaks
    `gateMetaForName` and loses the gate styling.
  - `orderGateViews` re-sorts gate tabs into canonical G1→G8 order by
    name, overriding the stored `position` — so drag-reorder of gate
    tabs won't stick.
  - There is no way, through the API, to give a view a custom free-text
    description. Any per-view descriptive text is limited to the 8
    predefined gate labels.

  **Proposed fix (why it's open):** Fix the frontend to loop and check each view is created successfully (not fail silently). The frontend creation of gate views fails ~50% of the time, possibly because it adds them too fast without confirmation.

  Future idea if requested: Create a backend YAML/JSON configuration system for gates/views as an alternative approach.

  As a secondary improvement, if per-view descriptions are a real
  requirement, add a nullable `description` column to `dp_project_views`,
  surface it in `ProjectViewDto` / `ProjectViewWriteBody`, and have the
  tab strip prefer the stored value over the `GATE_META` fallback.

  Related doc: [../projects/MANAGING-VIEWS.md](../projects/MANAGING-VIEWS.md)
  §1 (notes the absence of a description field, but predates the
  gate-tooltip explanation above).
