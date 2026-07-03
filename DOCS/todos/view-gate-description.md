# TODO — Project view "description" is frontend-only

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

  **Proposed fix (why it's open):** if per-view descriptions are a real
  requirement, add a nullable `description` column to `dp_project_views`,
  surface it in `ProjectViewDto` / `ProjectViewWriteBody`, and have the
  tab strip prefer the stored value over the `GATE_META` fallback. Until
  then, this is a documented limitation, not a code defect.

  Related doc: [../projects/MANAGING-VIEWS.md](../projects/MANAGING-VIEWS.md)
  §1 (notes the absence of a description field, but predates the
  gate-tooltip explanation above).
