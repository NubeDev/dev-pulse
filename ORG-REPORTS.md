# dev-pulse — Scope (Org-wide & Cross-org Ranking)

> Companion to [SCOPE.md](SCOPE.md). Proposes the **leaderboard**
> report kind as the first cross-cutting report shape: a single
> primitive that serves "rank everyone in the org," "rank across all
> orgs," and "compare these specific users" from one endpoint.
> Reuses every primitive already locked in [SCOPE.md](SCOPE.md) —
> envelope (§15.6), role→metric map (§15.7), trend buckets (§15.8),
> percentile semantics (§15.9), and the three org-scope modes (§8.1).
>
> Status: **proposal**. Nothing here is locked until promoted into
> [SCOPE.md](SCOPE.md) as §8.2 + a §15.15 Decision.

---

## 1. Why this report first

The user need is "I want to report on multiple users and compare
their output." The naive read of that ask is a pairwise compare-users
view; the right read is a **ranked list**, because:

- The leaderboard is the primitive. "Compare these N users" is a
  thin client of it: the UI calls the leaderboard endpoint once per
  metric of interest (small N, ≤ 50 subjects) and joins client-side.
  No backend pairwise mode — see §7.
- Ranking is the only report shape that scales the same query from
  *one user* to *one team* to *one org* to *all orgs* without
  changing surface, envelope, or aggregation rules.
- All three SCOPE.md §7 audiences want it:
  - **Managers** — "how is my team distributed this sprint."
  - **Execs** — "which orgs / which home-orgs are pulling weight."
  - **ICs** — "where do I sit, with neighbours anonymised."

So: ranking is the cross-cutting primitive. Compare-users is a UI
on top of it.

---

## 2. Subject axis (the one new concept)

Every existing report ranks **events through time** for a fixed
subject (the org, or the user). A leaderboard ranks **subjects**
against each other for a fixed metric. The subject axis is
orthogonal to the time / org / repo / team dimensions already in
SCOPE.md §8.

```
SubjectKind = user | team | org | home_org_label
```

- `user` — one row per `users.id` (post-bot-filter, see §6.3).
- `team` — one row per `teams.id`. Org-scoped by definition; in
  "all orgs combined" mode this kind is invalid (teams do not
  cross orgs) and the API rejects the envelope with a clear error
  rather than silently producing nonsense.
- `org` — one row per `orgs.id`. Only meaningful in
  "all orgs combined" or "per-org split" modes.
- `home_org_label` — one row per distinct value of
  `users.home_org_label` (SCOPE.md §5). This is the cross-company
  lens execs need for shared-org scenarios. Aggregation across
  the members of a label and NULL-label handling are pinned in
  §6.8.

---

## 3. Envelope shape

Extends SCOPE.md §15.6 with two required fields and a small set of
leaderboard-specific options. No existing field changes meaning.

```rust
struct LeaderboardEnvelope {
    // --- inherited from §15.6, unchanged ---
    window:        Window,            // §0.4
    org_scope:     OrgScope,          // §8.1 three modes
    repos:         Option<Vec<RepoId>>,
    teams:         Option<Vec<TeamId>>,
    actor_roles:   Option<Vec<ActorRole>>,
    tz:            Tz,

    // --- new, leaderboard-specific ---
    subject:       SubjectKind,       // §2 above
    rank_by:       MetricId,          // exactly one row from §15.7
    also_compute:  Option<Vec<MetricId>>,  // §6.3, carried in row.context
    subject_ids:   Option<Vec<SubjectId>>, // small-N filter, §6.9
    include_bots:  bool,              // default false, §6.4
    page:          PageRequest,       // §6.5 stable cursor
}
```

- `rank_by` is one named metric from SCOPE.md §15.7. Server-side
  sort and pagination are *only* on `rank_by` — composite scores
  are out (§6.7).
- `also_compute` carries additional §15.7 metrics into each row's
  `context` block so the UI can re-sort the visible page without
  a second request. It does not change rank order or pagination.
- `subject_ids` filters *before* ranking; ranks within the
  filtered set, not the global one. Capped at 50 (§6.9). The
  compare-users UI uses this; everything else leaves it `None`.
- Everything else is inherited so an existing report URL can be
  pivoted into a leaderboard by adding two query params.

---

## 4. Response shape

Mirrors SCOPE.md §15.6's headline+table+trend triple, with the
table re-typed as a ranked list:

```jsonc
{
  "envelope":   {
    /* echo, with the request's window resolved to absolute
       timestamps. Identical input + identical resolved_at must
       produce identical output, §6.6. */
    "resolved_at":     "2026-05-20T09:00:00Z",
    "resolved_window": { "from": "...", "to": "..." }
  },
  "headline":   { "total_subjects": 42, "events_total": 1287, ... },
  "rows": [
    {
      "rank":         1,
      "subject_id":   "...",
      "subject_kind": "user",
      "subject_label":"alice",
      "subject_org":  "...",    // only in per-org-split, §5
      "primary":      { "metric": "prs_merged", "value": 23 },
      "context":      {
        // §15.7 metadata always present:
        "active_days": 14, "repos_touched": 6,
        // additional §15.7 metrics requested via `also_compute`:
        "reviews_given": { "value": 41 },
        "pr_cycle_time_hours_p50": { "value": 19.4, "n": 23 }
      },
      "sparkline":    [ /* per-bucket counts, §15.8 rule */ ],
      "active_orgs":  3      // see §6.1
    }
    // ...
  ],
  "footer": {
    "unattributed_events":        17,   // §6.2
    "unattributed_events_metric": 11,   // §6.2, only for the rank_by metric
    "insufficient_data":          4,    // §6.5, duration metrics only
    "bots_suppressed":            2,    // §6.4
    "bots_suppressed_events":     38    // §6.4, reconciliation only
  },
  "page": { "next_cursor": "...", "has_more": true }
}
```

---

## 5. Org-scope interaction (the trap to avoid)

The leaderboard inherits SCOPE.md §8.1's three modes. Each one
produces a different leaderboard and the UI must label them
explicitly so users never compare across modes by accident.

| Mode | What "rank" means | Row identity |
|------|-------------------|--------------|
| **single-org**       | Rank within one codebase. | `subject` |
| **all-orgs-combined** | Rank by cross-org total, de-duplicated. | `subject` (one row even if active in N orgs) |
| **per-org-split**     | Rank by `(subject × org)` pair — shows context-switching. | `(subject_id, subject_org)` |

`per-org-split` is the only mode where a single user can appear
multiple times, and the only mode where `rows[].subject_org` is
populated. The frontend must visually group these together
(grouped table, not a flat list) or §8.1's "spread thin" insight
is lost in the sort order.

---

## 6. New decisions this report forces

Worth pinning before any code, because each one is a source of
silent divergence between REST / MCP / frontend (a SCOPE.md §11.4
trust violation).

### 6.1 Tie-break order, locked

`rank_by DESC → active_days DESC → subject_id ASC`.

Deterministic across all three surfaces. `subject_id` is the
final tie-break because labels (`login`, `team.slug`) can change;
ids do not.

### 6.2 Unattributed events, with explicit reconciliation

Events with `event_actors.user_id IS NULL` (SCOPE.md §15.7) are
**excluded from `subject=user` rows** but surfaced in the footer.
Two counts are required because the leaderboard filters by metric:

- `unattributed_events` — total unattributed events in the
  resolved window (matches the headline report).
- `unattributed_events_metric` — unattributed events that *would
  have contributed to `rank_by`* if attributed.

For count-style metrics the contract is:

```
headline.events_total
  == sum(rows[].primary.value)
   + footer.unattributed_events_metric
   + footer.bots_suppressed_events
```

For duration metrics this identity is meaningless (the values
are aggregates, not counts) and the reconciliation check is
skipped — but `unattributed_events_metric` is still reported.

### 6.3 Multi-metric rows via `also_compute`, not composite scores

`also_compute` lets a single request carry up to 5 §15.7 metrics
per row. The UI can re-sort the visible page client-side without
issuing N requests, and "compare these users" can fetch every
metric at once for ≤ 50 subjects (§6.9). What `also_compute` does
**not** change:

- Server-side sort and pagination are always on `rank_by`. A
  page boundary is meaningful only for the canonical metric.
- No weighted composite score is ever computed server-side
  (SCOPE.md §11.4 trust, §9 transparency).

If the UI wants to paginate by a different metric it must issue
a new request with that metric as `rank_by` — there is no
"resort the whole result set" affordance.

### 6.4 Bot suppression is a filter, not a rank rule

`include_bots` defaults `false`. Bots never silently disappear
from event totals (those still appear in the headline); they only
disappear from the ranked rows. Two footer counts:

- `bots_suppressed` — number of bot subjects hidden from rows.
- `bots_suppressed_events` — events those bots contributed,
  needed by the §6.2 reconciliation identity.

Mirrors SCOPE.md §15.7's bot caveat.

### 6.5 Stable cursor pagination, pinned to resolved window

`cursor = (resolved_window_end, rank_by_value, subject_id)`.

The resolved window is captured at request time
(`envelope.resolved_at` in §4) and pinned into the cursor, so
new events landing between page 1 and page 2 cannot reshuffle
or duplicate rows. A subsequent page request with a stale
`resolved_window_end` is honoured (server re-uses the pinned
window); a request whose envelope window has moved forward
returns a `400 cursor_window_mismatch` rather than silently
mixing two snapshots. Default page size 25; max 200.

### 6.6 Duration metrics: NULL ranks last

Subjects below SCOPE.md §15.9's sufficiency threshold for the
chosen duration metric have NULL aggregates and sort to the
bottom in a labelled "insufficient data" group — never as 0,
never silently mid-rank. The threshold lives in §15.9; the
leaderboard does not define its own. Counted in
`footer.insufficient_data`.

### 6.7 No composite "productivity score"

Rank one named metric at a time. See §6.3 for the multi-metric
escape hatch (`also_compute`). A weighted scalar across metrics
is rejected on two grounds:

- SCOPE.md §11.4 trust — a black-box score is unauditable.
- SCOPE.md §9 transparency — every number must be traceable to
  a §15.7 row.

### 6.8 `home_org_label` aggregation, pinned

When `subject = home_org_label`:

- **Aggregation** — count metrics sum across all members of the
  label; duration metrics use the same percentile aggregator as
  the per-user version (SCOPE.md §15.9), computed over the union
  of member events. No averaging-of-averages.
- **NULL labels** — users with `home_org_label IS NULL` are
  bucketed into a single synthetic row with `subject_id =
  "__unlabeled__"` and `subject_label = "(no home org)"`, so
  they are never silently dropped. Suppressing them requires an
  explicit envelope filter, not an accident of aggregation.
- **Single-member labels** are surfaced as-is; we do not hide
  them, because in shared-org scenarios a label with one member
  is real signal.

### 6.9 IC self-view is a separate report kind

The leaderboard endpoint requires manager/admin scope. An IC
asking "where do I sit?" calls a separate `my_standing` report
that returns:

- the viewer's own row in full,
- an anonymised neighbour window (±3 ranks, labels "—"),
- a headline computed **over the visible set only**.

Same SQL primitives underneath, but a distinct endpoint and
envelope so totals, pagination, and tie-break boundaries cannot
leak distributional information about colleagues. The earlier
"same SQL, only projection changes" framing was rejected
because `total_subjects` and page boundaries themselves are
information leaks. Permissioning lives at the
`with_principal + require_permission` boundary (SCOPE.md §15.12).

### 6.10 `subject_ids` is the small-N compare-users path

`subject_ids` is capped at **50** and is only honoured when its
length is ≤ that cap; larger requests are rejected with
`400 subject_ids_too_large`. In `subject_ids` mode pagination
is disabled (the server returns all matching rows in one
response), so the compare-users UI can request every metric of
interest with `also_compute` and never deal with cursors.

---

## 7. Explicitly out of scope (v1)

- **Composite / weighted scores** — see §6.7.
- **Pairwise diff page** ("user A vs user B side-by-side metric
  matrix") — pure UI: one leaderboard call with `subject_ids =
  [a, b]` and every metric of interest in `also_compute` (§6.3,
  §6.9). No backend pairwise endpoint.
- **Backend re-sort across metrics within one paginated result** —
  by design; see §6.3. Multi-metric pagination requires a new
  request with that metric as `rank_by`.
- **Anomaly / outlier callouts** on the leaderboard — separate
  report, separate envelope.
- **Custom metric definitions** — the §15.7 map is the universe.
  Adding a metric means a new row there, not a leaderboard-only
  extension.
- **Team-vs-team across orgs** — `subject = team` is org-scoped
  by definition (§2). Cross-org team comparison needs a team
  identity model that doesn't exist yet.

---

## 8. Promotion path into SCOPE.md

When the above is reviewed, fold into SCOPE.md as:

- **§8.2 Leaderboard report kind** — sections 1–5 above
  (vision, subject axis, envelope, response, org-scope
  interaction), trimmed to the SCOPE.md house style.
- **§15.15 Leaderboard semantics (Phase 3+)** — section 6 above
  (the ten locked decisions §6.1–§6.10), in the same Decision
  format as §15.7–§15.14, each with explicit *Revisit triggers*.
  Note that §6.9 (`my_standing` as a separate kind) and §6.10
  (`subject_ids` small-N path) imply two endpoints, not one —
  call this out in the §15.15 Decision so it isn't lost in the
  fold.

This file can then be deleted or kept as design rationale; the
locked text lives in SCOPE.md.
