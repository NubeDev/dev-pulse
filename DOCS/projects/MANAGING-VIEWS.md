# Managing project views (the G1…G8 tabs)

> Verified live against `https://dev-pulse.fly.dev` on 2026-07-03 by
> running every call below against the **ZC Daikin** project
> (`7924445c-38ae-4e4e-84b0-513a9cb46ccb`) — the one whose tab strip shows
> `G1 … G8`. Companion to [READING-PROJECTS.md](./READING-PROJECTS.md).

---

## 1. What a view is

A **view** is a saved, ordered tab on a project (the `G1 … G8` chips in
the project header). Each view carries:

- a **name** (the tab label, 1–60 chars),
- an optional **timeline window** — `start_date` + `due_date`
  (`YYYY-MM-DD`). This is what the UI renders as the *"5th week of
  July"* / *"due in 28d"* label under each tab. **There is no separate
  free-text "description" field** — the descriptive text you see on a tab
  is derived from this date window, not stored as prose.
- an optional **`group_by`** dimension (or flat),
- **`filter_clauses`** — which issues the tab shows,
- **`categories`** — ordered collapsible sections rendered *inside* the
  view,
- a **`sort`** order,
- a **`position`** in the strip.

Views are **private per user** (`visibility: "private"` in v1) — every
user has their own set on a project, and `owner_user_id` is always the
caller. All the endpoints below operate on *your* views only.

Auth is the standard cookie + `x-csrf-token` (see
[READING-PROJECTS.md §1](./READING-PROJECTS.md)); GETs need only the
cookie, mutations need the CSRF header.

---

## 2. `ProjectViewDto` shape

```json
{
  "id": "d95a1426-…",
  "project_id": "7924445c-…",
  "owner_user_id": "5bd9c5bd-…",
  "name": "G1",
  "group_by": null,
  "filter_clauses": [],
  "sort": "updated_desc",
  "position": 0,
  "visibility": "private",
  "start_date": "2026-06-01",
  "due_date": "2026-07-31",
  "categories": [],
  "created_at": "…",
  "updated_at": "…",
  "open_issue_count": 0,      // GET-list only
  "total_issue_count": 0      // GET-list only
}
```

`open_issue_count` / `total_issue_count` are **only populated by
`GET /projects/{id}/views`** (the list). Write responses and the
single-view GET return them as `null` to avoid a wasted count query.

---

## 3. List — `GET /projects/{id}/views`

```bash
curl -sS -b "$COOKIES" "$HOST/projects/<project_id>/views"
```

Returns *your* views in `position ASC` order, each with live issue
counts.

Single view: `GET /projects/{id}/views/{view_id}` (no counts).

---

## 4. Create — `POST /projects/{id}/views`

Only `name` is required. Everything else is optional.

```bash
curl -sS -b "$COOKIES" -X POST "$HOST/projects/<project_id>/views" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{
    "name": "G9",
    "sort": "updated_desc",
    "group_by": "status",
    "start_date": "2026-07-01",
    "due_date": "2026-07-31",
    "categories": ["backend", "frontend", "qa"],
    "filter_clauses": []
  }'
```

Response `201` — the full `ProjectViewDto`. The new view is **appended**
(`position` = current count; e.g. creating alongside G1–G8 gave
`position: 8`).

Field rules (all confirmed live):

| Field | Rule |
|---|---|
| `name` | required, 1–60 chars after trim |
| `sort` | one of `updated_desc`, `updated_asc`, `title_asc`. Empty string is rejected — send `"updated_desc"` to mean "default". Bad value → `400 invalid_sort` |
| `group_by` | `null` (flat) or a key from `/group-by-options` (`"status"`, `"tag:<key>"`) |
| `start_date` / `due_date` | `YYYY-MM-DD` or `null` |
| `categories` | see §6 |
| `filter_clauses` | see §5. `[]` = no filter |

> ⚠️ **Name uniqueness is NOT enforced on create** in the current
> deployment — POSTing a duplicate name returned `201`, not the `409`
> the older docs imply. Don't rely on the server to dedupe tab names.

---

## 5. Filter clauses — the correct shapes

`filter_clauses` is a discriminated array keyed on `dim`. The valid
`dim` values are **`status`, `assignee`, `label`, `tag`, `milestone`**.

> ⚠️ The example in the top-level [API-USAGE.md](../../API-USAGE.md) uses
> `{"dim":"state","op":"eq","value":"open"}`. That is **wrong** for this
> deployment — `state` is not a valid dim (→ `400 invalid_filter:
> unknown variant 'state'`), and there is **no `op` field**. Use the
> shapes below.

| dim | shape | notes |
|---|---|---|
| `status` | `{"dim":"status","value":"open"}` | value must be `open` or `closed` |
| `assignee` | `{"dim":"assignee","value":"<login>"}` | GitHub login |
| `label` | `{"dim":"label","value":"<label>"}` | |
| `tag` | `{"dim":"tag","key":"<key>","value":"<val>"}` | **needs `key` AND `value`** |
| `milestone` | `{"dim":"milestone","value":"<uuid>"}` | value must be a UUID |

Example — issues that are open, assigned to `octocat`, labelled `bug`:

```json
"filter_clauses": [
  {"dim":"status","value":"open"},
  {"dim":"assignee","value":"octocat"},
  {"dim":"label","value":"bug"}
]
```

Each clause is validated server-side; the error names the offending
clause index, e.g. `clause #2: milestone value must be a UUID`.

---

## 6. Categories

`categories` are **ordered category slugs** rendered as collapsible
sections *inside* the view. This is the "they can also have categories"
feature.

- Format: lowercase, `[a-z0-9_-]`, 1–50 chars each.
- Deduped, max 32.
- Sent as an ordered array — order is preserved and meaningful.

```json
"categories": ["backend", "frontend", "qa"]
```

Invalid slug → `400 invalid_categories`, e.g.
`category #0 'Backend Team!' must match [a-z0-9_-]` (uppercase and
spaces/punctuation are rejected — slugify first).

`categories` is independent of `group_by`: `group_by` picks a dynamic
grouping dimension (status/tags), while `categories` are author-defined
static sections.

---

## 7. Update — `PATCH /projects/{id}/views/{view_id}`

> **PATCH is a FULL REPLACE, not a partial merge.** The body has the
> exact same shape as POST. Any field you omit is reset to its default —
> confirmed live: a PATCH that omitted `group_by`, `start_date`,
> `due_date`, and `categories` wiped all four (`group_by → null`, dates
> `→ null`, `categories → []`). **Always send the complete intended
> state.**

```bash
curl -sS -b "$COOKIES" -X PATCH "$HOST/projects/<project_id>/views/<view_id>" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{
    "name": "G1 (open bugs)",
    "sort": "title_asc",
    "group_by": "status",
    "start_date": "2026-07-05",
    "due_date": "2026-08-15",
    "categories": ["backend", "frontend"],
    "filter_clauses": [
      {"dim":"assignee","value":"octocat"},
      {"dim":"label","value":"bug"}
    ]
  }'
```

Returns `200` with the updated DTO.

---

## 8. Reorder — `POST /projects/{id}/views/reorder`

Position is managed by an **atomic full-list rewrite** — you send the
complete ordered id list, and it must exactly equal your existing
view-id set on that project (no adds, no omissions).

```bash
curl -sS -b "$COOKIES" -X POST "$HOST/projects/<project_id>/views/reorder" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{"ordered_ids":["<v1>","<v2>","<v3>", … ]}'
```

Returns `200` with the full re-positioned list (positions renumbered
0..N-1). A partial or mismatched set → `400 invalid_reorder`
(`reorder ordered_ids must match the existing view set`).

Grab the current set first:

```bash
curl -sS -b "$COOKIES" "$HOST/projects/<project_id>/views" \
  | python3 -c "import json,sys; print([v['id'] for v in json.load(sys.stdin)])"
```

---

## 9. Delete — `DELETE /projects/{id}/views/{view_id}`

```bash
curl -sS -b "$COOKIES" -X DELETE \
  "$HOST/projects/<project_id>/views/<view_id>" -H "x-csrf-token: $CSRF"
```

`204` on success.

> ⚠️ **Not idempotent** — deleting an already-deleted view returns
> `404 view_not_found`, unlike the repo-unlink endpoint which is
> idempotent. Deleting does not renumber remaining positions; use
> `/reorder` afterward if you need a gapless strip.

---

## 10. End-to-end example (as run 2026-07-03, ZC Daikin)

```bash
HOST=https://dev-pulse.fly.dev
COOKIES=/tmp/dp-cookies.txt
PID=7924445c-38ae-4e4e-84b0-513a9cb46ccb

CSRF=$(curl -sS -c "$COOKIES" -X POST "$HOST/auth/login" \
  -H 'Content-Type: application/json' \
  -d '{"email":"dev@dev.com","password":"<secret>"}' \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['csrf_token'])")

# 1. Create a view with a timeline window + categories + a status filter
VID=$(curl -sS -b "$COOKIES" -X POST "$HOST/projects/$PID/views" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{"name":"G9","sort":"updated_desc","group_by":"status",
       "start_date":"2026-07-01","due_date":"2026-07-31",
       "categories":["backend","frontend","qa"],
       "filter_clauses":[{"dim":"status","value":"open"}]}' \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")

# 2. Edit it (FULL state — remember PATCH replaces)
curl -sS -b "$COOKIES" -X PATCH "$HOST/projects/$PID/views/$VID" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{"name":"G9 (open bugs)","sort":"title_asc","group_by":"status",
       "start_date":"2026-07-05","due_date":"2026-08-15",
       "categories":["backend","frontend"],
       "filter_clauses":[{"dim":"status","value":"open"},
                         {"dim":"label","value":"bug"}]}'

# 3. Move it to the front of the strip
IDS=$(curl -sS -b "$COOKIES" "$HOST/projects/$PID/views" \
  | python3 -c "import json,sys; \
      vs=json.load(sys.stdin); ids=[v['id'] for v in vs]; \
      ids.remove('$VID'); print(json.dumps(['$VID']+ids))")
curl -sS -b "$COOKIES" -X POST "$HOST/projects/$PID/views/reorder" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d "{\"ordered_ids\":$IDS}"

# 4. Delete it
curl -sS -b "$COOKIES" -X DELETE "$HOST/projects/$PID/views/$VID" \
  -H "x-csrf-token: $CSRF"
```

---

## 11. Errors encountered during testing

| Call | Cause | Response |
|---|---|---|
| filter `{"dim":"state",…}` | `state` isn't a valid dim (API-USAGE.md is stale) | `400 invalid_filter: unknown variant 'state'` |
| filter clause with `op` | there is no `op` field | ignored / clause fails validation |
| `{"dim":"tag","value":…}` without `key` | tag needs both `key` and `value` | `400 invalid_filter: missing field 'key'` |
| `{"dim":"milestone","value":"v1"}` | milestone value must be a UUID | `400 invalid_filter` |
| `{"dim":"status","value":"banana"}` | status only `open`/`closed` | `400 invalid_filter` |
| category `"Backend Team!"` | uppercase + punctuation | `400 invalid_categories` |
| `sort:"created_desc"` | not a valid sort | `400 invalid_sort` |
| `reorder` with partial id set | must equal full existing set | `400 invalid_reorder` |
| `DELETE` an already-deleted view | not idempotent | `404 view_not_found` |

---

## 12. Source of truth

Always re-check `GET /openapi.json` (`ProjectViewDto`,
`ProjectViewCreateBody`, `ProjectViewReorderBody`) — this doc is a
snapshot from live testing. Note that the top-level API-USAGE.md view
examples predate the current filter-clause schema; trust the shapes in
§5 here.
