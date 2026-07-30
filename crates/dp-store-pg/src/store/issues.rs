
use chrono::{DateTime, Utc};
use dp_domain::inbox::{InboxIssueRow, InboxStatus, UserIssueState};
use dp_domain::issue::{Issue, IssueUpsert, IssueUpsertOutcome};
use dp_domain::issue_mutation::{IssueMutation, IssueMutationResult};
use dp_domain::event::EventKind;
use dp_domain::store::{
    IssueListFilter, IssueMetric, IssueMetricGroupBy,
    IssueMetricRow, IssueMetricsFilter, IssueTimelineRow, PendingRemoteIssue, StoreError,
};
use dp_domain::webhook::WebhookDelivery;
use serde_json::Value as JsonValue;
use sqlx::Row;
use uuid::Uuid;


use super::{map_sqlx, not_found, PgStore};
use super::rows::*;


impl PgStore {

    pub(super) async fn list_issues_impl(&self, filter: &IssueListFilter) -> Result<Vec<Issue>, StoreError> {
        let q_norm = filter.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let state_text = filter.state.map(|s| s.as_str().to_string());
        let labels_json = labels_or_assignees_json(&filter.labels);
        let assignees_json = labels_or_assignees_json(&filter.assignees);
        // `?|` (JSONB "any of these top-level keys/elements") is the OR
        // counterpart to `@>` and rides the same GIN indexes on
        // `labels` / `assignees` from migration 0011. It wants
        // `text[]`, not jsonb — hence the plain slice binds below.
        let rows = sqlx::query(
            "SELECT i.id, i.org_id, i.repo_id, i.github_id, i.number, i.title,
                    i.body, i.state, i.labels, i.assignees, i.milestone,
                    i.version, i.github_node_id, i.updated_at, i.is_local,
                    pi.project_id AS project_id,
                    p.name        AS project_name
             FROM dp_issues i
             LEFT JOIN dp_project_issues pi ON pi.issue_id = i.id
             LEFT JOIN dp_projects p        ON p.id = pi.project_id
             WHERE ($1::uuid IS NULL OR i.repo_id = $1)
               AND ($2::uuid IS NULL OR i.org_id  = $2)
               AND ($3::text IS NULL OR i.state   = $3)
               AND ($4::text IS NULL OR i.assignees @> to_jsonb(ARRAY[$4::text]))
               AND ($5::text IS NULL OR i.title ILIKE '%' || $5 || '%')
               AND (cardinality($8::uuid[]) = 0 OR i.repo_id = ANY($8::uuid[]))
               AND (cardinality($9::uuid[]) = 0 OR i.org_id  = ANY($9::uuid[]))
               AND ($10::jsonb IS NULL OR i.assignees @> $10::jsonb)
               AND ($11::jsonb IS NULL OR i.labels    @> $11::jsonb)
               AND ($12::text  IS NULL OR i.author = $12)
               AND ($13::text  IS NULL OR i.state_reason = $13)
               AND ($14::timestamptz IS NULL OR i.updated_at >= $14)
               AND (NOT $15::bool OR (i.assignees = '[]'::jsonb AND i.labels = '[]'::jsonb))
               AND (cardinality($16::text[]) = 0 OR i.assignees ?| $16::text[])
               AND (cardinality($17::text[]) = 0 OR i.labels    ?| $17::text[])
               AND (cardinality($18::uuid[]) = 0 OR pi.project_id = ANY($18::uuid[]))
               AND (NOT $19::bool OR pi.project_id IS NULL)
             ORDER BY i.updated_at DESC
             LIMIT $6 OFFSET $7",
        )
        .bind(filter.repo_id)
        .bind(filter.org_id)
        .bind(state_text)
        .bind(filter.assignee.as_deref())
        .bind(q_norm)
        .bind(filter.limit)
        .bind(filter.offset)
        .bind(&filter.repo_ids)
        .bind(&filter.org_ids)
        .bind(assignees_json.as_ref())
        .bind(labels_json.as_ref())
        .bind(filter.author.as_deref())
        .bind(filter.state_reason.as_deref())
        .bind(filter.updated_since)
        .bind(filter.untriaged_only)
        .bind(&filter.assignees_any)
        .bind(&filter.labels_any)
        .bind(&filter.project_ids)
        .bind(filter.no_project)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_issue).collect()
    }

    pub(super) async fn count_issues_impl(&self, filter: &IssueListFilter) -> Result<i64, StoreError> {
        let q_norm = filter.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let state_text = filter.state.map(|s| s.as_str().to_string());
        let labels_json = labels_or_assignees_json(&filter.labels);
        let assignees_json = labels_or_assignees_json(&filter.assignees);
        // Mirrors `list_issues_impl`'s WHERE exactly — including the
        // project join — or the total would disagree with the rows the
        // list returns and the pager would show phantom pages.
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*)::bigint
             FROM dp_issues i
             LEFT JOIN dp_project_issues pi ON pi.issue_id = i.id
             WHERE ($1::uuid IS NULL OR i.repo_id = $1)
               AND ($2::uuid IS NULL OR i.org_id  = $2)
               AND ($3::text IS NULL OR i.state   = $3)
               AND ($4::text IS NULL OR i.assignees @> to_jsonb(ARRAY[$4::text]))
               AND ($5::text IS NULL OR i.title ILIKE '%' || $5 || '%')
               AND (cardinality($6::uuid[]) = 0 OR i.repo_id = ANY($6::uuid[]))
               AND (cardinality($7::uuid[]) = 0 OR i.org_id  = ANY($7::uuid[]))
               AND ($8::jsonb  IS NULL OR i.assignees @> $8::jsonb)
               AND ($9::jsonb  IS NULL OR i.labels    @> $9::jsonb)
               AND ($10::text  IS NULL OR i.author = $10)
               AND ($11::text  IS NULL OR i.state_reason = $11)
               AND ($12::timestamptz IS NULL OR i.updated_at >= $12)
               AND (NOT $13::bool OR (i.assignees = '[]'::jsonb AND i.labels = '[]'::jsonb))
               AND (cardinality($14::text[]) = 0 OR i.assignees ?| $14::text[])
               AND (cardinality($15::text[]) = 0 OR i.labels    ?| $15::text[])
               AND (cardinality($16::uuid[]) = 0 OR pi.project_id = ANY($16::uuid[]))
               AND (NOT $17::bool OR pi.project_id IS NULL)",
        )
        .bind(filter.repo_id)
        .bind(filter.org_id)
        .bind(state_text)
        .bind(filter.assignee.as_deref())
        .bind(q_norm)
        .bind(&filter.repo_ids)
        .bind(&filter.org_ids)
        .bind(assignees_json.as_ref())
        .bind(labels_json.as_ref())
        .bind(filter.author.as_deref())
        .bind(filter.state_reason.as_deref())
        .bind(filter.updated_since)
        .bind(filter.untriaged_only)
        .bind(&filter.assignees_any)
        .bind(&filter.labels_any)
        .bind(&filter.project_ids)
        .bind(filter.no_project)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(count)
    }

    pub(super) async fn get_issue_impl(&self, id: Uuid) -> Result<Option<Issue>, StoreError> {
        let row = sqlx::query(
            "SELECT id, org_id, repo_id, github_id, number, title, body, state,
                    labels, assignees, milestone, version,
                    github_node_id, updated_at, is_local
             FROM dp_issues WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.as_ref().map(row_to_issue).transpose()
    }

    pub(super) async fn get_issue_by_repo_and_number_impl(
        &self,
        repo_id: Uuid,
        number: i64,
    ) -> Result<Option<Issue>, StoreError> {
        let row = sqlx::query(
            "SELECT id, org_id, repo_id, github_id, number, title, body, state,
                    labels, assignees, milestone, version,
                    github_node_id, updated_at, is_local
             FROM dp_issues WHERE repo_id = $1 AND number = $2",
        )
        .bind(repo_id)
        .bind(number)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.as_ref().map(row_to_issue).transpose()
    }

    pub(super) async fn upsert_issue_from_github_impl(
        &self,
        upsert: &IssueUpsert,
        pending_remote_window: chrono::Duration,
    ) -> Result<(Issue, IssueUpsertOutcome), StoreError> {
        // The upsert is a single round-trip so insert / freshness
        // check / version bump / §13.7 guard all happen atomically:
        //
        //   INSERT … ON CONFLICT (repo_id, number) DO UPDATE …
        //   WHERE
        //     -- §13.7 guard: skip if a recent optimistic write is
        //     -- still in flight (the dp-rest §8 path cleared
        //     -- `pending_remote` on completion / rollback, so a
        //     -- TRUE flag with a fresh timestamp means "do not
        //     -- clobber").
        //     (dp_issues.pending_remote = FALSE
        //      OR dp_issues.pending_remote_at <= now() - window)
        //     -- Freshness: only bump on strictly-newer payloads.
        //     AND excluded.updated_at > dp_issues.updated_at
        //   RETURNING …, (xmax = 0) AS inserted
        //
        // `xmax = 0` is the canonical Postgres trick to tell INSERT
        // from UPDATE inside an UPSERT — the inserted row has a
        // zero transaction-deleter marker. We use it (combined with
        // a follow-up `is_some()` on the rowcount) to decode the
        // three writing outcomes.
        let labels_json = serde_json::to_value(&upsert.labels)
            .map_err(|e| StoreError::Invalid(format!("labels not serialisable: {e}")))?;
        let assignees_json = serde_json::to_value(&upsert.assignees)
            .map_err(|e| StoreError::Invalid(format!("assignees not serialisable: {e}")))?;
        let new_id = Uuid::new_v4();
        let row = sqlx::query(
            "INSERT INTO dp_issues (
                 id, org_id, repo_id, github_id, number, title, body, state,
                 labels, assignees, milestone, author, state_reason,
                 created_at, updated_at, closed_at, version, github_node_id
             ) VALUES (
                 $1, $2, $3, $4, $5, $6, $7, $8,
                 $9, $10, $11, $12, $13,
                 $14, $15, $16, 1, $18
             )
             ON CONFLICT (repo_id, number) DO UPDATE SET
                 title        = EXCLUDED.title,
                 body         = EXCLUDED.body,
                 state        = EXCLUDED.state,
                 labels       = EXCLUDED.labels,
                 assignees    = EXCLUDED.assignees,
                 milestone    = EXCLUDED.milestone,
                 author       = EXCLUDED.author,
                 state_reason = EXCLUDED.state_reason,
                 updated_at   = EXCLUDED.updated_at,
                 closed_at    = EXCLUDED.closed_at,
                 -- github_id stays put — once we learn an issue's
                 -- numeric id, it never changes (transfers move
                 -- the number, not the id).
                 version      = dp_issues.version + 1,
                 -- §3.10 — opportunistic backfill: a row that
                 -- pre-dates migration 0021 has NULL here; the
                 -- first webhook / reconciler payload after the
                 -- migration carries `node_id`, populating the
                 -- column so the Projects v2 mirror can skip
                 -- the lazy GraphQL lookup on the next save.
                 github_node_id = COALESCE(EXCLUDED.github_node_id, dp_issues.github_node_id)
             WHERE
                 (dp_issues.pending_remote = FALSE
                  OR dp_issues.pending_remote_at IS NULL
                  OR dp_issues.pending_remote_at <= (now() - ($17::bigint || ' seconds')::interval))
                 AND EXCLUDED.updated_at > dp_issues.updated_at
             RETURNING
                 id, org_id, repo_id, github_id, number, title, body, state,
                 labels, assignees, milestone, version,
                 github_node_id, updated_at,
                 (xmax = 0) AS inserted",
        )
        .bind(new_id)
        .bind(upsert.org_id)
        .bind(upsert.repo_id)
        .bind(upsert.github_id)
        .bind(upsert.number)
        .bind(&upsert.title)
        .bind(upsert.body.as_deref())
        .bind(upsert.state.as_str())
        .bind(&labels_json)
        .bind(&assignees_json)
        .bind(upsert.milestone.as_deref())
        .bind(upsert.author.as_deref())
        .bind(upsert.state_reason.as_deref())
        .bind(upsert.created_at)
        .bind(upsert.updated_at)
        .bind(upsert.closed_at)
        .bind(pending_remote_window.num_seconds())
        .bind(upsert.github_node_id.as_deref())
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        if let Some(row) = row {
            let inserted: bool = row.try_get("inserted").map_err(map_sqlx)?;
            let issue = row_to_issue(&row)?;
            let outcome = if inserted {
                IssueUpsertOutcome::Inserted
            } else {
                IssueUpsertOutcome::Updated
            };
            return Ok((issue, outcome));
        }

        // No row returned → either the freshness guard fired
        // (stale payload — local copy is at least as new) or the
        // §13.7 reconciler guard fired (pending_remote within
        // window). Disambiguate with a single follow-up read so
        // the caller's metrics are accurate and so the caller
        // always receives the *current* local row.
        let existing = sqlx::query(
            "SELECT id, org_id, repo_id, github_id, number, title, body, state,
                    labels, assignees, milestone, version,
                    github_node_id, updated_at,
                    pending_remote, pending_remote_at
             FROM dp_issues
             WHERE repo_id = $1 AND number = $2",
        )
        .bind(upsert.repo_id)
        .bind(upsert.number)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?
        .ok_or_else(|| {
            // Can't happen unless someone deleted the row between
            // the upsert and the follow-up read — surface loudly.
            StoreError::Invalid(format!(
                "upsert for ({}, {}) returned no row and follow-up read missed",
                upsert.repo_id, upsert.number
            ))
        })?;

        let issue = row_to_issue(&existing)?;
        let pending: bool = existing.try_get("pending_remote").map_err(map_sqlx)?;
        let pending_at: Option<DateTime<Utc>> =
            existing.try_get("pending_remote_at").map_err(map_sqlx)?;
        let now = Utc::now();
        let in_pending_window = pending
            && pending_at
                .map(|at| now.signed_duration_since(at) < pending_remote_window)
                .unwrap_or(false);
        let outcome = if in_pending_window {
            IssueUpsertOutcome::Deferred
        } else {
            IssueUpsertOutcome::Skipped
        };
        Ok((issue, outcome))
    }

    /// SCOPE.md §4.1 amendment — direct insert of a local-only
    /// issue. Allocates a synthetic per-repo negative number /
    /// `github_id` from `dp_repos.local_issue_counter` (decremented
    /// in the same transaction) so the existing `UNIQUE (repo_id,
    /// number)` and `UNIQUE (repo_id, github_id)` invariants hold
    /// without widening the columns to NULL.
    pub(super) async fn create_local_issue_impl(
        &self,
        org_id: Uuid,
        repo_id: Uuid,
        title: &str,
        body: Option<&str>,
    ) -> Result<Issue, StoreError> {
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;

        // Allocate the next negative slot. The first local issue
        // in a repo gets number = -1, the second -2, …
        let (next,): (i64,) = sqlx::query_as(
            "UPDATE dp_repos
                SET local_issue_counter = local_issue_counter - 1
              WHERE id = $1
            RETURNING local_issue_counter",
        )
        .bind(repo_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;

        let new_id = Uuid::new_v4();
        let now = Utc::now();
        let row = sqlx::query(
            "INSERT INTO dp_issues (
                 id, org_id, repo_id, github_id, number, title, body, state,
                 labels, assignees, milestone, author, state_reason,
                 created_at, updated_at, closed_at, version, github_node_id,
                 is_local
             ) VALUES (
                 $1, $2, $3, $4, $4, $5, $6, 'open',
                 '[]'::jsonb, '[]'::jsonb, NULL, NULL, NULL,
                 $7, $7, NULL, 1, NULL,
                 TRUE
             )
             RETURNING id, org_id, repo_id, github_id, number, title, body, state,
                       labels, assignees, milestone, version,
                       github_node_id, updated_at, is_local",
        )
        .bind(new_id)
        .bind(org_id)
        .bind(repo_id)
        .bind(next)
        .bind(title)
        .bind(body)
        .bind(now)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;

        let issue = row_to_issue(&row)?;
        tx.commit().await.map_err(map_sqlx)?;
        Ok(issue)
    }

    /// SCOPE.md §4.1.1 — direct CAS-gated field update for a
    /// local-only issue (no GitHub round-trip, no pending_remote
    /// dance). The WHERE clause performs the CAS; COALESCE on
    /// each lane preserves untouched fields. `is_local = TRUE` is
    /// in the WHERE clause too so this method cannot accidentally
    /// be used to bypass the GitHub two-way-sync on a real issue.
    pub(super) async fn update_local_issue_impl(
        &self,
        issue_id: Uuid,
        expected_version: i64,
        title: Option<&str>,
        body: Option<Option<&str>>,
        state: Option<&str>,
        labels: Option<&[String]>,
        assignees: Option<&[String]>,
    ) -> Result<Issue, StoreError> {
        // `body` uses Option<Option<&str>> so an explicit
        // `Some(None)` lane clears the column; `None` leaves it
        // alone. Encode the "clear" intent with a sentinel bool
        // bound separately so the COALESCE chain stays simple.
        let (body_provided, body_value): (bool, Option<&str>) = match body {
            None => (false, None),
            Some(v) => (true, v),
        };
        let labels_json = labels
            .map(|l| serde_json::to_value(l))
            .transpose()
            .map_err(|e| StoreError::Invalid(format!("labels not serialisable: {e}")))?;
        let assignees_json = assignees
            .map(|a| serde_json::to_value(a))
            .transpose()
            .map_err(|e| {
                StoreError::Invalid(format!("assignees not serialisable: {e}"))
            })?;
        // `closed_at` is derived from the state transition: closing
        // stamps now(); reopening clears it. When state isn't being
        // touched, leave both `state` and `closed_at` alone.
        let row = sqlx::query(
            "UPDATE dp_issues SET
                 title       = COALESCE($3, title),
                 body        = CASE WHEN $4::bool THEN $5 ELSE body END,
                 state       = COALESCE($6, state),
                 closed_at   = CASE
                                   WHEN $6 = 'closed' THEN COALESCE(closed_at, now())
                                   WHEN $6 = 'open'   THEN NULL
                                   ELSE closed_at
                               END,
                 labels      = COALESCE($7, labels),
                 assignees   = COALESCE($8, assignees),
                 version     = version + 1,
                 updated_at  = now()
              WHERE id = $1
                AND version = $2
                AND is_local = TRUE
              RETURNING id, org_id, repo_id, github_id, number, title, body, state,
                        labels, assignees, milestone, version,
                        github_node_id, updated_at, is_local",
        )
        .bind(issue_id)
        .bind(expected_version)
        .bind(title)
        .bind(body_provided)
        .bind(body_value)
        .bind(state)
        .bind(labels_json.as_ref())
        .bind(assignees_json.as_ref())
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        match row {
            Some(r) => row_to_issue(&r),
            None => {
                // Distinguish "no such local issue" from "stale
                // CAS" so the REST handler can return a useful
                // 404 vs 409.
                let exists: Option<(bool,)> = sqlx::query_as(
                    "SELECT is_local FROM dp_issues WHERE id = $1",
                )
                .bind(issue_id)
                .fetch_optional(self.pool.sqlx())
                .await
                .map_err(map_sqlx)?;
                match exists {
                    None => Err(not_found("issue", issue_id)),
                    Some((false,)) => Err(StoreError::Invalid(format!(
                        "issue {issue_id} is not a local-only issue"
                    ))),
                    Some((true,)) => Err(StoreError::Conflict(format!(
                        "stale expected_version {expected_version} for local issue {issue_id}"
                    ))),
                }
            }
        }
    }

    pub(super) async fn list_inbox_issues_impl(
        &self,
        user_id: Uuid,
        filter: &IssueListFilter,
    ) -> Result<Vec<InboxIssueRow>, StoreError> {
        let q_norm = filter.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let state_text = filter.state.map(|s| s.as_str().to_string());
        let labels_json = labels_or_assignees_json(&filter.labels);
        let assignees_json = labels_or_assignees_json(&filter.assignees);
        // LEFT JOIN so issues with no `dp_user_issue_state` row
        // surface as default-state (`Inbox`, last_seen_version 0).
        // Inbox visibility predicate:
        //   * status IS NULL OR status <> 'done'      — dismissed rows hide
        //   * status <> 'snoozed' OR snoozed_until < now()  — active snoozes hide
        let rows = sqlx::query(
            "SELECT i.id, i.org_id, i.repo_id, i.github_id, i.number, i.title, i.body,
                    i.state, i.labels, i.assignees, i.milestone, i.version,
                    i.github_node_id, i.updated_at, i.is_local,
                    COALESCE(s.last_seen_version, 0)            AS last_seen_version,
                    COALESCE(s.status, 'inbox')                 AS inbox_status,
                    s.snoozed_until                             AS snoozed_until
             FROM dp_issues i
             LEFT JOIN dp_user_issue_state s
                    ON s.user_id = $16::uuid AND s.issue_id = i.id
             WHERE (s.status IS NULL OR s.status <> 'done')
               AND (s.status IS NULL OR s.status <> 'snoozed'
                    OR s.snoozed_until IS NULL OR s.snoozed_until < now())
               AND ($1::uuid IS NULL OR i.repo_id = $1)
               AND ($2::uuid IS NULL OR i.org_id  = $2)
               AND ($3::text IS NULL OR i.state   = $3)
               AND ($4::text IS NULL OR i.assignees @> to_jsonb(ARRAY[$4::text]))
               AND ($5::text IS NULL OR i.title ILIKE '%' || $5 || '%')
               AND (cardinality($8::uuid[]) = 0 OR i.repo_id = ANY($8::uuid[]))
               AND (cardinality($9::uuid[]) = 0 OR i.org_id  = ANY($9::uuid[]))
               AND ($10::jsonb IS NULL OR i.assignees @> $10::jsonb)
               AND ($11::jsonb IS NULL OR i.labels    @> $11::jsonb)
               AND ($12::text  IS NULL OR i.author = $12)
               AND ($13::text  IS NULL OR i.state_reason = $13)
               AND ($14::timestamptz IS NULL OR i.updated_at >= $14)
               AND (NOT $15::bool OR (i.assignees = '[]'::jsonb AND i.labels = '[]'::jsonb))
               AND ($17::timestamptz IS NULL
                    OR (i.updated_at, i.id) < ($17::timestamptz, $18::uuid))
             ORDER BY i.updated_at DESC, i.id DESC
             LIMIT $6 OFFSET $7",
        )
        .bind(filter.repo_id)
        .bind(filter.org_id)
        .bind(state_text)
        .bind(filter.assignee.as_deref())
        .bind(q_norm)
        .bind(filter.limit)
        .bind(filter.offset)
        .bind(&filter.repo_ids)
        .bind(&filter.org_ids)
        .bind(assignees_json.as_ref())
        .bind(labels_json.as_ref())
        .bind(filter.author.as_deref())
        .bind(filter.state_reason.as_deref())
        .bind(filter.updated_since)
        .bind(filter.untriaged_only)
        .bind(user_id)
        .bind(filter.keyset_after.map(|(ts, _)| ts))
        .bind(filter.keyset_after.map(|(_, id)| id))
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_inbox_issue_row).collect()
    }

    pub(super) async fn count_inbox_issues_impl(
        &self,
        user_id: Uuid,
        filter: &IssueListFilter,
    ) -> Result<i64, StoreError> {
        let q_norm = filter.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let state_text = filter.state.map(|s| s.as_str().to_string());
        let labels_json = labels_or_assignees_json(&filter.labels);
        let assignees_json = labels_or_assignees_json(&filter.assignees);
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*)::bigint
             FROM dp_issues i
             LEFT JOIN dp_user_issue_state s
                    ON s.user_id = $14::uuid AND s.issue_id = i.id
             WHERE (s.status IS NULL OR s.status <> 'done')
               AND (s.status IS NULL OR s.status <> 'snoozed'
                    OR s.snoozed_until IS NULL OR s.snoozed_until < now())
               AND ($1::uuid IS NULL OR i.repo_id = $1)
               AND ($2::uuid IS NULL OR i.org_id  = $2)
               AND ($3::text IS NULL OR i.state   = $3)
               AND ($4::text IS NULL OR i.assignees @> to_jsonb(ARRAY[$4::text]))
               AND ($5::text IS NULL OR i.title ILIKE '%' || $5 || '%')
               AND (cardinality($6::uuid[]) = 0 OR i.repo_id = ANY($6::uuid[]))
               AND (cardinality($7::uuid[]) = 0 OR i.org_id  = ANY($7::uuid[]))
               AND ($8::jsonb  IS NULL OR i.assignees @> $8::jsonb)
               AND ($9::jsonb  IS NULL OR i.labels    @> $9::jsonb)
               AND ($10::text  IS NULL OR i.author = $10)
               AND ($11::text  IS NULL OR i.state_reason = $11)
               AND ($12::timestamptz IS NULL OR i.updated_at >= $12)
               AND (NOT $13::bool OR (i.assignees = '[]'::jsonb AND i.labels = '[]'::jsonb))",
        )
        .bind(filter.repo_id)
        .bind(filter.org_id)
        .bind(state_text)
        .bind(filter.assignee.as_deref())
        .bind(q_norm)
        .bind(&filter.repo_ids)
        .bind(&filter.org_ids)
        .bind(assignees_json.as_ref())
        .bind(labels_json.as_ref())
        .bind(filter.author.as_deref())
        .bind(filter.state_reason.as_deref())
        .bind(filter.updated_since)
        .bind(filter.untriaged_only)
        .bind(user_id)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(count)
    }

    pub(super) async fn mark_issues_seen_impl(
        &self,
        user_id: Uuid,
        issue_ids: &[Uuid],
    ) -> Result<(), StoreError> {
        if issue_ids.is_empty() {
            return Ok(());
        }
        // Upsert one row per (user_id, issue_id), pulling
        // `last_seen_version` from `dp_issues.version` so the row
        // always reflects what the user actually saw. ON CONFLICT
        // promotes the value monotonically (GREATEST) so a stale
        // "seen" write from a slow client cannot regress a higher
        // value already on the row.
        sqlx::query(
            "INSERT INTO dp_user_issue_state
                 (user_id, issue_id, last_seen_version, status, snoozed_until, updated_at)
             SELECT $1, i.id, i.version, 'inbox', NULL, now()
               FROM dp_issues i
              WHERE i.id = ANY($2::uuid[])
             ON CONFLICT (user_id, issue_id) DO UPDATE
                 SET last_seen_version =
                         GREATEST(dp_user_issue_state.last_seen_version,
                                  EXCLUDED.last_seen_version),
                     updated_at        = now()",
        )
        .bind(user_id)
        .bind(issue_ids)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    pub(super) async fn set_inbox_state_impl(
        &self,
        user_id: Uuid,
        issue_id: Uuid,
        status: InboxStatus,
        snoozed_until: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<UserIssueState, StoreError> {
        // Upsert; preserve `last_seen_version` on update so
        // snooze / dismiss never moves the seen marker. The
        // application validates (status, snoozed_until)
        // consistency — see the trait doc.
        let row = sqlx::query(
            "INSERT INTO dp_user_issue_state
                 (user_id, issue_id, last_seen_version, status, snoozed_until, updated_at)
             VALUES ($1, $2, 0, $3, $4, now())
             ON CONFLICT (user_id, issue_id) DO UPDATE
                 SET status        = EXCLUDED.status,
                     snoozed_until = EXCLUDED.snoozed_until,
                     updated_at    = now()
             RETURNING user_id, issue_id, last_seen_version, status, snoozed_until, updated_at",
        )
        .bind(user_id)
        .bind(issue_id)
        .bind(status.as_str())
        .bind(snoozed_until)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_user_issue_state(&row)
    }

    pub(super) async fn set_inbox_state_bulk_impl(
        &self,
        user_id: Uuid,
        issue_ids: &[Uuid],
        status: InboxStatus,
        snoozed_until: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<u64, StoreError> {
        if issue_ids.is_empty() {
            return Ok(0);
        }
        // Done / Inbox ignore the snooze deadline (Inbox clears it;
        // Done has no wake target). Only Snoozed carries it through.
        let effective_snooze = match status {
            InboxStatus::Snoozed => snoozed_until,
            InboxStatus::Inbox | InboxStatus::Done => None,
        };
        let res = sqlx::query(
            "INSERT INTO dp_user_issue_state
                 (user_id, issue_id, last_seen_version, status, snoozed_until, updated_at)
             SELECT $1, i.id, 0, $3, $4, now()
               FROM dp_issues i
              WHERE i.id = ANY($2::uuid[])
             ON CONFLICT (user_id, issue_id) DO UPDATE
                 SET status        = EXCLUDED.status,
                     snoozed_until = EXCLUDED.snoozed_until,
                     updated_at    = now()",
        )
        .bind(user_id)
        .bind(issue_ids)
        .bind(status.as_str())
        .bind(effective_snooze)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(res.rows_affected())
    }

    pub(super) async fn list_events_for_issue_impl(
        &self,
        repo_id: Uuid,
        number: i64,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<IssueTimelineRow>, StoreError> {
        // The §6 guarded expression index on dp_activity_events
        // ensures the cast cannot raise on malformed rows — the
        // `payload ? 'number' AND payload->>'number' ~ '^[0-9]+$'`
        // predicate is repeated in the WHERE clause verbatim so
        // the planner picks the partial expression index.
        let rows = sqlx::query(
            "SELECT id, kind, ts, payload
             FROM dp_activity_events
             WHERE repo_id = $1
               AND kind = ANY(ARRAY['issue_opened','issue_closed','issue_comment']::text[])
               AND payload ? 'number'
               AND payload->>'number' ~ '^[0-9]+$'
               AND (payload->>'number')::int = $2
             ORDER BY ts DESC, id DESC
             LIMIT $3 OFFSET $4",
        )
        .bind(repo_id)
        .bind(number)
        .bind(limit.max(1))
        .bind(offset.max(0))
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows.iter() {
            let id: Uuid = r.try_get("id").map_err(map_sqlx)?;
            let kind_text: String = r.try_get("kind").map_err(map_sqlx)?;
            let ts: DateTime<Utc> = r.try_get("ts").map_err(map_sqlx)?;
            let payload: JsonValue = r.try_get("payload").map_err(map_sqlx)?;
            let kind: EventKind = serde_json::from_value(JsonValue::String(kind_text.clone()))
                .map_err(|e| StoreError::Invalid(format!("unknown event kind {kind_text}: {e}")))?;
            let payload_summary = summarise_timeline_payload(kind, &payload);
            out.push(IssueTimelineRow {
                id,
                kind,
                ts,
                payload_summary,
            });
        }
        Ok(out)
    }

    pub(super) async fn count_events_for_issue_impl(
        &self,
        repo_id: Uuid,
        number: i64,
    ) -> Result<i64, StoreError> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*)::bigint
             FROM dp_activity_events
             WHERE repo_id = $1
               AND kind = ANY(ARRAY['issue_opened','issue_closed','issue_comment']::text[])
               AND payload ? 'number'
               AND payload->>'number' ~ '^[0-9]+$'
               AND (payload->>'number')::int = $2",
        )
        .bind(repo_id)
        .bind(number)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(count)
    }

    pub(super) async fn issue_metrics_impl(
        &self,
        filter: &IssueMetricsFilter,
    ) -> Result<Vec<IssueMetricRow>, StoreError> {
        // Common scope: caller-supplied org / repo id sets are
        // applied as `= ANY(...)` so an empty slice = "no
        // restriction". The §5.10 SQL shapes are spelled out
        // inline so the planner sees a stable shape per metric.
        let bucket_sql = match (filter.metric, filter.group_by) {
            // `wip` group-by is fixed to assignee (§5.10).
            (IssueMetric::Wip, _) => "assignee_login",
            (_, IssueMetricGroupBy::Repo) => "i.repo_id::text",
            (_, IssueMetricGroupBy::Org) => "i.org_id::text",
            (_, IssueMetricGroupBy::Assignee) => "assignee_login",
            (_, IssueMetricGroupBy::Week) => {
                "to_char(date_trunc('week', coalesce(i.closed_at, i.updated_at)), 'YYYY-MM-DD')"
            }
            (_, IssueMetricGroupBy::Day) => {
                "to_char(date_trunc('day', coalesce(i.closed_at, i.updated_at)), 'YYYY-MM-DD')"
            }
        };

        // The §5.10 corrected SQL — see header comments in
        // linear-projects-idea.md §5.10:
        //
        //   * `wip`         uses `CROSS JOIN LATERAL jsonb_array_elements_text(assignees)`
        //   * `untriaged`   uses `jsonb_array_length(...) = 0`
        //   * `lead_time`   uses `EXTRACT(EPOCH FROM (closed_at - created_at))`
        let (select_clause, from_extra, where_extra) = match filter.metric {
            IssueMetric::Throughput => (
                "COUNT(*)::float8 AS value, COUNT(*)::bigint AS cnt",
                "",
                "i.state = 'closed' AND ($3::timestamptz IS NULL OR i.closed_at >= $3)
                 AND ($4::timestamptz IS NULL OR i.closed_at < $4)",
            ),
            IssueMetric::LeadTime => (
                "COALESCE(percentile_cont(0.5) WITHIN GROUP (
                     ORDER BY EXTRACT(EPOCH FROM (i.closed_at - i.created_at))
                 ), 0)::float8 AS value,
                 COUNT(*)::bigint AS cnt",
                "",
                "i.state = 'closed' AND i.closed_at IS NOT NULL
                 AND ($3::timestamptz IS NULL OR i.closed_at >= $3)
                 AND ($4::timestamptz IS NULL OR i.closed_at < $4)",
            ),
            IssueMetric::Wip => (
                "COUNT(*)::float8 AS value, COUNT(*)::bigint AS cnt",
                "CROSS JOIN LATERAL jsonb_array_elements_text(i.assignees) AS assignee_login",
                "i.state = 'open'",
            ),
            IssueMetric::Stale => (
                "COUNT(*)::float8 AS value, COUNT(*)::bigint AS cnt",
                "",
                "i.state = 'open' AND i.updated_at < now() - interval '30 days'",
            ),
            IssueMetric::Untriaged => (
                "COUNT(*)::float8 AS value, COUNT(*)::bigint AS cnt",
                "",
                "i.state = 'open'
                 AND jsonb_array_length(i.assignees) = 0
                 AND jsonb_array_length(i.labels)    = 0",
            ),
        };

        let sql = format!(
            "SELECT {bucket} AS bucket, {select}
             FROM dp_issues i
             {from_extra}
             WHERE (cardinality($1::uuid[]) = 0 OR i.org_id  = ANY($1::uuid[]))
               AND (cardinality($2::uuid[]) = 0 OR i.repo_id = ANY($2::uuid[]))
               AND {where_extra}
             GROUP BY bucket
             ORDER BY bucket",
            bucket = bucket_sql,
            select = select_clause,
            from_extra = from_extra,
            where_extra = where_extra,
        );

        let rows = sqlx::query(&sql)
            .bind(&filter.org_ids)
            .bind(&filter.repo_ids)
            .bind(filter.since)
            .bind(filter.until)
            .fetch_all(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows.iter() {
            let bucket: String = r.try_get("bucket").map_err(map_sqlx)?;
            let value: f64 = r.try_get("value").map_err(map_sqlx)?;
            let count: i64 = r.try_get("cnt").map_err(map_sqlx)?;
            out.push(IssueMetricRow {
                bucket,
                value,
                count,
            });
        }
        Ok(out)
    }

    pub(super) async fn try_acquire_issue_pending_remote_impl(
        &self,
        issue_id: Uuid,
        expected_version: i64,
        actor_user_id: Uuid,
    ) -> Result<Option<i64>, StoreError> {
        // One atomic statement does the §8.2 step 5 CAS: bump
        // version, raise pending_remote, stamp _at + _actor. The
        // WHERE clause rejects both `expected_version` mismatch
        // and a second concurrent writer (`pending_remote = false`
        // guard). RETURNING gives us the post-bump version so the
        // caller can plumb it into the IssueMutation audit row.
        let row: Option<(i64,)> = sqlx::query_as(
            "UPDATE dp_issues
                SET version = version + 1,
                    pending_remote = TRUE,
                    pending_remote_at = now(),
                    pending_remote_actor = $3
              WHERE id = $1
                AND version = $2
                AND pending_remote = FALSE
              RETURNING version",
        )
        .bind(issue_id)
        .bind(expected_version)
        .bind(actor_user_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(row.map(|(v,)| v))
    }

    pub(super) async fn release_issue_pending_remote_impl(
        &self,
        issue_id: Uuid,
        bump_version_again: bool,
    ) -> Result<i64, StoreError> {
        // §8.2 step 7 (success) clears the flag only; §8.2 step 8
        // (failure) additionally bumps `version` again so any
        // concurrent reader sees the rollback as a change. The
        // CHECK constraint dp_issues_pending_remote_consistent
        // means we have to NULL all three pending_* columns
        // together.
        let sql = if bump_version_again {
            "UPDATE dp_issues
                SET pending_remote = FALSE,
                    pending_remote_at = NULL,
                    pending_remote_actor = NULL,
                    version = version + 1
              WHERE id = $1
              RETURNING version"
        } else {
            "UPDATE dp_issues
                SET pending_remote = FALSE,
                    pending_remote_at = NULL,
                    pending_remote_actor = NULL
              WHERE id = $1
              RETURNING version"
        };
        let row: Option<(i64,)> = sqlx::query_as(sql)
            .bind(issue_id)
            .fetch_optional(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        match row {
            Some((v,)) => Ok(v),
            None => Err(not_found("issue", issue_id)),
        }
    }

    pub(super) async fn get_issue_version_impl(&self, issue_id: Uuid) -> Result<i64, StoreError> {
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT version FROM dp_issues WHERE id = $1")
                .bind(issue_id)
                .fetch_optional(self.pool.sqlx())
                .await
                .map_err(map_sqlx)?;
        row.map(|(v,)| v).ok_or_else(|| not_found("issue", issue_id))
    }

    pub(super) async fn list_issues_with_pending_remote_older_than_impl(
        &self,
        cutoff: DateTime<Utc>,
    ) -> Result<Vec<PendingRemoteIssue>, StoreError> {
        // Partial index `dp_issues_pending_remote_idx` covers this
        // exactly — empty / near-empty in steady state.
        let rows = sqlx::query(
            "SELECT id, repo_id, version, pending_remote_actor, pending_remote_at
               FROM dp_issues
              WHERE pending_remote = TRUE
                AND pending_remote_at < $1
              ORDER BY pending_remote_at ASC",
        )
        .bind(cutoff)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let issue_id: Uuid = r.try_get("id").map_err(map_sqlx)?;
            let repo_id: Uuid = r.try_get("repo_id").map_err(map_sqlx)?;
            let version: i64 = r.try_get("version").map_err(map_sqlx)?;
            // `pending_remote_actor` is NOT NULL whenever
            // `pending_remote = TRUE` per the CHECK constraint, so
            // the unwrap-via-Option is safe.
            let actor_user_id: Uuid =
                r.try_get("pending_remote_actor").map_err(map_sqlx)?;
            let pending_remote_at: DateTime<Utc> =
                r.try_get("pending_remote_at").map_err(map_sqlx)?;
            out.push(PendingRemoteIssue {
                issue_id,
                repo_id,
                version,
                actor_user_id,
                pending_remote_at,
            });
        }
        Ok(out)
    }

    pub(super) async fn record_issue_mutation_impl(
        &self,
        mutation: &IssueMutation,
    ) -> Result<IssueMutation, StoreError> {
        sqlx::query(
            "INSERT INTO dp_issue_mutations (
                 id, actor_user_id, issue_id, repo_id,
                 op, version_before, version_after, diff, result,
                 github_delivery_id, error,
                 created_at, finished_at
             ) VALUES (
                 $1, $2, $3, $4,
                 $5, $6, $7, $8, $9,
                 $10, $11,
                 $12, $13
             )",
        )
        .bind(mutation.id)
        .bind(mutation.actor_user_id)
        .bind(mutation.issue_id)
        .bind(mutation.repo_id)
        .bind(issue_mutation_op_to_text(mutation.op))
        .bind(mutation.version_before)
        .bind(mutation.version_after)
        .bind(&mutation.diff)
        .bind(issue_mutation_result_to_text(mutation.result))
        .bind(mutation.github_delivery_id.as_deref())
        .bind(mutation.error.as_deref())
        .bind(mutation.created_at)
        .bind(mutation.finished_at)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(mutation.clone())
    }

    pub(super) async fn update_issue_mutation_result_impl(
        &self,
        id: Uuid,
        result: IssueMutationResult,
        github_delivery_id: Option<&str>,
        error: Option<&str>,
    ) -> Result<(), StoreError> {
        // Stamp `finished_at = now()` whenever the row leaves
        // `pending` — the CHECK on the table requires this. We
        // pass `now()` from Postgres, not the host's clock, so the
        // sweeper's audit row timestamp matches the wall-clock
        // observation.
        let n = sqlx::query(
            "UPDATE dp_issue_mutations
                SET result = $2,
                    github_delivery_id = COALESCE($3, github_delivery_id),
                    error = COALESCE($4, error),
                    finished_at = now()
              WHERE id = $1
                AND result = 'pending'",
        )
        .bind(id)
        .bind(issue_mutation_result_to_text(result))
        .bind(github_delivery_id)
        .bind(error)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        if n.rows_affected() == 0 {
            // Either the id is bogus or the row already left
            // `pending`. The sweeper / handler interleave is
            // designed so this is never a race; surface it
            // explicitly so a bug shows up loudly.
            return Err(not_found("dp_issue_mutations(pending)", id));
        }
        Ok(())
    }

    pub(super) async fn list_pending_issue_mutations_older_than_impl(
        &self,
        cutoff: DateTime<Utc>,
    ) -> Result<Vec<IssueMutation>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, actor_user_id, issue_id, repo_id, op,
                    version_before, version_after, diff, result,
                    github_delivery_id, error, created_at, finished_at
               FROM dp_issue_mutations
              WHERE result = 'pending'
                AND created_at < $1
              ORDER BY created_at ASC",
        )
        .bind(cutoff)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_issue_mutation).collect()
    }

    pub(super) async fn find_repo_id_by_github_id_impl(
        &self,
        github_repo_id: i64,
    ) -> Result<Option<Uuid>, StoreError> {
        // `dp_repos.github_id` is UNIQUE — index probe.
        let row: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM dp_repos WHERE github_id = $1")
                .bind(github_repo_id)
                .fetch_optional(self.pool.sqlx())
                .await
                .map_err(map_sqlx)?;
        Ok(row.map(|(id,)| id))
    }

    pub(super) async fn find_issue_id_by_repo_and_github_id_impl(
        &self,
        repo_id: Uuid,
        github_issue_id: i64,
    ) -> Result<Option<Uuid>, StoreError> {
        // The `(repo_id, github_id)` UNIQUE on `dp_issues` (per
        // `0001_init.sql`) makes this an index-only probe.
        let row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM dp_issues WHERE repo_id = $1 AND github_id = $2",
        )
        .bind(repo_id)
        .bind(github_issue_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(row.map(|(id,)| id))
    }

    pub(super) async fn is_issue_pending_remote_fresh_impl(
        &self,
        issue_id: Uuid,
        timeout: chrono::Duration,
    ) -> Result<bool, StoreError> {
        // Push the cutoff comparison into SQL so `now()` stays the
        // same clock the §8.2 CAS used to stamp `pending_remote_at`.
        // The seconds bind is i64 — saturating because chrono's
        // Duration can in principle hold values that won't fit, but
        // the production timeout knob is in tens of seconds.
        let secs = timeout.num_seconds().max(0);
        let row: Option<(bool,)> = sqlx::query_as(
            "SELECT (pending_remote
                  AND pending_remote_at IS NOT NULL
                  AND pending_remote_at >= now() - make_interval(secs => $2))
               FROM dp_issues
              WHERE id = $1",
        )
        .bind(issue_id)
        .bind(secs)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(row.map(|(b,)| b).unwrap_or(false))
    }

    pub(super) async fn buffer_pending_remote_webhook_impl(
        &self,
        issue_id: Uuid,
        delivery: &WebhookDelivery,
    ) -> Result<(), StoreError> {
        // No `ON CONFLICT` — duplicate `delivery_id` is a benign
        // re-deflection of the same logical webhook, and surfacing
        // the conflict matches the inbox's contract (the caller
        // translates it to "already buffered, drop").
        sqlx::query(
            "INSERT INTO dp_pending_remote_webhook_buffer \
                 (id, issue_id, delivery_id, event, payload, received_at) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(delivery.id)
        .bind(issue_id)
        .bind(&delivery.delivery_id)
        .bind(&delivery.event)
        .bind(&delivery.payload)
        .bind(delivery.received_at)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    pub(super) async fn take_buffered_webhooks_for_issue_impl(
        &self,
        issue_id: Uuid,
    ) -> Result<Vec<WebhookDelivery>, StoreError> {
        // `DELETE … RETURNING` is the at-least-once-replay primitive
        // §13.7 calls for: the buffered rows leave the table in the
        // same statement that produces the replay batch, so a crash
        // between this call and `apply_delivery` loses the buffer
        // copy. GitHub's at-least-once redelivery + the next
        // reconciler tick make this acceptable (the authoritative
        // state will be re-observed shortly).
        let rows = sqlx::query(
            "DELETE FROM dp_pending_remote_webhook_buffer \
              WHERE issue_id = $1 \
             RETURNING id, delivery_id, event, payload, received_at, \
                       NULL::timestamptz AS processed_at, \
                       NULL::text       AS error",
        )
        .bind(issue_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        // Oldest first — preserves the relative ordering of inbound
        // GitHub events on the issue. We sort in-memory because the
        // RETURNING clause does not guarantee row order.
        let mut out: Vec<WebhookDelivery> =
            rows.iter().map(row_to_webhook_delivery).collect::<Result<_, _>>()?;
        out.sort_by_key(|d| d.received_at);
        Ok(out)
    }
}
