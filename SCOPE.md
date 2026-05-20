# dev-pulse — Scope

> A GitHub reporting and insights tool for tracking developer activity, workload, and output across multiple organisations.

---

## 1. Vision

`dev-pulse` gives engineering managers, individual developers, and executives a clear, comparable view of *what is actually happening* across their GitHub orgs — who is shipping, who is reviewing, who is overloaded, and how different teams/companies compare when they share an org.

It is **not** a surveillance tool. It is a workload, contribution, and collaboration insight tool that surfaces signals that are otherwise hidden across many orgs and repos. The product is designed, from the UI down, to resist being used as a performance-ranking system (see §4).

---

## 2. Problem

GitHub exposes per-user activity via its API and profile pages, but it does **not** provide a unified, queryable, comparable view of that activity. Specifically, it does not offer:

- A single aggregated view of one developer's activity **across multiple orgs**.
- Per-user role/status tracking when a person belongs to **many orgs simultaneously**.
- A way to compare **two distinct companies** that collaborate inside a **shared org** (e.g. company A and company B both contribute to org-3 — how does contribution split between them?).
- Aggregated, time-windowed (day / week / month / custom range) views with grouping and percentile aggregation for individuals and teams.

Managers currently stitch this together manually from GitHub's UI, spreadsheets, and gut feel.

---

## 3. Goals

### Primary goals

1. **Multi-org user tracking** — follow a single user's activity across every org they belong to, in one place.
2. **Per-user org membership status** — for each user, record which orgs they are in, their role/team in each, and surface this as filterable context on every report.
3. **Cross-company comparison on a shared org** — given a shared org (e.g. org-3), label each user with a "home-org" and produce side-by-side contribution-split reports. **v1 input mechanism: manual mapping** (admin assigns each GitHub user to a home-org). Email-domain inference and GitHub-team-based inference are stretch goals.
4. **Per-user stats** — produce activity dashboards for an individual over any time window (day / week / month / custom range).

### Secondary goals

- Team-level rollups (sum stats by team, by home-org, by repo).
- Trend over time (e.g. is this person's review load growing? is this team's PR throughput dropping?).
- Identification of overload or under-contribution patterns without being punitive.

---

## 4. Non-goals (for now)

Non-goals are enforced by **design choices**, not just stated intent:

- **No individual performance ranking.** UI will not render leaderboards or single-number "developer scores." All comparisons require explicit group selection (team vs team, home-org vs home-org); the UI will not offer "rank all users by X" as an affordance.
- **No code quality analysis.** Bug rate, churn, defect density are out of scope.
- **No real-time alerting / on-call monitoring.**
- **No replacement for general project management tools** (Jira, Linear, etc.). Light interaction with GitHub Issues specifically is in scope — see §4.1 and [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md).
- **No tracking outside GitHub** (Slack, calendars, meetings).
- **No write operations from the scheduled fetcher.** The fetcher (§10) stays read-only. The single write path is the user-initiated GitHub Issues CRUD surface defined in [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md) — a deliberate, scoped exception with its own auth, audit, and blast-radius story.

These constraints exist because activity data, once available, gravitates toward perf-review use. The design choices above are the primary defence.

### 4.1 In scope — GitHub Issues CRUD (detailed in [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md))

GitHub Issues **create / read / update / close / reopen / comment** is in scope: a manager looking at an activity report can act on what they see (file a follow-up, reassign, close stale work) without leaving the tool. This was previously flagged as a future direction; it is now formally in scope. The full shape — including pinned repos, home-grown project tags, and the write path — lives in [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md).

The interaction with the rest of the architecture stays consistent:

- The **local store** model for issues (§5, §6) carries the fields needed for editing (title, body, labels, assignees, state, milestone), not only the counters needed for reporting.
- The **fetcher** (§10) stays read-only and scheduled. Issue writes are a **separate, synchronous, user-initiated** path to GitHub; the local store is updated optimistically and reconciled on the next fetcher tick or via the issue's webhook.
- The **auth model** (§15.1 GitHub App, §15.10 operator login) must carry sufficient write scope (`issues: write`) on the per-org App installation. Orgs whose install was granted read-only get a UI-visible "writes not available" affordance — never a 500.
- **Audit logging** (§15.13) gains dedicated verbs for issue mutations: `issue.create`, `issue.update`, `issue.close`, `issue.reopen`, `issue.comment`. Every mutation records actor, target issue, before/after diff of mutated fields, and the resulting GitHub delivery id.

Explicitly **not** in §16's v1 cut: PR mutations, discussions, reactions, attachments, and label/milestone administration (we *use* existing labels, we don't manage them). Those remain future-phase.

### 4.2 In scope — pinned favourites & project tags (detailed in [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md))

Users can pin a small set of **favourite repos and tags** that surface as a fast-access list in the UI (sidebar / dashboard) and as the default filter on issue-management views. Pins are **per-user** state, stored in the `dev-pulse` database (not on GitHub), and gated by the §15.11 access policy.

Users can also create **home-grown project tags** that group repos / issues / users / teams across orgs — the cross-org grouping primitive GitHub Projects v2 structurally cannot provide. See [SCOPE-PROJECTS.md](SCOPE-PROJECTS.md) for the full shape.

---

## 5. Key entities

- **User** — a GitHub user. Has zero or more org memberships.
- **Organisation** — a GitHub org. Has many users, many repos, many teams.
- **Team** — a named group inside an org. A user can belong to many teams.
- **Membership** — the join between a user and an org. Carries role, "home-org" label (for cross-company comparison), join date. Team membership is modelled separately via Team.
- **Repository** — belongs to an org. Activity events roll up here.
- **Activity event** — a unit of work attributed to a user, in a repo, in an org, at a point in time. See §6 for attribution caveats.
- **Report** — a query: *who*, *what activity*, *which orgs*, *what time range*, *grouped how*, *aggregated how*.

---

## 6. Activity signals tracked

All four categories:

- **Commits & pull requests** — commits authored, PRs opened / merged / closed. *Lines changed is captured but de-emphasised by default* — generated files, vendored code, and formatter churn make it noisy; reports will require opt-in filters (exclude paths, exclude bot-authored commits) before surfacing it prominently.
- **Code review activity** — reviews given, review comments, approve vs request-changes ratio, review turnaround time.
- **Issues & discussions** — issues opened / closed / commented, discussion participation.
- **CI / deploys / releases** — workflow runs triggered, deployments, releases cut.

Each signal is timestamped, attributed to a user, and tagged with org + repo so it can be sliced any way a report needs.

### Attribution caveats

Attributing an event to "a user" is not as straightforward as it looks. The system must handle:

- **Co-authored commits** (`Co-authored-by:` trailers) — credit each co-author, not just the primary.
- **Squash-merges** — author and committer differ; the merged commit's author is usually the right credit, not the merger.
- **Bot accounts** — `dependabot`, `renovate`, `github-actions`, etc. are excluded from human-developer stats by default but tracked separately.
- **Force-pushes and rebases** — can rewrite history and orphan events; the system records events as observed and does not retroactively delete superseded activity.
- **Unknown / unlinked authors** — commits with email addresses that don't resolve to a GitHub user are bucketed as "unattributed."

These caveats directly affect §9.3 (numbers must match GitHub when spot-checked) and are non-trivial implementation work.

---

## 7. Audiences and core use-cases

### Engineering managers
- "Show me my team's workload this week — collapsed across all orgs they touch." *(all-orgs-combined)*
- "Same team, but split by org — where is each person's time actually going?" *(per-org split)*
- "Just for org-1, what's my team doing?" *(single org)*
- "Who is overloaded with reviews this week?"
- "Compare team A vs team B output for the last quarter."

### Individual developers
- "What did I ship this week / month?" *(all-orgs-combined, personal)*
- "Per-org split of my own activity — am I spread too thin?" *(per-org split)*
- "How does my review load compare to my commit load?"

### Executives / cross-company
- "On org-3 (the shared org), how does contribution split between company A and company B this quarter?" *(single org, grouped by home-org)*
- "Across **every** org we operate, what's the total split between company A and company B?" *(all-orgs-combined, grouped by home-org)*
- "Per-org split — which specific orgs is each company concentrated in?" *(per-org split, grouped by home-org)*
- "How has the contribution split between the participating companies trended over the last 6 months?"

Framing is deliberately neutral ("contribution split", "proportionate share") rather than competitive ("who is carrying more"), consistent with §1's non-surveillance positioning.

---

## 8. Reporting dimensions

A report can slice and group by any combination of:

- **Time window** — day, week, month, custom date range.
- **User** — single user, set of users, all users in a team/org.
- **Org scope** — single org, multiple orgs, all orgs the user belongs to (see §8.1 — three modes).
- **Home-org label** — for cross-company comparison inside a shared org.
- **Team** — group by team(s) within an org.
- **Repo** — single repo or set of repos.
- **Activity type** — any subset of the four signal categories above.

### 8.1 Org breakdown modes (required, all three)

Every report **must** support three org-scope lenses, selectable at the top of the report. They answer different questions and the tool must keep them distinct so users never accidentally double-count.

| Mode             | Question it answers                                          | Example row                                 |
|------------------|--------------------------------------------------------------|---------------------------------------------|
| **Single org**   | "What happened *in this codebase* this period?"              | One row per user, scoped to org-3 only.     |
| **All orgs combined** | "What did this person/team do *in total* this period?"  | One row per user, summed across all orgs (de-duplicated — a person active in 3 orgs is still one row, not three). |
| **Per-org split**| "Where is their time going? Are they spread thin?"           | One row per (user × org) pair, so context-switching is visible. |

**De-duplication rule for "All orgs combined":** people active in multiple orgs count **once** in *People active*. Events (PRs, reviews, etc.) are summed across orgs. This is why the combined total in *People active* can be smaller than the sum of per-org totals — it's correct, not a bug, and the UI must label it so users don't misread it.

### Aggregation functions

- **Counts** for discrete events (commits, PRs, reviews, issues).
- **Sum / average** for numeric quantities.
- **Percentiles (p50, p90, p95)** for duration-style metrics — review turnaround time, time-to-first-review, time-to-merge. Means are not used for these (long-tail distortion).

---

## 9. Constraints

### Privacy and compliance (first-class constraint)

Tracking developer activity across orgs has material legal implications:

- **GDPR** applies if any tracked user is in the EU. Lawful basis, data subject access, right to erasure, and DPIA may all be required.
- **Works-council consultation** is typically required in DE / FR / NL before deploying employee-activity tooling.
- **US state privacy laws** (CCPA / CPRA and successors) may apply for California-based contributors.

**Rollout in any affected jurisdiction is gated on legal review.** Product features that aid compliance (data export per user, deletion-on-request, audit log of who-viewed-what) are in scope for v1.

### Transparency

Tracked contributors must be informed they are being tracked, what is collected, and how to request their data or its deletion. The exact mechanism is a design-doc question, but the principle is a v1 constraint.

---

## 10. Ingestion architecture (scheduled, not on-demand)

Reports must load fast. To guarantee that, **the system never calls GitHub during a page load.** All GitHub data is fetched ahead of time by a scheduled job and stored locally; reports query the local store only.

### How it works

- A **scheduled fetcher job** runs on a configurable interval — **default every 4 hours** — and pulls fresh data from GitHub for all tracked orgs, users, repos, and activity events.
- Fetched data is written to a **local persistent store**. (The choice of store is deferred to the design doc.)
- The web/report layer reads **only** from the local store. It never blocks on GitHub.
- The job is **idempotent and resumable**: a failed or interrupted run can re-run safely without duplicating events or corrupting counts.
- The job supports **manual trigger** ("refresh now") in addition to the schedule, for when someone needs current numbers without waiting for the next tick.

### Implications

- **Freshness is bounded by the schedule.** With a 4-hour cadence, reports are at most ~4 hours behind GitHub. The schedule is configurable per deployment — a team that wants hourly can dial it down; a team with tight rate limits can dial it up.
- **Page-load performance is decoupled from GitHub.** GitHub being slow, rate-limited, or down does not slow down the UI; it only delays the next refresh.
- **Rate limits are managed in one place** — the fetcher — rather than scattered across user-triggered requests.
- **Cost is predictable.** API usage is a function of schedule + org/repo count, not user traffic.

### Operational requirements

- Each scheduled run records a **run log** (started, finished, items fetched, errors, partial-failure flag).
- The UI shows users **"data as of <timestamp>"** on every report, so freshness is never hidden.
- If a run fails entirely, the prior data remains queryable and the UI flags it as stale.
- An admin can trigger a manual refresh from the UI; one is enough — concurrent runs are coalesced.

---

## 11. Success criteria

`dev-pulse` is successful when:

1. A manager can answer *"what did <person> do last week across all orgs?"* in under 30 seconds.
2. **All three org-scope lenses (§8.1) are available on every report** — single-org, all-orgs-combined, and per-org split — and users can toggle between them without re-running the query.
3. Cross-company comparison on a shared org is a single report, not a manual exercise.
4. Reports are trusted — sampled numbers match GitHub's own UI within a documented tolerance, with attribution edge-cases (§6) handled correctly, and de-duplication in the "all orgs combined" view is correct and clearly labelled.
5. Every report follows a consistent shape — **headline sentence + table + trend** — so users learn one interaction, not one per report.
6. **Page loads do not call GitHub.** All report data comes from the local store populated by the scheduled fetcher (§10).
7. **Data freshness is visible.** Every report shows "data as of <timestamp>" so users always know how current the numbers are.
8. No deployment ships in a jurisdiction without the legal-review sign-off from §9.

---

## 12. Open questions

- **Authentication model** — single PAT, GitHub App, or per-user OAuth? (GitHub App is the likely answer for multi-org scale, but needs confirming against rate limits and install model.)
- **Hosting** — self-hosted only, or SaaS later? Affects data residency / GDPR posture.
- **Data retention** — how far back do we backfill activity, and how long do we keep it? Interacts with §9 erasure obligations.
- **Stretch home-org inference** — when is email-domain or GitHub-team-based inference reliable enough to offer alongside manual mapping?
- **Initial backfill strategy** — first run of the fetcher needs to seed historical data; how far back, and how is the backfill paced against rate limits?

---

## 13. Assumptions to validate (before design doc)

- **Refresh cadence default** — working assumption: **every 4 hours**, configurable per deployment (§10). Reports are at most ~4 hours behind GitHub. Tighter cadences (hourly) are supported but trade off against rate-limit headroom.
- **Scale** — working assumption: **up to ~20 orgs, ~500 users, ~1000 repos, ~10k events/day** for the initial deployment. Comfortable for a scheduled-fetcher + local-store architecture. If the target is materially larger, the fetcher may need sharding.
- **Comparison baseline** — products in the adjacent space: LinearB, Swarmia, Haystack, Code Climate Velocity, Pluralsight Flow. The "why not buy" answer is **cross-company comparison on a shared org** (§3 goal 3) — none of those tools model a single org being shared by two companies with separate home-org identity. Worth a one-pager confirming this before committing to build.

---

## 14. Out of scope for this document

- Tech stack, choice of local store, API approach (REST vs GraphQL), job runner implementation — deferred to design doc.
- UI / UX wireframes — deferred.
- Pricing / packaging — deferred.

---

## 15. Decisions

Locked decisions with revisit triggers. Anything not listed here is
still an open question (§12). Phase 0 decisions in TODO §0 are
locked inputs and are not re-opened here — this section captures
decisions made during/for later phases.

### 15.1 Auth to GitHub: **GitHub App** (not PAT)

- **Decision (Phase 2):** dev-pulse authenticates to GitHub as a
  **GitHub App** with per-org installation, exchanging the App's
  private key (JWT) for a short-lived **installation access token**
  consumed by `octocrab`. PAT is rejected.
- **Why:**
  - Webhook delivery requires an App; PATs cannot register the
    org-wide event subscriptions that TODO §0.1 depends on.
  - Per-installation rate-limit bucket (5000 req/h **per org
    install**) scales with org count; a single PAT shares one
    bucket across all orgs and breaks SCOPE §13 scale.
  - Per-org install model means an admin in each org consents
    explicitly — cleaner audit story for SCOPE §9.
  - Permission scope can grow into write access (SCOPE §4.1
    issues CRUD) without re-onboarding orgs.
- **Revisit if:** GitHub changes App rate-limit policy, or a target
  deployment refuses to install Apps and demands PAT-only operation
  (then re-open as a deployment-mode flag, not a default).
- **Resolves:** TODO §6 (working assumption → confirmed), SCOPE §12
  authentication-model question (for *fetcher* auth; operator login
  is a separate question still open).

### 15.2 Backfill window default: **90 days**

- **Decision (Phase 2):** one-shot per-org backfill at install time
  covers the trailing **90 days** of activity, configurable via
  `starter-config` (`backfill.window_days`). Paced against the
  per-install rate-limit bucket.
- **Why:**
  - 90 days covers the longest report window the v1 UI surfaces
    ("last quarter") plus a margin for trend comparisons (§8.1).
  - Bounded window keeps install-time cost predictable; a fresh
    install of an org with 1000 repos completes inside the
    per-install bucket without throttling user-facing fetches.
  - SCOPE §9 erasure obligations get easier the less historical
    data we hold by default.
- **Revisit triggers:**
  - First target deployment requests deeper history → bump default
    and document rate-limit impact.
  - Trend reports (§3 secondary) need >90d of baseline → re-open.
  - Storage cost on the first production deployment exceeds budget
    → re-open downward.
- **Resolves:** TODO §6 (revisit after first target deployment) and
  SCOPE §12 backfill-strategy question.

### 15.3 Webhook HMAC secret: stored in `starter-secrets-file`, rotated with overlap

- **Decision (Phase 2):**
  - Webhook HMAC secret is stored under
    `secrets://github/webhook_hmac` in `starter-secrets-file`
    (age-encrypted), alongside the GitHub App private key and
    installation IDs.
  - **Rotation path:** the secrets file holds **up to two** valid
    secrets at any time — `current` and `previous`. The webhook
    receiver validates an incoming signature against `current`
    first; on mismatch it falls back to `previous` and logs a
    `webhook.hmac.rotated_fallback` metric. After GitHub's App
    settings have been updated to the new secret and metrics show
    zero `previous` hits for a full reconciler cycle (4h, §0.3),
    `previous` is removed.
  - **Replay safety across rotation:** `webhook_inbox.delivery_id`
    uniqueness is independent of which secret signed the request,
    so already-enqueued deliveries replay safely across a cutover.
  - **Fail-closed:** an incoming signature that matches **neither**
    `current` nor `previous` returns HTTP 401 and is **not**
    enqueued. There is no "accept-on-unknown-secret" mode.
- **Why:** rotation without downtime is a v1 requirement (legal
  asks for compromised-secret response within hours), and
  webhook deliveries in flight at cutover must not be lost — the
  overlap window covers them.
- **Revisit if:** secrets backend changes (hosted KMS for SaaS), or
  GitHub adds native key-id support to webhook signatures (then
  the overlap window collapses to a header-driven dispatch).

### 15.4 Octocrab rate-limit headroom: **pause when remaining < 100**

- **Decision (Phase 2):**
  - All octocrab calls go through a single client wrapper in
    `dp-fetcher` that tracks `X-RateLimit-Remaining` and
    `X-RateLimit-Reset` for **both** the primary REST bucket and
    the secondary (search / abuse-detection) bucket.
  - When `remaining < 100` on either bucket, the wrapper **pauses
    all outbound calls until that bucket's reset timestamp**,
    logging `ratelimit.paused{bucket}` and recording the pause in
    `fetch_runs.errors` (non-fatal, partial=true).
  - Webhook ingest is **unaffected** — it does not call GitHub. The
    pause only gates reconciler + backfill paths.
  - The threshold is exposed in `starter-config` as
    `github.ratelimit.min_remaining` (default `100`) so the first
    production deployment can tune without a rebuild.
  - On HTTP 429 / `Retry-After`, the wrapper honours the header
    even if `remaining` was above the threshold (defensive against
    secondary-limit surprises).
- **Why:**
  - 100-request headroom on a 5000/h bucket leaves room for
    user-triggered `fetch-now` (SCOPE §10) and webhook-replay
    catch-up without starving the reconciler.
  - Splitting primary vs secondary bucket avoids the common failure
    where search calls exhaust the secondary bucket while primary
    looks healthy.
- **Revisit if:**
  - First load test (TODO Phase 1) shows reconciler starvation →
    raise threshold.
  - GitHub introduces a third bucket or changes header names →
    update wrapper and re-pin.
  - SaaS deployment runs many installs through one wrapper instance
    → the threshold becomes per-installation, not global.

### 15.5 Phase 0 decisions (read-only inputs, not re-opened here)

TODO §0.1–§0.6 are settled and locked as inputs to Phase 2:

- §0.1 webhooks-primary + reconciler + bounded backfill —
  the architecture this phase implements.
- §0.2 multi-actor `event_actors` split — the schema this
  phase writes against.
- §0.3 per-`(org, repo, resource_kind)` cursors with etag —
  the cursor model the reconciler uses.
- §0.4 UTC + window contract — irrelevant to ingest; relevant to
  Phase 3.
- §0.5 soft-delete + pseudonymisation — the fetcher respects
  `users.deleted_at` (does not resurrect pseudonymised users on
  webhook receipt — see Phase 2 worker implementation note).
- §0.6 boundary rule + `scripts/check-boundaries.sh` — enforced
  in CI; this phase adds no `starter_*` imports to
  `dp-fetcher`.

### 15.6 Report envelope shape (Phase 3, locked for v1)

- **Decision (Phase 3):** every report in `dp-reports` accepts
  exactly one input envelope, used verbatim by the Phase 4 REST
  handlers and the Phase 5 MCP tool schemas so the three surfaces
  never drift:

  ```rust
  pub struct ReportEnvelope {
      pub orgs:           Vec<OrgId>,        // empty = all visible to caller
      pub users:          Vec<UserId>,       // empty = no user filter
      pub teams:          Vec<TeamId>,       // empty = no team filter
      pub window:         Window,            // §0.4: {label, tz, anchor}
      pub scope_mode:     ScopeMode,         // SingleOrg | AllOrgsCombined | PerOrgSplit
      pub group_by:       Vec<GroupBy>,      // User | Team | Repo | HomeOrg | Org
      pub activity_types: Vec<ActivityType>, // empty = all four §6 categories
      pub actor_roles:    Vec<ActorRole>,    // empty = role filter from metric default
  }
  ```

  - `Window` is the §0.4 contract `{label, tz, anchor}`; the
    resolved UTC `(start, end)` is **computed server-side** and
    echoed in the response (§0.4 + Phase 3 task list).
  - `scope_mode` drives the three SCOPE §8.1 lenses; the de-dup
    rule for `AllOrgsCombined` is `(user_id, event_id)` per §0.2.
  - `group_by` is ordered — the first dimension is the row key,
    the rest are sub-keys for nested rendering.
  - `actor_roles` overrides the per-metric default mapping in
    §15.7 when the caller wants a non-default lens (e.g. "PRs I
    *authored or co-authored*" vs the default "PRs authored").
- **Why locked now:** Phase 4 handlers (utoipa schemas) and Phase
  5 MCP `Tool::input_schema` both serialise this struct. Changing
  field names or shape after Phase 3 lands forces a coordinated
  three-surface migration and breaks any persisted saved-report
  URLs the frontend issues.
- **Revisit triggers:**
  - A use-case in SCOPE §7 needs a dimension this envelope cannot
    express (e.g. filter by label, by branch) → extend with a new
    *additive* optional field, never repurpose an existing one.
  - The MCP surface (Phase 5) discovers an agent ergonomics issue
    that requires a flatter schema → re-open with the Phase 4
    REST shape pinned, MCP gets a thin adapter.
- **Resolves:** Phase 3 task list line 1 ("every report accepts
  the same envelope").

### 15.7 Role → metric mapping (one filter per metric, no overlap)

- **Decision (Phase 3):** every count-style metric has **exactly
  one** `actor_roles` filter against `event_actors.role` (§0.2).
  No metric sums two roles by default; callers who want a union
  pass `actor_roles` explicitly in the envelope (§15.6).

  | Metric                       | `activity_events.kind`              | Default `actor_roles` filter      |
  |------------------------------|-------------------------------------|-----------------------------------|
  | commits authored             | `push.commit`                       | `role IN (author, co_author)`     |
  | commits committed (squash)   | `push.commit`                       | `role = committer`                |
  | PRs opened                   | `pull_request.opened`               | `role = author`                   |
  | PRs merged                   | `pull_request.merged`               | `role = merger`                   |
  | PRs closed (unmerged)        | `pull_request.closed`               | `role = closer`                   |
  | PRs reviewed                 | `pull_request_review`               | `role = reviewer`                 |
  | PR review comments           | `pull_request_review_comment`       | `role = commenter`                |
  | issues opened                | `issues.opened`                     | `role = author`                   |
  | issues closed                | `issues.closed`                     | `role = closer`                   |
  | issues commented             | `issue_comment`                     | `role = commenter`                |
  | issues assigned              | `issues.assigned`                   | `role = assignee`                 |
  | review requests received     | `pull_request_review_requested`     | `role = requester`                |
  | workflow runs triggered      | `workflow_run`                      | `role = author`                   |
  | deployments cut              | `deployment`                        | `role = author`                   |
  | releases cut                 | `release`                           | `role = author`                   |

  - `commits authored` unions `author + co_author` because SCOPE
    §6 mandates co-author credit; this is the *only* default-union
    metric and it is called out in the response field-doc.
  - Bot users (SCOPE §6 caveat) are filtered at the
    `users.is_bot = false` predicate, **not** in this role map —
    the role attribution is the same for humans and bots; the UI
    suppression is a separate step.
  - Unattributed events (`event_actors.user_id IS NULL`) still
    count in totals but never group into a per-user row.
- **Why locked now:** the role union for the same logical metric
  shifting between Phase 3 (reports) and Phase 5 (MCP) would
  cause silently divergent numbers across surfaces — a direct
  violation of SCOPE §11.4 trust.
- **Revisit triggers:**
  - GitHub adds a new event type (e.g. discussion answers) → add
    a row, do not edit an existing one.
  - A target deployment requests "PRs I touched" (union of
    author + reviewer + commenter) → that is a *new metric*, not
    a redefinition of an existing row.
- **Resolves:** Phase 3 task list line "counts for events
  (filtered by `actor_roles`)".

### 15.8 Trend bucket granularity (window-length driven)

- **Decision (Phase 3):** the trend chart's bucket size is a pure
  function of the resolved UTC window length, picked server-side
  so every surface (REST, MCP, frontend) renders identical
  buckets for the same envelope:

  | Window length (UTC days) | Bucket  | Postgres truncation                     |
  |--------------------------|---------|-----------------------------------------|
  | ≤ 31                     | day     | `date_trunc('day',  ts AT TIME ZONE tz)`|
  | 32 – 183                 | week    | `date_trunc('week', ts AT TIME ZONE tz)`|
  | > 183                    | month   | `date_trunc('month',ts AT TIME ZONE tz)`|

  - Truncation is performed in the **window TZ** (§0.4
    `Window.tz`), then the bucket-start is converted back to UTC
    for the response. This makes "week starting Monday" mean
    Monday-in-Berlin for a Berlin viewer and Monday-in-UTC for
    `anchor = utc`.
  - The response carries the resolved bucket size as
    `trend.bucket ∈ "day" | "week" | "month"` so the frontend
    labels axes without re-deriving the rule.
  - Empty buckets are emitted as zero-count rows so the chart has
    no gaps; the frontend never has to infer missing periods.
- **Why locked now:** without a fixed mapping, two callers
  hitting the same envelope on different surfaces can get charts
  with different bucket sizes — confusing and a SCOPE §11.4 trust
  hit.
- **Revisit triggers:**
  - SCOPE §7 exec audiences request "trailing 18 months as
    weeks" → add an optional envelope override
    (`trend_bucket: Option<TrendBucket>`); the default stays the
    table above.
  - Performance: monthly aggregation over 5y of events on the
    first production deployment is too slow → consider a
    materialised `event_actor_facts` pre-aggregate (already
    flagged in Phase 1 §6 of TODO).
- **Resolves:** Phase 3 task list (trend implied by SCOPE §11.5
  "headline + table + trend") and the Phase 7 frontend's chart
  contract.

### 15.9 Percentile semantics (`percentile_cont`, NULL when n < 5)

- **Decision (Phase 3):** duration-style metrics (review
  turnaround, time-to-first-review, time-to-merge, lead time) are
  aggregated with Postgres `percentile_cont(p) WITHIN GROUP
  (ORDER BY duration_seconds)` for `p ∈ {0.50, 0.90, 0.95}`. The
  arithmetic mean is **not** computed (SCOPE §8 "Means are not
  used for these (long-tail distortion)").

  - **Sample size floor:** when the per-row sample count is
    `< 5`, all three percentile fields are serialised as `null`
    (not zero, not omitted) and the response carries the actual
    `sample_n` so the UI can render "—" / "n too small" instead
    of a noisy single-data-point "p95 = 12h".
  - **Continuous, not discrete:** `percentile_cont` interpolates
    between two ranked values; `percentile_disc` returns an
    actual observed value. We pick `_cont` because the duration
    distribution is continuous (seconds) and interpolation gives
    a smoother trend across small samples once they cross the
    n ≥ 5 floor.
  - **Units:** durations are stored and returned in **seconds
    (int)**; rendering to "1h 23m" is a frontend concern.
  - The same n < 5 rule applies bucket-wise in trend charts —
    each bucket independently nulls out its percentile triple
    when sparse.
- **Why locked now:** showing a "p95 = 12h" from a single
  observation has burned every prior dev-analytics product on
  the comparison list (SCOPE §13). Decide once, surface
  consistently across REST/MCP/frontend.
- **Revisit triggers:**
  - A target deployment with very small teams (n < 5 per week is
    normal) asks for a lower floor → expose
    `percentile.min_sample_n` in `starter-config`; default stays
    5.
  - Postgres performance on `percentile_cont` over wide windows
    degrades → consider `percentile_disc` or pre-aggregation.
- **Resolves:** Phase 3 task list "p50/p90/p95 for durations
  (`percentile_cont`). No means."

### 15.10 Operator login: GitHub OAuth via `starter-auth-oauth` (Phase 4)

- **Decision (Phase 4):** Operators authenticate via GitHub OAuth
  using `starter-auth-oauth` with the GitHub provider.
  First-callback auto-provisions the `users` row and mints the
  standard `sas_sid` + `starter_csrf` session. Local
  email+password signup stays `SIGNUP_MODE=disabled`; the
  CLI-seeded admin from `starter-auth-users::admin::create-admin`
  is the break-glass path.
- **`github_orgs` stamping:** a post-callback wrapper in
  `dp-server::auth` calls `GET /user/orgs` via the Phase 2
  octocrab client wrapper, writes the org login list into
  `Principal.extra.oauth.github_orgs`. Cached on the session row;
  refreshed lazily per `auth.github.org_refresh_interval` (default
  1h in `dp-config`). Never on the request hot path.
- **Why:** `starter-auth-oauth` is the composition-rule-compliant
  shape (no bespoke OAuth, no starter edit). GitHub OAuth surfaces
  the org membership needed for the authz gate (§15.11) without
  org-admin install permissions.
- **Revisit if:** air-gapped deployment blocks GitHub OAuth → layer
  a second provider (OIDC). Or `starter-auth-oauth` gains a
  `post_provision_hook` → remove the wrapper.
- **Resolves:** SCOPE §12 "authentication model" for *operator*
  login; TODO §6 open question (auth choice).

### 15.11 Access gate: `starter-authz` allow-list on `oauth.github_orgs` (Phase 4)

- **Decision (Phase 4):** access to every protected route is gated
  by `starter-authz` (`StaticRbacEngine`) loaded from
  `crates/dp-server/policy/dev-pulse.toml`. One allow rule:
  `condition = "oauth.github_orgs intersects
  auth.github.allow_orgs"` over `resource = "*"`, `actions =
  ["*"]`. `default_policy = true` (built-in role defaults).
- **allow-list:** `auth.github.allow_orgs: Vec<String>` in
  `dp-config`. Adding an org is a config edit.
- **Out-of-org:** user row + session exist (provisioned on
  callback) but `require_permission(...)` returns `403
  awaiting_access`. `auth.denied_org` audit row written.
- **Why:** centralised policy, not per-handler if-checks. The
  allow-list in config keeps the policy file stable across
  deployments.
- **Revisit if:** per-user overrides needed → add deny rules per
  principal; UI-editable policies → swap for `DbPolicyEngine`.

### 15.12 `with_principal` + `require_permission` boundary (Phase 4)

- **Decision (Phase 4):** every route is protected by both
  `with_principal` (authn) and `require_permission` (authz) except:
  (a) `POST /webhooks/github` (HMAC), (b) OAuth login/callback
  routes from `starter-auth-oauth`, (c) session routes from
  `starter-auth-users`.
- **Protected-path array:** one list in `dp-server::build`:
  `&["/reports/*", "/users", "/orgs", "/teams", "/home-org",
  "/admin/*"]`. Smoke test catches drift.
- **Revisit if:** a public health endpoint is needed → add
  exclusion with a SCOPE update.

### 15.13 Audit action vocabulary, v1 pinned (Phase 4)

- **Decision (Phase 4):** `audit_log.action` is a `const` enum in
  `dp-rest::audit`:
  `report.read`, `home_org.set`, `admin.refresh`,
  `user.anonymise`, `user.export`, `runs.list`,
  `auth.signed_in`, `auth.denied_org`.
- **Writer:** one helper `dp_rest::audit::record()`. No second
  writer. New verbs extend the enum (code change, not config).
- **Revisit if:** Phase 5 MCP needs a distinct action → extend
  the enum.

### 15.14 One `DevPulseApi` OpenAPI document (Phase 4)

- **Decision (Phase 4):** one `#[derive(OpenApi)] DevPulseApi` in
  `dp-rest::openapi` aggregates every handler plus `#[utoipa::path]`
  shims for the starter-crate OAuth/session routes. Snapshot test
  pins `tests/openapi.snapshot.json`.
- **Why:** per consumer-rules §6.7, one doc, one client. Per-module
  splits fragment Phase 7 TS generation and Phase 5 MCP schemas.
- **Revisit if:** starter crates add native utoipa annotations →
  remove the shims.

### 15.15 GitHub App default permission set: `issues: write` (SCOPE-PROJECTS §13.6)

- **Decision (projects-issues stage 8):** the GitHub App's default
  permission set declares `issues: write` whenever the
  `dp-config` flag `github.app.request_issues_write` is `true`
  (its default in new deployments). Setting the flag to `false`
  hard-disables the SCOPE-PROJECTS §8 issue mutation surface and
  drops `issues` from the App manifest — the documented escape
  hatch for deployments whose security policy forbids any App
  with write scope.
- **Why:** SCOPE.md §15.1 already named "permission scope can
  grow into write access … without re-onboarding orgs" as a
  property of the per-org App install model. SCOPE-PROJECTS §13.6
  ratifies this for v1; stage 8 wires the toggle and the §8.4
  "writes not available" 403 path so an org whose admin
  consented read-only sees a deterministic refusal (code
  `writes_not_available_for_org`) with the per-install
  `manage_url` deep-link, not a 500.
- **Migration (§13.6 banner).** `GET /me/app-install-banner`
  returns one row per org the viewer is in, each marked
  `writes_available: bool` with a copy-able admin-text snippet
  the viewer can paste into Slack / email. The banner is the
  one-shot prompt; the §8.4 affordance on individual issues is
  the steady-state fallback. Both surfaces share the same
  `dp_rest::require_issues_write` verdict so they cannot disagree.
- **Revisit if:** a deployment needs a finer per-permission
  toggle (e.g. some orgs writable, others not) — at which point
  the per-org `OrgAppInstall.permissions` snapshot is already
  the system of record and the §13.6 banner would surface the
  finer breakdown without a SCOPE change.
