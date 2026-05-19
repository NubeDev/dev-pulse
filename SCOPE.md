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
- **No replacement for general project management tools** (Jira, Linear, etc.). Light interaction with GitHub Issues specifically is a likely future direction — see §4.1.
- **No tracking outside GitHub** (Slack, calendars, meetings).
- **No write operations to GitHub in v1.** v1 is read-only: the fetcher pulls data, nothing pushes back. This keeps the auth model, audit story, and blast radius simple. Write operations (issue CRUD) are a deliberate future-phase addition — see §4.1.

These constraints exist because activity data, once available, gravitates toward perf-review use. The design choices above are the primary defence.

### 4.1 Likely future direction — GitHub Issues CRUD

Not in v1, but the architecture should not preclude it: we may later add **create / read / update / close** for GitHub Issues directly inside `dev-pulse`, so a manager looking at an activity report can act on what they see (file a follow-up, reassign, close stale work) without leaving the tool.

What this means for v1 scope:

- The **local store** modelling for issues (§5, §6) should accommodate fields needed for editing (title, body, labels, assignees, state, milestone), not just the counters needed for reporting. We don't have to *use* them, but we shouldn't have to re-shape the schema later.
- The **auth model** decision (currently in §12 open questions) should account for the *possibility* of needing write scopes later, even if v1 ships read-only. Picking an auth approach that can't grow into write access would be a mistake.
- The **fetcher** (§10) should not be the only path between `dev-pulse` and GitHub forever — when write operations land, they will be **synchronous, user-initiated calls to GitHub** (not scheduled), and the local store will be updated either optimistically or on the next fetcher tick.
- **Audit logging** (already implied by §9 transparency) extends to writes: every issue mutation must record who did it, when, against which issue.

What is explicitly **not** decided here: which issue operations land first, whether comments and reactions are included, and whether this extends beyond issues (PRs, discussions). Those are future-phase scope questions.

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
