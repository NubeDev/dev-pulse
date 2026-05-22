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

#### 4.1.1 Local-only issues

The Add-issue → **Create new** dialog on a project exposes two sibling buttons:

- **Create** — inserts a row directly into `dp_issues` with `is_local = TRUE` and a synthetic per-repo negative `number` (allocated from `dp_repos.local_issue_counter`). No GitHub call, no `issues: write` requirement; works on read-only org installations. The row appears in the project's issue list with a `local` chip in place of the repo badge and a dashed amber border so it reads as "note, not GitHub issue" at a glance.
- **Create and sync to GitHub** — the historical behaviour: backend POSTs to GitHub, mirrors the response into `dp_issues`, attaches to the project.

Local-only rows participate in project / view / tag membership, the inbox, and the audit log identically to GitHub-backed rows. They never appear on github.com. A future "Sync to GitHub" action on the issue detail pane will promote a local row in place (UPDATE `is_local = FALSE` and rewrite `number` / `github_id` / `github_node_id` from the GitHub create response) so every membership / tag / inbox-state row keyed off `dp_issues.id` survives the promotion.

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

### 8.2 Leaderboard report kind (cross-cutting primitive)

Every report shape locked elsewhere in §8 ranks **events through
time** for a fixed subject. The **leaderboard** is the orthogonal
shape: it ranks **subjects** against each other for a fixed §15.7
metric, under the same three §8.1 org-scope modes. It is the
cross-cutting primitive — the same query scales from one user to
one team to one org to all orgs without changing surface, envelope,
or aggregation rules. Compare-users is a thin client of it (§15.15
decision 10), not a separate backend mode.

The decisions that pin the leaderboard's semantics — tie-break,
reconciliation, pagination, bot suppression, NULL handling,
`home_org_label` aggregation, and the rejection of composite
scores — live in **§15.15**. This section pins the shape only.

#### Subject axis

```
SubjectKind = user | team | org | home_org_label
```

- `user` — one row per `users.id` (post-bot-filter, §15.15
  decision 4).
- `team` — one row per `teams.id`. Org-scoped by definition; in
  "all orgs combined" mode this kind is **invalid** and the API
  rejects the envelope rather than producing nonsense.
- `org` — one row per `orgs.id`. Only meaningful in
  "all orgs combined" or "per-org split" modes.
- `home_org_label` — one row per distinct value of
  `users.home_org_label` (§3 goal 3). NULL labels bucket into a
  synthetic `__unlabeled__` row (§15.15 decision 8).

#### Envelope

Extends §15.6 with the fields specific to ranking subjects. No
existing §15.6 field changes meaning, so an existing report URL
can be pivoted into a leaderboard by adding two query params.

```rust
pub struct LeaderboardEnvelope {
    // inherited from §15.6, unchanged
    pub window:        Window,
    pub org_scope:     ScopeMode,        // §8.1 three modes
    pub repos:         Option<Vec<RepoId>>,
    pub teams:         Option<Vec<TeamId>>,
    pub actor_roles:   Option<Vec<ActorRole>>,
    pub tz:            Tz,
    // leaderboard-specific
    pub subject:       SubjectKind,
    pub rank_by:       MetricId,             // exactly one §15.7 row
    pub also_compute:  Option<Vec<MetricId>>, // §15.15 dec. 3, cap 5
    pub subject_ids:   Option<Vec<SubjectId>>,// §15.15 dec. 10, cap 50
    pub include_bots:  bool,                  // default false
    pub page:          PageRequest,           // §15.15 dec. 5
}
```

- `rank_by` is exactly one §15.7 metric. Server-side sort and
  pagination are *only* on `rank_by` — composite scores are
  rejected (§15.15 decision 7).
- `also_compute` carries additional §15.7 metrics into each row's
  `context` block so the UI can re-sort the visible page without
  a second request. It does not change rank order or pagination.
- `subject_ids` filters *before* ranking; the resulting ranks are
  within the filtered set, not the global one. In this mode
  pagination is disabled (§15.15 decision 10).

#### Response

Mirrors §15.6's headline+table+trend triple with the table re-typed
as a ranked list:

```jsonc
{
  "envelope": {
    /* request echo, with the window resolved to absolute UTC
       timestamps. Identical input + identical resolved_at must
       produce identical output (§15.15 decision 5). */
    "resolved_at":     "2026-05-20T09:00:00Z",
    "resolved_window": { "from": "...", "to": "..." }
  },
  "headline": { "total_subjects": 42, "events_total": 1287, ... },
  "rows": [
    {
      "rank":         1,
      "subject_id":   "...",
      "subject_kind": "user",
      "subject_label":"alice",
      "subject_org":  "...",     // only in per-org-split
      "primary":      { "metric": "prs_merged", "value": 23 },
      "context":      {
        "active_days": 14, "repos_touched": 6,
        "reviews_given": { "value": 41 },
        "pr_cycle_time_hours_p50": { "value": 19.4, "n": 23 }
      },
      "sparkline":    [ /* per-bucket counts, §15.8 */ ],
      "active_orgs":  3
    }
  ],
  "footer": {
    "unattributed_events":        17,   // §15.15 decision 2
    "unattributed_events_metric": 11,   // §15.15 decision 2
    "insufficient_data":          4,    // §15.15 decision 6
    "bots_suppressed":            2,    // §15.15 decision 4
    "bots_suppressed_events":     38    // §15.15 decision 4
  },
  "page": { "next_cursor": "...", "has_more": true }
}
```

#### Org-scope interaction (the trap to avoid)

Each §8.1 mode produces a different leaderboard. The UI must label
the mode explicitly so users never compare results across modes by
accident.

| Mode               | What "rank" means                                                  | Row identity                                                |
|--------------------|--------------------------------------------------------------------|-------------------------------------------------------------|
| **single-org**     | Rank within one codebase.                                          | `subject`                                                   |
| **all-orgs-combined** | Rank by cross-org total, de-duplicated.                         | `subject` (one row even if active in N orgs)                |
| **per-org-split**  | Rank by `(subject × org)` pair — surfaces context-switching.       | `(subject_id, subject_org)` — only mode where rows repeat   |

`per-org-split` is the only mode where a single user can appear
multiple times and the only mode where `rows[].subject_org` is
populated. The frontend must visually group those rows together
(grouped table, not a flat list) or §8.1's "spread thin" insight
is lost in the sort order.

#### Endpoint shape — note

§15.15 decisions 9 and 10 together imply **two endpoints, not one**:
the manager/admin-scoped `leaderboard` endpoint described above,
plus the IC-self-view `my_standing` endpoint (decision 9) which
reuses the same SQL primitives behind a separate envelope so
`total_subjects` and page boundaries cannot leak distributional
information about colleagues. This split is load-bearing — see
§15.15 for the rationale.

#### Reconciliation with §4 non-goals

§4 forbids "rank all users by X" as a UI affordance. The
leaderboard endpoint is **not** that affordance: it is the backend
primitive that powers manager team-distribution views (§7), exec
contribution-split views (§7), and the IC `my_standing` view
(§15.15 decision 9). The UI rules from §4 still apply — no
single-number "developer score", no surfaced "rank all users"
button — and §15.15 decision 7 makes the composite-score
prohibition a backend invariant, not a UI policy.

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

### 15.16 Project grouping is home-grown tags, not GitHub Projects v2 (workflow surface)

- **Decision (workflow surface, ≡ SCOPE-PROJECTS §13.1):**
  dev-pulse's project-grouping primitive is the `tags` +
  `tag_links` schema described in §17.2. GitHub Projects v2 is
  **not** the system of record.
- **Why:** cross-org by construction (Projects is org-scoped),
  polymorphic over repos / issues / users / teams (Projects is
  issue / PR / draft-only), one storage backend, no extra GitHub
  App scope, fully owned schema. Detailed comparison in §17.1.
- **Revisit if:** the first target deployment is GitHub Enterprise-
  only AND already uses Projects v2 heavily AND refuses to maintain
  a parallel tagging structure — then promote the one-way Projects
  import goal (§16 secondary) to v1 and treat Projects as a
  read-only source for tag links. The system of record stays
  dev-pulse either way.
- **Resolves:** SCOPE-PROJECTS §13.1 promotion into this document.

### 15.17 Tag links are polymorphic across four kinds (workflow surface)

- **Decision (workflow surface, ≡ SCOPE-PROJECTS §13.2):**
  `tag_links.kind ∈ {repo, issue, user, team}` — no more, no less
  for v1.
- **Why:** these four cover every "what is this project made of?"
  question the §17 use-cases need. PR-level tagging is tempting
  but PRs are tightly coupled to their repo — filtering by repo
  gets you the PRs anyway, without a fifth polymorphism arm to
  carry.
- **Revisit if:** a use-case appears that genuinely needs PR-level
  tagging independent of repo (e.g. "tag the long-lived release
  PRs across all repos in a project"). Add as a fifth kind;
  existing rows untouched.
- **Resolves:** SCOPE-PROJECTS §13.2 promotion into this document.

### 15.18 Issue writes are synchronous, user-initiated only; MCP mutations out for v1

- **Decision (workflow surface, ≡ SCOPE-PROJECTS §13.3):** the only
  path from dev-pulse to a GitHub write is a `dp-rest` handler
  responding to a user request. The fetcher never writes.
  Background jobs never write. **The MCP surface (§15.14, Phase 5)
  is read-only for v1.** Exposing mutations over MCP requires a
  principal model that MCP clients (API token / delegated OAuth,
  not a session cookie) can satisfy with the same audit guarantees
  as the REST path — that design is its own scope item, not a
  Phase 5 task.
- **Why:** keeps the audit story clean (every write has an actor
  whose authority and identity we can prove), keeps blast radius
  bounded (a fetcher bug cannot mutate GitHub), keeps rate-limit
  accounting simple (write budget is user-traffic-shaped, not
  schedule-shaped).
- **Revisit if:** (a) a use-case appears that genuinely needs a
  scheduled mutation (e.g. "auto-close stale issues after 90d") —
  treat it as a new feature with its own scope review; do not
  retrofit the fetcher; or (b) Phase 5 MCP picks up enough traction
  that delegated-mutation becomes a real ask — open a follow-up
  scope doc covering the principal model first.
- **Resolves:** SCOPE-PROJECTS §13.3 promotion into this document.

### 15.19 Optimistic local writes with reconciler-backed truth

- **Decision (workflow surface, ≡ SCOPE-PROJECTS §13.4):** the §18.2
  write path applies the mutation to the local store *before* the
  GitHub call returns; on failure it rolls forward (re-applies
  pre-mutation values, bumps `version`). Optimistic concurrency
  uses a local `issues.version` int as the CAS token; **no DB
  row-lock is held across the GitHub round-trip**. The fetcher /
  webhook reconciler is the final source of truth and may overwrite
  the optimistic row subject to §15.21.
- **Why:** UI responsiveness — a closed issue should look closed
  immediately, not after a 600ms round-trip. CAS-on-version
  preserves correctness without exposing connection-pool slots to
  upstream stalls.
- **Revisit if:** reconciler-vs-optimistic drift becomes a
  user-visible bug pattern (e.g. flickering state). The
  alternative is pessimistic-write (wait for GitHub before
  updating the local row), which is simpler but slower.
- **Resolves:** SCOPE-PROJECTS §13.4 promotion into this document.

### 15.20 Pin cap, sidebar render cap, and tag scope cap (workflow surface)

- **Decision (workflow surface, ≡ SCOPE-PROJECTS §13.5, working
  assumption, soft-locked):**
  - **20 pins per user** (data-model cap; §16.1).
  - **50 rendered sidebar entries per user** after tag expansion
    (UI cap; overflow collapses into "…and N more").
  - **No hard cap on tag-links per tag**, but a warning surfaces
    above **500 links on one tag** (signal of misuse — the user
    probably wants two tags). Check fires on insert; the response
    carries the warning, the operation still commits.
  - **50-tag cap on `tags` filter when `GroupBy::Tag` is requested**
    (§17.7).
- **Why:** data-model cap protects writes; render cap protects the
  UI; link warning protects query performance; group-by cap
  protects report cost. All four numbers are exposed in `dp-config`
  for tuning without a rebuild.
- **Revisit if:** first deployment hits any limit naturally.
- **Resolves:** SCOPE-PROJECTS §13.5 promotion into this document.

### 15.21 Reconciler defers to in-flight optimistic writes

- **Decision (workflow surface, ≡ SCOPE-PROJECTS §13.7):** the
  fetcher / webhook reconciler **must not** overwrite an `issues`
  row where `pending_remote = true` and `pending_remote_at` is
  younger than `issues.pending_remote_timeout_secs` (default 60s;
  see §18.5). Webhook payloads for such rows are **buffered, not
  applied**, until the flag clears or the timeout sweeper rolls the
  row back. After the flag clears, the next fetcher tick — or the
  buffered webhook payload, replayed — becomes authoritative.
- **Why:** without this rule, a fetcher tick that races the GitHub
  round-trip in §18.2 will write the pre-mutation state back over
  the optimistic row, the UI flickers to old values, then the next
  tick re-applies the truth. The CAS in §18.2 protects writers from
  each other; this decision protects writers from the reconciler.
- **Revisit if:** the timeout default (60s) is wrong for the first
  production deployment — too short causes spurious rollbacks under
  upstream slowness, too long delays reconciliation of
  genuinely-stuck rows. Tunable in `dp-config`.
- **Resolves:** SCOPE-PROJECTS §13.7 promotion into this document.

---

## 16. Pinned repos & tags (workflow surface)

> Promoted from SCOPE-PROJECTS.md §6. The companion doc is retained
> as design rationale (see §20); this section is the normative one.

### Goals

Per-user pinned repos and tags — a small, ordered list the user
curates themselves, surfaced as the default filter on the
issue-management views (§18) and as a sidebar quick-list. Pins are
**per-user UI state**, not a reports dimension; they live only in
the dev-pulse DB, gated by §15.11.

### 16.1 Behaviour

- A user can pin any **repo** they can see (per the §15.11 access
  gate) and any **tag** that is visible to them (per §17.4).
- Pins are **ordered** — the user controls position via drag /
  reorder. Newly-added pins go to the end.
- Pinning a **tag** is equivalent to pinning every repo currently
  linked to it, for the purposes of "what shows up in my sidebar
  list" — but it stays a *single* pin in the data model, so if the
  tag gains a repo tomorrow, that repo appears in the user's
  sidebar without further action. This is the headline reason to
  pin tags rather than repos.
- The **pin cap** (working assumption: 20 pins per user) protects
  the *data model*, not the rendered sidebar — a single tag pin
  can expand to many entries. A separate **render cap** (working
  assumption: 50 entries) governs the sidebar; above it the
  overflow collapses into a "…and N more" disclosure that opens
  the full list. Both caps live in `dp-config` (§15.20).
- Over-the-cap pins are rejected with a clear error, not silently
  dropped.

### 16.2 Surfaces

- **Sidebar quick-list** on every page: pinned items in the user's
  chosen order.
- **Default repo filter** on the Issues view and the Workbench
  dashboard.
- Pins are **not** a report dimension — they are personal UI
  state. They do not appear in the §15.6 envelope and do not affect
  anyone else's view.

### 16.3 Storage

```
user_pins(
  user_id     UUID  NOT NULL,
  kind        TEXT  NOT NULL CHECK (kind IN ('repo','tag')),
  target_id   UUID  NOT NULL,
  position    INT   NOT NULL,
  pinned_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (user_id, kind, target_id)
)
-- unique (user_id, position) enforced at write time, not as a
-- DB constraint, to allow atomic reorder.
```

### 16.4 API

- `GET    /me/pins`             — ordered list with hydrated targets.
- `POST   /me/pins`             — `{ kind, target_id }`, appends.
- `DELETE /me/pins/{kind}/{id}` — remove.
- `PUT    /me/pins/order`       — full ordered id list, atomic.

### 16.5 Audit

Verbs added to the §15.13 vocabulary:
`pin.add`, `pin.remove`, `pin.reorder`.

---

## 17. Project tags — home-grown (workflow surface)

> Promoted from SCOPE-PROJECTS.md §7. The companion doc is retained
> as design rationale (see §20); this section is the normative one.

### 17.1 Why home-grown and not GitHub Projects

GitHub Projects v2 was considered and rejected as the system of
record for v1:

- **Org-scoped.** A Project belongs to one org or one user.
  dev-pulse's headline goal (§3) is *cross-org / cross-company*
  views. A grouping primitive that cannot span orgs fights the
  product.
- **GraphQL-only**, separate API surface from the REST `octocrab`
  flow in §15.4 — new client wrapper, new rate-limit bucket math,
  new error vocabulary.
- **Cannot tag users or teams** — Projects items are limited to
  Issues / PRs / draft items. We want to group people too
  ("Phoenix squad"), and Projects structurally cannot.
- **Separate App permission** (`project: write`) with a separate
  per-install consent step.
- **External state we don't own** — cache invalidation, deleted-
  project edge cases, API version churn.

Home-grown wins on all four points: cross-org natively, one storage
backend, polymorphic over four target kinds, no extra GitHub scope,
fully owned schema. We keep the **one-way Projects import** door
open as a secondary goal — read-only mirror into a tag, never the
system of record.

### 17.2 Storage

```
tags(
  id              UUID PRIMARY KEY,
  scope_kind      TEXT NOT NULL CHECK (scope_kind IN ('user','team','org')),
  -- Exactly one of the three scope_*_id columns is non-NULL,
  -- matching scope_kind. Enforced by CHECK; gives us real FKs
  -- and ON DELETE CASCADE per kind.
  scope_user_id   UUID REFERENCES users(id) ON DELETE CASCADE,
  scope_team_id   UUID REFERENCES teams(id) ON DELETE CASCADE,
  scope_org_id    UUID REFERENCES orgs(id)  ON DELETE CASCADE,
  name            TEXT NOT NULL,
  color           TEXT NOT NULL,            -- semantic name: 'indigo', 'red', ...
  description     TEXT,
  created_by      UUID NOT NULL REFERENCES users(id),
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  archived_at     TIMESTAMPTZ,
  CHECK (
    (scope_kind = 'user' AND scope_user_id IS NOT NULL
      AND scope_team_id IS NULL AND scope_org_id IS NULL) OR
    (scope_kind = 'team' AND scope_team_id IS NOT NULL
      AND scope_user_id IS NULL AND scope_org_id IS NULL) OR
    (scope_kind = 'org'  AND scope_org_id  IS NOT NULL
      AND scope_user_id IS NULL AND scope_team_id IS NULL)
  )
);
-- Case-insensitive per-scope uniqueness; expression index, not a
-- column-list UNIQUE constraint.
CREATE UNIQUE INDEX tags_scope_name_uniq
  ON tags (scope_kind, COALESCE(scope_user_id, scope_team_id, scope_org_id), lower(name));

tag_links(
  id           UUID PRIMARY KEY,
  tag_id       UUID NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  kind         TEXT NOT NULL CHECK (kind IN ('repo','issue','user','team')),
  -- Exactly one target_* column is non-NULL, matching kind.
  target_repo_id  UUID REFERENCES repos(id)  ON DELETE CASCADE,
  target_issue_id UUID REFERENCES issues(id) ON DELETE CASCADE,
  target_user_id  UUID REFERENCES users(id)  ON DELETE CASCADE,
  target_team_id  UUID REFERENCES teams(id)  ON DELETE CASCADE,
  added_by     UUID NOT NULL REFERENCES users(id),
  added_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
  CHECK (
    (kind = 'repo'  AND target_repo_id  IS NOT NULL
      AND target_issue_id IS NULL AND target_user_id IS NULL AND target_team_id IS NULL) OR
    (kind = 'issue' AND target_issue_id IS NOT NULL
      AND target_repo_id IS NULL AND target_user_id IS NULL AND target_team_id IS NULL) OR
    (kind = 'user'  AND target_user_id  IS NOT NULL
      AND target_repo_id IS NULL AND target_issue_id IS NULL AND target_team_id IS NULL) OR
    (kind = 'team'  AND target_team_id  IS NOT NULL
      AND target_repo_id IS NULL AND target_issue_id IS NULL AND target_user_id IS NULL)
  )
);
CREATE UNIQUE INDEX tag_links_tag_target_uniq
  ON tag_links (tag_id, kind,
                COALESCE(target_repo_id, target_issue_id, target_user_id, target_team_id));
```

Notes:

- `scope_kind = 'user'` is the "personal tag" case — only the owner
  can see it.
- `archived_at` is soft-delete on the tag itself. Archived tags do
  not appear in pickers but their links survive so historical
  reports filtered by the tag still resolve.
- **Soft-deleted link targets** (user pseudonymised per §0.5, repo
  removed from an install, issue deleted on GitHub) are filtered at
  query time but the `tag_links` row stays for audit. A periodic
  integrity job hard-prunes rows whose target has been gone longer
  than the §0.5 retention window.
- No global tag namespace. Two orgs can both have a tag named
  "Phoenix" without colliding (the unique index is per-scope).
- **`color`** is a semantic palette name (`indigo`, `red`, `teal`,
  …), **not** a frontend design-system token id — decouples stored
  rows from design-token churn.

### 17.3 Polymorphism — why it's the whole point

A single tag can simultaneously link:

- **repos** across multiple orgs (cross-org grouping — the §3 goal
  Projects cannot meet),
- **issues** that are not yet pulled out into a project ("anything
  blocking the Phoenix migration"),
- **users** who are working on it ("the Phoenix squad" — used as a
  shortcut filter on reports), and
- **teams** as a coarser grouping.

Reports (§15.6) carry `tags: Vec<TagId>` and `repos: Vec<RepoId>`
as additive optional fields per the §15.6 revisit rule. When
provided, the report unions the link targets into the existing
`users` / `teams` / *(implicit repos via orgs)* filters.

### 17.4 Visibility & permissions

A tag is **visible** to a user iff its scope is visible to them:

- `scope_kind = 'user'` — only the owner.
- `scope_kind = 'team'` — any user the §15.11 policy lets see the
  team.
- `scope_kind = 'org'`  — any user the §15.11 policy lets see the
  org.

A tag's **links** are **filtered at query time** to only those the
viewer can see. The viewer never sees a `tag_links` row for a repo
/ issue / user / team they have no access to — but the tag itself
is not denied. This avoids the awkward "tag exists for some
people, vanishes for others" UI failure.

**Link counts** in `GET /tags` and `GET /tags/{id}` are reported
**after** the viewer-visibility filter — i.e. the count the viewer
would see if they expanded the tag, not the true count. Reporting
the true count would leak the existence of resources the viewer has
no access to.

**Default tag scope.** New-tag UI defaults to `org` scope (when the
viewer is in exactly one visible org) or prompts the viewer to pick
(when they are in several); `user` scope is the opt-in. The product
framing is cross-org grouping for managers — defaulting to the
shared artefact prevents the "I made it but my teammate can't see
it" surprise.

**Mutation:**
- Anyone who can see a tag can **propose** a link, but only the
  tag's `scope` members can **commit** one. User-scope tags: owner
  only. Team/org-scope: any member.
- Edit (rename / recolour / archive): scope members only.
- Hard delete: never via API (only archive). DB cleanup of
  archived tags is an admin job.

### 17.5 API

- `GET    /tags`             — list visible tags, with viewer-
  filtered link counts; paginated per the §15.6 page contract.
- `POST   /tags`             — create.
- `PATCH  /tags/{id}`        — rename / recolour / archive.
- `GET    /tags/{id}`        — single tag; links are paginated
  separately (`?links_page=…`) to keep a single response bounded
  even for tags near the §15.20 500-link soft warning.
- `POST   /tags/{id}/links`  — `[{kind, target_id}, ...]`, batch.
  **Transactional all-or-nothing**: any per-item validation failure
  rejects the whole batch with a per-item error array; no partial
  commit.
- `DELETE /tags/{id}/links`  — batch unlink, same all-or-nothing
  semantics.
- `GET    /me/tags`          — convenience: tags I own or am a
  scope member of.

### 17.6 Audit

New verbs: `tag.create`, `tag.update`, `tag.archive`, `tag.link`,
`tag.unlink`. Each `tag.link` / `tag.unlink` records the
`(tag_id, kind, target_id)` tuple.

### 17.7 Tags as a reports dimension

Two additive changes to the §15.6 envelope, both governed by its
"additive, never repurpose" revisit rule:

1. **`tags: Vec<TagId>`** — new optional filter field. A tag in
   this list contributes an **additional `OR`-predicate** to the
   report's WHERE clause; it does **not** widen or redefine the
   existing `users` / `teams` / `orgs` filters. The predicate is
   the union of the tag's visible link targets resolved to the
   metric's natural attribution column (see (3) below).
2. **`repos: Vec<RepoId>`** — new optional filter field. Required
   alongside `tags` because tag links of kind `repo` cannot
   otherwise be expressed: the §15.6 envelope has no other
   repo-level filter, and silently widening to the whole org is the
   opposite of what a `repo`-linked tag asks for.
3. **`group_by: Vec<GroupBy>`** gains a `Tag` variant. Grouping by
   tag produces one row per tag in the `tags` filter; the filter is
   **required** when `GroupBy::Tag` is present, capped at the
   §15.20 working assumption of 50. "All visible tags" as a default
   is rejected — it is a query and UI footgun once a deployment
   accumulates hundreds of tags.

**Metric × link-kind mapping** (resolves "what does a tag with only
`issue` links mean for `commits authored`?"):

| Tag link kind | Contributes to                                            |
|---------------|-----------------------------------------------------------|
| `repo`        | every metric — filters on `activity_events.repo_id`.      |
| `user`        | every metric — filters on `event_actors.user_id`.         |
| `team`        | every metric — expands to team members at query time.     |
| `issue`       | **only** issue-centric metrics (issues opened / closed /  |
|               | commented / assigned per §15.7). Ignored for commit-,     |
|               | PR-, review-, and workflow-centric metrics.               |

An `issue`-linked tag with no other link kinds, queried against a
commit metric, produces an empty result with an explicit
`empty_reason = "tag links do not match metric attribution"` in the
response — not a silent zero.

**Double-counting rule:** an event counted toward tag A is **also**
counted toward tag B if both tags link the same target. Tags
overlap by design; per-tag totals do not have to sum to the overall
total. This falls out naturally from the union semantics in (1) and
is surfaced in the UI the same way as the §8.1 "all orgs combined"
de-dup note.

---

## 18. GitHub Issues CRUD (workflow surface)

> Promoted from SCOPE-PROJECTS.md §8. The companion doc is retained
> as design rationale (see §20); this section is the normative one.

### 18.1 In-scope operations (v1)

Per-issue:

- **Create** — title (required), body, labels, assignees, milestone.
  Repo selected from the viewer's accessible repos.
- **Update** — same fields as create, plus state transitions
  (close / reopen). Partial updates only — the API takes a diff,
  not the full issue.
- **Comment** — add a comment. Edit / delete of comments deferred.

Everything else (bulk ops, PRs, discussions, reactions, label /
milestone admin) is non-goal per §4 and §4.1.

### 18.2 Write path (synchronous, user-initiated)

The path is built around a **local `version: int` column** on
`issues`, monotonically bumped on every fetched update *and* every
optimistic local write. It is the optimistic-concurrency token;
nothing in this path relies on GitHub returning a 409 (the Issues
REST API does not support `If-Match` / `If-Unmodified-Since` and
does not 409 on stale state).

1. UI loads the form, captures the current `issues.version` as
   `expected_version`, and submits it back on POST.
2. UI POST → `dp-rest` handler.
3. §15.11 policy check: viewer can see the target repo.
4. **Permission check** against the per-org App installation: does
   it carry `issues: write`? If not, return
   `403 writes_not_available_for_org` with a UI-friendly message.
5. **Optimistic CAS** — in a short transaction, update the local
   row only `WHERE id = ? AND version = expected_version`, setting
   `version = version + 1`, `pending_remote = true`,
   `pending_remote_at = now()`, `pending_remote_actor = ?`. Zero
   rows updated ⇒ stale; return `409 stale_local_version` with the
   current row so the UI can reload and re-prompt. **No row-lock is
   held across the network call.**
6. **Synchronous GitHub call** via the §15.4 octocrab wrapper (same
   rate-limit guard, same retry rules).
7. On success — clear `pending_remote`, record the GitHub delivery
   id on the `IssueMutation` audit row, commit.
8. On failure — re-apply the pre-mutation field values *and* bump
   `version` again (so any concurrent reader sees a change), clear
   `pending_remote`, surface the GitHub error verbatim (422
   validation / 403 scope / 5xx upstream). Audit row recorded with
   `failed` status either way.

The scheduled fetcher (§10) is unchanged. It will re-observe the
mutation when it ticks (or sooner, via the issue's webhook) and
reconcile any drift between optimistic and authoritative state,
subject to §15.21.

### 18.3 Conflict handling

- **Stale local write (CAS miss in §18.2 step 5):** reject with
  `409 stale_local_version`, return the current row, ask the UI to
  reload and re-prompt the user.
- **Concurrent dev-pulse writers on the same issue:** resolved by
  the CAS in §18.2 step 5 — the second writer's `expected_version`
  no longer matches and they get a clean `409`. No locks held
  across the GitHub round-trip; no connection-pool exposure to
  upstream stalls.
- **GitHub-side concurrent edit between form load and submit:** the
  webhook for that edit will bump `issues.version` locally before
  the submit arrives (typical case); when it does, the CAS misses
  and the user is asked to reload. When the webhook *loses* the
  race (sub-second submit), the local row is silently overwritten —
  same last-write-wins behaviour as the GitHub web UI. Documented
  limit, not a bug.
- **Webhook arrives mid-flight** with the same change: see §15.21 —
  the reconciler does not touch a row with `pending_remote = true`
  younger than the timeout; the buffered payload (or the next
  reconciliation pass) confirms or overwrites.

### 18.4 Permissions surfaced honestly

If the per-org App install was granted **read-only**:

- The UI shows a clearly-labelled "writes not available for
  `org-x`" banner on every issue in that org's repos.
- Create / edit / comment controls are visibly disabled with hover
  text explaining why and pointing at the org admin docs.
- The API still returns `403 writes_not_available_for_org` if a
  caller bypasses the UI, with `Retry-After`-style guidance in the
  body.

No silent failures. No surprise 500s. The org-admin who scoped the
install down made a choice and the UI respects it.

### 18.5 Audit

New verbs on top of §15.13:
`issue.create`, `issue.update`, `issue.close`, `issue.reopen`,
`issue.comment`.

Every row records:

- `actor` (dev-pulse user),
- `target` (repo + issue number, plus our internal issue id),
- `diff` — JSON of mutated fields, `{ before, after }`, with
  `before` omitted on create,
- `result` — `committed` / `failed` / `pending_remote_timeout`,
- `github_delivery_id` when available,
- `error` — verbatim GitHub error for `failed` rows.

**`pending_remote_timeout`** fires when a mutation has been in
`pending_remote = true` for longer than
`issues.pending_remote_timeout_secs` (default 60s, in `dp-config`)
— i.e. the synchronous handler crashed or its request was killed
between §18.2 step 5 and step 7. A background sweeper (re-using
the reconciler's schedule) finds these rows, rolls them back to
the pre-mutation values, bumps `version`, clears the flag, and
writes the audit row with this status. The UI shows a "mutation
timed out — please retry" toast on next view. No data is held in
the pending state indefinitely.

This satisfies the §9 transparency requirement: a user can request
a full export of mutations they performed and a full export of
mutations performed *against* an issue they own.

---

## 19. Auth implications for the workflow surface

> Promoted from SCOPE-PROJECTS.md §9. The companion doc is retained
> as design rationale (see §20); this section is the normative one.

Layered on §15.1 (GitHub App) and §15.10 (operator OAuth login):

- The GitHub App's **default permission set** gains `issues: write`,
  gated by the `github.app.request_issues_write` flag per §15.15 /
  §13.6 of the companion doc. Existing read-only installs are not
  auto-upgraded — org admins re-consent when they want writes;
  GitHub's permission-change flow handles this, and the §15.15
  migration banner is the user-facing prompt.
- Operator OAuth scope is **unchanged** — write authority is
  delegated through the App install, **not** the user's OAuth
  token. This keeps personal tokens out of the write path and means
  revoking a user inside dev-pulse (§0.5) also revokes their
  ability to mutate GitHub via the tool.
- The §15.11 access gate is the only authorisation check for
  *visibility*. Mutation adds an *additional* check (§18.2 step 4)
  against the App install's scope.
- Audit (§15.13) gains the verbs listed in §17.6 and §18.5; the
  one writer (`dp_rest::audit::record()`) per §15.13 still applies.

---

## 20. Relationship to SCOPE-PROJECTS.md

[SCOPE-PROJECTS.md](SCOPE-PROJECTS.md) is **retained as design
rationale**, not deleted. Its §1–§12 narrative (vision, why
home-grown over GitHub Projects v2, conflict-handling walk-throughs,
open questions) explains the *reasoning* behind §16–§19 above and
the §15.15–§15.21 decisions. When the two documents disagree, this
file is the normative source; the companion is read as the design
diary.

The companion's §13.1–§13.7 decisions are promoted here verbatim
(modulo cross-reference numbering) as §15.16–§15.21 plus the
already-landed §15.15 — that mapping table:

| SCOPE-PROJECTS decision | Normative location in this file |
|-------------------------|---------------------------------|
| §13.1 home-grown tags   | §15.16                          |
| §13.2 polymorphic links | §15.17                          |
| §13.3 sync writes only  | §15.18                          |
| §13.4 optimistic CAS    | §15.19                          |
| §13.5 caps              | §15.20                          |
| §13.6 issues:write      | §15.15 (landed earlier)         |
| §13.7 reconciler guard  | §15.21                          |

### 15.22 Leaderboard semantics (Phase 3+)

Pins the ten decisions that make the §8.2 leaderboard report kind
behave identically across REST, MCP, and frontend. Each one is a
source of silent divergence between surfaces (a §11.4 trust
violation) if left unpinned.

**Two-endpoint shape (load-bearing):** decisions 9 and 10 together
mean the leaderboard surface ships as **two endpoints, not one**:

- The manager/admin `leaderboard` endpoint (decisions 1–8, 10).
- The IC `my_standing` endpoint (decision 9) — same SQL
  primitives, separate envelope, separate permission, headline
  computed over the visible set only.

This split must survive the §8.2 → §15.15 fold: collapsing the two
into one endpoint with a projection flag re-introduces the
`total_subjects` / page-boundary leak that decision 9 explicitly
rejects.

#### 15.15.1 Tie-break order, locked

- **Decision (Phase 3+):** `rank_by DESC → active_days DESC →
  subject_id ASC`. Deterministic across REST, MCP, and frontend.
  `subject_id` is the final tie-break because labels (`login`,
  `team.slug`) can change; ids do not.
- **Revisit if:** a new metric class makes `active_days` a poor
  secondary signal — replace it deliberately, do not silently add
  a third sort key.

#### 15.15.2 Unattributed events, with explicit reconciliation

- **Decision (Phase 3+):** events with `event_actors.user_id IS
  NULL` (§15.7) are excluded from `subject = user` rows but
  surfaced in the footer with **two** counts:
  - `unattributed_events` — total unattributed in the resolved
    window (matches the headline report).
  - `unattributed_events_metric` — unattributed events that would
    have contributed to `rank_by` if attributed.
- **Reconciliation identity (count metrics only):**

  ```
  headline.events_total
    == sum(rows[].primary.value)
     + footer.unattributed_events_metric
     + footer.bots_suppressed_events
  ```

  For duration metrics the identity is meaningless (values are
  aggregates, not counts) and the check is skipped, but
  `unattributed_events_metric` is still reported. A debug-build
  assertion enforces the identity for count metrics.
- **Scope of the identity:** `headline` and `footer` carry
  **full-result** totals, not per-page totals. The
  reconciliation identity holds across the union of all pages,
  not page-by-page — `sum(rows[].primary.value)` in the identity
  means the sum over the entire result set, computed before
  pagination is applied.
- **Revisit if:** §15.7 grows a metric class that violates both
  the count and duration semantics (e.g. ratios) → carve out a
  third exemption deliberately.

#### 15.15.3 Multi-metric rows via `also_compute`, not composite scores

- **Decision (Phase 3+):** `also_compute` carries up to **5**
  additional §15.7 metrics per row in `row.context`. Server-side
  sort and pagination remain on `rank_by` only — `also_compute`
  is a display affordance, not a sort affordance.
- **Why:** the UI can re-sort the visible page client-side
  without a second request, and the compare-users mode (decision
  10) can fetch every metric of interest in one call.
- **Page-boundary invariance:** changing `also_compute` between
  page requests must not shift which subjects appear on which
  page; tests cover this.
- **Revisit if:** the 5-metric cap is too tight for a real
  compare-users flow → raise it, but keep the cap (response size
  is a real constraint).

#### 15.15.4 Bot suppression is a filter, not a rank rule

- **Decision (Phase 3+):** `include_bots` defaults `false`. Bots
  never silently disappear from event totals (they still appear
  in the headline); they only disappear from the ranked rows.
  Two footer counts:
  - `bots_suppressed` — number of bot subjects hidden from rows.
  - `bots_suppressed_events` — events those bots contributed,
    needed by the §15.15.2 identity.
- **Revisit if:** a deployment wants bots inline by default (CI
  bots as first-class contributors) → flip the default in
  `starter-config`, not in the wire format.

#### 15.15.5 Stable cursor pagination, pinned to resolved window

- **Decision (Phase 3+):** `cursor = (resolved_window_end,
  rank_by_value, subject_id)`. The resolved window is captured
  at request time (`envelope.resolved_at` in §8.2) and pinned
  into the cursor, so events landing between page 1 and page 2
  cannot reshuffle or duplicate rows. A subsequent page request
  with a stale `resolved_window_end` is honoured (server reuses
  the pinned window); a request whose envelope window has moved
  forward returns `400 cursor_window_mismatch` rather than
  silently mixing two snapshots. Default page size 25; max 200.
- **Revisit if:** a UI needs jumping (page N of 50) rather than
  forward-only cursors → add an offset-mode variant; do not
  retrofit the cursor.

#### 15.15.6 Duration metrics: NULL ranks last

- **Decision (Phase 3+):** subjects below §15.9's sufficiency
  threshold for the chosen duration metric have NULL aggregates
  and sort to the bottom in a labelled "insufficient data"
  group — never as 0, never silently mid-rank. Counted in
  `footer.insufficient_data`. The threshold lives in §15.9; the
  leaderboard does not define its own.
- **Revisit if:** §15.9's threshold becomes configurable per
  deployment → the leaderboard footer label must reflect the
  active threshold, not a hard-coded "n < 5".

#### 15.15.7 No composite "productivity score"

- **Decision (Phase 3+):** rank exactly one named §15.7 metric
  at a time. A weighted scalar across metrics is rejected on two
  grounds:
  - §11.4 trust — a black-box score is unauditable.
  - §9 transparency — every number must be traceable to a §15.7
    row.
  The multi-metric escape hatch is `also_compute` (decision 3),
  which never affects rank order.
- **Revisit if:** never silently. Re-opening this requires an
  explicit §9 transparency review and a §11.4 trust review.

#### 15.15.8 `home_org_label` aggregation, pinned

- **Decision (Phase 3+):** when `subject = home_org_label`:
  - **Aggregation** — count metrics sum across all members of
    the label; duration metrics use the same percentile
    aggregator as the per-user version (§15.9), computed over
    the union of member events. **No averaging-of-averages.**
  - **NULL labels** — users with `home_org_label IS NULL` are
    bucketed into a single synthetic row with `subject_id =
    "__unlabeled__"` and `subject_label = "(no home org)"`, so
    they are never silently dropped. Suppressing them requires
    an explicit envelope filter, not an accident of aggregation.
  - **Single-member labels** are surfaced as-is — in shared-org
    scenarios a label with one member is real signal, not noise.
- **Revisit if:** §3 goal 3's manual mapping is supplemented by
  email-domain inference → decide whether inferred labels share
  the `__unlabeled__` bucket or get their own provisional row.

#### 15.15.9 IC self-view is a **separate** report kind (`my_standing`)

- **Decision (Phase 3+):** the leaderboard endpoint requires
  manager/admin scope. An IC asking "where do I sit?" calls a
  **separate `my_standing` endpoint** that returns:
  - the viewer's own row in full,
  - an anonymised neighbour window (±3 ranks, labels "—"),
  - a headline computed **over the visible set only**.
- **Why separate, not a projection flag:** `total_subjects` and
  page boundaries are themselves distributional information about
  colleagues. A single endpoint with "IC mode" projection would
  leak both. Same SQL primitives underneath; distinct envelope,
  distinct permission, distinct response.
- **Permissioning:** lives at the `with_principal +
  require_permission` boundary (§15.12). The leaderboard
  endpoint and `my_standing` endpoint carry distinct permission
  predicates.
- **Revisit if:** a deployment exposes leaderboards to ICs
  directly (e.g. a small co-op where rankings are public) →
  config-flag `my_standing.fallback_to_leaderboard`, do not
  merge the endpoints.

#### 15.15.10 `subject_ids` is the small-N compare-users path

- **Decision (Phase 3+):** `subject_ids` is capped at **50** and
  is honoured only when its length is ≤ that cap; larger
  requests are rejected with `400 subject_ids_too_large`. In
  `subject_ids` mode pagination is **disabled** — the server
  returns all matching rows in one response — so the
  compare-users UI can request every metric of interest via
  `also_compute` (decision 3) and never deal with cursors.
- **Cursor conflict is a typed 400, not silent:** a request that
  both sets `subject_ids` and carries a `page.cursor` is rejected
  with `400 pagination_disabled_for_subject_ids`. Quietly
  ignoring the cursor would let REST and MCP drift on behaviour;
  surfacing the conflict keeps the three surfaces loud.
- **Why:** §7's "compare these users" use-case is the only
  place a backend pairwise endpoint would be justified;
  `subject_ids` + `also_compute` covers it without a new surface
  (see §8.2 promotion path).
- **Revisit if:** real compare flows routinely hit > 50 subjects
  → raise the cap deliberately, do not re-enable pagination in
  this mode (the UI semantics depend on "all rows in one
  response").
