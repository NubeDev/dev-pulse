#![allow(unused_imports)]
use std::error::Error as StdError;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dp_domain::audit::AuditEntry;
use dp_domain::event::{ActivityEvent, ActorRole, EventActor};
use dp_domain::fetch::{FetchCursor, FetchRun, FetchRunErrorSample, FetchRunKind, ResourceKind};
use dp_domain::freshness::DataAsOf;
use dp_domain::identity::{IdentityLinkPending, UserIdentity, VerifiedVia};
use dp_domain::inbox::{InboxIssueRow, InboxStatus, UserIssueState};
use dp_domain::membership::Membership;
use dp_domain::milestone::{Milestone, MilestoneState, MilestoneUpsert};
use dp_domain::org::Org;
use dp_domain::pin::{Pin, PinKind};
use dp_domain::repo::Repo;
use dp_domain::setting::UserSetting;
use dp_domain::issue::{Issue, IssueState, IssueUpsert, IssueUpsertOutcome, RepoSummary};
use dp_domain::issue_mutation::{IssueMutation, IssueMutationOp, IssueMutationResult};
use dp_domain::event::EventKind;
use dp_domain::tag::Tag;
use dp_domain::tag_link::{TagLink, TagLinkKind};
use dp_domain::board_link::{
    BoardItem, BoardItemMirrorOutcome, BoardLink, BoardLinkUpsert,
};
use dp_domain::issue_dates::{IssueDates, ProjectV2MirrorTask, ProjectV2MirrorTaskKind};
use dp_domain::project::{
    PortfolioQueryFilter, PortfolioRawRow, Project, ProjectIssueAddOutcome, ProjectIssueAddSkip,
    ProjectListFilter, ProjectRepo, ProjectStatus, ProjectUpsert,
};
use dp_domain::project_view::{
    ProjectView, ProjectViewFilterClause, ProjectViewUpsert, ProjectViewVisibility,
};
use dp_domain::store::{
    EventActorRow, IssueDatesMirrorOutcome, IssueListFilter, IssueMetric, IssueMetricGroupBy,
    IssueMetricRow, IssueMetricsFilter, IssueTimelineRow, PendingRemoteIssue, RepoListFilter,
    RepoSyncStatus, Store, StoreError,
};
use dp_domain::team::Team;
use dp_domain::user::User;
use dp_domain::webhook::WebhookDelivery;
use dp_domain::window::Window;
use serde_json::Value as JsonValue;
use sqlx::Row;
use starter_store_postgres::Pool;
use uuid::Uuid;

use crate::encode::{
    actor_role_from_text, actor_role_to_text, event_kind_from_text, event_kind_to_text,
    tag_link_kind_from_text, tag_scope_kind_from_text,
    fetch_run_kind_from_text, fetch_run_kind_to_text, membership_role_from_text,
    membership_role_to_text, resource_kind_from_text, resource_kind_to_text,
};

use super::{invalid, map_sqlx, not_found, parse_tag_name_kv, PgStore};
use super::rows::*;


impl PgStore {

    pub(super) async fn upsert_repo_impl(&self, repo: &Repo) -> Result<Repo, StoreError> {
        let row = sqlx::query(
            "INSERT INTO dp_repos (id, org_id, github_id, name) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (org_id, github_id) DO UPDATE SET \
                 name = EXCLUDED.name \
             RETURNING id, org_id, github_id, name",
        )
        .bind(repo.id)
        .bind(repo.org_id)
        .bind(repo.github_id)
        .bind(&repo.name)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_repo(&row)
    }

    pub(super) async fn upsert_repo_metadata_impl(
        &self,
        m: &dp_domain::RepoMetadata,
    ) -> Result<(), StoreError> {
        // COALESCE on nullable text/timestamp fields so a webhook
        // delivery that doesn't carry e.g. `description` doesn't
        // wipe a previously-recorded value. Counter fields are
        // written as supplied — the caller upserts metadata only
        // when the payload included a fresh repo object.
        sqlx::query(
            "INSERT INTO dp_repo_metadata ( \
                 repo_id, stars, forks, watchers, open_issues_remote, \
                 primary_language, default_branch, description, homepage, \
                 is_archived, is_fork, is_private, pushed_at, metadata_updated_at \
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) \
             ON CONFLICT (repo_id) DO UPDATE SET \
                 stars               = EXCLUDED.stars, \
                 forks               = EXCLUDED.forks, \
                 watchers            = EXCLUDED.watchers, \
                 open_issues_remote  = EXCLUDED.open_issues_remote, \
                 primary_language    = COALESCE(EXCLUDED.primary_language, dp_repo_metadata.primary_language), \
                 default_branch      = COALESCE(EXCLUDED.default_branch,   dp_repo_metadata.default_branch), \
                 description         = COALESCE(EXCLUDED.description,      dp_repo_metadata.description), \
                 homepage            = COALESCE(EXCLUDED.homepage,         dp_repo_metadata.homepage), \
                 is_archived         = EXCLUDED.is_archived, \
                 is_fork             = EXCLUDED.is_fork, \
                 is_private          = EXCLUDED.is_private, \
                 pushed_at           = COALESCE(EXCLUDED.pushed_at,        dp_repo_metadata.pushed_at), \
                 metadata_updated_at = EXCLUDED.metadata_updated_at",
        )
        .bind(m.repo_id)
        .bind(m.stars)
        .bind(m.forks)
        .bind(m.watchers)
        .bind(m.open_issues_remote)
        .bind(m.primary_language.as_deref())
        .bind(m.default_branch.as_deref())
        .bind(m.description.as_deref())
        .bind(m.homepage.as_deref())
        .bind(m.is_archived)
        .bind(m.is_fork)
        .bind(m.is_private)
        .bind(m.pushed_at)
        .bind(m.metadata_updated_at)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    pub(super) async fn get_repo_metadata_impl(
        &self,
        repo_id: Uuid,
    ) -> Result<Option<dp_domain::RepoMetadata>, StoreError> {
        let row = sqlx::query(
            "SELECT repo_id, stars, forks, watchers, open_issues_remote, \
                    primary_language, default_branch, description, homepage, \
                    is_archived, is_fork, is_private, pushed_at, metadata_updated_at \
             FROM dp_repo_metadata WHERE repo_id = $1",
        )
        .bind(repo_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.as_ref().map(row_to_repo_metadata).transpose()
    }

    pub(super) async fn pr_size_stats_for_repo_impl(
        &self,
        repo_id: Uuid,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
    ) -> Result<dp_domain::RepoPrSizeStats, StoreError> {
        // PR-size distribution from the JSONB payloads of merged
        // PR events. `payload->>'additions'` etc. yield text;
        // ::numeric coerces and silently rejects rows that lack
        // the field (the cast errors out), so we filter with
        // `payload ? 'additions'` first to keep the cast safe.
        //
        // Sample-size guard (SCOPE §15.9): if `n < 5`, every
        // percentile field is returned as NULL; the caller maps
        // that to `Option::None`. We surface the *real* `n` so the
        // UI can communicate "n too small" instead of "no data".
        let row = sqlx::query(
            "WITH sized AS ( \
                 SELECT \
                     (payload->>'additions')::numeric     AS additions, \
                     (payload->>'deletions')::numeric     AS deletions, \
                     (payload->>'changed_files')::numeric AS changed_files, \
                     (payload->>'commits')::numeric       AS commits \
                 FROM dp_activity_events \
                 WHERE repo_id = $1 \
                   AND kind = 'pull_request_merged' \
                   AND ts >= $2 AND ts < $3 \
                   AND payload ? 'additions' \
                   AND payload ? 'deletions' \
                   AND payload ? 'changed_files' \
             ) \
             SELECT \
                 COUNT(*)::bigint AS n, \
                 percentile_cont(0.5)  WITHIN GROUP (ORDER BY additions)                                   AS add_p50, \
                 percentile_cont(0.9)  WITHIN GROUP (ORDER BY additions)                                   AS add_p90, \
                 percentile_cont(0.95) WITHIN GROUP (ORDER BY additions)                                   AS add_p95, \
                 percentile_cont(0.5)  WITHIN GROUP (ORDER BY deletions)                                   AS del_p50, \
                 percentile_cont(0.9)  WITHIN GROUP (ORDER BY deletions)                                   AS del_p90, \
                 percentile_cont(0.95) WITHIN GROUP (ORDER BY deletions)                                   AS del_p95, \
                 percentile_cont(0.5)  WITHIN GROUP (ORDER BY additions + deletions)                       AS tot_p50, \
                 percentile_cont(0.9)  WITHIN GROUP (ORDER BY additions + deletions)                       AS tot_p90, \
                 percentile_cont(0.95) WITHIN GROUP (ORDER BY additions + deletions)                       AS tot_p95, \
                 percentile_cont(0.5)  WITHIN GROUP (ORDER BY changed_files)                               AS cf_p50, \
                 percentile_cont(0.9)  WITHIN GROUP (ORDER BY changed_files)                               AS cf_p90, \
                 percentile_cont(0.95) WITHIN GROUP (ORDER BY changed_files)                               AS cf_p95, \
                 percentile_cont(0.5)  WITHIN GROUP (ORDER BY commits)                                     AS co_p50, \
                 percentile_cont(0.9)  WITHIN GROUP (ORDER BY commits)                                     AS co_p90, \
                 percentile_cont(0.95) WITHIN GROUP (ORDER BY commits)                                     AS co_p95 \
             FROM sized",
        )
        .bind(repo_id)
        .bind(since)
        .bind(until)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        let n: i64 = row.try_get("n").map_err(map_sqlx)?;
        // Sample-size guard — § 15.9: with n < 5 percentile_cont
        // is mathematically defined but actionably noisy. Force
        // every triple to None and let the wire layer / UI render
        // a placeholder.
        let triple = |prefix: &str| -> Result<dp_domain::PercentileTriple, StoreError> {
            if n < 5 {
                return Ok(dp_domain::PercentileTriple::default());
            }
            Ok(dp_domain::PercentileTriple {
                p50: row.try_get(format!("{prefix}_p50").as_str()).map_err(map_sqlx)?,
                p90: row.try_get(format!("{prefix}_p90").as_str()).map_err(map_sqlx)?,
                p95: row.try_get(format!("{prefix}_p95").as_str()).map_err(map_sqlx)?,
            })
        };

        Ok(dp_domain::RepoPrSizeStats {
            sample_n: n,
            additions: triple("add")?,
            deletions: triple("del")?,
            total_lines: triple("tot")?,
            changed_files: triple("cf")?,
            commits: triple("co")?,
        })
    }

    pub(super) async fn ci_stats_for_repo_impl(
        &self,
        repo_id: Uuid,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
    ) -> Result<dp_domain::RepoCiStats, StoreError> {
        // CI workflow-run stats from the JSONB payload of
        // `workflow_run` events. Counts split by `conclusion`;
        // duration percentiles over `updated_at - run_started_at`
        // for rows where both timestamps parse and the delta is
        // strictly positive (negative / zero deltas would
        // otherwise distort the median for very fast cached
        // runs).
        //
        // Two filters keep the SQL safe against payloads missing
        // the keys (older fixtures, synthetic deliveries):
        //   * `payload ? 'conclusion'` for counts
        //   * `payload ? 'run_started_at' AND ? 'updated_at'` for
        //     duration percentiles
        //
        // Sample-size guard (SCOPE §15.9) applies to durations
        // only — counts are exact and useful even at small n.
        let row = sqlx::query(
            "WITH base AS ( \
                 SELECT \
                     payload->>'conclusion' AS conclusion, \
                     CASE \
                         WHEN payload ? 'run_started_at' AND payload ? 'updated_at' \
                         THEN EXTRACT(EPOCH FROM ( \
                                 (payload->>'updated_at')::timestamptz \
                                 - (payload->>'run_started_at')::timestamptz \
                             )) \
                         ELSE NULL \
                     END AS duration_s \
                 FROM dp_activity_events \
                 WHERE repo_id = $1 \
                   AND kind = 'workflow_run' \
                   AND ts >= $2 AND ts < $3 \
                   AND payload ? 'conclusion' \
             ) \
             SELECT \
                 COUNT(*)::bigint                                                 AS total_runs, \
                 COUNT(*) FILTER (WHERE conclusion = 'success')::bigint          AS success, \
                 COUNT(*) FILTER (WHERE conclusion = 'failure')::bigint          AS failure, \
                 COUNT(*) FILTER (WHERE conclusion = 'cancelled')::bigint        AS cancelled, \
                 COUNT(*) FILTER ( \
                     WHERE conclusion NOT IN ('success', 'failure', 'cancelled') \
                 )::bigint                                                        AS other, \
                 COUNT(*) FILTER (WHERE duration_s IS NOT NULL AND duration_s > 0)::bigint AS dur_n, \
                 percentile_cont(0.5)  WITHIN GROUP (ORDER BY duration_s) \
                     FILTER (WHERE duration_s IS NOT NULL AND duration_s > 0)    AS dur_p50, \
                 percentile_cont(0.9)  WITHIN GROUP (ORDER BY duration_s) \
                     FILTER (WHERE duration_s IS NOT NULL AND duration_s > 0)    AS dur_p90, \
                 percentile_cont(0.95) WITHIN GROUP (ORDER BY duration_s) \
                     FILTER (WHERE duration_s IS NOT NULL AND duration_s > 0)    AS dur_p95 \
             FROM base",
        )
        .bind(repo_id)
        .bind(since)
        .bind(until)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        let total_runs: i64 = row.try_get("total_runs").map_err(map_sqlx)?;
        let success: i64 = row.try_get("success").map_err(map_sqlx)?;
        let failure: i64 = row.try_get("failure").map_err(map_sqlx)?;
        let cancelled: i64 = row.try_get("cancelled").map_err(map_sqlx)?;
        let other: i64 = row.try_get("other").map_err(map_sqlx)?;
        let dur_n: i64 = row.try_get("dur_n").map_err(map_sqlx)?;

        let success_rate = if success + failure == 0 {
            None
        } else {
            Some(success as f64 / (success + failure) as f64)
        };
        let duration_seconds = if dur_n < 5 {
            dp_domain::PercentileTriple::default()
        } else {
            dp_domain::PercentileTriple {
                p50: row.try_get("dur_p50").map_err(map_sqlx)?,
                p90: row.try_get("dur_p90").map_err(map_sqlx)?,
                p95: row.try_get("dur_p95").map_err(map_sqlx)?,
            }
        };

        Ok(dp_domain::RepoCiStats {
            total_runs,
            success,
            failure,
            cancelled,
            other,
            success_rate,
            duration_sample_n: dur_n,
            duration_seconds,
        })
    }

    pub(super) async fn activity_heatmap_for_repo_impl(
        &self,
        repo_id: Uuid,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
        timezone: &str,
    ) -> Result<dp_domain::RepoActivityHeatmap, StoreError> {
        // `(day_of_week, hour_of_day)` histogram of activity
        // events for one repo. The grid is always dense — we left
        // outer join against `generate_series` so every cell is
        // present with a `0` count, and the caller doesn't have
        // to know which buckets the DB happened to see.
        //
        // `AT TIME ZONE $4` shifts the UTC `ts` into the
        // requested zone *before* extraction, so "8am" means 8am
        // local to the viewer. PG validates the zone string and
        // raises `invalid_parameter_value` (mapped to a
        // `StoreError::Backend` here) on bad input — the REST
        // layer catches typos before they reach SQL.
        //
        // Postgres' `EXTRACT(DOW ...)` returns 0 = Sunday … 6 =
        // Saturday; we re-map to the ISO convention (0 = Monday
        // … 6 = Sunday) in the SELECT so the wire format matches
        // [`HeatmapBucket`]'s docs.
        let rows = sqlx::query(
            "WITH grid AS ( \
                 SELECT d::int2 AS dow, h::int2 AS hour \
                 FROM generate_series(0, 6) AS d \
                 CROSS JOIN generate_series(0, 23) AS h \
             ), \
             counted AS ( \
                 SELECT \
                     ((EXTRACT(DOW  FROM (ts AT TIME ZONE $4))::int + 6) % 7)::int2 AS dow, \
                     EXTRACT(HOUR FROM (ts AT TIME ZONE $4))::int2 AS hour, \
                     COUNT(*)::bigint AS count \
                 FROM dp_activity_events \
                 WHERE repo_id = $1 \
                   AND ts >= $2 AND ts < $3 \
                 GROUP BY 1, 2 \
             ) \
             SELECT g.dow, g.hour, COALESCE(c.count, 0)::bigint AS count \
             FROM grid g \
             LEFT JOIN counted c ON c.dow = g.dow AND c.hour = g.hour \
             ORDER BY g.dow, g.hour",
        )
        .bind(repo_id)
        .bind(since)
        .bind(until)
        .bind(timezone)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        let mut buckets = Vec::with_capacity(168);
        let mut total: i64 = 0;
        for row in &rows {
            let dow: i16 = row.try_get("dow").map_err(map_sqlx)?;
            let hour: i16 = row.try_get("hour").map_err(map_sqlx)?;
            let count: i64 = row.try_get("count").map_err(map_sqlx)?;
            total += count;
            buckets.push(dp_domain::HeatmapBucket { dow, hour, count });
        }

        Ok(dp_domain::RepoActivityHeatmap {
            timezone: timezone.to_string(),
            total,
            buckets,
        })
    }

    pub(super) async fn review_velocity_for_repo_impl(
        &self,
        repo_id: Uuid,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
    ) -> Result<dp_domain::RepoReviewVelocity, StoreError> {
        // Time-to-merge straight from the merged-PR webhook
        // payload — both `created_at` (PR open) and `merged_at`
        // ship in the same row, so no self-join is needed.
        //
        // Strict-positive delta filter: clock skew between
        // GitHub-side timestamps has been observed to produce
        // `merged_at < created_at` by a handful of seconds; we
        // drop those rather than letting them turn into negative
        // durations that would compress the percentile distance.
        //
        // `EventKind::PullRequestMerged` is the wire kind for
        // closed-and-merged PRs (squash, rebase, or merge-commit
        // all funnel into the same event); closed-without-merge
        // is a different kind so we don't accidentally count
        // abandoned PRs.
        let row = sqlx::query(
            "WITH base AS ( \
                 SELECT \
                     CASE \
                         WHEN payload ? 'created_at' AND payload ? 'merged_at' \
                         THEN EXTRACT(EPOCH FROM ( \
                                 (payload->>'merged_at')::timestamptz \
                                 - (payload->>'created_at')::timestamptz \
                             )) \
                         ELSE NULL \
                     END AS ttm_s \
                 FROM dp_activity_events \
                 WHERE repo_id = $1 \
                   AND kind = 'pull_request_merged' \
                   AND ts >= $2 AND ts < $3 \
             ) \
             SELECT \
                 COUNT(*) FILTER (WHERE ttm_s IS NOT NULL AND ttm_s > 0)::bigint AS sample_n, \
                 percentile_cont(0.5)  WITHIN GROUP (ORDER BY ttm_s) \
                     FILTER (WHERE ttm_s IS NOT NULL AND ttm_s > 0) AS p50, \
                 percentile_cont(0.9)  WITHIN GROUP (ORDER BY ttm_s) \
                     FILTER (WHERE ttm_s IS NOT NULL AND ttm_s > 0) AS p90, \
                 percentile_cont(0.95) WITHIN GROUP (ORDER BY ttm_s) \
                     FILTER (WHERE ttm_s IS NOT NULL AND ttm_s > 0) AS p95 \
             FROM base",
        )
        .bind(repo_id)
        .bind(since)
        .bind(until)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        let sample_n: i64 = row.try_get("sample_n").map_err(map_sqlx)?;
        let time_to_merge_seconds = if sample_n < 5 {
            dp_domain::PercentileTriple::default()
        } else {
            dp_domain::PercentileTriple {
                p50: row.try_get("p50").map_err(map_sqlx)?,
                p90: row.try_get("p90").map_err(map_sqlx)?,
                p95: row.try_get("p95").map_err(map_sqlx)?,
            }
        };

        Ok(dp_domain::RepoReviewVelocity {
            sample_n,
            time_to_merge_seconds,
        })
    }

    pub(super) async fn contributor_diversity_for_repo_impl(
        &self,
        repo_id: Uuid,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
    ) -> Result<dp_domain::RepoContributorDiversity, StoreError> {
        // Bus-factor view of the repo's merged-PR authorship.
        //
        // The (event, user) grain matters: we *don't* dedupe to
        // "events with at least one author from user X". Counting
        // pairs means a co-authored PR splits its weight, which
        // matches the operational question — "how much load
        // disappears if X is unavailable?" — better than a
        // binary present/absent.
        //
        // The aggregate is computed in two passes inside one
        // round trip via CTEs: per-author counts, then totals
        // (distinct authors, sample size, top-1 and top-3 sums).
        // ARRAY_AGG ordered DESC + slicing in SQL keeps the
        // top-N picks server-side; we never ship author rows
        // back over the wire (SCOPE §4 — diversity, not ranking).
        //
        // §15.9: top1 / top3 shares are masked to NULL when
        // `sample_n < 5` — concentration ratios on n=2 always
        // look catastrophic and are noise.
        let row = sqlx::query(
            "WITH per_author AS ( \
                 SELECT ea.user_id, COUNT(*)::bigint AS c \
                 FROM dp_event_actors ea \
                 JOIN dp_activity_events e ON e.id = ea.event_id \
                 WHERE e.repo_id = $1 \
                   AND e.kind = 'pull_request_merged' \
                   AND e.ts >= $2 AND e.ts < $3 \
                   AND ea.role = 'author' \
                 GROUP BY ea.user_id \
             ), \
             ordered AS ( \
                 SELECT c FROM per_author ORDER BY c DESC \
             ) \
             SELECT \
                 COALESCE(SUM(c), 0)::bigint                          AS sample_n, \
                 COUNT(*)::bigint                                     AS distinct_authors, \
                 COALESCE((SELECT c FROM ordered LIMIT 1), 0)::bigint AS top1, \
                 COALESCE( \
                     (SELECT SUM(c) FROM (SELECT c FROM ordered LIMIT 3) t), \
                     0 \
                 )::bigint                                            AS top3 \
             FROM per_author",
        )
        .bind(repo_id)
        .bind(since)
        .bind(until)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        let sample_n: i64 = row.try_get("sample_n").map_err(map_sqlx)?;
        let distinct_authors: i64 = row.try_get("distinct_authors").map_err(map_sqlx)?;
        let top1: i64 = row.try_get("top1").map_err(map_sqlx)?;
        let top3: i64 = row.try_get("top3").map_err(map_sqlx)?;

        let (top1_share, top3_share) = if sample_n < 5 {
            (None, None)
        } else {
            let n = sample_n as f64;
            (Some(top1 as f64 / n), Some(top3 as f64 / n))
        };

        Ok(dp_domain::RepoContributorDiversity {
            sample_n,
            distinct_authors,
            top1_share,
            top3_share,
        })
    }

    pub(super) async fn get_repo_impl(&self, id: Uuid) -> Result<Option<Repo>, StoreError> {
        // Point lookup by PK. Used by the §8 issue write path to
        // resolve `repo_id -> (org_id, name)` before calling the
        // GitHub backend; without this override the default trait
        // impl returns `None` and every issue mutation 404s.
        let row = sqlx::query(
            "SELECT id, org_id, github_id, name FROM dp_repos WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.as_ref().map(row_to_repo).transpose()
    }

    pub(super) async fn list_repos_impl(&self, filter: &RepoListFilter) -> Result<Vec<RepoSummary>, StoreError> {
        // Open-issue count + last-activity timestamp are computed
        // via LATERAL subselects so the repo→issue join doesn't
        // multiply rows. Both subselects hit the indexes already
        // declared on dp_issues (repo_updated, org_state). For the
        // expected scale (100s of repos) this stays well under
        // 100ms; if it ever creeps up the obvious fix is a
        // materialised `dp_repo_stats` table refreshed by the
        // webhook worker.
        let q_norm = filter.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let rows = sqlx::query(
            "SELECT r.id, r.org_id, o.login AS org_login, r.name,
                    COALESCE(c.open_issue_count, 0) AS open_issue_count,
                    a.last_activity_at
             FROM dp_repos r
             JOIN dp_orgs o ON o.id = r.org_id
             LEFT JOIN LATERAL (
                 SELECT COUNT(*)::bigint AS open_issue_count
                 FROM dp_issues i WHERE i.repo_id = r.id AND i.state = 'open'
             ) c ON TRUE
             LEFT JOIN LATERAL (
                 SELECT MAX(updated_at) AS last_activity_at
                 FROM dp_issues i WHERE i.repo_id = r.id
             ) a ON TRUE
             WHERE ($1::uuid IS NULL OR r.org_id = $1)
               AND ($2::text IS NULL
                    OR r.name ILIKE '%' || $2 || '%'
                    OR o.login ILIKE '%' || $2 || '%')
             ORDER BY a.last_activity_at DESC NULLS LAST, o.login ASC, r.name ASC
             LIMIT $3 OFFSET $4",
        )
        .bind(filter.org_id)
        .bind(q_norm)
        .bind(filter.limit)
        .bind(filter.offset)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_repo_summary).collect()
    }

    pub(super) async fn count_repos_impl(&self, filter: &RepoListFilter) -> Result<i64, StoreError> {
        let q_norm = filter.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*)::bigint
             FROM dp_repos r
             JOIN dp_orgs o ON o.id = r.org_id
             WHERE ($1::uuid IS NULL OR r.org_id = $1)
               AND ($2::text IS NULL
                    OR r.name ILIKE '%' || $2 || '%'
                    OR o.login ILIKE '%' || $2 || '%')",
        )
        .bind(filter.org_id)
        .bind(q_norm)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(count)
    }

    pub(super) async fn get_repo_sync_status_impl(
        &self,
        repo_id: Uuid,
    ) -> Result<Option<RepoSyncStatus>, StoreError> {
        // Synthesise per-repo freshness from dp_fetch_cursors. The
        // table carries one row per (org, repo, resource_kind);
        // newest `updated_at` is the most recent successful pull.
        let row: Option<(Option<DateTime<Utc>>,)> = sqlx::query_as(
            "SELECT MAX(updated_at)
             FROM dp_fetch_cursors
             WHERE repo_id = $1",
        )
        .bind(repo_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some((Some(ts),)) => Ok(Some(RepoSyncStatus {
                last_synced_at: Some(ts),
                last_attempt_at: Some(ts),
                last_error: None,
            })),
            _ => Ok(Some(RepoSyncStatus {
                last_synced_at: None,
                last_attempt_at: None,
                last_error: None,
            })),
        }
    }
}
