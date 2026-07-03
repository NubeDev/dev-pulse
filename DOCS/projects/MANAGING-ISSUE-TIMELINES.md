# Managing issue timelines (start / due dates)

> Verified live against `https://dev-pulse.fly.dev` on 2026-07-03 using
> project **ABC-TEST**
> (`c8767ca3-a42c-4c6a-9929-d28f36b16423`). Companion to
> [MANAGING-VIEWS.md](./MANAGING-VIEWS.md) and
> [READING-PROJECTS.md](./READING-PROJECTS.md).

---

## 1. What an issue timeline is

Every issue can carry a **local timeline** — a `start_at` / `due_at`
pair stored in `dp_issue_dates`, separate from the issue's own row and
from any GitHub milestone. These are what drive the Gantt / schedule
view. They apply to **any** issue, local (`is_local: true`) or
GitHub-backed.

The two fields:

| Field | Type | Meaning |
|---|---|---|
| `start_at` | `date-time` \| null | Planned start instant (inclusive). |
| `due_at` | `date-time` \| null | Planned due instant (inclusive). |

Timeline dates are **independent of the issue's `version`/CAS** — the
`PATCH /issues/{id}/dates` endpoint takes no `expected_version`.

Auth: standard cookie + `x-csrf-token` (GET needs cookie only). See
[READING-PROJECTS.md §1](./READING-PROJECTS.md).

---

## 1b. First you need an issue: local vs GitHub

A timeline hangs off an issue, so before setting dates you create the
issue with `POST /issues`. The `local` flag decides whether it touches
GitHub — both paths were tested live and both accept the same
`/dates` PATCH afterwards.

### Local-only (`local: true`)

Creates the row directly in `dp_issues`, **never** calls GitHub. Gets a
**synthetic negative `number`** and `is_local: true`. Use for internal /
planning items that shouldn't appear on github.com.

```bash
curl -sS -b "$COOKIES" -X POST "$HOST/issues" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d "{\"repo_id\":\"$REPO_ID\",\"title\":\"[G1] Executive summary sign-off\",
       \"local\":true,\"project_id\":\"$PID\",\"expected_version\":$VER}"
# => { "repo_id":…, "number": -2, "issue_id": … }     ← negative number
```

### Real GitHub issue (`local: false`, the default)

Actually creates the issue on github.com via the app token, then mirrors
it back into `dp_issues`. Gets a **real positive `number`**.

```bash
curl -sS -b "$COOKIES" -X POST "$HOST/issues" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d "{\"repo_id\":\"$REPO_ID\",\"title\":\"[G3] MVP build kickoff\",
       \"body\":\"…\",\"local\":false,\"project_id\":\"$PID\",\"expected_version\":$VER}"
# => { "repo_id":…, "number": 2, "issue_id": … }       ← real GitHub #2
```

Both still require `repo_id` (even local — it scopes the issue). When
`project_id` is set without a `view_id`, `expected_version` (the
project's current `version`) is required and the attach bumps it.

### Telling them apart afterwards

> ⚠️ **`is_local` is omitted from the response when false**, not sent as
> `false`. Confirmed live: a GitHub-backed issue's DTO has **no**
> `is_local` key at all, while a local issue carries `"is_local": true`.
> Read it defensively — `is_local = dto.get("is_local", False)`. The
> sign of `number` is the other tell: negative ⇒ local, positive ⇒
> GitHub.

Full create semantics are in
[READING-PROJECTS.md §3.1](./READING-PROJECTS.md).

---

## 2. Read — `GET /issues/{id}/dates`

```bash
curl -sS -b "$COOKIES" "$HOST/issues/<issue_id>/dates"
```

```json
{
  "issue_id": "ec42b219-…",
  "start_at": "2026-07-06T00:00:00Z",
  "due_at":   "2026-07-19T00:00:00Z",
  "mirror_node_id": null,
  "mirror_synced_at": null,
  "mirror_error": null,
  "updated_at": "2026-07-03T01:03:24Z"
}
```

Lazily returns a full row even when no dates were ever set (all-`null`),
so the picker renders uniformly. The `mirror_*` fields track sync of
this timeline to a GitHub Projects v2 item — `null` until the first
successful mirror; ignore them for local issues.

---

## 3. Write — `PATCH /issues/{id}/dates`

```bash
curl -sS -b "$COOKIES" -X PATCH "$HOST/issues/<issue_id>/dates" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{"start_at":"2026-07-06T00:00:00Z","due_at":"2026-07-19T00:00:00Z"}'
```

- Both fields optional; **omit or send `null` to clear** that side.
- Must be full **`date-time`** (`YYYY-MM-DDTHH:MM:SSZ`), **not** a bare
  `YYYY-MM-DD` — a bare date fails deserialization with `422`.

### 3.1 The only server-side guard: `start_at <= due_at`

The one invariant the backend enforces is **within a single issue**:
start must be on or before due. Violating it returns `400`:

```bash
curl -sS -b "$COOKIES" -X PATCH "$HOST/issues/<issue_id>/dates" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{"start_at":"2026-07-25T00:00:00Z","due_at":"2026-07-19T00:00:00Z"}'
# => {"error":"start_at must be <= due_at","code":"invalid_date_window"}  (400)
```

---

## 4. Gate ordering (G2 can't start before G1) — a CLIENT-SIDE rule

The gates `G1 … G8` (see [MANAGING-VIEWS.md §gates]) form a sequential
progression: G1 (Executive Summary) → G2 (Proof of Concept) → G3 (MVP
Build) → … → G8 (Scale & Support). A sane schedule requires each gate's
work to start no earlier than the previous gate finishes:

> **G2 must not start before G1 is due. More generally: for gates
> `Gᵢ` and `Gᵢ₊₁`, `start(Gᵢ₊₁) >= due(Gᵢ)`.**

**The API does NOT enforce this.** Confirmed live: setting a G2 issue to
start `2026-07-10` while its G1 issue is due `2026-07-19` was accepted
with `200` — there is no cross-issue / cross-gate dependency check in the
backend. The `start_at <= due_at` guard is *per issue only*.

So this ordering is a **convention your client must enforce** before
PATCHing. The pattern:

```bash
# 1. Read the predecessor gate's due date.
G1_DUE=$(curl -sS -b "$COOKIES" "$HOST/issues/<g1_issue_id>/dates" \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['due_at'] or '')")

# 2. Reject a G2 start that precedes it — BEFORE calling the API.
G2_START="2026-07-10T00:00:00Z"
python3 - "$G1_DUE" "$G2_START" <<'PY'
import sys
g1_due, g2_start = sys.argv[1], sys.argv[2]
if g1_due and g2_start < g1_due:
    sys.exit(f"REJECT: G2 start {g2_start} is before G1 due {g1_due} — "
             "gate order violated; move G2 start to >= G1 due.")
print("OK: gate order satisfied")
PY

# 3. Only if the check passes, PATCH the dates.
curl -sS -b "$COOKIES" -X PATCH "$HOST/issues/<g2_issue_id>/dates" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d "{\"start_at\":\"$G2_START\",\"due_at\":\"2026-08-02T00:00:00Z\"}"
```

Notes on applying the rule:

- Compare **`start(Gᵢ₊₁)` against `due(Gᵢ)`**, not start-vs-start — a
  later gate may legitimately start the day the previous one is due.
- ISO-8601 UTC strings are lexicographically comparable, so a plain
  string `<` is a correct chronological comparison (as used above).
- If a gate has no `due_at` set yet (`null`), there's nothing to gate
  against — treat the constraint as vacuously satisfied (the snippet's
  `if g1_due and …` does this).
- Enforce the whole chain the same way: when moving any gate `Gᵢ`,
  check it against `Gᵢ₋₁`'s due, and optionally re-validate `Gᵢ₊₁`'s
  start against `Gᵢ`'s new due.

---

## 5. End-to-end example (as run 2026-07-03, ABC-TEST)

Three gate issues span both creation paths: **G1 and G2 are local-only**
(`is_local: true`, numbers `-2` / `-3`), **G3 is a real GitHub issue**
(`local: false`, number `2` on `NubeIO/jonathan-testing-gates`). Their
windows follow the gate order (G1 Jul 6–19, G2 Jul 20–Aug 2, G3 Aug
3–16) — each starts on/after the previous is due.

```bash
HOST=https://dev-pulse.fly.dev
COOKIES=/tmp/dp-cookies.txt
PID=c8767ca3-a42c-4c6a-9929-d28f36b16423
REPO_ID=3eeffaf9-7762-47e8-afd5-aaf1a280cb14

CSRF=$(curl -sS -c "$COOKIES" -X POST "$HOST/auth/login" \
  -H 'Content-Type: application/json' \
  -d '{"email":"dev@dev.com","password":"<secret>"}' \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['csrf_token'])")

# current project version (needed for each attach; it bumps per attach)
VER=$(curl -sS -b "$COOKIES" "$HOST/projects/$PID" \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['version'])")

# G1 + G2 as LOCAL issues (§1b), G3 as a REAL GitHub issue.
# Each returns issue_id; number is <0 for local, >0 for GitHub.
G1_ID=$(curl -sS -b "$COOKIES" -X POST "$HOST/issues" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d "{\"repo_id\":\"$REPO_ID\",\"title\":\"[G1] Executive summary sign-off\",\"local\":true,\"project_id\":\"$PID\",\"expected_version\":$VER}" \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['issue_id'])"); VER=$((VER+1))

G2_ID=$(curl -sS -b "$COOKIES" -X POST "$HOST/issues" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d "{\"repo_id\":\"$REPO_ID\",\"title\":\"[G2] Proof of concept build\",\"local\":true,\"project_id\":\"$PID\",\"expected_version\":$VER}" \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['issue_id'])"); VER=$((VER+1))

G3_ID=$(curl -sS -b "$COOKIES" -X POST "$HOST/issues" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d "{\"repo_id\":\"$REPO_ID\",\"title\":\"[G3] MVP build kickoff\",\"body\":\"real GitHub issue\",\"local\":false,\"project_id\":\"$PID\",\"expected_version\":$VER}" \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['issue_id'])")

# Timelines — each gate starts on/after the previous is due
curl -sS -b "$COOKIES" -X PATCH "$HOST/issues/$G1_ID/dates" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{"start_at":"2026-07-06T00:00:00Z","due_at":"2026-07-19T00:00:00Z"}'  # G1: Jul 6→19

curl -sS -b "$COOKIES" -X PATCH "$HOST/issues/$G2_ID/dates" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{"start_at":"2026-07-20T00:00:00Z","due_at":"2026-08-02T00:00:00Z"}'  # G2: Jul 20→Aug 2

curl -sS -b "$COOKIES" -X PATCH "$HOST/issues/$G3_ID/dates" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{"start_at":"2026-08-03T00:00:00Z","due_at":"2026-08-16T00:00:00Z"}'  # G3: Aug 3→16
```

Observed results:

| Step | Request | Result |
|---|---|---|
| G1 created local | `local:true` | `number: -2`, `is_local:true` |
| G3 created on GitHub | `local:false` | `number: 2`, real `NubeIO/jonathan-testing-gates#2` |
| G1 within its window | start Jul 6, due Jul 19 | `200` |
| within-issue guard | start Jul 25, due Jul 19 | `400 invalid_date_window` |
| G2 after G1 due | start Jul 20 | `200` (gate order OK) |
| G3 (GitHub issue) timeline | start Aug 3, due Aug 16 | `200` — dates work identically on GitHub-backed issues |
| G2 before G1 due | start Jul 10 | `200` — **API allowed it**; only the client-side check in §4 stops this |

---

## 6. Errors encountered during testing

| Call | Cause | Response |
|---|---|---|
| bare date `"2026-07-06"` | needs full date-time | `422` (deserialize error) |
| start after due | within-issue window guard | `400 invalid_date_window` |
| G2 start before G1 due | **no** cross-gate guard exists server-side | `200` — must be caught client-side (§4) |

---

## 7. Source of truth

`GET /openapi.json` → `PatchIssueDatesRequest`, `IssueDatesDto`. The
`start_at <= due_at` CHECK lives in the migration; the gate-ordering
rule in §4 is a product convention, not a DB/API constraint — enforce it
in the client.
