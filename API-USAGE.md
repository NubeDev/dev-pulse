# dev-pulse API usage

A practical guide to driving dev-pulse from scripts and `curl`. Focus is on
**authentication**, **discovering the OpenAPI spec**, and the two most common
operator tasks: **managing repos** and **managing projects**.

Replace `https://<host>` with your deployment URL throughout.

---

## 1. OpenAPI spec

The full machine-readable schema is served at:

```
GET https://<host>/openapi.json
```

Open it in any OpenAPI viewer, or query it inline:

```bash
# List all paths
curl -sS https://<host>/openapi.json \
  | python3 -c "import json,sys; d=json.load(sys.stdin); \
      [print(m.upper(), p) for p in sorted(d['paths']) for m in d['paths'][p]]"

# Inspect a specific endpoint
curl -sS https://<host>/openapi.json \
  | python3 -c "import json,sys; print(json.dumps(json.load(sys.stdin) \
      ['paths']['/projects']['post'], indent=2))"

# Show a schema by name
curl -sS https://<host>/openapi.json \
  | python3 -c "import json,sys; print(json.dumps(json.load(sys.stdin) \
      ['components']['schemas']['CreateProjectRequest'], indent=2))"
```

Always check `openapi.json` first — request/response shapes evolve and the
spec is the source of truth.

---

## 2. Authentication

dev-pulse uses **cookie sessions** plus a **CSRF token** for mutating
requests. There are two login paths: email/password (form login) and GitHub
OAuth (UI flow). For scripting, use email/password.

### 2.1 Log in (email + password)

```bash
COOKIES=/tmp/dp-cookies.txt

CSRF=$(curl -sS -c "$COOKIES" -X POST https://<host>/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"email":"you@example.com","password":"<secret>"}' \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['csrf_token'])")

echo "CSRF=$CSRF"
```

After login:

- The session cookie (`sas_*`) is stored in `$COOKIES` and must be sent with
  every subsequent request (`-b "$COOKIES"`).
- The returned `csrf_token` must be sent in the `x-csrf-token` header on
  every **non-GET** request. Without it, mutating calls are rejected.

### 2.2 Verify the session

```bash
curl -sS -b "$COOKIES" https://<host>/me/orgs
# => [ { "id": "<uuid>", "github_id": 12345, "login": "acme", "name": null } ]
```

Save the org id — most of the operator workflows below need it.

### 2.3 GET vs. mutating requests

| Verb | Cookie | CSRF header |
|------|:------:|:-----------:|
| `GET`, `HEAD` | required | not required |
| `POST`, `PUT`, `PATCH`, `DELETE` | required | required |

If you see `403 csrf_*` errors, your `x-csrf-token` is missing or stale —
re-run the login step to get a fresh token.

---

## 3. Managing repos

Repos in dev-pulse mirror GitHub repos. They are populated by:

1. The reconciler (scheduled background ticks against repos already known to
   the system).
2. Webhook deliveries (a payload for an unknown repo triggers an upsert).
3. **`POST /repos/sync`** — the operator-facing import + sync API used
   below. This is the path you'll use when onboarding new GitHub repos.

### 3.1 Import a new repo (and trigger first sync)

```bash
curl -sS -b "$COOKIES" -X POST https://<host>/repos/sync \
  -H 'Content-Type: application/json' \
  -H "x-csrf-token: $CSRF" \
  -d '{"org":"acme","name":"my-new-repo"}'
```

Response (`HTTP 202`):

```json
{ "queued": true, "repo_id": "<uuid>", "imported": true }
```

- `imported: true`  — the repo was unknown and was just upserted from GitHub.
- `imported: false` — the repo already existed; a sync was queued anyway.

Failure modes:

- `404 repo_not_found` — GitHub has no such repo (or the app token can't see
  it). Check the slug and the app installation.
- `400 missing_fields` — `org` or `name` is blank.
- `503` — the reconciler is not configured on this deployment.

### 3.2 Re-sync an existing repo by id

```bash
curl -sS -b "$COOKIES" -X POST \
  https://<host>/repos/<repo_id>/sync \
  -H "x-csrf-token: $CSRF"
```

### 3.3 Check sync freshness

```bash
curl -sS -b "$COOKIES" \
  https://<host>/repos/<repo_id>/sync-status
```

Returns the per-resource cursor age used by the "stale" badge in the UI.

### 3.4 List known repos

```bash
curl -sS -b "$COOKIES" "https://<host>/repos?limit=200"
```

Paginated; matches the operator-side repo table.

### 3.5 Bulk import example

```bash
for name in repo-a repo-b repo-c; do
  curl -sS -b "$COOKIES" -X POST https://<host>/repos/sync \
    -H 'Content-Type: application/json' \
    -H "x-csrf-token: $CSRF" \
    -d "{\"org\":\"acme\",\"name\":\"$name\"}" \
    -w " HTTP=%{http_code}\n"
  sleep 0.3
done
```

The 300 ms sleep avoids hammering the GitHub-side rate budget when
discovering many new repos in one pass.

---

## 4. Managing projects

A **project** is a dev-pulse-native grouping that aggregates one or more
repos for reporting (portfolio reports, milestones, board links, …).

### 4.1 Create a project

`org_id` is required and must come from `/me/orgs`.

```bash
curl -sS -b "$COOKIES" -X POST https://<host>/projects \
  -H 'Content-Type: application/json' \
  -H "x-csrf-token: $CSRF" \
  -d '{"org_id":"<org-uuid>","name":"Project Phoenix"}'
```

Response (`HTTP 200`): the full `ProjectDto` including the new `id`.

Optional fields: `description`, `start_at`, `due_at`, `lead_user_id`,
`status` (`active` by default; see `ProjectStatusDto` in the spec).

Errors: `409 project_name_taken` if an active project with that name already
exists in the org.

### 4.2 List / get / update projects

```bash
# List
curl -sS -b "$COOKIES" "https://<host>/projects?limit=200"

# Get one
curl -sS -b "$COOKIES" https://<host>/projects/<project_id>

# Partial update (PATCH uses optimistic concurrency — see PatchProjectRequest)
curl -sS -b "$COOKIES" -X PATCH https://<host>/projects/<project_id> \
  -H 'Content-Type: application/json' \
  -H "x-csrf-token: $CSRF" \
  -d '{"version":1,"description":"Updated"}'
```

### 4.3 Link a repo to a project

```bash
curl -sS -b "$COOKIES" -X PUT \
  "https://<host>/projects/<project_id>/repos/<repo_id>" \
  -H "x-csrf-token: $CSRF"
```

Idempotent — calling twice is fine. A repo can be linked to multiple
projects.

### 4.4 List a project's repos

```bash
curl -sS -b "$COOKIES" \
  https://<host>/projects/<project_id>/repos
```

### 4.5 Unlink a repo

```bash
curl -sS -b "$COOKIES" -X DELETE \
  "https://<host>/projects/<project_id>/repos/<repo_id>" \
  -H "x-csrf-token: $CSRF"
```

Returns `204` whether or not the link existed (idempotent).

### 4.6 Archive a project

```bash
curl -sS -b "$COOKIES" -X POST \
  https://<host>/projects/<project_id>/archive \
  -H "x-csrf-token: $CSRF"
```

---

## 5. Project views

A **view** is a saved, ordered tab on a project that pre-applies filters,
sort, and an optional group-by. Each user has their own private set of views
per project (`visibility: "private"` in v1).

### 5.1 List views

```bash
curl -sS -b "$COOKIES" \
  https://<host>/projects/<project_id>/views
```

Returns the caller's views in `position ASC` order. Each `ProjectViewDto`
carries `id`, `name`, `filter_clauses`, `sort`, `group_by`, `position`,
`visibility`, and live `open_issue_count` / `total_issue_count`.

### 5.2 Discover available group-by dimensions

The `group_by` value isn't a free-text string — it must come from the
project's dynamic dimension catalogue:

```bash
curl -sS -b "$COOKIES" \
  https://<host>/projects/<project_id>/group-by-options
```

Returns `{ "dims": [ { "key": "status", … }, { "key": "tag:area", … } ] }`.
`status` is always present; `tag:<key>` entries appear once per distinct tag
key observed on the project's issues.

### 5.3 Create a view

```bash
curl -sS -b "$COOKIES" -X POST \
  https://<host>/projects/<project_id>/views \
  -H 'Content-Type: application/json' \
  -H "x-csrf-token: $CSRF" \
  -d '{
        "name": "Open bugs",
        "sort": "updated_desc",
        "group_by": "status",
        "filter_clauses": [
          { "dim": "state", "op": "eq", "value": "open" },
          { "dim": "tag:type", "op": "eq", "value": "bug" }
        ]
      }'
```

Field rules:

- `name` — required, 1–60 chars after trim.
- `sort` — required; one of `updated_desc`, `updated_asc`, `title_asc`.
  Empty string is rejected; send `"updated_desc"` to mean "default".
- `filter_clauses` — array of canonical `{ "dim": …, "op": …, "value": … }`
  objects. Each clause is validated server-side. Pass `[]` for "no filter".
- `group_by` — `null` for a flat view, or a key from
  `/group-by-options` (e.g. `"status"`, `"tag:area"`).
- `start_date`, `due_date` — optional `YYYY-MM-DD` for timeline-oriented
  views.

Response (`HTTP 201`): the full `ProjectViewDto` including the new `id` and
the appended `position`.

Errors: `409` on name clash within the caller's views, `400` on invalid
`sort` / `filter_clauses` / `group_by`.

### 5.4 Update a view

```bash
curl -sS -b "$COOKIES" -X PATCH \
  https://<host>/projects/<project_id>/views/<view_id> \
  -H 'Content-Type: application/json' \
  -H "x-csrf-token: $CSRF" \
  -d '{
        "name": "Open bugs (high priority)",
        "sort": "updated_desc",
        "filter_clauses": [
          { "dim": "state", "op": "eq", "value": "open" },
          { "dim": "tag:priority", "op": "eq", "value": "high" }
        ]
      }'
```

Note: the PATCH body has the same shape as POST — there is no partial-update
semantics on views. Always send the full intended state.

### 5.5 Reorder views

`position` is managed by an atomic full-list rewrite. Send the complete
ordered id list:

```bash
curl -sS -b "$COOKIES" -X POST \
  https://<host>/projects/<project_id>/views/reorder \
  -H 'Content-Type: application/json' \
  -H "x-csrf-token: $CSRF" \
  -d '{ "ordered_ids": ["<view_id_1>","<view_id_2>","<view_id_3>"] }'
```

`ordered_ids` must equal the caller's existing view-id set on that project —
omitting or adding ids returns `400`.

### 5.6 Delete a view

```bash
curl -sS -b "$COOKIES" -X DELETE \
  https://<host>/projects/<project_id>/views/<view_id> \
  -H "x-csrf-token: $CSRF"
```

Returns `204` on success.

---

## 6. End-to-end example: onboard a repo into a project

```bash
COOKIES=/tmp/dp-cookies.txt
HOST=https://<host>

# 1. Login
CSRF=$(curl -sS -c "$COOKIES" -X POST "$HOST/auth/login" \
  -H 'Content-Type: application/json' \
  -d '{"email":"you@example.com","password":"<secret>"}' \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['csrf_token'])")

# 2. Find the org
ORG_ID=$(curl -sS -b "$COOKIES" "$HOST/me/orgs" \
  | python3 -c "import json,sys; print(json.load(sys.stdin)[0]['id'])")

# 3. Import + sync the GitHub repo
REPO_ID=$(curl -sS -b "$COOKIES" -X POST "$HOST/repos/sync" \
  -H 'Content-Type: application/json' \
  -H "x-csrf-token: $CSRF" \
  -d '{"org":"acme","name":"my-repo"}' \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['repo_id'])")

# 4. Create the project
PROJECT_ID=$(curl -sS -b "$COOKIES" -X POST "$HOST/projects" \
  -H 'Content-Type: application/json' \
  -H "x-csrf-token: $CSRF" \
  -d "{\"org_id\":\"$ORG_ID\",\"name\":\"My Project\"}" \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")

# 5. Link them
curl -sS -b "$COOKIES" -X PUT \
  "$HOST/projects/$PROJECT_ID/repos/$REPO_ID" \
  -H "x-csrf-token: $CSRF"

echo "Linked repo $REPO_ID to project $PROJECT_ID"
```

---

## 7. Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `401 unauthorized` | session cookie missing/expired | re-run `/auth/login` |
| `403 csrf_*` | `x-csrf-token` missing or stale | re-login, capture fresh token |
| `404 repo_not_found` on `/repos/sync` | GitHub-side repo absent or app token can't see it | verify slug + app installation |
| `409 project_name_taken` | active project with same name exists in org | choose a different name or archive the old one |
| `503` on sync routes | reconciler not configured on this deployment | enable the GitHub client in server config |

For anything not covered here, the OpenAPI spec (`/openapi.json`) lists every
endpoint and schema dev-pulse exposes.