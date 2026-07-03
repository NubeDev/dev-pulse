# Project + Issue CRUD via the API

> Verified against `https://dev-pulse.fly.dev` on 2026-07-03 by actually
> running every call below (not just reading the spec). Covers **projects**,
> **issues**, and **exec-summary** end to end.

---

## 1. Auth recap

Login once, reuse the cookie jar for every call below. The CSRF token is
only required on non-GET requests (`x-csrf-token` header):

```bash
HOST=https://dev-pulse.fly.dev
COOKIES=/tmp/dp-cookies.txt

CSRF=$(curl -sS -c "$COOKIES" -X POST "$HOST/auth/login" \
  -H 'Content-Type: application/json' \
  -d '{"email":"you@example.com","password":"<secret>"}' \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['csrf_token'])")
```

Unauthenticated reads return `401` with an empty body.

---

## 2. Projects

### 2.1 Create — `POST /projects`

```bash
ORG_ID=<org-uuid>   # from GET /me/orgs

curl -sS -b "$COOKIES" -X POST "$HOST/projects" \
  -H 'Content-Type: application/json' \
  -H "x-csrf-token: $CSRF" \
  -d "{\"org_id\":\"$ORG_ID\",\"name\":\"ABC-TEST\"}"
```

Returns `200` with the full `ProjectDto` (`version: 1`). `409
project_name_taken` if an active project with that name already exists in
the org. Optional fields: `description`, `start_at`, `due_at`,
`lead_user_id`, `status` (defaults `active`).

### 2.2 Read

```bash
# List — filtered, paginated
curl -sS -b "$COOKIES" "$HOST/projects?status=active&q=abc&limit=50&offset=0"

# count-only (cheap badge/counter query)
curl -sS -b "$COOKIES" "$HOST/projects?count_only=1"

# Get one
curl -sS -b "$COOKIES" "$HOST/projects/<project_id>"
```

List query params (all confirmed live): `org_id`, `status`
(`active|backlog|done|archived`), `q` (case-insensitive substring on
`name`), `limit` (1..=200, default 50), `offset`, `count_only` (0/1).
Server-fixed order: status tier → `due_at ASC NULLS LAST` → `name`.

`GET /projects/{id}` on an unknown id → `404 project_not_found`.

`ProjectDto` fields: `id`, `org_id`, `name`, `description`, `status`,
`lead_user_id`, `start_at`, `due_at`, `issue_count`,
`closed_issue_count`, `board_link_count` (always `0` today),
`primary_milestone_id`, `version` (CAS counter — echo back as
`expected_version` on every mutating call), `created_by`, `created_at`,
`updated_at`.

### 2.3 Update — `PATCH /projects/{id}`

CAS-gated. You must send the project's **current** `version`, not a
stale one — every prior mutation (including attaching/detaching an
issue, see §3.4) bumps it, so re-`GET` first if unsure.

```bash
curl -sS -b "$COOKIES" -X PATCH "$HOST/projects/<project_id>" \
  -H 'Content-Type: application/json' \
  -H "x-csrf-token: $CSRF" \
  -d '{"expected_version":4,"description":"Test project for API docs"}'
```

Confirmed live: a stale `expected_version` returns `409
stale_project_version` with the row's *actual* current version in the
error message — use that to retry rather than guessing.

All fields besides `expected_version` are optional and omitted = unchanged
(`name`, `description`, `status`, `start_at`, `due_at`, `lead_user_id`).
To clear a nullable field, send it explicitly as `null`.

### 2.4 Archive — `POST /projects/{id}/archive`

```bash
curl -sS -b "$COOKIES" -X POST "$HOST/projects/<project_id>/archive" \
  -H "x-csrf-token: $CSRF" \
  -H 'Content-Type: application/json' \
  -d '{"expected_version":4}'
```

CAS-gated same as PATCH. Idempotent — archiving an already-archived
project echoes the row back without bumping `version` or writing an
audit row. There is no hard-delete endpoint for projects; archive is the
terminal state.

### 2.5 Link / list / unlink repos

```bash
# Link (idempotent, PUT)
curl -sS -b "$COOKIES" -X PUT \
  "$HOST/projects/<project_id>/repos/<repo_id>" -H "x-csrf-token: $CSRF"

# List
curl -sS -b "$COOKIES" "$HOST/projects/<project_id>/repos"

# Unlink (idempotent, always 204)
curl -sS -b "$COOKIES" -X DELETE \
  "$HOST/projects/<project_id>/repos/<repo_id>" -H "x-csrf-token: $CSRF"
```

---

## 3. Issues

### 3.1 Create — `POST /issues`

Two modes, selected by the `local` flag:

- `local: false` (default) — creates a **real GitHub issue** via the
  GitHub API, then mirrors it into `dp_issues`.
- `local: true` — **skips GitHub entirely**. Creates the row directly in
  `dp_issues` with `is_local = true` and a synthetic negative `number`.
  Not visible on github.com. No `issues_write` install-permission check
  needed since nothing is written to GitHub.

`repo_id` is **always required**, even in local mode — confirmed by
testing (`422 missing field repo_id` if omitted). The issue is still
scoped to a repo for project-membership/org filters; it just never syncs
there.

```bash
REPO_ID=<repo-uuid>          # an existing repo already known to dev-pulse
PROJECT_ID=<project-uuid>    # optional: attach in the same call

curl -sS -b "$COOKIES" -X POST "$HOST/issues" \
  -H 'Content-Type: application/json' \
  -H "x-csrf-token: $CSRF" \
  -d "{\"repo_id\":\"$REPO_ID\",\"title\":\"ABC-TEST local issue\",\"local\":true,\"project_id\":\"$PROJECT_ID\",\"expected_version\":1}"
```

Response: `{ "repo_id", "number": -1, "issue_id" }`.

Attach semantics when `project_id` is set:
- `project_id` only → project-level add ("All" tab). **Requires
  `expected_version`** (the project's current `version`) — `400
  missing_expected_version` if omitted. Bumps the project's version.
- `project_id` + `view_id` → attaches to that saved view's membership
  only; no CAS, no project version bump.

### 3.2 Read

```bash
# Single issue
curl -sS -b "$COOKIES" "$HOST/issues/<issue_id>"

# All issues currently attached to a project
curl -sS -b "$COOKIES" "$HOST/projects/<project_id>/issues"

# Per-issue local date fields (start/due, independent of GitHub)
curl -sS -b "$COOKIES" "$HOST/issues/<issue_id>/dates"

# Activity timeline (newest first)
curl -sS -b "$COOKIES" "$HOST/issues/<issue_id>/timeline"
```

`IssueDto` includes `is_local: true|false` and `repo_slug` — the
reliable way to tell a locally-created issue apart from a GitHub-synced
one is `is_local`, not the negative `number` (negative number is just
the synthetic-id convention local issues currently use).

### 3.3 Update — `PATCH /issues/{id}`

CAS-gated on the issue's own `version` (separate from the project's
version). Confirmed this works identically for local and GitHub-backed
issues — a local issue's PATCH never calls GitHub, it just writes
`dp_issues` directly.

```bash
curl -sS -b "$COOKIES" -X PATCH "$HOST/issues/<issue_id>" \
  -H 'Content-Type: application/json' \
  -H "x-csrf-token: $CSRF" \
  -d '{"title":"New title","body":"New body","expected_version":1}'
```

All patch fields optional, omitted = untouched: `title`, `body`
(explicit `null` clears), `labels` (replaces the whole list), `assignees`
(replaces the whole list), `state` (`"open"` / `"closed"` — routes to
close/reopen audit verbs). Stale `expected_version` → `409` (reload and
retry).

**Comments are not supported on local issues** — confirmed:
`POST /issues/{id}/comments` on a local issue returns `400
local_issue_no_comments`. Comments only work on GitHub-backed issues.

```bash
# Only works when is_local = false
curl -sS -b "$COOKIES" -X POST "$HOST/issues/<issue_id>/comments" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{"body":"a comment","expected_version":1}'
```

### 3.4 Dates — `PATCH /issues/{id}/dates`

Independent of the issue's own `version`/CAS — no `expected_version`
needed. Both fields optional; omit or `null` clears. **Must be full
`date-time`, not a bare date** — confirmed `{"start_at":"2026-07-03"}`
alone fails with `422 premature end of input`; use
`"2026-07-03T00:00:00Z"`.

```bash
curl -sS -b "$COOKIES" -X PATCH "$HOST/issues/<issue_id>/dates" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{"start_at":"2026-07-03T00:00:00Z","due_at":"2026-07-10T00:00:00Z"}'
```

`400 invalid_date_window` if `start_at > due_at`.

### 3.5 Detach from a project — `DELETE /projects/{id}/issues/{issue_id}`

This detaches the issue from the project; it does **not** delete the
issue itself (there is no issue-delete endpoint — GitHub issues can't be
deleted via API, and local issues follow the same lifecycle for
consistency). CAS-gated on the **project's** version (not the issue's),
via a query param:

```bash
curl -sS -b "$COOKIES" -X DELETE \
  "$HOST/projects/<project_id>/issues/<issue_id>?expected_version=2" \
  -H "x-csrf-token: $CSRF"
```

`204` on success. Confirmed this bumps the project's `version` (so a
following PATCH must re-`GET` first). A no-op detach (issue wasn't
attached) → `404 project_issue_not_found`.

### 3.6 Bulk-add existing issues to a project — `POST /projects/{id}/issues`

For attaching issues that already exist (as opposed to §3.1's
create-and-attach-in-one-call):

```bash
curl -sS -b "$COOKIES" -X POST "$HOST/projects/<project_id>/issues" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{"issue_ids":["<id1>","<id2>"],"expected_version":4}'
```

Returns a `BulkAddResult` with per-row outcomes. Capped at
`BULK_ADD_ISSUE_CAP`; over the cap → `400 bulk_add_too_large`.

---

## 4. End-to-end example (as run 2026-07-03)

```bash
HOST=https://dev-pulse.fly.dev
COOKIES=/tmp/dp-cookies.txt
ORG_ID=4d76fd47-cfa6-4187-8119-fcf9b3bd4b6a
REPO_ID=3eeffaf9-7762-47e8-afd5-aaf1a280cb14   # NubeIO/jonathan-testing-gates

CSRF=$(curl -sS -c "$COOKIES" -X POST "$HOST/auth/login" \
  -H 'Content-Type: application/json' \
  -d '{"email":"dev@dev.com","password":"<secret>"}' \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['csrf_token'])")

# 1. Create project
PROJECT=$(curl -sS -b "$COOKIES" -X POST "$HOST/projects" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d "{\"org_id\":\"$ORG_ID\",\"name\":\"ABC-TEST\"}")
PROJECT_ID=$(echo "$PROJECT" | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")

# 2. Create a local (non-GitHub) issue, attached to it
curl -sS -b "$COOKIES" -X POST "$HOST/issues" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d "{\"repo_id\":\"$REPO_ID\",\"title\":\"ABC-TEST local issue\",\"local\":true,\"project_id\":\"$PROJECT_ID\",\"expected_version\":1}"

# 3. Verify
curl -sS -b "$COOKIES" "$HOST/projects/$PROJECT_ID/issues"
```

Result: project `ABC-TEST` (`c8767ca3-a42c-4c6a-9929-d28f36b16423`) with
one local issue (`is_local: true`, `number: -1`), never touching GitHub.

---

## 5. Errors encountered during testing

| Call | Mistake | Response |
|---|---|---|
| `POST /issues` without `repo_id` | assumed local issues don't need a repo | `422` — repo_id is required regardless of `local` |
| `PATCH /issues/{id}/dates` with `"2026-07-03"` | bare date, not date-time | `422 premature end of input` |
| `POST /issues/{id}/comments` on a local issue | comments require GitHub sync | `400 local_issue_no_comments` |
| `PATCH /projects/{id}` with a version from before a detach | detach bumps project version too | `409 stale_project_version` (message includes the real current version) |

---

## 6. Source of truth

Always re-check `GET /openapi.json` — this doc is a snapshot from live
testing, not a spec transcription. See [API-USAGE.md](../../API-USAGE.md)
§1 for spec-inspection one-liners.

---

## 7. Executive summary

Each project has one exec-summary — a structured, section-grouped
document (product summary, scope, requirements, hardware, commercial,
approval, plus image/document attachments and a changelog) that goes
through a `draft → in_review → approved` sign-off workflow, independent
of the project's own `version`/CAS. Verified live against `ABC-TEST`
(`c8767ca3-a42c-4c6a-9929-d28f36b16423`) on 2026-07-03.

### 7.1 Read — `GET /projects/{id}/exec-summary`

```bash
curl -sS -b "$COOKIES" "$HOST/projects/<project_id>/exec-summary"
```

Lazy-created: even a project that's never had a PATCH returns a full
`ExecSummaryDto` with every field `null`/empty and `completion.percent:
0` — no separate "create" call needed, confirmed live.

`ExecSummaryDto` top level: `project_id`, `updated_at`, then 6 sections
(`summary`, `scope`, `requirements`, `hardware`, `commercial`,
`approval`), plus `images[]`, `documents[]`, `changelog[]`,
`completion { percent, sections: {<id>: bool} }`, and
`skipped_sections[]`.

### 7.2 Update — `PATCH /projects/{id}/exec-summary`

Sparse, section-grouped merge — send only the sections/fields you're
changing; omitted sections and omitted fields within a sent section are
left untouched. No `expected_version`/CAS on this endpoint (unlike
project/issue PATCH).

```bash
curl -sS -b "$COOKIES" -X PATCH "$HOST/projects/<project_id>/exec-summary" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{
    "summary": {
      "product_name": "ABC Test Widget",
      "objective": "Validate the dev-pulse exec-summary API end to end.",
      "problem": "No documented example of the exec-summary CRUD flow.",
      "value": "Gives the team a tested reference doc.",
      "success_criteria": "All exec-summary endpoints exercised and documented."
    },
    "scope": {
      "in_scope": "Exec-summary create/read/update/submit/approve/revert.",
      "out_of_scope": "GitHub issue sync, billing."
    }
  }'
```

Each of `summary`, `scope`, `requirements`, `hardware`, `commercial`,
`approval` in the body is an independent optional "patch" object mirroring
its DTO's fields (all nullable strings, plus `requirements.protocols:
string[]` and `commercial`'s two `*_cents` int fields / `target_gp_pct`
float). Confirmed live: PATCHing one section doesn't touch the others,
and `completion.percent`/`completion.sections.<id>` recompute
automatically on every PATCH — a section flips to `true` once it has at
least one non-null field (confirmed: `commercial` flipped `true` after
setting just `revenue_model`, before every field was filled).

**`skipped_sections`** marks sections "N/A" — this also counts them as
complete for the percent/threshold calc without requiring any fields.
Send the **whole replacement array** (it's not additive):

```bash
curl -sS -b "$COOKIES" -X PATCH "$HOST/projects/<project_id>/exec-summary" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{"skipped_sections": ["images", "documents", "approval", "changelog"]}'
```

### 7.3 Submit — `POST /projects/{id}/exec-summary/submit`

`draft → in_review`. No request body. Server-side completion gate:
confirmed live it rejects below **80%** with the exact missing sections:

```bash
curl -sS -b "$COOKIES" -X POST "$HOST/projects/<project_id>/exec-summary/submit" \
  -H "x-csrf-token: $CSRF"
```

```json
{"error":"incomplete","code":"incomplete","percent":75,"threshold":80,"missing":["approval","changelog"]}
```

`approval` and `changelog` behave like any other section for this gate
— get them to `true` either by giving `approval.status` real content
through the workflow, or (as done in the ABC-TEST run) adding them to
`skipped_sections` if sign-off tracking genuinely doesn't apply. Once
≥80%, submit succeeds and stamps `approval.status: "in_review"` +
`approval.submitted_at`.

### 7.4 Approve — `POST /projects/{id}/exec-summary/approve`

`in_review → approved`. Project-lead only in the design (v1 handler
didn't reject the dev-seed account in testing — don't rely on that).
Optional body:

```bash
curl -sS -b "$COOKIES" -X POST "$HOST/projects/<project_id>/exec-summary/approve" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{"approval_notes":"Looks good, approved for docs test."}'
```

Stamps `approval.status: "approved"` + `approval.approved_at`. Confirmed
the state machine is enforced: calling `approve` directly from `draft`
(skipping `submit`) → `409 exec_summary_status_conflict` with
`"exec summary status is Draft, expected in_review"`.

### 7.5 Revert — `POST /projects/{id}/exec-summary/revert`

`* → draft`, from any status. No body.

```bash
curl -sS -b "$COOKIES" -X POST "$HOST/projects/<project_id>/exec-summary/revert" \
  -H "x-csrf-token: $CSRF"
```

Confirmed live: resets `approval.status` to `"draft"` but **does not
clear** `submitted_at` / `approved_at` / `approval_notes` — they remain
as history on the row even after revert. Content sections are untouched
(revert only affects the approval state machine).

### 7.6 Changelog

`changelog[]` stayed empty through create → PATCH → submit → approve →
revert in this test run — none of those calls populate it. There's a
restore endpoint (`POST .../exec-summary/changelog/{entry_id}/restore`,
rolls the live content back to a prior snapshot and appends a *new*
changelog entry) but no create/list-only endpoint, so entries are
evidently written by something else (a periodic snapshot job is the
likely mechanism, going by the DTO's `has_snapshot` field) — **not
confirmed**, flagging as open rather than guessing further.

### 7.7 Errors encountered (exec-summary)

| Call | Mistake | Response |
|---|---|---|
| `submit` below 80% completion | tried to submit with `approval`/`changelog` sections empty | `400 incomplete` with `percent`, `threshold`, `missing[]` |
| `approve` called from `draft` (skipped `submit`) | wrong assumption about transition flexibility | `409 exec_summary_status_conflict` |

### 7.8 End-to-end example (as run 2026-07-03, project `ABC-TEST`)

Content sourced from a real product page — [NubeIO IO-22 (UIUO)
overview](https://nubeio.github.io/rbx-docs/products/io-controllers/io-22-uiuo/overview/)
— to show realistic field values rather than placeholder text.

```bash
PROJECT_ID=c8767ca3-a42c-4c6a-9929-d28f36b16423

# Section 1: summary + scope
curl -sS -b "$COOKIES" -X PATCH "$HOST/projects/$PROJECT_ID/exec-summary" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{
    "summary": {
      "product_name": "IO-22 (UIUO)",
      "part_number": "IO-8UIUO-4DI-4AOV-6RO",
      "objective": "Provide an expansion IO card for the ACBM-IO-22 bus with 8 fully software-configurable universal I/O channels plus dedicated digital inputs, analog voltage outputs, and relay outputs for HVAC/building control.",
      "problem": "Traditional IO cards require fixed hardware jumpers/DIP switches per channel type, forcing rigid configurations that dont adapt to different sensor/actuator mixes across projects.",
      "value": "Per-channel software configurability via BACnet lets a single card serve as analog input, analog output, digital input, or RTD measurement.",
      "differentiators": "Per-channel software configurability with no hardware jumpers; integrated 16-bit AD74412R processing engine; 500V per-channel isolation.",
      "success_criteria": "8 UIUO channels + 4 DI + 4 AOV + 6 RO all field-configurable via BACnet with no physical rework."
    },
    "scope": {
      "in_scope": "8x universal I/O (0-10V/4-20mA sensing and output, RTD 2-wire), 4x DI, 4x AOV, 6x RO. BACnet-based configuration.",
      "out_of_scope": "Wireless/LoRaWAN connectivity — use the ACBL gateway for LoRaWAN. Standalone operation without an ACBM controller.",
      "dependencies": "Requires an ACBM controller connected via the ACBM-IO-22 expansion bus."
    }
  }'

# Section 2: requirements + hardware + commercial
curl -sS -b "$COOKIES" -X PATCH "$HOST/projects/$PROJECT_ID/exec-summary" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{
    "requirements": {
      "must_have": "8x UIUO channels (16-bit ADC/DAC), 4x DI, 4x AOV, 6x RO; BACnet configuration.",
      "architecture": "ACBM-IO-22 expansion bus, stack-on module.",
      "protocols": ["BACnet"],
      "power": "Loop-powered current input option: internal 24V excitation, <=20mA/channel.",
      "mounting": "Stack-on module or standalone expansion card on ACBM-IO-22 bus."
    },
    "hardware": {
      "hardware_features": "16-bit ADC/DAC; AD74412R processing engine; RTD 2-wire (0-1 MOhm range).",
      "physical_notes": "Overall accuracy TUE +/-0.1% FSR; isolation 500V per channel."
    },
    "commercial": {
      "target_market": "Building automation, HVAC controls, facilities management.",
      "revenue_model": "Hardware sale (expansion card)."
    }
  }'

# Mark sections genuinely not covered by the product page as N/A
curl -sS -b "$COOKIES" -X PATCH "$HOST/projects/$PROJECT_ID/exec-summary" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{"skipped_sections":["images","documents","approval","changelog"]}'

# Sign off
curl -sS -b "$COOKIES" -X POST "$HOST/projects/$PROJECT_ID/exec-summary/submit" -H "x-csrf-token: $CSRF"
curl -sS -b "$COOKIES" -X POST "$HOST/projects/$PROJECT_ID/exec-summary/approve" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{"approval_notes":"Approved — sourced from NubeIO IO-22 (UIUO) product docs."}'
```

Final state: `completion.percent: 100`, `approval.status: "approved"`,
project `ABC-TEST` now carries a realistic exec-summary describing the
IO-22 (UIUO) product (8x universal I/O, 4x DI, 4x AOV, 6x RO, BACnet
configuration, ACBM-IO-22 bus).

Note: `certification`, `enclosure`, `mounting_type`, and `operating_env`
were left `null`/marked "Not documented on the product page" — the
source page didn't cover them, and PATCH doesn't require every field in
a section to flip its completion boolean to `true` (see §7.2).
