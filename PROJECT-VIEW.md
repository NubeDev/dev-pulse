# Project view — Design Proposal

> Goal: one workbench for a project that groups, filters, and saves views
> over its issues — **agnostic** to taxonomy (gates, phases, sprints,
> workstreams), without hardcoding any of them. Tabs exist, but they are
> **user-defined saved views**, not a fixed gate strip.

---

## 1. What we already have

Don't rebuild:

- `dp_projects` + `dp_project_issues` (§17) — the project ↔ issue link is
  already present; the detail page loads `useProjectIssues(projectId)` and
  renders a flat list ([project-detail-page.tsx](frontend/src/projects/project-detail-page.tsx)).
- `dp_tags` + `dp_tag_links` (§16, [tagging.md](tagging.md) §1, §4) — kv
  tags (`gate:g3-mvp-build`, `category:firmware`) are a first-class
  concept with `kind`, `key`, `value` derived columns and per-target
  reverse indexes.
- `dp_milestones`, `dp_issue_types` ([tagging.md](tagging.md) §9) — repo
  milestones and org issue types mirrored read-only, with `due_on DATE`
  and `github_node_id TEXT`.
- `dp_issues.labels` JSONB — already populated by the fetcher; every
  GitHub label string is observable per issue.
- `dp_users` — exists since migration `0001_init.sql`; safe to
  `REFERENCES dp_users(id)` for view ownership (§6.1).
- Project detail header + Meta strip + Issues card are already wired
  ([project-detail-page.tsx](frontend/src/projects/project-detail-page.tsx)).

What's **missing** for this proposal:

- A grouping engine for the issues list (today: flat list).
- A filter chip surface above the list.
- Saved Views — named, persisted bundles of `(group_by, filter, sort)`
  shown as a horizontal tab strip.
- A Milestones strip rendering active milestones with progress + adopt.

---

## 2. The thesis

**One project = one workbench.** Everything orthogonal — group, filter,
sort, save — is a control on that workbench, not a separate screen. The
project detail page therefore has three zones, top to bottom:

1. **Header** — identity (name, status, repo tags, org).
2. **Strips** — Meta (dates, counts) and Milestones (time-bearing progress).
3. **Workbench** — Views tab strip → Toolbar (Group · Filter · Sort) →
   Sectioned issues list.

The whole design is two orthogonal primitives wired together:

- **Group-by** — choose one key; the list renders as collapsible sections
  bucketed by that key's values.
- **Filter** — additive `key:value` chips, AND-combined; narrows what
  group-by then buckets.

Saved Views are just `(group_by, filter, sort)` triples with a name.

---

## 3. Why not fixed gate tabs

The user's first sketch was `[ALL │ Gate 1 │ Gate 2]`. We rejected it.

| Concern | Fixed gate tabs | This design |
|---|---|---|
| Different taxonomies (Phase/Sprint/Workstream) | Rebuild the tab strip | Free — group-by reads tag keys live |
| 8 gates per [tagging.md](tagging.md) gate scheme | Tab strip overflows | Sections scroll; counts visible per bucket |
| "Firmware in G3" cross-cut | Second filter UI on top of tabs | One control, composable |
| Milestone progress | No place to live | Dedicated strip — time + % |
| Pinned personal views | Not supported | Saved Views = user-defined tabs |
| Future Kanban / drag | Tabs ≠ columns | Add `View mode: List │ Board`; Board reuses group-by as columns |

The user's "tag a tab with the parent tag id" intuition is also rejected:
hierarchy already lives in the **key** of a kv tag. Every tag with key
`gate` is implicitly grouped under "gate" — group-by is the join. Adding
parent IDs to tags couples grouping (a *view* concern) to identity (a
*tag* concern) and forecloses regrouping by another key.

---

## 4. Page layout (ASCII)

### 4.1 Default landing — Group by Gate, no filter

```
┌──────────────────────────────────────────────────────────────────────────────────────────┐
│  [≡]  Projects  ›  aaa                                                    ●1 identity  ◀ │
├──────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                          │
│  aaa  ●Active   [ACX-hardware]  [@NubeDev's untitled project]              [⚙ Settings]  │
│                                                                                          │
│  ┌────────────────────────────────────────────────────────────────────────────────────┐  │
│  │ Meta                                                                               │  │
│  │  START         DUE          ISSUES              LINKED BOARDS                      │  │
│  │  01/05/2026    30/05/2026   12/47 closed (26%)  2                                  │  │
│  └────────────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                          │
│  Milestones  ▸                                                                           │
│  ┌─────────────────────┐ ┌─────────────────────┐ ┌─────────────────────┐ ┌────────────┐  │
│  │ v0.3 Beta           │ │ Hardware spin 2     │ │ Compliance pass     │ │ + Adopt    │  │
│  │ due in 6d  ░░▓▓▓▓▓░ │ │ due in 21d ░▓▓░░░░░ │ │ overdue 3d ▓▓▓▓░░░░ │ │            │  │
│  │ 8/14  ★ primary     │ │ 3/11                │ │ 5/12                │ │            │  │
│  └─────────────────────┘ └─────────────────────┘ └─────────────────────┘ └────────────┘  │
│                                                                                          │
│  ╭─ Views ─────────────────────────────────────────────────────────────────────────────╮ │
│  │ [● All] [ Gates ] [ Firmware in G3 ] [ Blocked ] [ My queue ]            [+ New]   │ │
│  ╰─────────────────────────────────────────────────────────────────────────────────────╯ │
│                                                                                          │
│  Group by: ▾ Gate     Filter: [+ Add]    Sort: ▾ Updated ↓        [+ Add issues]         │
│                                                                                          │
│  ▼ G3 · MVP build                                                          6 open · 2 ✓  │
│   ┌────────────────────────────────────────────────────────────────────────────────────┐ │
│   │ OPEN  NubeIO/ACX-firmware#42   bootloader OTA retry loop                           │ │
│   │       [category:firmware] [type:bug] [priority:high]            updated 2h ago     │ │
│   │ OPEN  NubeIO/ACX-firmware#39   sensor calibration drift                            │ │
│   │       [category:firmware] [type:bug]                            updated 5h ago     │ │
│   │ OPEN  NubeIO/ACX-hardware#1    client feedback                                     │ │
│   │       [category:hardware] [type:feature]                        updated 1d ago     │ │
│   │ ✓     NubeIO/ACX-backend#88    expose /telemetry endpoint                          │ │
│   │       [category:backend] [type:feature]                         closed 3d ago      │ │
│   └────────────────────────────────────────────────────────────────────────────────────┘ │
│                                                                                          │
│  ▶ G4 · Client acceptance                                                  3 open · 1 ✓  │
│  ▶ G2 · PoC                                                                2 open · 4 ✓  │
│  ▼ No gate                                                                 1 open · 0 ✓  │
│   ┌────────────────────────────────────────────────────────────────────────────────────┐ │
│   │ OPEN  NubeIO/ACX-app#7        login screen polish                                  │ │
│   │       [category:app]                                            updated 4d ago     │ │
│   └────────────────────────────────────────────────────────────────────────────────────┘ │
│                                                                                          │
└──────────────────────────────────────────────────────────────────────────────────────────┘
```

### 4.2 Saved view — "Firmware in G3"

```
  ╭─ Views ─────────────────────────────────────────────────────────────────────────────╮
  │ [ All ] [ Gates ] [● Firmware in G3 ] [ Blocked ] [ My queue ]            [+ New]   │
  ╰─────────────────────────────────────────────────────────────────────────────────────╯

  Group by: ▾ None     Filter: [gate:g3-mvp-build ×] [category:firmware ×] [+ Add]    Sort: ▾ Updated ↓

  Issues  (2 open · 0 closed)
  ┌────────────────────────────────────────────────────────────────────────────────────┐
  │ OPEN  NubeIO/ACX-firmware#42   bootloader OTA retry loop                           │
  │       [category:firmware] [type:bug] [priority:high]            updated 2h ago     │
  │ OPEN  NubeIO/ACX-firmware#39   sensor calibration drift                            │
  │       [category:firmware] [type:bug]                            updated 5h ago     │
  └────────────────────────────────────────────────────────────────────────────────────┘
```

### 4.3 Cross-cut — Group by Category, Filter `gate:g6-production-ready`

Answers the "all hardware blockers before G6" query from the gate scheme:

```
  Group by: ▾ Category     Filter: [gate:g6-production-ready ×] [+ Add]    Sort: ▾ Updated ↓

  ▼ Hardware                                                                4 open · 1 ✓
  ▼ Firmware                                                                2 open · 0 ✓
  ▶ Backend                                                                 1 open · 3 ✓
  ▶ No category                                                             0 open · 0 ✓
```

Same engine, different lens. No code, schema, or report changes.

---

## 5. Controls — behaviour

### 5.1 Group-by dropdown

Options sourced **dynamically** from data observable on the project's
issues, so new taxonomies appear automatically:

- `None` — flat list (current behaviour).
- `Milestone` — buckets by `dp_milestones.id` joined via
  `dp_issues.milestone_id` (added in [tagging.md](tagging.md) §9).
- `Issue type` — buckets by `dp_issue_types.id`.
- `Status` — `open` / `closed`.
- **Every distinct `key`** from `dp_tags WHERE kind='kv'` that has at
  least one `dp_tag_links` row pointing at one of this project's issues
  → one option per key (`gate`, `category`, `priority`, `team`, …).
- **Sticky keys from saved views.** Any `tag:<key>` referenced by an
  active saved view (§6.1) for this project is **also** appended to the
  dropdown, even if no current issue carries that tag. Avoids the
  dead-end where a saved view shows `Group by: Gate` but the user can't
  re-pick `Gate` after switching away. Sticky entries are marked
  `Gate · (no current data)` and resolve to an empty list with the
  banner described in §6.1.

`Repo` grouping is **deferred** — today a project's issue set is
effectively single-repo per [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md), so
the bucket would always be a single section. Re-evaluate when
cross-repo projects become common.

#### Bucket ordering

Default: **count desc** (largest bucket first).

**Exception — known ordinal taxonomies.** A small config list per
deployment maps a tag-key to an explicit value order. The launch list:

```
gate:     [g1-executive-summary, g2-poc, g3-mvp-build, g4-client-acceptance,
           g5-product-refinement, g6-production-ready, g7-go-to-market,
           g8-scale-support]
priority: [p0, p1, p2, p3]
```

When group-by matches an ordinal key, buckets render in declared order
regardless of count. Unknown values within an ordinal key sort after
the declared ones, count-desc. This unblocks the demo screenshot (§4.1)
which shows G3, G4, G2 in *gate order*, not count order.

A synthetic `No <key>` bucket is pinned last and only rendered when
non-empty **after** filters apply (see §5.4 below).

### 5.2 Filter chips

`Filter: [+ Add]` opens a two-step typeahead:

1. Pick a **dimension**: `tag-key`, `milestone`, `issue-type`,
   `status`, `repo`, `assignee`.
2. Pick a **value** from the values present on this project's issues.

Multiple chips AND-combine. Chips show `×` to remove.

#### Filter ↔ bucket-count interaction

Bucket counts are **post-filter**. When `group_by=tag:gate` and
`filter=category:firmware` are both set, the count next to `G3` is the
number of firmware-tagged issues in G3, not the total in G3. The pre-
filter count would make the visible totals next to a collapsed section
lie about what's inside it, which is the worst failure mode for a
triage surface.

Consequences:

- Buckets with `open + closed == 0` after filtering are **hidden** —
  including the synthetic `No <key>` bucket. The §5.1 rule "pinned
  last when non-empty" already covers this; the count it checks is the
  post-filter count.
- Sticky-from-saved-view bucket keys (§5.1) that match zero issues
  post-filter still surface as a single empty section with the
  recovery banner, so the user can see *why* the view is empty rather
  than a blank page.

### 5.3 Sort

`Updated ↓` (default), `Created ↓`, `Title A→Z`. Priority sort is
deferred until a `priority:*` tag-key is established.

### 5.4 URL hash persistence

The toolbar state serialises to the route hash so refresh and
copy-paste preserve the view. There are **three** shapes; precedence
between them is strict:

```
A. clean saved view:   #/projects/{id}?view={viewId}
B. dirty saved view:   #/projects/{id}?view={viewId}&group=...&filter=...&sort=...
C. ad-hoc:             #/projects/{id}?group=...&filter=...&sort=...
```

**Precedence on load.** If `view=<id>` is present:

1. Hydrate the saved view's `(group, filter, sort)`.
2. If any of `group` / `filter` / `sort` is **also** present, treat
   them as **overrides** on top of the saved view. The Views strip
   shows `● <name> *` (dirty marker), with `[Save changes]` /
   `[Discard]` follow-up.
3. If none of the override params are present, the strip shows the
   clean state `● <name>`.

If `view=<id>` is absent, the URL is interpreted as ad-hoc; the strip
shows `All` selected.

A `view` referencing a deleted/inaccessible id falls back to ad-hoc
with any present override params, plus a one-shot toast
`"This view no longer exists."`

**Separator.** Filter chips use **`;`** as the chip separator and
**`:`** as the dim/value separator, e.g.
`filter=tag:gate:g3-mvp-build;tag:category:firmware;milestone:<uuid>`.
`,` is unsafe — [tagging.md](tagging.md) §3 grammar permits commas
inside tag values (`area:auth,oauth` is a legal tag name once split on
the first colon). `;` is not legal in tag values nor in milestone UUIDs.
Alternative considered: repeating `&filter=` params. Rejected because
the whole hash is consumed by one router and repeated keys make the
dirty-vs-clean comparison in (B) harder.

### 5.5 Milestones strip

One card per **active** milestone (state=open) on any linked repo,
sorted by `due_on ASC NULLS LAST`. Each card:

- name, due-relative (`due in 6d`, `overdue 3d`, `no due date`)
- progress bar `closed / total`
- `★ primary` chip if `dp_projects.primary_milestone_id` matches
- overflow `⋯`: `Adopt as primary` ([tagging.md](tagging.md) §9.5),
  `Filter to milestone`, `Open on GitHub`

Clicking the card body adds `milestone:<dp_milestones.id>` (the
internal UUID, not the GitHub name) to Filter and scrolls to the list.
Milestone *names* are not unique — the same `"v0.3 Beta"` can exist
in two linked repos with different `due_on`. The chip's **rendered
label** is the milestone name (plus repo short-name when ambiguous);
the chip's **value** is always a UUID. Closed milestones live behind a
`▸ Show closed` toggle at the end of the strip (matches
[tagging.md](tagging.md) §9 closed-milestone treatment).

---

## 6. Storage

### 6.1 `dp_project_views` — new table

```sql
CREATE TABLE dp_project_views (
    id            UUID PRIMARY KEY,
    project_id    UUID NOT NULL
                  REFERENCES dp_projects(id) ON DELETE CASCADE,
    owner_user_id UUID NOT NULL
                  REFERENCES dp_users(id) ON DELETE CASCADE,
    name          TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 60),
    -- group_by: NULL | 'milestone' | 'issue_type' | 'status'
    --        | 'tag:<key>'        where <key> matches dp_tags.key grammar
    group_by      TEXT,
    -- filter_json: array of canonical filter clauses. Exactly one shape per dim:
    --   {"dim":"tag",        "key":"<tag-key>", "value":"<tag-value>"}
    --   {"dim":"milestone",  "value":"<dp_milestones.id UUID>"}
    --   {"dim":"issue_type", "value":"<dp_issue_types.id UUID>"}
    --   {"dim":"status",     "value":"open"|"closed"}
    --   {"dim":"assignee",   "value":"<github-login>"}
    -- Enforced by trigger fn dp_project_views_filter_check() (raises on
    -- unknown dim, missing required keys, or wrong jsonb_typeof).
    filter_json   JSONB NOT NULL DEFAULT '[]'::jsonb
                  CHECK (jsonb_typeof(filter_json) = 'array'),
    sort          TEXT NOT NULL DEFAULT 'updated_desc',
    position      INT  NOT NULL,
    visibility    TEXT NOT NULL DEFAULT 'private'
                  CHECK (visibility IN ('private', 'project')),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, owner_user_id, name)
);

CREATE INDEX dp_project_views_project_idx
    ON dp_project_views (project_id, owner_user_id, position);
```

Notes:

- `visibility='project'` is the upgrade path — a project-wide shared
  view visible to every project member. v1 ships **private only**; the
  enum value reserves the slot so we don't need a migration when shared
  views land. The CHECK still passes for `'private'` rows.
- `position` is per `(project_id, owner_user_id)`; reorder is a
  client-driven PATCH that rewrites positions in a tx.
- No CASCADE on tag key changes — group-by `tag:<key>` is a soft
  reference. If the key disappears from the project's issues, the view
  renders as a flat list with the (now unmatched) filter chips intact
  and a `This view's group key has no matching tags` banner.

### 6.2 No new tables for groups themselves

Buckets are derived at query time from `dp_tags` / `dp_tag_links` /
`dp_milestones` / `dp_issue_types`. There is no `dp_groups` or
`dp_project_tabs` — that would be the parent-tag-id mistake.

### 6.3 Primary milestone — `dp_projects` addition

```sql
ALTER TABLE dp_projects
    ADD COLUMN primary_milestone_id UUID
        REFERENCES dp_milestones(id) ON DELETE SET NULL;
```

Set/cleared by the `Adopt as primary` action ([tagging.md](tagging.md)
§9.5). Read-only field on `ProjectDto` — surfaced in the Milestones
strip's `★ primary` chip and the Meta strip if non-null.

---

## 7. REST surface

### 7.1 New routes

```
GET    /projects/{id}/views                 -> ProjectView[]   (owner_user_id = me)
POST   /projects/{id}/views                 -> ProjectView
GET    /projects/{id}/views/{view_id}       -> ProjectView
PATCH  /projects/{id}/views/{view_id}       -> ProjectView     (rename, group/filter/sort, position)
DELETE /projects/{id}/views/{view_id}       -> 204
POST   /projects/{id}/views/reorder         -> ProjectView[]   (atomic position rewrite)
```

`reorder` body is the **full ordered id list** for the caller's views
on this project:

```json
{ "ordered_ids": ["<uuid>", "<uuid>", "<uuid>"] }
```

Server validates the set equals the caller's existing view ids on this
project (no adds/removes via reorder) and rewrites `position = 0..N-1`
in one transaction. Returns the updated list in the new order.

`POST` and `PATCH` validate `group_by` against the dynamic option set
(§5.1) on the server: a `tag:<key>` is accepted iff `<key>` parses per
[tagging.md](tagging.md) §3 grammar; existence of matching tag links
is *not* required (views can pre-exist their data).

### 7.2 Group-by + filter on issues read

Existing `GET /projects/{id}/issues` (used by `useProjectIssues`) gains
optional query params:

```
?group_by=tag:gate
?filter=tag:gate:g3-mvp-build;tag:category:firmware;milestone:<uuid>;status:open
?sort=updated_desc
```

Separator is `;` (see §5.4 for rationale). Each clause matches one
`filter_json` entry in §6.1.

Response shape: when `group_by` is set, the response is a flat issue
array **plus** a `buckets` sidecar:

```json
{
  "issues": [ ... ],
  "buckets": [
    { "key": "g3-mvp-build", "label": "G3 · MVP build", "open": 6, "closed": 2 },
    { "key": "g4-client-acceptance", "label": "G4 · Client acceptance", "open": 3, "closed": 1 },
    { "key": null, "label": "No gate", "open": 1, "closed": 0 }
  ]
}
```

The client never re-buckets — the server's count is authoritative so
collapsed sections show truthful counts without paging the whole list.

### 7.3 Group-by dimension catalogue

```
GET /projects/{id}/group-by-options  -> { dims: [
    { id: "milestone",   label: "Milestone" },
    { id: "issue_type",  label: "Issue type" },
    { id: "status",      label: "Status" },
    { id: "tag:gate",    label: "Gate" },
    { id: "tag:category",label: "Category" },
    ...
] }
```

Computed from the project's observable tag-keys, milestones, types,
plus sticky keys from this user's saved views (§5.1). Cached in-process
with a **60-second TTL keyed by `project_id`**, invalidated eagerly by
tag-link writes touching this project's issues and by view
create/update/delete. The distinct-tag-keys query is non-trivial at
scale (full scan of `dp_tag_links` for the project's issue set), so
the request-lifetime cache from the previous draft is insufficient —
the toolbar re-renders on every filter change and would re-hit the DB
within one user session.

---

## 8. Frontend slices

Land in this order; each is shippable on its own.

### Slice 1 — Milestones strip

- New component `frontend/src/projects/milestones-strip.tsx`.
- New hooks: `useProjectMilestones(projectId)`, `useAdoptMilestone(projectId)`.
- Reads `dp_milestones` joined to the project's linked repos.
- **No click behaviour.** Cards render name, due-relative, progress,
  primary chip. The `Filter to milestone` overflow item is disabled
  until Slice 3. Avoids shipping a throwaway pseudo-filter that
  Slice 3 immediately replaces.

### Slice 2 — Group-by dropdown + sectioned list

- New component `frontend/src/projects/project-workbench.tsx` wraps
  the current Issues card.
- Toolbar: `Group by` dropdown only. `Filter` and `Sort` are stubbed
  out as disabled controls so the visual shape lands first.
- `useProjectIssues` extended with `group_by` param; renders one
  `<CollapsibleSection>` per server bucket.
- URL hash: `?group=<dim>`.

### Slice 3 — Filter chips

- `<FilterChipBar>` component with two-step typeahead.
- `useProjectIssues` extended with `filter` param.
- URL hash: `?filter=k:v;k:v` (see §5.4).
- Wires up Slice 1's `Filter to milestone` overflow item and makes the
  milestone card body clickable.

### Slice 4 — Saved Views

- `dp_project_views` migration + REST + client hooks.
- `<ViewsTabStrip>` component above the toolbar.
- Dirty-state `*` marker + `[Save changes]` / `[Discard]` follow-up.
- Reorder via drag-to-reorder on the tab strip.

### Slice 5 — Primary milestone

- `dp_projects.primary_milestone_id` migration + DTO field.
- `★ primary` chip + `Adopt as primary` overflow action wired to a new
  `POST /projects/{id}/adopt-milestone` route.

Slices 1+2 together already deliver the screenshot the user sketched
(`ALL │ Gate 1 │ Gate 2`) — via Group-by Gate, not hardcoded tabs.
Slice 4 is what turns those buckets into pinnable tabs.

---

## 9. Non-goals

- **No drag-between-buckets in v1.** Group-by is a *view* operation; the
  underlying tag/milestone change is a separate write. Slice 6+ may add
  drag → tag mutation, but only after the [tagging.md](tagging.md) §5.2
  push pipeline is proven.
- **No Board (Kanban) mode in v1.** The data model supports it (same
  group-by key drives columns), but the UI is deferred.
- **No cross-project views.** `dp_project_views.project_id` is NOT NULL.
  A "my work across all projects" surface is a separate feature on the
  Workflow page, not this design.
- **No shared (project-wide) views in v1.** Reserved by the
  `visibility` enum but always `'private'` until project membership
  semantics for view ownership are settled.
- **No nested group-by.** One key at a time. Two-level grouping
  (Gate → Category) is **not** equivalent to Group=Gate + Filter=Category=X:
  the filter collapses the data to one category, losing the "all
  categories side-by-side within each gate" view that a true nested
  grouping would give. We accept this loss — the user can switch the
  outer dimension by changing Group-by, and a Board mode (deferred)
  will eventually give two axes (group-by = columns, sub-group = row
  bands). Saved Views make the swap cheap.

---

## 10. Open questions

1. **Ordinal taxonomy config location.** §5.1 hardcodes `gate` and
   `priority` in a deployment config. Open: does this live in
   `config.toml`, a `dp_taxonomy_orders` table, or compiled into
   `dp-domain`? Compiled is simplest for v1; a table is needed once
   tenants can define their own gates.
2. **Shared view auth.** `visibility='project'` needs "project member"
   semantics that we don't have yet — does view edit require
   `(projects, write)` or a new `(project_views, *)` permission?
   Defer until shared views are scoped.
3. **Server vs client bucketing for `tag:<key>` with very high
   cardinality.** A `team:<name>` key with 200 distinct values produces
   200 sections. Cap at 50 buckets server-side with an `Other` bucket?
   Likely yes; revisit once real data exists.
4. **Sticky-key cleanup.** §5.1 keeps tag-keys referenced by saved
   views in the dropdown even when no current data matches. Open: do
   we surface a "View references a tag-key with no data" health badge
   on the Views strip, or wait for user reports?