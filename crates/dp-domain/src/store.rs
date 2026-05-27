//! [`Store`] — the persistence contract every dev-pulse surface
//! talks to. `dp-store-pg` implements it against Postgres.
//!
//! The method set is the v1 surface called out in TODO §Phase 1 plus
//! the obvious supporting upserts (orgs, teams, repos, memberships)
//! and run-log writes (`start_fetch_run`, `finish_fetch_run`). New
//! methods land here when a downstream crate needs them — not before.
//!
//! Errors flow through one type: [`StoreError`]. Concrete backends
//! (postgres, fakes in tests) wrap their native errors into
//! `StoreError::Backend(Box<dyn Error + Send + Sync>)` rather than
//! leaking sqlx / tokio-postgres types up the stack — that's what
//! lets `dp-domain` stay storage-agnostic per TODO §0.6.

use std::error::Error as StdError;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::app_install::OrgAppInstall;
use crate::audit::AuditEntry;
use crate::event::{ActivityEvent, ActorRole, EventActor, EventKind};
use crate::fetch::{FetchCursor, FetchRun, FetchRunErrorSample, FetchRunKind, ResourceKind};
use crate::freshness::DataAsOf;
use crate::inbox::{InboxIssueRow, InboxStatus, UserIssueState};
use crate::issue::{Issue, IssueState, IssueUpsert, IssueUpsertOutcome, RepoSummary};
use crate::issue_dates::{
    IssueDates, ProjectV2MirrorTask, ProjectV2MirrorTaskKind, RepoProjectLink,
};
use crate::issue_mutation::{IssueMutation, IssueMutationResult};
use crate::membership::Membership;
use crate::org::Org;
use crate::pin::Pin;
use crate::repo::{Repo, RepoMetadata};
use crate::tag::Tag;
use crate::tag_link::{TagLink, TagLinkKind};
use crate::team::Team;
use crate::user::User;
use crate::webhook::WebhookDelivery;
use crate::window::Window;

/// All [`Store`] methods return `Result<_, StoreError>`. Variants are
/// the smallest set we can usefully distinguish at the boundary
/// without leaking backend types.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// The requested row does not exist.
    #[error("not found: {entity} {id}")]
    NotFound {
        /// Entity name (`"user"`, `"org"`, …) — free-form for now.
        entity: &'static str,
        /// Identifier looked up (rendered with `Display`).
        id: String,
    },

    /// A unique constraint violation that the caller can reasonably
    /// recover from (e.g. webhook replay — same `delivery_id` twice).
    #[error("conflict: {0}")]
    Conflict(String),

    /// Input that failed a domain invariant the schema does not catch
    /// (e.g. a window with `end <= start`). Reserved for future use;
    /// listed now so `non_exhaustive` is honest.
    #[error("invalid input: {0}")]
    Invalid(String),

    /// Anything else from the backend — connection drops, serializer
    /// errors, hard SQL failures. Boxed so we don't drag sqlx into
    /// `dp-domain`.
    #[error("backend error: {0}")]
    Backend(#[source] Box<dyn StdError + Send + Sync>),
}

/// p50 / p90 / p95 percentile triple over a numeric distribution.
///
/// All three are `Some(_)` when the underlying sample has at least
/// the §15.9 minimum (`n >= 5`); below that, every percentile is
/// `None` and the caller should render "—" rather than a noisy
/// single-data-point reading. The actual sample size travels
/// alongside (e.g. on [`RepoPrSizeStats::sample_n`]) so the UI can
/// communicate why the values are missing.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct PercentileTriple {
    /// 50th percentile (median).
    pub p50: Option<f64>,
    /// 90th percentile.
    pub p90: Option<f64>,
    /// 95th percentile.
    pub p95: Option<f64>,
}

/// Per-repo pull-request size distribution over a time window.
///
/// SCOPE §4 fit: every percentile describes the **repo's** change
/// volume, never an individual contributor's. The leaderboard /
/// user-report surfaces explicitly do not call this method.
///
/// Backed by the JSONB payload GitHub already ships on every
/// `pull_request` webhook (`additions`, `deletions`, `changed_files`,
/// `commits`) — no schema change, no extra API call. Scope is
/// `EventKind::PullRequestMerged` so closed-without-merge PRs
/// (which routinely include speculative diff sizes) don't skew
/// the distribution.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RepoPrSizeStats {
    /// Number of merged PRs whose payload carried diff-size fields
    /// inside `[since, until)`. Below 5 the `*_lines` / `files` /
    /// `commits` percentiles are all `None`.
    pub sample_n: i64,
    /// Lines added (`payload->>'additions'`).
    pub additions: PercentileTriple,
    /// Lines removed (`payload->>'deletions'`).
    pub deletions: PercentileTriple,
    /// `additions + deletions` per PR.
    pub total_lines: PercentileTriple,
    /// Files touched (`payload->>'changed_files'`).
    pub changed_files: PercentileTriple,
    /// Commits in the PR (`payload->>'commits'`).
    pub commits: PercentileTriple,
}

/// Per-repo CI workflow-run statistics over a time window.
///
/// SCOPE §4 fit: counts and durations describe the **repo's** CI
/// health, never an individual contributor's. The leaderboard /
/// user-report surfaces explicitly do not call this method.
///
/// Backed by the JSONB payload GitHub ships on every
/// `workflow_run.completed` webhook (`conclusion`,
/// `run_started_at`, `updated_at`, `name`). No schema change, no
/// extra API call. Duration percentiles use `updated_at -
/// run_started_at` for the run; the `n < 5` sample-size guard
/// applies (SCOPE §15.9).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RepoCiStats {
    /// Total workflow runs in `[since, until)` whose payload
    /// carried a `conclusion`.
    pub total_runs: i64,
    /// `conclusion = 'success'`.
    pub success: i64,
    /// `conclusion = 'failure'`.
    pub failure: i64,
    /// `conclusion = 'cancelled'`.
    pub cancelled: i64,
    /// Any other terminal conclusion (`skipped`, `neutral`,
    /// `timed_out`, `action_required`, `stale`).
    pub other: i64,
    /// `success / (success + failure)` as a fraction in `[0.0,
    /// 1.0]`, or `None` when `success + failure == 0` (cancellations
    /// and skips don't carry signal here).
    pub success_rate: Option<f64>,
    /// Number of runs whose payload carried both
    /// `run_started_at` and `updated_at` and a finite duration
    /// (the input set for `duration_seconds`).
    pub duration_sample_n: i64,
    /// Percentiles over `updated_at - run_started_at`, in seconds.
    /// `None` triple when `duration_sample_n < 5`.
    pub duration_seconds: PercentileTriple,
}

/// Repo-scoped activity heatmap: counts of activity events
/// bucketed by `(day_of_week, hour_of_day)` in a fixed timezone.
///
/// The bucket grid is dense — 7 days × 24 hours = 168 cells, all
/// returned even when the count is zero — so the UI never has to
/// fill in gaps and the JSON is self-describing.
///
/// SCOPE §4 fit: describes the **repo's** activity cadence (when
/// the team tends to push / open PRs / merge), never an
/// individual contributor's. Intentionally not surfaced on the
/// user-report or leaderboard pages.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RepoActivityHeatmap {
    /// IANA timezone name used to derive the local day-of-week
    /// and hour-of-day (e.g. `"UTC"`, `"America/Los_Angeles"`).
    pub timezone: String,
    /// Total events across all buckets in the window — handy for
    /// the "n too small" guard at the UI layer.
    pub total: i64,
    /// 168 cells: one per `(dow, hour)` pair. `dow` is 0..=6
    /// using the ISO convention (0 = Monday, 6 = Sunday), `hour`
    /// is 0..=23.
    pub buckets: Vec<HeatmapBucket>,
}

/// One `(day_of_week, hour_of_day)` cell in a [`RepoActivityHeatmap`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HeatmapBucket {
    /// 0 = Monday … 6 = Sunday (ISO 8601).
    pub dow: i16,
    /// 0..=23 in the heatmap's timezone.
    pub hour: i16,
    /// Number of events whose `ts` fell in this bucket.
    pub count: i64,
}

/// Repo-scoped review-velocity statistics: how long PRs in this
/// repo took to go from `created_at` to `merged_at`.
///
/// Backed by the JSONB payload GitHub already ships on every
/// `pull_request.closed` (merged) webhook — both timestamps are
/// in the same row, so no self-join is needed. PRs closed
/// without merging are excluded by `EventKind::PullRequestMerged`.
///
/// SCOPE §4 fit: percentiles describe the **repo's** merge
/// cadence (how quickly the team turns code around), never an
/// individual contributor's. The leaderboard / user-report
/// surfaces explicitly do not call this method.
///
/// Sample-size guard (SCOPE §15.9): percentiles are `None` when
/// `sample_n < 5` so a 1-PR-this-month window doesn't look like
/// "median merge: 4h".
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RepoReviewVelocity {
    /// Number of merged PRs in `[since, until)` whose payload
    /// carried both `created_at` and `merged_at` with a
    /// strictly-positive delta.
    pub sample_n: i64,
    /// Percentiles over `merged_at - created_at`, in seconds.
    /// `None` triple when `sample_n < 5`.
    pub time_to_merge_seconds: PercentileTriple,
}

/// Repo-scoped contributor-diversity ("bus factor") statistics.
///
/// Answers the operational question *"if our top contributor
/// went on leave, how much of this repo's merge volume would
/// stall?"* — without naming anyone. Concentration is reported
/// as the **share of merges attributable to the top 1 / top 3
/// authors**, alongside the raw distinct-author count.
///
/// SCOPE §4 fit: this is a property of the **repo's** risk
/// profile, not a ranking of contributors. The wire shape
/// deliberately omits any user identifier, and the leaderboard
/// / user-report surfaces do not call this method.
///
/// Backed by the existing `(event, author)` join on
/// `EventKind::PullRequestMerged` events — no schema change. We
/// count one row per *(merged-PR-event, author-user)* pair, so
/// a PR with multiple authors contributes a small fraction to
/// each rather than double-counting.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RepoContributorDiversity {
    /// Total (event, author) pairs across merged PRs in
    /// `[since, until)`. Used as the denominator for the
    /// `top*_share` fields and the §15.9 sample-size guard.
    pub sample_n: i64,
    /// Number of distinct PR authors observed in the window.
    pub distinct_authors: i64,
    /// Fraction in `[0.0, 1.0]` of `sample_n` attributable to
    /// the single most-active author. `None` when `sample_n < 5`
    /// — the "bus factor" framing isn't meaningful at low n.
    pub top1_share: Option<f64>,
    /// Fraction in `[0.0, 1.0]` of `sample_n` attributable to
    /// the three most-active authors combined. Equals
    /// `top1_share` when `distinct_authors <= 3`. `None` under
    /// the same n < 5 guard.
    pub top3_share: Option<f64>,
}

/// The set of [`EventActor`] rows joined back through their parent
/// [`ActivityEvent`], shaped for the report layer's de-dup
/// (`(user_id, event_id)`) and role-filter logic.
///
/// This is the row type `list_event_actor_rows_in_window` returns.
/// It is the smallest projection that lets reports compute all three
/// org-scope lenses (SCOPE §8.1) without a second query per row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventActorRow {
    /// FK to `activity_events.id`.
    pub event_id: Uuid,
    /// User credited for this row.
    pub user_id: Uuid,
    /// Role the user played (filter target per metric).
    pub role: ActorRole,
    /// Org the event happened in (lens / scope target).
    pub org_id: Uuid,
    /// Repo the event happened in.
    pub repo_id: Uuid,
    /// Event kind (filter target for "PRs merged" etc.).
    pub kind: EventKind,
    /// Event timestamp (UTC). Already filtered to the window, but
    /// returned so the trend-bucket logic can group by day/week.
    pub ts: DateTime<Utc>,
}

/// The persistence surface dev-pulse talks to.
///
/// Implementations live outside this crate (`dp-store-pg` for
/// Postgres, in-memory fakes in tests). Every method is `async`.
#[async_trait]
pub trait Store: Send + Sync {
    // ---- users ----------------------------------------------------

    /// Upsert by `github_id`. Returns the resulting row (after the
    /// upsert is applied) so the caller can see the assigned `id`.
    async fn upsert_user(&self, user: &User) -> Result<User, StoreError>;

    /// Fetch by primary key.
    async fn get_user(&self, id: Uuid) -> Result<User, StoreError>;

    /// Fetch by GitHub numeric id (the stable id GitHub exposes).
    async fn get_user_by_github_id(&self, github_id: i64) -> Result<User, StoreError>;

    /// Look up a non-deleted user by GitHub login. Returns `Ok(None)`
    /// when no row matches. Used by the webhook / commit-trailer
    /// path to avoid minting a synthetic duplicate when GitHub has
    /// already given us a real `github_id` row for the same login
    /// via the reconciler.
    ///
    /// If multiple rows share the login (e.g. a synthetic + a real
    /// row created in different orderings), implementations should
    /// prefer the row with the *positive* (real) `github_id` so the
    /// caller can collapse onto the canonical row.
    ///
    /// Default impl falls back to a `list_users` scan so test fakes
    /// don't need to override; production backends should use the
    /// `dp_users_login_idx` index.
    async fn find_user_by_login(&self, login: &str) -> Result<Option<User>, StoreError> {
        let needle = login.to_ascii_lowercase();
        let mut best: Option<User> = None;
        for u in self.list_users().await? {
            if u.login.to_ascii_lowercase() != needle {
                continue;
            }
            // Prefer a real (positive) github_id over a synthetic one;
            // among reals, prefer the lowest (oldest) id so this agrees
            // with the canonical rule in migration 0003.
            let better = match &best {
                None => true,
                Some(cur) => match (cur.github_id >= 0, u.github_id >= 0) {
                    (false, true) => true,
                    (true, true) => u.github_id < cur.github_id,
                    (false, false) => u.github_id < cur.github_id,
                    (true, false) => false,
                },
            };
            if better {
                best = Some(u);
            }
        }
        Ok(best)
    }

    /// List all non-deleted users.
    async fn list_users(&self) -> Result<Vec<User>, StoreError>;

    /// Set the operator-controlled role on a user
    /// (DOCS/SCOPE-AUTHZ-USERS.md §3). Returns the post-update row.
    ///
    /// Default impl returns `StoreError::Backend` so test fakes that
    /// don't need the surface keep compiling; production behavior
    /// lives on `PgStore`.
    async fn set_user_role(
        &self,
        _id: Uuid,
        _role: crate::user::Role,
    ) -> Result<User, StoreError> {
        Err(StoreError::Backend(
            "set_user_role not implemented by this backend".into(),
        ))
    }

    /// Soft-delete + pseudonymise (TODO §0.5). Rewrites
    /// `login`/`email`/`name` to `deleted-user-<hash>` form, sets
    /// `deleted_at`, leaves the row id stable so referential
    /// integrity holds.
    async fn pseudonymise_user(&self, id: Uuid) -> Result<(), StoreError>;

    // ---- identities (users.md §4 Slice A) ------------------------
    //
    // Every method has a default impl so the many in-memory `Store`
    // fakes in the workspace keep compiling. Production behavior
    // lives on `PgStore`; tests that care about the identity
    // surface override on a per-fake basis.

    /// List every GitHub identity claimed by `user_id`, primary
    /// first then by `linked_at DESC`. Empty vec when the user has
    /// no identities (`identity_set_empty` is enforced at the
    /// principal layer, not here).
    async fn list_identities_for_user(
        &self,
        _user_id: Uuid,
    ) -> Result<Vec<crate::identity::UserIdentity>, StoreError> {
        Ok(vec![])
    }

    /// Look up the dp-user that owns `github_user_id`. Returns
    /// `Ok(None)` when the GitHub identity is not claimed by
    /// anyone in dev-pulse.
    async fn find_user_by_github_user_id(
        &self,
        _github_user_id: i64,
    ) -> Result<Option<crate::user::User>, StoreError> {
        Ok(None)
    }

    /// Reserve an OAuth `state` nonce for a link round-trip. The
    /// caller is expected to redirect to GitHub with the returned
    /// nonce as the `state` query parameter.
    async fn create_identity_link_pending(
        &self,
        _pending: &crate::identity::IdentityLinkPending,
    ) -> Result<(), StoreError> {
        Err(StoreError::Backend(
            "identity link pending not implemented by this backend".into(),
        ))
    }

    /// Atomically look up + delete a pending row by nonce. Returns
    /// `Ok(None)` when the nonce is unknown or already consumed —
    /// the REST handler treats both as `IdentityLinkRejection::
    /// NonceInvalid`.
    async fn consume_identity_link_pending(
        &self,
        _nonce: Uuid,
    ) -> Result<Option<crate::identity::IdentityLinkPending>, StoreError> {
        Ok(None)
    }

    /// Delete every `dp_identity_link_pending` row past its
    /// `expires_at`. Returns the number of rows removed. Intended
    /// to be called from a periodic GC sweep; the OAuth callback
    /// also rejects expired rows on the read path so missing the
    /// sweep degrades to wasted disk, not security.
    async fn purge_expired_identity_link_pending(
        &self,
        _now: DateTime<Utc>,
    ) -> Result<u64, StoreError> {
        Ok(0)
    }

    /// Link a GitHub identity to a dp-user. Implementations must
    /// enforce the "identity belongs to at most one dp-user" rule
    /// and return [`StoreError::Conflict`] (mapped to HTTP 409 +
    /// audit `IDENTITY_CLAIM_CONFLICT`) when another dp-user
    /// already owns `github_user_id`. If the user has no other
    /// identities, the new row must be stamped `is_primary = true`
    /// in the same transaction so the
    /// `dp_user_identities_primary_idx` invariant holds.
    async fn link_identity(
        &self,
        _identity: &crate::identity::UserIdentity,
    ) -> Result<crate::identity::UserIdentity, StoreError> {
        Err(StoreError::Backend(
            "link_identity not implemented by this backend".into(),
        ))
    }

    /// Unlink an identity. Implementations must reject (return
    /// [`StoreError::Invalid`]) if removing the row would leave
    /// the dp-user with zero identities, or if the row is the
    /// current primary (the caller must `set_primary_identity` to
    /// another row first). ON DELETE CASCADE drops the
    /// `dp_membership_identities` rows; the implementation is
    /// responsible for collapsing now-unprovenanced
    /// `dp_memberships` in the same transaction.
    async fn unlink_identity(
        &self,
        _user_id: Uuid,
        _github_user_id: i64,
    ) -> Result<(), StoreError> {
        Err(StoreError::Backend(
            "unlink_identity not implemented by this backend".into(),
        ))
    }

    /// Flip the primary flag to `(user_id, github_user_id)`. Done
    /// in one transaction so no reader observes two primary rows
    /// for the same user. Returns [`StoreError::NotFound`] if the
    /// identity is not owned by `user_id`.
    async fn set_primary_identity(
        &self,
        _user_id: Uuid,
        _github_user_id: i64,
    ) -> Result<(), StoreError> {
        Err(StoreError::Backend(
            "set_primary_identity not implemented by this backend".into(),
        ))
    }

    // ---- orgs / teams / repos ------------------------------------

    /// Upsert org by `github_id`.
    async fn upsert_org(&self, org: &Org) -> Result<Org, StoreError>;

    /// Upsert team by `(org_id, github_id)`.
    async fn upsert_team(&self, team: &Team) -> Result<Team, StoreError>;

    /// Upsert repo by `(org_id, github_id)`.
    async fn upsert_repo(&self, repo: &Repo) -> Result<Repo, StoreError>;

    /// Upsert the mutable GitHub-side snapshot for a repo
    /// ([`RepoMetadata`]). Default implementation is a no-op so
    /// backends and test fakes that don't model metadata stay
    /// unaffected; the Postgres backend overrides this.
    ///
    /// Implementations should preserve known-good nullable values
    /// when the supplied row's optional field is `None`
    /// (`COALESCE(EXCLUDED.x, dp_repo_metadata.x)`). Counter fields
    /// (`stars`, `forks`, `watchers`, `open_issues_remote`) are
    /// always written as supplied — the caller is responsible for
    /// only invoking this method with a payload that carried fresh
    /// counter values.
    async fn upsert_repo_metadata(
        &self,
        _metadata: &RepoMetadata,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    /// Read the [`RepoMetadata`] snapshot for a repo, if one has
    /// been recorded. Default returns `None` so backends that don't
    /// model metadata yet present as "snapshot unavailable" to the
    /// UI rather than erroring.
    async fn get_repo_metadata(
        &self,
        _repo_id: Uuid,
    ) -> Result<Option<RepoMetadata>, StoreError> {
        Ok(None)
    }

    /// Aggregate the pull-request size distribution for one repo
    /// across `[since, until)` (UTC, half-open). See
    /// [`RepoPrSizeStats`].
    ///
    /// Default returns a zero-sample row so test fakes don't have
    /// to model JSONB aggregation. The Postgres backend overrides.
    async fn pr_size_stats_for_repo(
        &self,
        _repo_id: Uuid,
        _since: DateTime<Utc>,
        _until: DateTime<Utc>,
    ) -> Result<RepoPrSizeStats, StoreError> {
        Ok(RepoPrSizeStats {
            sample_n: 0,
            additions: PercentileTriple::default(),
            deletions: PercentileTriple::default(),
            total_lines: PercentileTriple::default(),
            changed_files: PercentileTriple::default(),
            commits: PercentileTriple::default(),
        })
    }

    /// Aggregate CI workflow-run statistics for one repo across
    /// `[since, until)` (UTC, half-open). See [`RepoCiStats`].
    ///
    /// Default returns a zero-sample row so test fakes don't have
    /// to model JSONB aggregation. The Postgres backend overrides.
    async fn ci_stats_for_repo(
        &self,
        _repo_id: Uuid,
        _since: DateTime<Utc>,
        _until: DateTime<Utc>,
    ) -> Result<RepoCiStats, StoreError> {
        Ok(RepoCiStats {
            total_runs: 0,
            success: 0,
            failure: 0,
            cancelled: 0,
            other: 0,
            success_rate: None,
            duration_sample_n: 0,
            duration_seconds: PercentileTriple::default(),
        })
    }

    /// Aggregate `(day_of_week, hour_of_day)` activity counts for
    /// one repo across `[since, until)` (UTC, half-open). The
    /// caller supplies an IANA `timezone` to bucket against — UI
    /// surfaces typically pass the viewer's local zone so "9am"
    /// means 9am to whoever is reading.
    ///
    /// Default returns an empty 168-cell grid so test fakes don't
    /// have to model `EXTRACT()` aggregation. The Postgres
    /// backend overrides.
    async fn activity_heatmap_for_repo(
        &self,
        _repo_id: Uuid,
        _since: DateTime<Utc>,
        _until: DateTime<Utc>,
        timezone: &str,
    ) -> Result<RepoActivityHeatmap, StoreError> {
        let mut buckets = Vec::with_capacity(168);
        for dow in 0..7 {
            for hour in 0..24 {
                buckets.push(HeatmapBucket { dow, hour, count: 0 });
            }
        }
        Ok(RepoActivityHeatmap {
            timezone: timezone.to_string(),
            total: 0,
            buckets,
        })
    }

    /// Aggregate review-velocity (time-to-merge) statistics for
    /// one repo across `[since, until)` (UTC, half-open). See
    /// [`RepoReviewVelocity`].
    ///
    /// Default returns a zero-sample row so test fakes don't have
    /// to model JSONB aggregation. The Postgres backend overrides.
    async fn review_velocity_for_repo(
        &self,
        _repo_id: Uuid,
        _since: DateTime<Utc>,
        _until: DateTime<Utc>,
    ) -> Result<RepoReviewVelocity, StoreError> {
        Ok(RepoReviewVelocity {
            sample_n: 0,
            time_to_merge_seconds: PercentileTriple::default(),
        })
    }

    /// Aggregate contributor-diversity ("bus factor") statistics
    /// for one repo across `[since, until)` (UTC, half-open). See
    /// [`RepoContributorDiversity`].
    ///
    /// Default returns a zero-sample row so test fakes don't have
    /// to model the (event, actor) join. The Postgres backend
    /// overrides.
    async fn contributor_diversity_for_repo(
        &self,
        _repo_id: Uuid,
        _since: DateTime<Utc>,
        _until: DateTime<Utc>,
    ) -> Result<RepoContributorDiversity, StoreError> {
        Ok(RepoContributorDiversity {
            sample_n: 0,
            distinct_authors: 0,
            top1_share: None,
            top3_share: None,
        })
    }

    /// Upsert a `(user, org)` membership, preserving `home_org` if
    /// already set — `set_home_org` is the only way to change it.
    async fn upsert_membership(&self, membership: &Membership) -> Result<Membership, StoreError>;

    /// List memberships for one user. Empty vec if none.
    async fn list_memberships_for_user(&self, user_id: Uuid)
        -> Result<Vec<Membership>, StoreError>;

    /// Set / clear the home-org label on a `(user, org)` membership
    /// (SCOPE §3 manual mapping). `None` clears it.
    async fn set_home_org(
        &self,
        user_id: Uuid,
        org_id: Uuid,
        home_org: Option<Uuid>,
    ) -> Result<(), StoreError>;

    /// Atomically flip the user's home org to `(user_id, org_id)`.
    ///
    /// Postcondition: among the user's memberships, exactly one row
    /// has `home_org = Some(org_id)` — the `(user_id, org_id)` row —
    /// and every other membership row for the same user has
    /// `home_org = None`. Implementations must apply the
    /// set-and-clear in one transaction so a reader can never observe
    /// two `home_org` values for the same user (Phase 4 D-home-org
    /// atomicity).
    ///
    /// Returns [`StoreError::NotFound`] if there is no `(user_id,
    /// org_id)` membership row to flip — the caller has to add the
    /// user to the org first.
    ///
    /// Default impl is the obvious non-atomic two-step using
    /// [`Self::set_home_org`]; production backends override it for
    /// the transactional guarantee.
    async fn set_home_org_for_user(
        &self,
        user_id: Uuid,
        org_id: Uuid,
    ) -> Result<(), StoreError> {
        // Best-effort default: clear-all-then-set. Backends that care
        // about the atomicity guarantee override this.
        let memberships = self.list_memberships_for_user(user_id).await?;
        for m in &memberships {
            if m.org_id != org_id && m.home_org.is_some() {
                self.set_home_org(user_id, m.org_id, None).await?;
            }
        }
        self.set_home_org(user_id, org_id, Some(org_id)).await
    }

    /// List every org dev-pulse has observed. Stage 4 of Phase 4
    /// surfaces this for `GET /orgs`. Default impl returns an empty
    /// vec so test fakes that don't seed orgs stay compiling.
    async fn list_orgs(&self) -> Result<Vec<crate::org::Org>, StoreError> {
        Ok(vec![])
    }

    /// List every team inside one org. Stage 4 of Phase 4 surfaces
    /// this for `GET /teams?org_id=…`.
    async fn list_teams_for_org(
        &self,
        _org_id: Uuid,
    ) -> Result<Vec<crate::team::Team>, StoreError> {
        Ok(vec![])
    }

    /// List the users that have a membership in `org_id`. Stage 4 of
    /// Phase 4 surfaces this for `GET /users?org_id=…`.
    async fn list_users_for_org(
        &self,
        _org_id: Uuid,
    ) -> Result<Vec<crate::user::User>, StoreError> {
        Ok(vec![])
    }

    // ---- repos / issues read surface (workflow drill-down) -------

    /// Paginated listing for the workflow's "Repos" pane. Backs
    /// `GET /repos` (dp-rest). Filters are conjunctive; defaults
    /// (no filter) return every repo across every org the store
    /// knows about.
    ///
    /// Implementations should return rows ordered by
    /// `last_activity_at DESC NULLS LAST, name ASC` so the UI gets
    /// "hottest first" for free.
    async fn list_repos(
        &self,
        _filter: &RepoListFilter,
    ) -> Result<Vec<RepoSummary>, StoreError> {
        Ok(vec![])
    }

    /// Total count matching the same `filter` (ignoring `limit` /
    /// `offset`). Pairs with [`Store::list_repos`] so the UI can
    /// render an "X of Y" pager.
    async fn count_repos(&self, _filter: &RepoListFilter) -> Result<i64, StoreError> {
        Ok(0)
    }

    /// Paginated listing for the workflow's "Issues" pane. Backs
    /// `GET /issues`. Sort: `updated_at DESC` to match GitHub's
    /// default "recently updated" view.
    async fn list_issues(
        &self,
        _filter: &IssueListFilter,
    ) -> Result<Vec<Issue>, StoreError> {
        Ok(vec![])
    }

    /// Total count for the issue filter.
    async fn count_issues(&self, _filter: &IssueListFilter) -> Result<i64, StoreError> {
        Ok(0)
    }

    /// Fetch a single repo row by primary key. Used by the §8 issue
    /// write path to resolve `repo_id -> (org_id, name)` before
    /// calling the GitHub backend. Default impl returns `None`;
    /// in-memory test fakes that don't seed repos stay compiling.
    async fn get_repo(&self, _id: Uuid) -> Result<Option<crate::repo::Repo>, StoreError> {
        Ok(None)
    }

    /// Fetch a single org row by primary key. Pairs with
    /// [`Store::get_repo`] to resolve `(org_login, repo_name)` for
    /// the §8 GitHub call. Default impl scans [`Store::list_orgs`]
    /// so storage implementations that override the listing surface
    /// don't need a separate point lookup.
    async fn get_org(&self, id: Uuid) -> Result<Option<crate::org::Org>, StoreError> {
        Ok(self.list_orgs().await?.into_iter().find(|o| o.id == id))
    }

    /// Fetch a single issue by primary key. The §8 detail pane
    /// uses this to re-read after a successful CAS write.
    async fn get_issue(&self, _id: Uuid) -> Result<Option<Issue>, StoreError> {
        Ok(None)
    }

    /// Fetch a single issue by `(repo_id, number)`. Backs
    /// `GET /repos/{repo_id}/issues/{number}` — the canonical
    /// deep-link shape the audit log already records.
    async fn get_issue_by_repo_and_number(
        &self,
        _repo_id: Uuid,
        _number: i64,
    ) -> Result<Option<Issue>, StoreError> {
        Ok(None)
    }

    /// Insert-or-update an issue row from an ingest payload
    /// (`IssueUpsert`). Returns the post-write [`Issue`] (so the
    /// caller can echo it / hand it to the inbox layer) plus an
    /// [`IssueUpsertOutcome`] reporting what actually happened.
    ///
    /// **Versioning.** On insert `version = 1`. On update, the
    /// store bumps `version` by 1 *only* when the inbound
    /// `updated_at` is strictly newer than the row's local
    /// `updated_at`; otherwise the call is a no-op
    /// ([`IssueUpsertOutcome::Skipped`]) — this keeps the §8 CAS
    /// counter monotonic without churn from re-backfills.
    ///
    /// **§13.7 reconciler guard.** When the row is in
    /// `pending_remote = TRUE` and `pending_remote_at` is younger
    /// than `pending_remote_window`, the upsert refuses to write
    /// and returns [`IssueUpsertOutcome::Deferred`]. The caller
    /// (webhook drain loop or CLI backfill) decides whether to
    /// buffer or skip — the row stays untouched either way so the
    /// in-flight optimistic write lands first.
    ///
    /// The default impl returns
    /// (synthetic `Issue` from `upsert`, `Skipped`) so in-memory
    /// fakes that don't implement the column compile unchanged.
    /// Production stores override.
    async fn upsert_issue_from_github(
        &self,
        upsert: &IssueUpsert,
        _pending_remote_window: chrono::Duration,
    ) -> Result<(Issue, IssueUpsertOutcome), StoreError> {
        // Synthetic row — never persisted; tests that exercise the
        // real ingest path use the dp-store-pg impl.
        let issue = Issue {
            id: Uuid::nil(),
            org_id: upsert.org_id,
            repo_id: upsert.repo_id,
            github_id: upsert.github_id,
            number: upsert.number,
            title: upsert.title.clone(),
            body: upsert.body.clone(),
            state: upsert.state,
            labels: upsert.labels.clone(),
            assignees: upsert.assignees.clone(),
            milestone: upsert.milestone.clone(),
            version: 1,
            github_node_id: upsert.github_node_id.clone(),
            updated_at: upsert.updated_at,
            is_local: false,
        };
        Ok((issue, IssueUpsertOutcome::Skipped))
    }

    /// Create a brand-new **local-only** issue (SCOPE.md §4.1
    /// amendment). Unlike [`Self::upsert_issue_from_github`] this
    /// path does *not* go through GitHub at all: the row is
    /// inserted directly with `is_local = TRUE` and synthetic
    /// negative `github_id` / `number` allocated from the per-repo
    /// `dp_repos.local_issue_counter`. Returns the materialised
    /// `Issue` so the caller (REST handler) can echo it back and
    /// attach it to the project / view in the same request.
    ///
    /// Default impl returns
    /// [`StoreError::Invalid`] so in-memory fakes that don't
    /// model this surface compile without forcing every test
    /// fixture to stub it.
    async fn create_local_issue(
        &self,
        _org_id: Uuid,
        _repo_id: Uuid,
        _title: &str,
        _body: Option<&str>,
    ) -> Result<Issue, StoreError> {
        Err(StoreError::Invalid(
            "create_local_issue not implemented by this Store".into(),
        ))
    }

    /// SCOPE.md §4.1.1 — direct field update for a local-only
    /// issue. CAS-gated on `expected_version` (the dp_issues row's
    /// `version` the UI rendered the form against). Any `Some(_)`
    /// field on the patch is written; `None` lanes leave the
    /// existing value untouched. State transitions accept
    /// `"open"` / `"closed"` strings and stamp `closed_at`
    /// accordingly. Returns the post-write [`Issue`].
    ///
    /// Returns [`StoreError::Conflict`] on stale `expected_version`
    /// — the REST handler maps this to `409 stale_local_version`
    /// so the UI's CAS surface stays the same as for GitHub-backed
    /// rows.
    ///
    /// Default impl is the same not-implemented stub as
    /// [`Self::create_local_issue`] for the same reasons.
    async fn update_local_issue(
        &self,
        _issue_id: Uuid,
        _expected_version: i64,
        _title: Option<&str>,
        _body: Option<Option<&str>>,
        _state: Option<&str>,
        _labels: Option<&[String]>,
        _assignees: Option<&[String]>,
    ) -> Result<Issue, StoreError> {
        Err(StoreError::Invalid(
            "update_local_issue not implemented by this Store".into(),
        ))
    }

    // ---- per-user inbox (triage spine, slice 1) -------------------
    //
    // Backs the `★ My queue` smart view + inbox UX
    // (`linear-projects-idea.md` §3.8). All methods key on
    // `(user_id, issue_id)`; row absence means "default state" —
    // implicitly `Inbox`, `last_seen_version = 0`. The store layer
    // materialises that convention on read so callers never have
    // to special-case the missing row.

    /// Issue rows for the user's inbox view, with the per-user
    /// inbox metadata folded in (unread bit + status +
    /// snoozed_until). The filter narrows the candidate issue set
    /// in the same way as [`list_issues`]; the join with
    /// `dp_user_issue_state` adds:
    ///
    ///   * `status <> 'done'` (Done rows are dismissed and never
    ///     appear in the inbox view), and
    ///   * `status <> 'snoozed' OR snoozed_until < now()` (active
    ///     snoozes are hidden; expired snoozes surface again).
    ///
    /// Sort: `updated_at DESC` (same as `list_issues`).
    ///
    /// Default impl returns empty — only `dp-store-pg` provides a
    /// real implementation; the in-memory fakes used by other
    /// crates do not need inbox semantics.
    async fn list_inbox_issues(
        &self,
        _user_id: Uuid,
        _filter: &IssueListFilter,
    ) -> Result<Vec<InboxIssueRow>, StoreError> {
        Ok(vec![])
    }

    /// Total count of inbox-visible rows for the same filter that
    /// would drive [`list_inbox_issues`]. Matches the contract of
    /// [`count_issues`].
    async fn count_inbox_issues(
        &self,
        _user_id: Uuid,
        _filter: &IssueListFilter,
    ) -> Result<i64, StoreError> {
        Ok(0)
    }

    /// Mark a batch of issues as "read up to their current
    /// `dp_issues.version`" for one user. Upserts one row per
    /// `(user_id, issue_id)` in `dp_user_issue_state`, setting
    /// `last_seen_version = (SELECT version FROM dp_issues …)`.
    /// Existing `status` / `snoozed_until` values are preserved
    /// (this is the "you read it" signal, not the "you dismissed
    /// it" signal). Idempotent — re-marking a row sets the value
    /// to the same version (or higher if the issue has been
    /// updated in the meantime).
    ///
    /// Empty `issue_ids` is a no-op (the empty-list edge case
    /// belongs to the caller's UX, not the store).
    async fn mark_issues_seen(
        &self,
        _user_id: Uuid,
        _issue_ids: &[Uuid],
    ) -> Result<(), StoreError> {
        Err(StoreError::Invalid(
            "inbox state not supported by this store".into(),
        ))
    }

    /// Set `(status, snoozed_until)` for one `(user_id, issue_id)`.
    /// Upserts the row, preserving `last_seen_version` (the snooze
    /// / dismiss / restore actions do not move the seen marker).
    /// Returns the resulting row so the caller can echo it back to
    /// the UI without a second round-trip.
    ///
    /// Validation the store leaves to the caller: when
    /// `status == Inbox` or `Done`, `snoozed_until` should be
    /// `None`; when `status == Snoozed`, `snoozed_until` should be
    /// `Some(future_instant)`. The store does not enforce this so
    /// the UX can transiently set inconsistent pairs (e.g. clear
    /// a snooze by writing `Inbox` without first wiping the date).
    async fn set_inbox_state(
        &self,
        _user_id: Uuid,
        _issue_id: Uuid,
        _status: InboxStatus,
        _snoozed_until: Option<DateTime<Utc>>,
    ) -> Result<UserIssueState, StoreError> {
        Err(StoreError::Invalid(
            "inbox state not supported by this store".into(),
        ))
    }

    /// Bulk variant of [`Store::set_inbox_state`]: apply one
    /// `(status, snoozed_until)` pair to a batch of issues for one
    /// user. Empty `issue_ids` is a no-op. Returns the number of
    /// rows touched (inserted + updated).
    ///
    /// Semantics:
    /// * `status = Inbox`   — restore to the inbox; clears any snooze.
    /// * `status = Snoozed` — `snoozed_until` should be `Some(future)`.
    /// * `status = Done`    — dismiss; ignores `snoozed_until`.
    ///
    /// Last-seen-version is preserved on existing rows (this is the
    /// dismiss / snooze / restore action, not a "saw it" signal).
    /// New rows are inserted with `last_seen_version = 0` so the
    /// next render still shows them as unread until the user
    /// actually opens them.
    async fn set_inbox_state_bulk(
        &self,
        _user_id: Uuid,
        _issue_ids: &[Uuid],
        _status: InboxStatus,
        _snoozed_until: Option<DateTime<Utc>>,
    ) -> Result<u64, StoreError> {
        Err(StoreError::Invalid(
            "bulk inbox state not supported by this store".into(),
        ))
    }

    // ---- issue timeline (triage slice 2 — §5.6) -------------------

    /// Page of `dp_activity_events` rows scoped to one issue, used
    /// by `GET /issues/{id}/timeline`. Rows are produced newest
    /// first so the peek panel can render without re-sorting.
    ///
    /// Implementations match on the §6 expression-index predicate:
    /// `repo_id = $repo_id AND kind IN ('issue_opened',
    /// 'issue_closed', 'issue_comment') AND payload ? 'number' AND
    /// payload->>'number' ~ '^[0-9]+$' AND
    /// (payload->>'number')::int = $number`. The guard makes the
    /// cast safe under malformed history.
    ///
    /// Default impl returns empty so non-Postgres fakes (used in
    /// other crates' tests) don't fail; only `dp-store-pg`
    /// provides a real implementation.
    async fn list_events_for_issue(
        &self,
        _repo_id: Uuid,
        _number: i64,
        _limit: i64,
        _offset: i64,
    ) -> Result<Vec<IssueTimelineRow>, StoreError> {
        Ok(Vec::new())
    }

    /// Total event count matching [`list_events_for_issue`]. Same
    /// scope; used for the `total` envelope field.
    async fn count_events_for_issue(
        &self,
        _repo_id: Uuid,
        _number: i64,
    ) -> Result<i64, StoreError> {
        Ok(0)
    }

    // ---- repo sync status (triage slice 2 — §5.9) -----------------

    /// Read sync freshness for one repo, synthesised from the
    /// per-resource [`FetchCursor`] rows. Returns `None` if no
    /// cursor exists yet (the repo has never been synced).
    async fn get_repo_sync_status(
        &self,
        _repo_id: Uuid,
    ) -> Result<Option<RepoSyncStatus>, StoreError> {
        Ok(None)
    }

    // ---- issue metrics report (triage slice 2 — §5.10) ------------

    /// Compute one issue-report metric over the §5.10 SQL shapes.
    /// Implementations dispatch on the metric kind.
    async fn issue_metrics(
        &self,
        _filter: &IssueMetricsFilter,
    ) -> Result<Vec<IssueMetricRow>, StoreError> {
        Ok(Vec::new())
    }

    // ---- events + actors -----------------------------------------

    /// Insert (or upsert by `external_id`) one event row.
    /// Returns the resulting row. Idempotent on `external_id`.
    async fn record_event(&self, event: &ActivityEvent) -> Result<ActivityEvent, StoreError>;

    /// Attach actor rows to an event. Multi-actor by design (TODO
    /// §0.2) — pass every actor for the event in one call so the
    /// implementation can batch the insert. Idempotent on the
    /// composite key `(event_id, user_id, role)`.
    async fn add_event_actors(&self, actors: &[EventActor]) -> Result<(), StoreError>;

    /// Return every `(event_actor × event)` row whose event timestamp
    /// falls in `window`, optionally filtered to a set of orgs /
    /// repos / users / roles. The report layer's primary read.
    ///
    /// Filters are conjunctive; an empty slice means "no filter on
    /// this dimension".
    async fn list_event_actor_rows_in_window(
        &self,
        window: &Window,
        orgs: &[Uuid],
        repos: &[Uuid],
        users: &[Uuid],
        roles: &[ActorRole],
    ) -> Result<Vec<EventActorRow>, StoreError>;

    // ---- cursors + run log ---------------------------------------

    /// Read the cursor for `(org_id, repo_id, resource_kind)`. Returns
    /// `NotFound` if there has never been one written.
    async fn get_cursor(
        &self,
        org_id: Uuid,
        repo_id: Option<Uuid>,
        resource_kind: ResourceKind,
    ) -> Result<FetchCursor, StoreError>;

    /// Upsert the cursor for `(org_id, repo_id, resource_kind)`.
    /// Composite PK — at most one row per tuple.
    async fn put_cursor(&self, cursor: &FetchCursor) -> Result<(), StoreError>;

    /// Insert a new `fetch_runs` row with `started = now()`. Returns
    /// the assigned id.
    async fn start_fetch_run(&self, kind: FetchRunKind) -> Result<Uuid, StoreError>;

    /// Mark a run finished, with item / error / partial flags.
    async fn finish_fetch_run(
        &self,
        id: Uuid,
        items: i64,
        errors: i64,
        partial: bool,
    ) -> Result<(), StoreError>;

    /// Attach a bounded sample of per-item failures to a run so
    /// `/admin/runs` can explain *why* `errors > 0`. Called by the
    /// reconciler / backfill / webhook worker just before
    /// [`Self::finish_fetch_run`]; clean runs skip this call.
    ///
    /// Implementations should overwrite (not append) so a retry of
    /// the close path doesn't double-write. The default impl is a
    /// no-op — backends that don't track samples (in-memory test
    /// fakes) silently drop them, which keeps existing fakes
    /// compiling without forcing them to grow new storage.
    async fn record_fetch_run_errors(
        &self,
        _id: Uuid,
        _samples: &[FetchRunErrorSample],
    ) -> Result<(), StoreError> {
        Ok(())
    }

    /// List the most recent `limit` runs of any kind, newest first.
    async fn list_recent_fetch_runs(&self, limit: i64) -> Result<Vec<FetchRun>, StoreError>;

    /// Paginated projection over `dp_fetch_runs` ordered newest
    /// first. Phase 4 stage 5 surfaces this on `GET /admin/runs`.
    ///
    /// Default impl falls back to [`Self::list_recent_fetch_runs`]
    /// reading `limit + offset` rows and discarding the prefix —
    /// inefficient but keeps every existing fake compiling. The PG
    /// backend overrides with `LIMIT … OFFSET …`.
    async fn list_fetch_runs(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<FetchRun>, StoreError> {
        let take = limit.max(0);
        let skip = offset.max(0);
        let total = take.saturating_add(skip);
        let mut rows = self.list_recent_fetch_runs(total).await?;
        let skip = skip as usize;
        if skip >= rows.len() {
            return Ok(Vec::new());
        }
        Ok(rows.split_off(skip))
    }

    /// Page through every `event_actor` row credited to `user_id`,
    /// joined back to its parent event. Ordered by `(ts ASC,
    /// event_id ASC)` for a stable streaming order across pages.
    ///
    /// Phase 4 stage 5 uses this to chunk the GDPR export so a
    /// 500MB user history does not need to materialise in process
    /// memory. Default impl returns the empty vec so test fakes
    /// that don't model events stay green.
    async fn list_event_actor_rows_for_user_page(
        &self,
        _user_id: Uuid,
        _offset: i64,
        _limit: i64,
    ) -> Result<Vec<EventActorRow>, StoreError> {
        Ok(Vec::new())
    }

    /// Snapshot the data-freshness envelope every report response
    /// carries (SCOPE §11.7 / TODO §0.3).
    ///
    /// Returns:
    ///
    /// * `webhook_latest` — `MAX(finished)` of `dp_fetch_runs` rows
    ///   where `kind = webhook_worker` and `finished IS NOT NULL`.
    /// * `reconciler_latest` — same, for `kind = reconciler`.
    /// * `per_org` — `MAX(updated_at)` of `dp_fetch_cursors` grouped
    ///   by `org_id`. Orgs that have no cursor rows yet (no
    ///   reconciler tick has ever touched them) are absent rather
    ///   than mapped to a sentinel.
    ///
    /// Cheap — three indexed aggregates, no per-row scan. Reports
    /// call this once per request and the result rides on the
    /// response envelope.
    async fn data_as_of(&self) -> Result<DataAsOf, StoreError>;

    // ---- webhook inbox -------------------------------------------

    /// Enqueue a webhook delivery. Unique constraint on `delivery_id`
    /// surfaces replays as [`StoreError::Conflict`] — the receiver
    /// translates that into a 200 OK (idempotent).
    async fn enqueue_webhook(&self, delivery: &WebhookDelivery) -> Result<(), StoreError>;

    /// Claim up to `max` unprocessed deliveries for the worker to
    /// drain. Implementations should use `SELECT ... FOR UPDATE SKIP
    /// LOCKED` (Postgres) so multiple workers don't fight over the
    /// same row.
    async fn claim_webhooks(&self, max: i64) -> Result<Vec<WebhookDelivery>, StoreError>;

    /// Mark a delivery processed (success path).
    async fn mark_webhook_processed(&self, id: Uuid) -> Result<(), StoreError>;

    /// Record a processing failure on a delivery so the worker can
    /// retry. Stores the error text and leaves `processed_at` NULL.
    async fn mark_webhook_failed(&self, id: Uuid, error: &str) -> Result<(), StoreError>;

    // ---- audit log ------------------------------------------------

    /// Insert one `dp_audit_log` row (SCOPE §9). Phase 4 D4.4 pins
    /// the `action` vocabulary in `dp-rest::audit`; this method is
    /// vocabulary-free so other surfaces can write their own verbs
    /// later. Default impl is a no-op so test fakes that don't care
    /// about the audit trail stay green.
    async fn record_audit_log(&self, _entry: &AuditEntry) -> Result<(), StoreError> {
        Ok(())
    }

    // ---- pins (SCOPE-PROJECTS §6) -------------------------------
    //
    // Default impls return empty / no-op so the existing in-memory
    // fakes used by `dp-reports`, `dp-rest`, `dp-mcp`, and the
    // fetcher integration tests do not have to grow new code to
    // keep compiling. The Postgres backend overrides each one.

    /// List a user's pins, ordered by `position` ascending. Returns
    /// an empty vec if the user has no pins. Default: empty.
    async fn list_pins_for_user(&self, _user_id: Uuid) -> Result<Vec<Pin>, StoreError> {
        Ok(Vec::new())
    }

    /// Append a pin to the end of a user's list. Implementations
    /// must reject the insert (return [`StoreError::Invalid`]) if it
    /// would exceed the configured per-user pin cap (working
    /// assumption 20; §6.1 + §13.5). The composite PK
    /// `(user_id, kind, target_id)` makes re-pinning idempotent at
    /// the schema level — a duplicate is a [`StoreError::Conflict`].
    async fn add_pin(&self, _pin: &Pin) -> Result<Pin, StoreError> {
        Err(StoreError::Invalid("pins not supported by this store".into()))
    }

    /// Remove a pin by its composite key. Returns
    /// [`StoreError::NotFound`] if the pin does not exist.
    async fn remove_pin(
        &self,
        _user_id: Uuid,
        _kind: crate::pin::PinKind,
        _target_id: Uuid,
    ) -> Result<(), StoreError> {
        Err(StoreError::Invalid("pins not supported by this store".into()))
    }

    /// Atomically rewrite the ordering of a user's pins. The slice
    /// is the new `(kind, target_id)` order — entry `i` becomes
    /// `position = i`. Implementations apply the rewrite in one
    /// transaction; partial reorders are not visible to readers.
    /// Returns [`StoreError::Invalid`] if `order` does not exactly
    /// cover the user's current pins.
    async fn reorder_pins(
        &self,
        _user_id: Uuid,
        _order: &[(crate::pin::PinKind, Uuid)],
    ) -> Result<(), StoreError> {
        Err(StoreError::Invalid("pins not supported by this store".into()))
    }

    // ---- per-user settings (migration 0029) ---------------------

    /// List every setting for a user, ordered by `key` ascending.
    /// Returns an empty vec when the user has no settings (a
    /// brand-new account hits this path on first render). The
    /// REST layer is responsible for redacting `value` on
    /// [`UserSetting::is_secret`] rows before returning to the
    /// client.
    async fn list_user_settings(
        &self,
        _user_id: Uuid,
    ) -> Result<Vec<crate::setting::UserSetting>, StoreError> {
        Ok(Vec::new())
    }

    /// Fetch a single setting by `(user_id, key)`. Returns
    /// `Ok(None)` when the row does not exist — this is *not* an
    /// error condition; `GET /me/settings/{key}` on an unset key
    /// returns a `404 setting_unset` shaped from this.
    async fn get_user_setting(
        &self,
        _user_id: Uuid,
        _key: &str,
    ) -> Result<Option<crate::setting::UserSetting>, StoreError> {
        Ok(None)
    }

    /// Upsert one `(user_id, key)` row. The store sets
    /// `updated_at = now()` on every write. Returns the row
    /// after the upsert lands.
    async fn upsert_user_setting(
        &self,
        _setting: &crate::setting::UserSetting,
    ) -> Result<crate::setting::UserSetting, StoreError> {
        Err(StoreError::Invalid(
            "user settings not supported by this store".into(),
        ))
    }

    /// Delete one `(user_id, key)` row. Returns
    /// [`StoreError::NotFound`] when the row does not exist so
    /// the REST layer can return a structured `404 setting_unset`.
    async fn delete_user_setting(
        &self,
        _user_id: Uuid,
        _key: &str,
    ) -> Result<(), StoreError> {
        Err(StoreError::NotFound {
            entity: "user_setting",
            id: _key.to_string(),
        })
    }

    // ---- tags + tag links (SCOPE-PROJECTS §7) -------------------

    /// Fetch a tag by primary key. Returns [`StoreError::NotFound`]
    /// if the row does not exist.
    async fn get_tag(&self, _id: Uuid) -> Result<Tag, StoreError> {
        Err(StoreError::NotFound {
            entity: "tag",
            id: _id.to_string(),
        })
    }

    /// Create a tag. The per-scope case-insensitive uniqueness on
    /// `(scope_kind, scope_id, lower(name))` is enforced by the
    /// migration-0005 expression index; the Postgres backend
    /// translates the unique-constraint violation into
    /// [`StoreError::Conflict`].
    async fn create_tag(&self, _tag: &Tag) -> Result<Tag, StoreError> {
        Err(StoreError::Invalid("tags not supported by this store".into()))
    }

    /// Patch a tag's `name` / `color` / `description` / `archived_at`
    /// in place. The `scope_*` columns and `created_by` are
    /// immutable from this method — promotion across scopes is a §12
    /// open question and out of scope for v1. Passing `None` for a
    /// field leaves it unchanged; passing `Some(None)` for
    /// `description` / `archived_at` clears it.
    async fn update_tag(
        &self,
        _id: Uuid,
        _name: Option<&str>,
        _color: Option<&str>,
        _description: Option<Option<&str>>,
        _archived_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    ) -> Result<Tag, StoreError> {
        Err(StoreError::Invalid("tags not supported by this store".into()))
    }

    /// List every tag visible to a viewer. Visibility filtering
    /// (§7.4) is the *caller's* responsibility — pass the set of
    /// scope ids the viewer can see for each scope kind.
    /// Implementations return rows whose `scope_kind` matches one of
    /// the provided scope id slices. An empty slice for a scope
    /// kind means "no tags of that kind". Archived tags are
    /// excluded unless `include_archived` is true.
    async fn list_tags_visible_to(
        &self,
        _viewer_user_id: Uuid,
        _visible_team_ids: &[Uuid],
        _visible_org_ids: &[Uuid],
        _include_archived: bool,
    ) -> Result<Vec<Tag>, StoreError> {
        Ok(Vec::new())
    }

    /// List the links attached to a tag, optionally filtered to a
    /// subset of link kinds (empty slice = all kinds). The
    /// viewer-visibility filter in §7.4 is the caller's job; this
    /// method returns every link the tag carries.
    async fn list_tag_links(
        &self,
        _tag_id: Uuid,
        _kinds: &[TagLinkKind],
    ) -> Result<Vec<TagLink>, StoreError> {
        Ok(Vec::new())
    }

    /// Reverse lookup: every link of `kind` whose `target_*_id`
    /// matches one of `target_ids`. Used by the issue detail and
    /// list handlers to embed `tags` on each `IssueDto` in one
    /// round-trip instead of N per-tag scans. Viewer-visibility
    /// filtering of the *tags* the links point at is the caller's
    /// job — this method returns every link in the table.
    async fn list_tag_links_for_targets(
        &self,
        _kind: TagLinkKind,
        _target_ids: &[Uuid],
    ) -> Result<Vec<TagLink>, StoreError> {
        Ok(Vec::new())
    }

    /// Attach a batch of links to a tag, **transactionally
    /// all-or-nothing** (§7.5). If any link fails validation
    /// (duplicate, target not visible, wrong kind, missing target
    /// row), the whole batch is rejected. The unique index
    /// `dp_tag_links_tag_target_uniq` provides the duplicate check
    /// at the schema level.
    async fn add_tag_links(&self, _links: &[TagLink]) -> Result<Vec<TagLink>, StoreError> {
        Err(StoreError::Invalid("tags not supported by this store".into()))
    }

    /// Detach a batch of links by id, transactionally all-or-nothing
    /// (§7.5). Returns [`StoreError::NotFound`] if any id is
    /// missing — no partial unlinks.
    async fn remove_tag_links(&self, _link_ids: &[Uuid]) -> Result<(), StoreError> {
        Err(StoreError::Invalid("tags not supported by this store".into()))
    }

    /// Resolve a set of tag ids to the `(repo_id, issue_id,
    /// user_id, team_id)` targets they currently link, for the
    /// §15.6 report-filter path (SCOPE-PROJECTS §7.7). Implementations
    /// apply the viewer-visibility filter using the supplied
    /// allow-lists.
    async fn resolve_tag_targets(
        &self,
        _tag_ids: &[Uuid],
        _visible_repo_ids: &[Uuid],
        _visible_user_ids: &[Uuid],
        _visible_team_ids: &[Uuid],
    ) -> Result<Vec<TagLink>, StoreError> {
        Ok(Vec::new())
    }

    // ---- milestones (tagging.md §9.3) -----------------------------
    //
    // GitHub repo milestones, mirrored per-repo by the fetcher.
    // Storage shipped in migration 0030_milestones.sql; the fetcher
    // integration that actually populates these rows arrives in a
    // follow-up slice. Default impls return safe empties so test
    // fakes and any future stores stay compiling.

    /// Upsert a milestone by its natural key `(repo_id,
    /// github_number)`. The surrogate `id` is preserved on
    /// conflict so any future FK from `dp_issues.milestone_id`
    /// stays stable across re-fetches.
    ///
    /// On a successful upsert the store resets
    /// `remote_missing_streak` to 0 — observing the milestone via
    /// `list_milestones` is the strongest possible evidence that
    /// it is **not** missing on the remote.
    async fn upsert_milestone(
        &self,
        _upsert: &crate::milestone::MilestoneUpsert,
    ) -> Result<crate::milestone::Milestone, StoreError> {
        Err(StoreError::Invalid(
            "milestones not supported by this store".into(),
        ))
    }

    /// List milestones for a single repo. `include_closed = false`
    /// returns only `state = 'open'` rows (the common case for the
    /// triage rail); `true` returns both states sorted by
    /// `(state, due_on NULLS LAST, github_number)` so the operator
    /// sees the active set first then the historical tail.
    async fn list_milestones_for_repo(
        &self,
        _repo_id: Uuid,
        _include_closed: bool,
    ) -> Result<Vec<crate::milestone::Milestone>, StoreError> {
        Ok(Vec::new())
    }

    /// List milestones across every repo currently associated with
    /// a project (via `dp_project_repos`). Backs the Slice 1
    /// `MilestonesStrip` on the project detail page
    /// (PROJECT-VIEW.md §5.5). Sorted by `due_on ASC NULLS LAST,
    /// title ASC` so the strip puts soonest-due first and the
    /// no-date milestones at the end.
    ///
    /// `include_closed = false` returns only `state = 'open'`
    /// rows (the default — the strip's primary case). `true`
    /// returns the open set followed by closed rows so the
    /// `▸ Show closed` toggle can render historical context.
    async fn list_project_milestones(
        &self,
        _project_id: Uuid,
        _include_closed: bool,
    ) -> Result<Vec<crate::milestone::Milestone>, StoreError> {
        Ok(Vec::new())
    }

    /// Set or clear a project's primary milestone pointer
    /// (PROJECT-VIEW.md §5.5 / §9.5). `milestone_id = Some(mid)`
    /// adopts; `None` clears. Returns the updated [`Project`].
    ///
    /// Implementations must validate that the milestone (when
    /// `Some`) belongs to a repo currently linked to the project,
    /// returning [`StoreError::Invalid`] otherwise — the strip
    /// only surfaces milestones already in `list_project_milestones`,
    /// but a stale UI must not be able to point at an unrelated
    /// milestone via a direct API call.
    async fn set_project_primary_milestone(
        &self,
        _project_id: Uuid,
        _milestone_id: Option<Uuid>,
    ) -> Result<crate::project::Project, StoreError> {
        Err(StoreError::Invalid(
            "primary milestone adoption not supported by this store".into(),
        ))
    }

    /// Hard-delete a milestone row by its surrogate id. Used by
    /// the `DELETE /projects/{id}/milestones/{ms_id}` two-way-sync
    /// path after the GitHub-side delete succeeds — without it
    /// the local mirror would re-surface the row until the next
    /// reconciler tick reconciled the `remote_missing_streak`
    /// counter to 3.
    ///
    /// Implementations should clear any `dp_projects.primary_milestone_id`
    /// pointer that referenced this row (the FK is `ON DELETE
    /// SET NULL` so the database does it for us). Returns
    /// [`StoreError::NotFound`] when no row matched, so callers
    /// can distinguish "already gone" from "really deleted".
    async fn delete_milestone(
        &self,
        _milestone_id: Uuid,
    ) -> Result<(), StoreError> {
        Err(StoreError::Invalid(
            "milestone deletion not supported by this store".into(),
        ))
    }

    // ---- project executive summary (DOCS/SCOPE-PROJECT-EXECUTIVE-SUMMARY.md) ----
    //
    // One [`ProjectExecSummary`] per `dp_projects` row, lazily
    // materialised on first edit. Schema in migration
    // `0045_project_exec_summary.sql`. Default impls return safe
    // empties / `Invalid` so test fakes stay compiling until they
    // opt-in to the surface.
    //
    // The status state machine (`draft → in_review → approved`)
    // lives on the dedicated `submit_*` / `approve_*` / `revert_*`
    // methods so the rule is in one place; `patch_*` never touches
    // `status`.

    /// Fetch the exec summary row + computed
    /// [`ExecSummaryCompletion`] for a project. Returns `Ok(None)`
    /// when no row exists yet (lazy materialisation — callers
    /// should treat this as an all-fields-null draft).
    async fn get_project_exec_summary(
        &self,
        _project_id: Uuid,
    ) -> Result<
        Option<(
            crate::project_exec_summary::ProjectExecSummary,
            crate::project_exec_summary::ExecSummaryCompletion,
        )>,
        StoreError,
    > {
        Ok(None)
    }

    /// Lazy-create the exec summary row for a project. Idempotent —
    /// returns the existing row when one is already present. The
    /// REST PATCH path calls this before applying the patch so the
    /// happy path is a single round-trip in the common case.
    async fn upsert_project_exec_summary(
        &self,
        _project_id: Uuid,
    ) -> Result<crate::project_exec_summary::ProjectExecSummary, StoreError> {
        Err(StoreError::Invalid(
            "project exec summary not supported by this store".into(),
        ))
    }

    /// Apply a sparse [`ProjectExecSummaryPatch`] to the row. Only
    /// fields present in the patch are updated; `updated_at` is
    /// bumped unconditionally. Status is never mutated here — use
    /// `submit_project_exec_summary` / `approve_project_exec_summary`
    /// / `revert_project_exec_summary` instead.
    async fn patch_project_exec_summary(
        &self,
        _project_id: Uuid,
        _patch: &crate::project_exec_summary::ProjectExecSummaryPatch,
    ) -> Result<crate::project_exec_summary::ProjectExecSummary, StoreError> {
        Err(StoreError::Invalid(
            "project exec summary not supported by this store".into(),
        ))
    }

    /// `draft → in_review`. Sets `submitted_at = now()`. The
    /// completion-threshold check lives in the REST layer (it needs
    /// the same computed completion the GET returns); the store
    /// only enforces the state-machine transition and returns
    /// [`StoreError::Conflict`] if the current status is not
    /// `draft`.
    async fn submit_project_exec_summary(
        &self,
        _project_id: Uuid,
    ) -> Result<crate::project_exec_summary::ProjectExecSummary, StoreError> {
        Err(StoreError::Invalid(
            "project exec summary not supported by this store".into(),
        ))
    }

    /// `in_review → approved`. Sets `approved_at = now()` and
    /// optionally overwrites `approval_notes`. Lead-only authz
    /// lives in the REST layer; the store just enforces the
    /// transition and returns [`StoreError::Conflict`] when the
    /// current status is not `in_review`.
    async fn approve_project_exec_summary(
        &self,
        _project_id: Uuid,
        _approval_notes: Option<&str>,
    ) -> Result<crate::project_exec_summary::ProjectExecSummary, StoreError> {
        Err(StoreError::Invalid(
            "project exec summary not supported by this store".into(),
        ))
    }

    /// `* → draft`. Unconditional (E3 of the scope doc). Preserves
    /// `submitted_at` / `approved_at` so the history of the most
    /// recent transitions stays visible in the UI even after a
    /// revert.
    async fn revert_project_exec_summary(
        &self,
        _project_id: Uuid,
    ) -> Result<crate::project_exec_summary::ProjectExecSummary, StoreError> {
        Err(StoreError::Invalid(
            "project exec summary not supported by this store".into(),
        ))
    }

    /// List reference images attached to the Hardware section,
    /// ordered by `(ord, created_at)`.
    async fn list_exec_summary_images(
        &self,
        _project_id: Uuid,
    ) -> Result<Vec<crate::project_exec_summary::ExecSummaryImage>, StoreError> {
        Ok(Vec::new())
    }

    /// Insert a confirmed image row. `blob_ref` is the opaque
    /// serde-json round-trip of the starter `BlobRef`; the store
    /// never inspects its shape (B2 of the storage scope).
    async fn insert_exec_summary_image(
        &self,
        _project_id: Uuid,
        _blob_ref: &crate::project_exec_summary::BlobRefJson,
        _filename: &str,
        _content_type: &str,
        _caption: Option<&str>,
        _ord: Option<i32>,
    ) -> Result<crate::project_exec_summary::ExecSummaryImage, StoreError> {
        Err(StoreError::Invalid(
            "project exec summary not supported by this store".into(),
        ))
    }

    /// Fetch one image row by surrogate id — used by the blob proxy
    /// route to resolve `BlobRef` + content-type + filename without
    /// a full per-project scan.
    async fn get_exec_summary_image(
        &self,
        _image_id: Uuid,
    ) -> Result<Option<crate::project_exec_summary::ExecSummaryImage>, StoreError> {
        Ok(None)
    }

    /// Patch the editable bits of an image row (caption / ord).
    /// Pass `None` for "leave as-is", `Some(None)` for "set to NULL"
    /// on `caption`. `ord` is `Option<i32>` (always present-or-absent;
    /// no NULL semantics on a non-NULL column).
    async fn update_exec_summary_image(
        &self,
        _image_id: Uuid,
        _caption: Option<Option<String>>,
        _ord: Option<i32>,
    ) -> Result<crate::project_exec_summary::ExecSummaryImage, StoreError> {
        Err(StoreError::Invalid(
            "project exec summary not supported by this store".into(),
        ))
    }

    /// Delete an image row by surrogate id. Returns
    /// [`StoreError::NotFound`] when no row matched. The blob
    /// bytes themselves are not deleted here — that's a
    /// follow-up sweep job per the storage scope's E3 hard rule
    /// (combinators don't silently mutate engine state).
    async fn delete_exec_summary_image(
        &self,
        _image_id: Uuid,
    ) -> Result<(), StoreError> {
        Err(StoreError::Invalid(
            "project exec summary not supported by this store".into(),
        ))
    }

    /// List supporting documents for a project, newest first.
    async fn list_exec_summary_documents(
        &self,
        _project_id: Uuid,
    ) -> Result<Vec<crate::project_exec_summary::ExecSummaryDocument>, StoreError> {
        Ok(Vec::new())
    }

    /// Fetch one document row by surrogate id — used by the blob
    /// proxy route. Same rationale as
    /// [`Store::get_exec_summary_image`].
    async fn get_exec_summary_document(
        &self,
        _document_id: Uuid,
    ) -> Result<Option<crate::project_exec_summary::ExecSummaryDocument>, StoreError> {
        Ok(None)
    }

    /// Insert a confirmed document row.
    async fn insert_exec_summary_document(
        &self,
        _project_id: Uuid,
        _blob_ref: &crate::project_exec_summary::BlobRefJson,
        _title: &str,
        _doc_type: Option<&str>,
        _notes: Option<&str>,
        _required_action: Option<&str>,
        _uploaded_by: Option<&str>,
    ) -> Result<crate::project_exec_summary::ExecSummaryDocument, StoreError> {
        Err(StoreError::Invalid(
            "project exec summary not supported by this store".into(),
        ))
    }

    /// Patch a document's editable fields. Same `Option<Option<T>>`
    /// "absent vs null" convention as the scalar patch.
    #[allow(clippy::too_many_arguments)]
    async fn update_exec_summary_document(
        &self,
        _document_id: Uuid,
        _title: Option<String>,
        _doc_type: Option<Option<String>>,
        _notes: Option<Option<String>>,
        _required_action: Option<Option<String>>,
    ) -> Result<crate::project_exec_summary::ExecSummaryDocument, StoreError> {
        Err(StoreError::Invalid(
            "project exec summary not supported by this store".into(),
        ))
    }

    /// Delete a document row. Same caveat as image delete — bytes
    /// are reaped separately.
    async fn delete_exec_summary_document(
        &self,
        _document_id: Uuid,
    ) -> Result<(), StoreError> {
        Err(StoreError::Invalid(
            "project exec summary not supported by this store".into(),
        ))
    }

    /// List change-log entries newest-first (by `changed_at` DESC,
    /// then `created_at` DESC for stable ordering when two entries
    /// share a date).
    async fn list_exec_summary_changelog(
        &self,
        _project_id: Uuid,
    ) -> Result<Vec<crate::project_exec_summary::ExecSummaryChangelogEntry>, StoreError> {
        Ok(Vec::new())
    }

    /// Append a change-log entry. E5 of the scope doc: the UI's
    /// default affords add only; updates / deletes require an
    /// explicit confirm.
    async fn insert_exec_summary_changelog(
        &self,
        _insert: &crate::project_exec_summary::ExecSummaryChangelogInsert,
    ) -> Result<crate::project_exec_summary::ExecSummaryChangelogEntry, StoreError> {
        Err(StoreError::Invalid(
            "project exec summary not supported by this store".into(),
        ))
    }

    /// Delete a change-log entry. Reserved for the admin-only
    /// confirm path; the regular UI never reaches this.
    async fn delete_exec_summary_changelog(
        &self,
        _entry_id: Uuid,
    ) -> Result<(), StoreError> {
        Err(StoreError::Invalid(
            "project exec summary not supported by this store".into(),
        ))
    }

    // ---- GitHub App installation permissions (SCOPE-PROJECTS §8.4, §13.6) ----
    //
    // The reconciler / install-callback writes one row per org
    // capturing whether the install was granted `issues: write`.
    // The §8 write surface reads through this; the §13.6 banner
    // endpoint enumerates orgs whose row says writes are
    // unavailable. Stage 8 lands the trait method as a `None`-by-
    // default read; the postgres backend grows the
    // `dp_org_app_installs` table in a later migration of this
    // same job. Fakes / test stubs inherit the default and behave
    // as if no orgs have writes available — a fail-closed posture
    // that matches the §8.4 §13.6 decision.

    /// Look up the per-org GitHub App install record (if any).
    ///
    /// Returns `Ok(None)` when no install row has been observed
    /// for `org_id` yet — callers treat this as **writes not
    /// available** (§8.4 fail-closed). Returns `Ok(Some(_))` with
    /// the latest observed permissions otherwise.
    ///
    /// The default impl returns `Ok(None)` so existing test fakes
    /// and the partially-migrated postgres backend stay compiling
    /// through stage 8; a follow-up stage of this job overrides
    /// it with the real Postgres query.
    async fn get_org_app_install(
        &self,
        _org_id: Uuid,
    ) -> Result<Option<OrgAppInstall>, StoreError> {
        Ok(None)
    }

    // ---- issue mutations (SCOPE-PROJECTS §8.2 + §8.5 + §13.7) ----
    //
    // Storage landed in `0007_issues_optimistic_cas.sql` (stage 9 of
    // this same job): four new columns on `dp_issues` (`version`,
    // `pending_remote`, `pending_remote_at`, `pending_remote_actor`)
    // plus the `dp_issue_mutations` audit table. The trait surface
    // exposed here is the *primitive* set the §8.2 write path and
    // the §8.5 sweeper compose against — no GitHub I/O, no
    // octocrab; that wiring lives in the dp-rest handler.
    //
    // The CAS is split into two halves on purpose: writers
    // `try_acquire_issue_pending_remote` (bumps version, sets the
    // pending flag) *before* the GitHub round-trip, and
    // `release_issue_pending_remote` (clears the flag, optionally
    // bumps version again for the §8.2 step 8 rollback) *after*.
    // No row-lock is held across the network call (§13.4).

    /// §8.2 step 5: atomic CAS that bumps `dp_issues.version` and
    /// raises the `pending_remote` flag in one statement.
    ///
    /// The SQL clause is `WHERE id = ? AND version = ? AND
    /// pending_remote = false` — that is, the CAS rejects both
    /// `expected_version` mismatch *and* the case where another
    /// in-flight write already holds the slot.
    ///
    /// Returns:
    ///
    /// * `Ok(Some(new_version))` — one row updated, write may
    ///   proceed; `new_version = expected_version + 1`.
    /// * `Ok(None)` — zero rows updated; the dp-rest handler
    ///   translates this into the `409 stale_local_version`
    ///   response (§8.3).
    async fn try_acquire_issue_pending_remote(
        &self,
        _issue_id: Uuid,
        _expected_version: i64,
        _actor_user_id: Uuid,
    ) -> Result<Option<i64>, StoreError> {
        Err(StoreError::Invalid(
            "issue mutations not supported by this store".into(),
        ))
    }

    /// §8.2 step 7 (success) or §8.2 step 8 (failure / rollback) —
    /// clears `pending_remote`, `pending_remote_at`, and
    /// `pending_remote_actor` in a single statement. When
    /// `bump_version_again` is `true` (the §8.2 step 8 path) the
    /// SQL also runs `version = version + 1` so any concurrent
    /// reader sees the rollback as a change. Returns the row's
    /// `version` after this update.
    ///
    /// Idempotent: a row that is no longer pending (e.g. a sweeper
    /// already touched it) does not error — the method updates
    /// zero rows in that case and returns the current version.
    async fn release_issue_pending_remote(
        &self,
        _issue_id: Uuid,
        _bump_version_again: bool,
    ) -> Result<i64, StoreError> {
        Err(StoreError::Invalid(
            "issue mutations not supported by this store".into(),
        ))
    }

    /// Read `dp_issues.version` only. Tests and the §8.3 conflict
    /// response use this to surface the current version to the UI
    /// without rehydrating the whole row.
    async fn get_issue_version(
        &self,
        _issue_id: Uuid,
    ) -> Result<i64, StoreError> {
        Err(StoreError::Invalid(
            "issue mutations not supported by this store".into(),
        ))
    }

    /// §13.7 reconciler guard helper. Returns rows where
    /// `pending_remote = true` and `pending_remote_at < cutoff`.
    /// Drives the §8.5 timeout sweeper — every row returned needs
    /// (a) `release_issue_pending_remote(_, true)` to bump version
    /// and clear the flag and (b) a `pending_remote_timeout` audit
    /// row.
    async fn list_issues_with_pending_remote_older_than(
        &self,
        _cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<PendingRemoteIssue>, StoreError> {
        Ok(Vec::new())
    }

    /// Record a new [`IssueMutation`] row in `Pending` state. Called
    /// from §8.2 step 5, immediately after
    /// `try_acquire_issue_pending_remote` succeeded.
    async fn record_issue_mutation(
        &self,
        _mutation: &IssueMutation,
    ) -> Result<IssueMutation, StoreError> {
        Err(StoreError::Invalid(
            "issue mutations not supported by this store".into(),
        ))
    }

    /// Transition an [`IssueMutation`] out of `Pending` (§8.2 step 7
    /// / step 8 / sweeper). Sets `result`, optionally
    /// `github_delivery_id` / `error`, and stamps `finished_at =
    /// now()`. Updating an already-finished row is a no-op (the
    /// CHECK constraint on `dp_issue_mutations.result` would not
    /// catch the race, but the sweeper / handler interleaving is
    /// designed so only one writer ever calls this for a given id).
    async fn update_issue_mutation_result(
        &self,
        _id: Uuid,
        _result: IssueMutationResult,
        _github_delivery_id: Option<&str>,
        _error: Option<&str>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Invalid(
            "issue mutations not supported by this store".into(),
        ))
    }

    /// Find audit rows stuck in `Pending` past the
    /// `issues.pending_remote_timeout_secs` window. Mirror of
    /// [`Store::list_issues_with_pending_remote_older_than`] for
    /// the audit table — the sweeper joins the two by `issue_id`
    /// to decide whether to emit a fresh `pending_remote_timeout`
    /// row or update the existing one.
    async fn list_pending_issue_mutations_older_than(
        &self,
        _cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<IssueMutation>, StoreError> {
        Ok(Vec::new())
    }

    // ---- §13.7 reconciler guard + webhook replay buffer ---------------
    //
    // These primitives back the SCOPE-PROJECTS §13.7 invariant: the
    // fetcher / webhook reconciler must *not* overwrite a `dp_issues`
    // row whose `pending_remote = TRUE` and whose `pending_remote_at`
    // is younger than `issues.pending_remote_timeout_secs`. Webhook
    // payloads that would otherwise be applied to such a row are
    // buffered into `dp_pending_remote_webhook_buffer` and replayed
    // through the normal handler path once the flag clears (§8.2
    // step 7 / step 8 / §8.5 sweeper).
    //
    // Default impls keep test fakes and the in-memory MCP store
    // compiling; the `dp-store-pg` backend overrides each one.

    /// Look up `dp_repos.id` from GitHub's numeric repo id.
    /// Returns `Ok(None)` if no local repo row exists — the
    /// guard's "first sighting" branch. The §13.7 webhook guard
    /// uses this to resolve `payload.repository.id` to a local
    /// repo without forcing an upsert (which would mutate state
    /// before the guard decision had been made).
    async fn find_repo_id_by_github_id(
        &self,
        _github_repo_id: i64,
    ) -> Result<Option<Uuid>, StoreError> {
        Ok(None)
    }

    /// Look up `dp_issues.id` from `(repo_id, github_issue_id)`.
    /// Returns `Ok(None)` when no such row exists yet — meaning
    /// nothing on the dev-pulse side can be pending and the caller
    /// should just apply the delivery normally.
    ///
    /// `github_issue_id` is GitHub's per-issue numeric id (the
    /// `issue.id` field in webhook payloads), not the
    /// repo-relative `issue.number`. The §8 write path keys on
    /// `id`, not `number`, because numbers are reassigned when an
    /// issue is transferred between repos.
    async fn find_issue_id_by_repo_and_github_id(
        &self,
        _repo_id: Uuid,
        _github_issue_id: i64,
    ) -> Result<Option<Uuid>, StoreError> {
        Ok(None)
    }

    /// §13.7 guard predicate. Returns `true` when the row exists,
    /// `pending_remote = TRUE`, and `pending_remote_at >= now() -
    /// timeout`. A `false` result means the reconciler may apply
    /// its payload to the row.
    ///
    /// Centralising the timeout comparison in the store keeps the
    /// clock authoritative (SQL `now()` rather than the host wall
    /// clock) on the postgres backend, matching the §8.2 / §8.5
    /// `pending_remote_at` write side.
    async fn is_issue_pending_remote_fresh(
        &self,
        _issue_id: Uuid,
        _timeout: chrono::Duration,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Stash a webhook delivery on the §13.7 buffer so it can be
    /// replayed after the pending_remote flag clears. Inserted
    /// rows are de-duped on `delivery_id` (matching the inbox's
    /// at-least-once-from-GitHub invariant): a duplicate
    /// `delivery_id` returns `StoreError::Conflict`, which the
    /// caller should treat as a benign "already buffered, nothing
    /// more to do".
    async fn buffer_pending_remote_webhook(
        &self,
        _issue_id: Uuid,
        _delivery: &WebhookDelivery,
    ) -> Result<(), StoreError> {
        Err(StoreError::Invalid(
            "pending_remote webhook buffer not supported by this store".into(),
        ))
    }

    /// Drain every buffered webhook for `issue_id`, oldest first
    /// (`ORDER BY buffered_at`). Returned rows are deleted from
    /// the buffer in the same SQL statement so the replay is at-
    /// least-once but not at-most-once: a crash between this call
    /// and `apply_delivery` loses the buffered copy. That is
    /// considered acceptable — GitHub's at-least-once webhook
    /// delivery contract plus the next reconciler tick will
    /// re-observe the same authoritative state shortly.
    async fn take_buffered_webhooks_for_issue(
        &self,
        _issue_id: Uuid,
    ) -> Result<Vec<WebhookDelivery>, StoreError> {
        Ok(Vec::new())
    }

    // ---- issue dates (triage slice 2 — §3.10) --------------------

    /// Read the `dp_issue_dates` sidecar row for an issue, or
    /// `None` when none exists yet (the issue has never had dates
    /// set). Default impl returns `None` so in-memory fakes don't
    /// need to model dates.
    async fn get_issue_dates(
        &self,
        _issue_id: Uuid,
    ) -> Result<Option<IssueDates>, StoreError> {
        Ok(None)
    }

    /// Synchronous upsert of `(start_at, due_at)` on
    /// `dp_issue_dates`. Returns the post-upsert row so the
    /// handler can echo the canonical timestamps back to the UI.
    /// The schema CHECK guards `start_at <= due_at`; violations
    /// surface as [`StoreError::Invalid`] in the postgres backend.
    /// Default impl rejects the call so misuse from fakes is loud.
    async fn upsert_issue_dates(
        &self,
        _issue_id: Uuid,
        _start_at: Option<DateTime<Utc>>,
        _due_at: Option<DateTime<Utc>>,
    ) -> Result<IssueDates, StoreError> {
        Err(StoreError::Invalid(
            "issue dates not supported by this store".into(),
        ))
    }

    /// Write the mirror outcome back to `dp_issue_dates`. On
    /// success: clears `mirror_error`, stamps `mirror_synced_at`,
    /// and persists the Projects v2 *item* node id (so the next
    /// mirror reuses it). On failure: stamps `mirror_error` only.
    /// Default impl is a no-op so the date upsert always succeeds
    /// even when the store lacks the table.
    async fn record_issue_dates_mirror_result(
        &self,
        _issue_id: Uuid,
        _outcome: IssueDatesMirrorOutcome<'_>,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    /// Stamp `dp_issues.github_node_id` for an issue that was
    /// missing one at mirror time. Called by the §3.10 mirror
    /// adapter after a lazy `repository.issue(number)` GraphQL
    /// resolve so the next mirror skips the lookup. Default
    /// impl is a no-op — the trait does not require stores to
    /// persist the cache, the mirror just re-resolves each time.
    async fn set_issue_github_node_id(
        &self,
        _issue_id: Uuid,
        _node_id: &str,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    /// Enqueue a `dp_projectv2_mirror_tasks` row. Best-effort by
    /// contract — the handler ignores errors from this call so
    /// the local upsert is never blocked. Default impl is a no-op.
    async fn enqueue_projectv2_mirror_task(
        &self,
        _issue_id: Uuid,
        _repo_id: Uuid,
        _kind: ProjectV2MirrorTaskKind,
        _payload: serde_json::Value,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    /// Drain up to `max` pending `mirror_dates` / `pull_back` rows
    /// ordered by `enqueued_at ASC`. Slice-3 worker entry point;
    /// returns the empty vec here so existing fakes stay green.
    async fn claim_projectv2_mirror_tasks(
        &self,
        _max: i64,
    ) -> Result<Vec<ProjectV2MirrorTask>, StoreError> {
        Ok(Vec::new())
    }

    // ---- projects (linear-projects-v2.md slice A) ----------------

    /// List projects matching `filter`, ordered for the §6.2 list
    /// page: `status` ASC then `due_at ASC NULLS LAST` then `name`.
    /// Default impl returns the empty vec so in-memory fakes that
    /// don't care about projects stay quiet.
    async fn list_projects(
        &self,
        _filter: &crate::project::ProjectListFilter,
    ) -> Result<Vec<crate::project::Project>, StoreError> {
        Ok(Vec::new())
    }

    /// Run the portfolio-report query — SCOPE-PROJECT-REPORTS.md §6+§10.
    /// Returns one [`PortfolioRawRow`] per visible project, page-bounded
    /// by `filter.limit` / `filter.offset`. The `total` column on each
    /// row carries the pre-page count via `COUNT(*) OVER ()`. Default
    /// impl returns the empty vec.
    async fn list_project_portfolio(
        &self,
        _filter: &crate::project::PortfolioQueryFilter,
    ) -> Result<Vec<crate::project::PortfolioRawRow>, StoreError> {
        Ok(Vec::new())
    }

    /// Count projects matching `filter`. Used by the §6.1 sidebar
    /// counts (`?count_only=1`). Default impl returns 0.
    async fn count_projects(
        &self,
        _filter: &crate::project::ProjectListFilter,
    ) -> Result<i64, StoreError> {
        Ok(0)
    }

    /// Fetch a single project by id, or `None` when absent.
    async fn get_project(
        &self,
        _id: Uuid,
    ) -> Result<Option<crate::project::Project>, StoreError> {
        Ok(None)
    }

    /// Insert a new project. The store assigns `id`, stamps
    /// `created_at` / `updated_at`, initialises `version = 1`, and
    /// zeroes the denormalised issue counts. Default impl rejects
    /// the call so fakes that haven't opted in fail loudly rather
    /// than silently swallowing the write.
    async fn create_project(
        &self,
        _upsert: &crate::project::ProjectUpsert,
    ) -> Result<crate::project::Project, StoreError> {
        Err(StoreError::Invalid(
            "projects not supported by this store".into(),
        ))
    }

    /// Update a project under §8.2 CAS. `expected_version` matches
    /// the row's current `version`; a mismatch returns
    /// [`StoreError::Conflict`] and the caller surfaces it as a
    /// 409. `created_by` is immutable per §9.2 and is therefore
    /// not in the upsert payload's update path.
    ///
    /// `org_id` on the upsert is ignored on update — projects do
    /// not move between orgs (v1: §4 — cross-org rollups are §10).
    async fn update_project(
        &self,
        _id: Uuid,
        _expected_version: i64,
        _upsert: &crate::project::ProjectUpsert,
    ) -> Result<crate::project::Project, StoreError> {
        Err(StoreError::Invalid(
            "projects not supported by this store".into(),
        ))
    }

    /// Archive a project (§9.2 elevated op). Sets `status =
    /// 'archived'`, bumps `version`, and stamps `updated_at`.
    /// Idempotent — archiving an already-archived project returns
    /// the row unchanged (no `version` bump). `expected_version`
    /// CAS gate matches `update_project`.
    async fn archive_project(
        &self,
        _id: Uuid,
        _expected_version: i64,
    ) -> Result<crate::project::Project, StoreError> {
        Err(StoreError::Invalid(
            "projects not supported by this store".into(),
        ))
    }

    /// Attach a batch of issues to a project. CAS-gated on the
    /// project's `version` (§7.2). Per-row outcomes flow back via
    /// [`crate::project::ProjectIssueAddOutcome`]:
    ///
    /// * `added` — rows the store accepted; the project's
    ///   `version`, `issue_count`, and `closed_issue_count` are
    ///   updated to reflect the additions.
    /// * `skipped` — rows refused because of the v1 `UNIQUE
    ///   (issue_id)` constraint (already in another project), an
    ///   unknown issue id, or a cross-org issue. The project's
    ///   `version` is still bumped if at least one issue was
    ///   added; otherwise it is unchanged.
    ///
    /// `expected_version` mismatch returns
    /// [`StoreError::Conflict`].
    async fn add_issues_to_project(
        &self,
        _project_id: Uuid,
        _expected_version: i64,
        _issue_ids: &[Uuid],
        _actor: Option<Uuid>,
    ) -> Result<crate::project::ProjectIssueAddOutcome, StoreError> {
        Err(StoreError::Invalid(
            "projects not supported by this store".into(),
        ))
    }

    /// Detach an issue from a project. CAS-gated on the project's
    /// `version`. Returns the post-removal `Project` row so the
    /// caller can echo the new counts back to the UI. A no-op
    /// remove (the issue is not currently in this project) is a
    /// [`StoreError::NotFound`] — the REST layer collapses that to
    /// 404 so retries are idempotent at the application boundary.
    async fn remove_issue_from_project(
        &self,
        _project_id: Uuid,
        _issue_id: Uuid,
        _expected_version: i64,
    ) -> Result<crate::project::Project, StoreError> {
        Err(StoreError::Invalid(
            "projects not supported by this store".into(),
        ))
    }

    /// Resolve the (single, per the v1 `UNIQUE (issue_id)` rule)
    /// project an issue is attached to, or `None` when the issue
    /// is not in any project. Backs `GET /issues/{id}/project`
    /// (§7.2) and the §6.5 detail-pane chip.
    async fn get_project_for_issue(
        &self,
        _issue_id: Uuid,
    ) -> Result<Option<crate::project::Project>, StoreError> {
        Ok(None)
    }

    /// List the `dp_issues.id`s currently attached to a project.
    /// Returned in `added_at ASC` order so the §6.3 issue list can
    /// render a stable "first added" sort without a join through
    /// `dp_issues`. The full issue rows come from the existing
    /// [`Store::list_issues`] surface — this method only resolves
    /// membership.
    async fn list_issue_ids_for_project(
        &self,
        _project_id: Uuid,
    ) -> Result<Vec<Uuid>, StoreError> {
        Ok(Vec::new())
    }

    /// Resolve the `(issue_id, tag_value)` pairs for every kv tag
    /// link attached to one of the project's issues whose
    /// `dp_tags.key = tag_key` and `dp_tags.archived_at IS NULL`
    /// (PROJECT-VIEW.md §5.1 / §7.2 — backs `Group by: tag:<key>`).
    ///
    /// An issue can appear multiple times in the result when it
    /// carries multiple distinct values for the same key (e.g.
    /// `category:firmware` + `category:hardware`). Issues without
    /// any matching tag are not returned — the caller's
    /// `list_issue_ids_for_project` set drives the "No <key>"
    /// bucket synthetically.
    ///
    /// Implementations must restrict to `dp_tags.kind = 'kv'` and
    /// ignore archived tags. Tag scope (`scope_org_id` /
    /// `scope_user_id` / `scope_team_id`) is **not** filtered here;
    /// any tag visible to the issue is fair game for grouping. The
    /// REST authz layer above is the gate.
    ///
    /// Default impl returns an empty vec so non-Postgres fakes
    /// don't need to override; production backends override.
    async fn list_project_issue_tag_values(
        &self,
        _project_id: Uuid,
        _tag_key: &str,
    ) -> Result<Vec<(Uuid, String)>, StoreError> {
        Ok(Vec::new())
    }

    /// Variant of [`list_project_issue_tag_values`] scoped to an
    /// explicit issue id set. Used by saved-view tabs (PROJECT-
    /// VIEW.md §5.4) whose membership lives in
    /// `dp_project_view_issues` and may not intersect with the
    /// project's "All"-tab `dp_project_issues` rows — the
    /// project-scoped variant would miss them.
    ///
    /// Same kv / non-archived semantics as the project-scoped
    /// variant. Default impl returns an empty vec so non-Postgres
    /// fakes don't need to override.
    async fn list_issue_tag_values(
        &self,
        _issue_ids: &[Uuid],
        _tag_key: &str,
    ) -> Result<Vec<(Uuid, String)>, StoreError> {
        Ok(Vec::new())
    }

    /// Return the distinct `dp_tags.key` values present on the
    /// project's issues — non-NULL, non-archived, `kind='kv'`. Used
    /// by `GET /projects/{id}/group-by-options` to drive the
    /// dynamic dimension dropdown (PROJECT-VIEW.md §5.1).
    ///
    /// Default impl returns an empty vec.
    async fn list_project_issue_tag_keys(
        &self,
        _project_id: Uuid,
    ) -> Result<Vec<String>, StoreError> {
        Ok(Vec::new())
    }

    // ---- project saved views (PROJECT-VIEW.md §6.1, Slice 4) -----

    /// List the caller's saved views for a project, ordered by
    /// `position ASC` (then `created_at ASC` as a stable tiebreak
    /// for the unlikely case two rows share a position after a
    /// partial reorder). v1 scopes to a single owner — shared
    /// views (`visibility='project'`) are reserved for a later
    /// slice and never returned here.
    ///
    /// Default impl returns an empty vec.
    async fn list_project_views(
        &self,
        _project_id: Uuid,
        _owner_user_id: Uuid,
    ) -> Result<Vec<crate::project_view::ProjectView>, StoreError> {
        Ok(Vec::new())
    }

    /// Fetch a single saved view by id. Returns `None` when the
    /// row is missing or `owner_user_id` does not match (so the
    /// REST layer can collapse not-mine to 404 without leaking the
    /// id's existence). Default impl returns `None`.
    async fn get_project_view(
        &self,
        _id: Uuid,
        _owner_user_id: Uuid,
    ) -> Result<Option<crate::project_view::ProjectView>, StoreError> {
        Ok(None)
    }

    /// Insert a new saved view. The store assigns `id`, stamps
    /// `created_at` / `updated_at`, and appends `position = N`
    /// where N is the count of pre-existing views for
    /// `(project_id, owner_user_id)`. The `UNIQUE (project_id,
    /// owner_user_id, name)` constraint surfaces a duplicate name
    /// as [`StoreError::Conflict`].
    ///
    /// Default impl rejects so fakes that haven't opted in fail
    /// loudly rather than silently dropping the write.
    async fn create_project_view(
        &self,
        _project_id: Uuid,
        _owner_user_id: Uuid,
        _upsert: &crate::project_view::ProjectViewUpsert,
    ) -> Result<crate::project_view::ProjectView, StoreError> {
        Err(StoreError::Invalid(
            "project views not supported by this store".into(),
        ))
    }

    /// Update a saved view. Mutates `name`, `group_by`,
    /// `filter_clauses`, `sort`, and `visibility` from the upsert
    /// payload; `position` is rewritten only by
    /// [`Store::reorder_project_views`] (kept off the PATCH so
    /// rename and reorder don't race).
    ///
    /// Returns [`StoreError::NotFound`] when the row is missing
    /// or owned by another user; [`StoreError::Conflict`] when a
    /// rename collides with another view's name.
    async fn update_project_view(
        &self,
        _id: Uuid,
        _owner_user_id: Uuid,
        _upsert: &crate::project_view::ProjectViewUpsert,
    ) -> Result<crate::project_view::ProjectView, StoreError> {
        Err(StoreError::Invalid(
            "project views not supported by this store".into(),
        ))
    }

    /// Delete a saved view. Returns `Ok(())` on success;
    /// [`StoreError::NotFound`] when the row is missing or not
    /// owned by the caller. Positions of sibling views are **not**
    /// rewritten on delete — the tab strip tolerates gaps and the
    /// next reorder normalises them.
    async fn delete_project_view(
        &self,
        _id: Uuid,
        _owner_user_id: Uuid,
    ) -> Result<(), StoreError> {
        Err(StoreError::Invalid(
            "project views not supported by this store".into(),
        ))
    }

    /// Rewrite positions for the caller's views on a project. The
    /// `ordered_ids` slice must equal the caller's existing view
    /// ids on this project (no adds, no removes) — set mismatch
    /// returns [`StoreError::Invalid`]. On success the rows are
    /// stamped `position = 0..N-1` in one transaction and the
    /// updated list is returned in the new order.
    async fn reorder_project_views(
        &self,
        _project_id: Uuid,
        _owner_user_id: Uuid,
        _ordered_ids: &[Uuid],
    ) -> Result<Vec<crate::project_view::ProjectView>, StoreError> {
        Err(StoreError::Invalid(
            "project views not supported by this store".into(),
        ))
    }

    // ---- per-view (per-tab) issue membership ----------------------
    //
    // Parallel to the project-level membership pair above; a row in
    // `dp_project_view_issues` marks an issue as manually placed on
    // a saved view. The "All" tab (no view selected) keeps using
    // [`Store::list_issue_ids_for_project`].

    /// List the issue ids manually placed on `view_id`, in the
    /// order they were added (oldest first). Caller is responsible
    /// for ensuring the view exists and is owned by the requester
    /// — this method does not gate on owner.
    async fn list_issue_ids_for_view(
        &self,
        _view_id: Uuid,
    ) -> Result<Vec<Uuid>, StoreError> {
        Ok(Vec::new())
    }

    /// Idempotently attach issues to a saved view. The natural-key
    /// PK `(view_id, issue_id)` makes a re-add a no-op. Caller
    /// must have already attached the issues to the parent
    /// project — this method does not validate cross-project
    /// membership; the FK on `issue_id` is the only guardrail.
    async fn add_issues_to_view(
        &self,
        _view_id: Uuid,
        _issue_ids: &[Uuid],
    ) -> Result<(), StoreError> {
        Err(StoreError::Invalid(
            "project view membership not supported by this store".into(),
        ))
    }

    /// Idempotently detach an issue from a saved view. A no-op
    /// detach (the issue wasn't on the view) returns `Ok(())` so
    /// retries don't 404 the caller.
    async fn remove_issue_from_view(
        &self,
        _view_id: Uuid,
        _issue_id: Uuid,
    ) -> Result<(), StoreError> {
        Err(StoreError::Invalid(
            "project view membership not supported by this store".into(),
        ))
    }

    // ---- project ↔ repo associations -----------------------------

    /// List the repos associated with a project (soft scoping for
    /// the §6.3 issue picker). Returned in `added_at ASC` order so
    /// the UI renders a stable "first added" sequence.
    async fn list_project_repos(
        &self,
        _project_id: Uuid,
    ) -> Result<Vec<crate::project::ProjectRepo>, StoreError> {
        Ok(Vec::new())
    }

    /// Idempotently attach a repo to a project. The natural-key
    /// `(project_id, repo_id)` PK makes a re-add a no-op. Returns
    /// the row (newly-inserted or pre-existing).
    async fn add_project_repo(
        &self,
        _project_id: Uuid,
        _repo_id: Uuid,
        _actor: Option<Uuid>,
    ) -> Result<crate::project::ProjectRepo, StoreError> {
        Err(StoreError::Invalid(
            "project repos not supported by this store".into(),
        ))
    }

    /// Detach a repo from a project. Idempotent — a no-op delete
    /// returns `Ok(())` so retries don't 404 the caller. (Unlike
    /// `delete_board_link`, the row carries no auxiliary state
    /// the caller might need to learn about a stale UI.)
    async fn remove_project_repo(
        &self,
        _project_id: Uuid,
        _repo_id: Uuid,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    // ---- project ↔ board mirror (linear-projects-v2.md slice B) --

    /// List every `dp_project_board_links` row for a project, in
    /// `created_at ASC` order so the §6.3 "Linked GitHub boards"
    /// block renders a stable order matching the link-now sequence.
    /// Default impl returns the empty vec so fakes that don't care
    /// about mirroring stay quiet.
    async fn list_board_links(
        &self,
        _project_id: Uuid,
    ) -> Result<Vec<crate::board_link::BoardLink>, StoreError> {
        Ok(Vec::new())
    }

    /// Fetch a single board link by primary key, or `None` when
    /// absent. Backs the §7.3 DELETE handler's existence check
    /// (so a stale UI gets a clean 404 instead of an opaque error).
    async fn get_board_link(
        &self,
        _id: Uuid,
    ) -> Result<Option<crate::board_link::BoardLink>, StoreError> {
        Ok(None)
    }

    /// Insert a new `dp_project_board_links` row. The store
    /// assigns `id` and stamps `created_at` / `updated_at`. The
    /// natural-key `(project_id, github_board_node_id)` UNIQUE
    /// constraint surfaces a re-link of the same board as
    /// [`StoreError::Conflict`] — callers translate that to 409
    /// so the UI can render "already linked".
    ///
    /// Default impl rejects so fakes that haven't opted in fail
    /// loudly instead of silently swallowing the write.
    async fn create_board_link(
        &self,
        _upsert: &crate::board_link::BoardLinkUpsert,
    ) -> Result<crate::board_link::BoardLink, StoreError> {
        Err(StoreError::Invalid(
            "board links not supported by this store".into(),
        ))
    }

    /// Delete a board link. Cascades through to every
    /// `dp_project_board_items` row under it (FK
    /// `ON DELETE CASCADE`). A no-op delete (the link id does not
    /// resolve) is [`StoreError::NotFound`] so retries are
    /// idempotent at the application boundary.
    async fn delete_board_link(
        &self,
        _id: Uuid,
    ) -> Result<(), StoreError> {
        Err(StoreError::NotFound {
            entity: "board_link",
            id: "<unsupported>".into(),
        })
    }

    /// Refresh the cached `github_board_title` / `github_board_url`
    /// / `github_board_cached_at` columns on a link row. Called by
    /// the §7.3 picker on every read and by the nightly safety-net
    /// job so renamed / deleted boards surface within 24h instead
    /// of waiting on a user-visible read.
    ///
    /// Default impl is a no-op so non-pg fakes treat the call as
    /// "cache already fresh".
    async fn refresh_board_link_cache(
        &self,
        _id: Uuid,
        _title: Option<&str>,
        _url: Option<&str>,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    /// List every `dp_project_board_items` row for one issue across
    /// all of its (project's) linked boards. Backs the §6.5
    /// detail-pane `SyncStatus` aggregate — one row per (link,
    /// issue) outcome, so the UI can render
    /// "N of N boards ✓ HH:mm:ss" with per-board disclosure.
    async fn list_board_items_for_issue(
        &self,
        _issue_id: Uuid,
    ) -> Result<Vec<crate::board_link::BoardItem>, StoreError> {
        Ok(Vec::new())
    }

    /// Fetch the `dp_project_board_items` row for a (link, issue)
    /// pair, or `None` when the pair has never been mirrored. The
    /// mirror worker uses this to decide whether to issue
    /// `addProjectV2ItemById` (no existing row) or
    /// `updateProjectV2ItemFieldValue` against the stored
    /// `item_node_id`.
    async fn get_board_item(
        &self,
        _link_id: Uuid,
        _issue_id: Uuid,
    ) -> Result<Option<crate::board_link::BoardItem>, StoreError> {
        Ok(None)
    }

    /// Record the outcome of one mirror attempt against a
    /// (link, issue) pair. On success: upserts
    /// `dp_project_board_items` with the returned `item_node_id`,
    /// stamps `last_synced_at`, clears `last_error`, **and** rolls
    /// the success up to `dp_project_board_links.last_mirror_at` /
    /// clears `last_mirror_error`. On failure: writes `last_error`
    /// on the item row (without changing `item_node_id`) and rolls
    /// the failure up to `last_mirror_error` on the link.
    ///
    /// The upsert / aggregate roll-up runs in one transaction so
    /// the §6.5 `SyncStatus` aggregate can never observe a
    /// half-recorded state. Default impl is a no-op so non-pg
    /// fakes treat the call as silently dropped.
    async fn record_board_item_result(
        &self,
        _link_id: Uuid,
        _issue_id: Uuid,
        _outcome: crate::board_link::BoardItemMirrorOutcome<'_>,
    ) -> Result<(), StoreError> {
        Ok(())
    }
}

/// Outcome of a single Projects v2 mirror attempt, fed back into
/// [`Store::record_issue_dates_mirror_result`]. Borrowed strings
/// so the worker can pass GraphQL error text straight from its
/// transport buffer without an intermediate allocation.
#[derive(Debug, Clone, Copy)]
pub enum IssueDatesMirrorOutcome<'a> {
    /// Mirror succeeded; `node_id` is the Projects v2 *item* node
    /// id GitHub returned (persist so the next edit updates the
    /// same item instead of creating a duplicate card).
    Success {
        /// The Projects v2 item node id to persist.
        node_id: &'a str,
    },
    /// Mirror failed; `error` is the verbatim GraphQL error text.
    Failure {
        /// Error text to persist to `mirror_error`.
        error: &'a str,
    },
}

/// Compact projection of `dp_issues` rows the §8.5 sweeper needs:
/// the issue id, the version after the abandoned CAS, the actor
/// who started the write, and the `pending_remote_at` timestamp.
/// Returned by [`Store::list_issues_with_pending_remote_older_than`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRemoteIssue {
    /// `dp_issues.id`.
    pub issue_id: Uuid,
    /// `dp_issues.repo_id`. Denormalised for the audit row.
    pub repo_id: Uuid,
    /// Current `dp_issues.version` (post-CAS, pre-rollback).
    pub version: i64,
    /// The dp-pulse user who initiated the abandoned write.
    pub actor_user_id: Uuid,
    /// When the abandoned CAS landed. The sweeper picks rows where
    /// this is older than `now() - pending_remote_timeout_secs`.
    pub pending_remote_at: DateTime<Utc>,
}

/// One row in the timeline returned by
/// [`Store::list_events_for_issue`]. Mirrors the shape `GET
/// /issues/{id}/timeline` emits — see `linear-projects-idea.md`
/// §5.6.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueTimelineRow {
    /// `dp_activity_events.id`.
    pub id: Uuid,
    /// Event kind, parsed back into the typed enum.
    pub kind: EventKind,
    /// Source timestamp (`ts`), UTC.
    pub ts: DateTime<Utc>,
    /// One-line summary derived from `payload` — `"opened"`,
    /// `"closed"`, `"commented: <body excerpt>"`, …
    pub payload_summary: String,
}

/// Repo sync freshness — synthesised from `dp_fetch_cursors` plus
/// scheduler state. See `linear-projects-idea.md` §3.9 / §5.9.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSyncStatus {
    /// Newest `dp_fetch_cursors.updated_at` seen for this repo.
    pub last_synced_at: Option<DateTime<Utc>>,
    /// Same source as `last_synced_at` until the schema grows a
    /// dedicated `attempted_at` column (no error column exists
    /// today; treat success and attempt as the same instant).
    pub last_attempt_at: Option<DateTime<Utc>>,
    /// Last sync error message, or `None` when the latest sync
    /// succeeded. Currently always `None` — the cursor row carries
    /// no error column; an explicit error projection would arrive
    /// in a follow-up migration.
    pub last_error: Option<String>,
}

/// Which §5.10 report metric to compute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueMetric {
    /// Closed issues per bucket.
    Throughput,
    /// Median open → close duration per bucket (seconds).
    LeadTime,
    /// Currently-open assigned count.
    Wip,
    /// Open + idle (`updated_at < now() - interval '30 days'`).
    Stale,
    /// Open + no assignee + no label.
    Untriaged,
}

/// Group-by axis for §5.10 metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueMetricGroupBy {
    /// Group by `repo_id`.
    Repo,
    /// Group by `org_id`.
    Org,
    /// Group by per-row `assignee` (`jsonb_array_elements_text`).
    Assignee,
    /// Group by ISO week (`date_trunc('week', ...)`).
    Week,
    /// Group by ISO day (`date_trunc('day', ...)`).
    Day,
}

/// Filter passed to [`Store::issue_metrics`].
#[derive(Debug, Clone)]
pub struct IssueMetricsFilter {
    /// Which metric to compute.
    pub metric: IssueMetric,
    /// Group-by axis.
    pub group_by: IssueMetricGroupBy,
    /// Inclusive lower bound on the event timestamp (`since`).
    pub since: Option<DateTime<Utc>>,
    /// Exclusive upper bound on the event timestamp (`until`).
    pub until: Option<DateTime<Utc>>,
    /// Restrict to these orgs (caller's `org_ids ∩ wire scope`).
    pub org_ids: Vec<Uuid>,
    /// Restrict to these repos.
    pub repo_ids: Vec<Uuid>,
}

/// One row in the §5.10 reports response.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueMetricRow {
    /// Bucket label — repo slug, org login, login, or RFC3339 date.
    pub bucket: String,
    /// Metric value. Unit depends on the metric: count for
    /// throughput / wip / stale / untriaged, seconds (median) for
    /// lead_time.
    pub value: f64,
    /// Row count contributing to `value` (used by the lead-time
    /// median so the frontend can show "n=12").
    pub count: i64,
}

/// Filter for [`Store::list_repos`] / [`Store::count_repos`].
///
/// All fields are conjunctive. `limit` is capped at
/// [`MAX_LIST_LIMIT`] by the dp-rest layer before it reaches the
/// store; the store treats it as a hard upper bound.
#[derive(Debug, Clone, Default)]
pub struct RepoListFilter {
    /// Restrict to one org. `None` ⇒ every org.
    pub org_id: Option<Uuid>,
    /// Case-insensitive substring search on `dp_repos.name` and
    /// `dp_orgs.login`. `None` or empty ⇒ no search.
    pub q: Option<String>,
    /// Page size. 1..=[`MAX_LIST_LIMIT`].
    pub limit: i64,
    /// Page offset.
    pub offset: i64,
}

/// Filter for [`Store::list_issues`] / [`Store::count_issues`] /
/// [`Store::list_inbox_issues`].
///
/// Fields combine conjunctively (AND across the struct).
/// Repeatable fields (`repo_ids`, `org_ids`, `assignees`, `labels`)
/// are ALSO conjunctive within themselves — matching Linear's
/// pill semantics, where adding a second label narrows the set
/// rather than widening it.
///
/// Scalar fields (`repo_id`, `org_id`, `assignee`) are retained for
/// back-compat with the early `GET /issues` callers. When both
/// scalar and array forms are populated, the predicate is the
/// intersection (both apply). The dp-rest layer normalises a
/// scalar into the matching array before calling, so most
/// callers should only populate the array form.
#[derive(Debug, Clone, Default)]
pub struct IssueListFilter {
    /// Restrict to one repo (back-compat shorthand for
    /// `repo_ids = vec![…]`).
    pub repo_id: Option<Uuid>,
    /// Restrict to one org (back-compat shorthand for
    /// `org_ids = vec![…]`).
    pub org_id: Option<Uuid>,
    /// Filter by state. `None` ⇒ open + closed.
    pub state: Option<IssueState>,
    /// Match an assignee login (back-compat shorthand for
    /// `assignees = vec![…]`).
    pub assignee: Option<String>,
    /// Case-insensitive substring search on `dp_issues.title`.
    pub q: Option<String>,
    /// Page size. 1..=[`MAX_LIST_LIMIT`].
    pub limit: i64,
    /// Page offset.
    pub offset: i64,

    // ---- triage-spine extensions (slice 1) --------------------

    /// Match issues whose `repo_id` is in this set. Empty ⇒ no
    /// constraint. Logically OR within the set (any of these
    /// repos) but AND with the other filter fields.
    pub repo_ids: Vec<Uuid>,
    /// Match issues whose `org_id` is in this set. Empty ⇒ no
    /// constraint. The `/me/queue` handler always populates this
    /// with the caller's org set so per-row authz is enforced in
    /// SQL even if the policy layer ever degrades open.
    pub org_ids: Vec<Uuid>,
    /// Match issues having **all** of these assignees (JSONB
    /// containment AND). Empty ⇒ no constraint.
    pub assignees: Vec<String>,
    /// Match issues having **all** of these labels (JSONB
    /// containment AND). Empty ⇒ no constraint.
    pub labels: Vec<String>,
    /// Match issues whose `author` column equals this value. Rows
    /// where `author IS NULL` (un-backfilled) never match — same
    /// behaviour as any other scalar filter.
    pub author: Option<String>,
    /// Match issues whose `state_reason` column equals this value
    /// (e.g. `"completed"` / `"not_planned"` / `"reopened"`).
    pub state_reason: Option<String>,
    /// Match issues with `updated_at >= updated_since`.
    pub updated_since: Option<DateTime<Utc>>,
    /// Untriaged smart-view shortcut: when true, restrict to rows
    /// with **no** assignees and **no** labels. Combines with the
    /// rest of the filter (so "Untriaged in org X" is one call).
    pub untriaged_only: bool,
    /// Optional keyset cursor used by `/me/queue` pagination.
    /// When `Some((ts, id))`, the store emits a strictly-less-than
    /// page on `(updated_at, id)` so concurrent inbox mutations do
    /// not produce drift across pages. Empty for non-keyset
    /// callers.
    pub keyset_after: Option<(DateTime<Utc>, Uuid)>,
}

/// Hard upper bound on `limit` across the workflow read surface.
pub const MAX_LIST_LIMIT: i64 = 200;

/// Default `limit` when the caller omits one.
pub const DEFAULT_LIST_LIMIT: i64 = 50;

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time check that [`Store`] is object-safe — every
    /// surface (rest / mcp / cli / reports) holds an
    /// `Arc<dyn Store>`, so a regression would break the world.
    #[allow(dead_code)]
    fn store_is_object_safe(_s: &dyn Store) {}

    #[test]
    fn store_error_displays_known_variants() {
        let e = StoreError::NotFound {
            entity: "user",
            id: "00000000-0000-0000-0000-000000000000".into(),
        };
        assert!(format!("{e}").contains("not found"));
        let c = StoreError::Conflict("dup delivery_id".into());
        assert!(format!("{c}").contains("conflict"));
    }
}
