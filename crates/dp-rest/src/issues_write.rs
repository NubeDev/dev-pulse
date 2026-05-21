//! Issue write handlers — `POST /issues`, `PATCH /issues/{id}`,
//! `POST /issues/{id}/comments`.
//!
//! This is the dp-rest mount layer for the SCOPE §18 / SCOPE-PROJECTS §8
//! write path. Each handler composes the building blocks staged in
//! [`crate::issues`] (`acquire_issue_mutation_slot`,
//! `commit_issue_mutation`, `rollback_issue_mutation`):
//!
//! 1. §18.2 step 3 — visibility check (handled upstream by the
//!    `(issues, write)` permission gate on the router).
//! 2. §18.2 step 4 — install-permission check via
//!    [`crate::app_permissions::require_issues_write`]. Failure ⇒
//!    `403 writes_not_available_for_org`.
//! 3. §18.2 step 5 — optimistic CAS via
//!    [`acquire_issue_mutation_slot`]. CAS miss ⇒
//!    `409 stale_local_version` per §18.3 / §8.3.
//! 4. §18.2 step 6 — synchronous GitHub call through
//!    [`IssueWriteBackend`].
//! 5. §18.2 step 7 — success ⇒ [`commit_issue_mutation`] (clears
//!    `pending_remote`, writes the audit row, and drains the §13.7
//!    webhook buffer).
//! 6. §18.2 step 8 — failure ⇒ [`rollback_issue_mutation`] (bumps
//!    `version` again, clears `pending_remote`, writes the audit row
//!    with the verbatim GitHub error, drains the §13.7 buffer).
//!
//! The `POST /issues` create path is structurally different — there
//! is no local row to CAS against until GitHub assigns the issue a
//! number. For create we (a) check writes-available, (b) call
//! GitHub, (c) record an audit-only `issue.create` row. The fetcher
//! / webhook receiver materialise the `dp_issues` row on the next
//! reconciler pass; the write path itself does not synthesise one.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    extract::{Extension, Path, State},
    response::Json,
    routing::{patch, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::issue_mutation::IssueMutationOp;
use dp_domain::store::Store;

use crate::app_permissions::require_issues_write;
use crate::audit::{self, Principal};
use crate::error::ApiError;
use crate::issues::{
    acquire_issue_mutation_slot, commit_issue_mutation, rollback_issue_mutation, AcquireOutcome,
};
use crate::issues_read::IssueDto;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Backend trait — the GitHub I/O seam every handler reaches through.
// ---------------------------------------------------------------------------

/// All GitHub-side I/O the §8 write path performs flows through this
/// trait. Production binaries wire an octocrab-backed implementation
/// from the bin layer; tests pass a fake.
///
/// Each method takes the resolved repository identity
/// (`org_login` + `repo_name`) so the trait stays unaware of
/// dev-pulse-internal store layout — the handler does the
/// `repo_id -> (Org, Repo)` resolution before calling.
#[async_trait]
pub trait IssueWriteBackend: Send + Sync + 'static {
    /// `POST /repos/{owner}/{repo}/issues`. Returns the GitHub-side
    /// issue number assigned to the new row.
    async fn create_issue(
        &self,
        owner_login: &str,
        repo_name: &str,
        title: &str,
        body: Option<&str>,
    ) -> Result<i64, IssueWriteError>;

    /// `PATCH /repos/{owner}/{repo}/issues/{number}`. Carries the
    /// merged field set the user requested. The trait does not
    /// inspect which fields are `Some(_)` — it forwards the patch
    /// verbatim.
    ///
    /// On success returns `Some(payload)` carrying the GitHub-side
    /// issue JSON when the backend can supply it. The handler
    /// hands the payload to
    /// [`dp_fetcher::worker::handlers::parse_issue_upsert`] and
    /// upserts it into `dp_issues` so the local row reflects the
    /// write before the next webhook / reconciler tick — closing
    /// the two-way sync loop the UI relies on. Backends that can
    /// not surface the payload (the unconfigured stub, certain
    /// fakes) return `None` and the handler falls back to the
    /// pre-write local row.
    async fn update_issue(
        &self,
        owner_login: &str,
        repo_name: &str,
        number: i64,
        patch: &IssuePatch,
    ) -> Result<Option<serde_json::Value>, IssueWriteError>;

    /// `POST /repos/{owner}/{repo}/issues/{number}/comments`.
    async fn create_comment(
        &self,
        owner_login: &str,
        repo_name: &str,
        number: i64,
        body: &str,
    ) -> Result<(), IssueWriteError>;

    /// `GET /repos/{owner}/{repo}/issues/{number}` — single-issue
    /// refetch used by the lazy resync path (`POST /issues/{id}/refresh`
    /// and the post-comment refresh in [`create_comment`]). Returns
    /// `Some(payload)` carrying the GitHub-side JSON so the caller
    /// can upsert immediately. Backends that can not surface a
    /// payload return `Ok(None)` and the handler falls back to the
    /// stored row — the default impl returns `Ok(None)` so the
    /// unconfigured stub and test fakes don't have to opt in.
    async fn refresh_issue(
        &self,
        _owner_login: &str,
        _repo_name: &str,
        _number: i64,
    ) -> Result<Option<serde_json::Value>, IssueWriteError> {
        Ok(None)
    }
}

/// All errors the [`IssueWriteBackend`] may surface. Split on the
/// dimension the handler cares about — transient vs validation —
/// because the rollback path is the same shape regardless (re-apply
/// pre-mutation fields + bump version), but the surfaced API error
/// changes (`502 upstream_unavailable` vs `422 validation_failed`).
#[derive(Debug, thiserror::Error)]
pub enum IssueWriteError {
    /// GitHub returned 4xx (most commonly 422 for validation). The
    /// body is the verbatim GitHub error text — safe to surface.
    #[error("github validation: {0}")]
    Validation(String),
    /// GitHub returned 5xx or the request errored at the transport
    /// layer. The §18.2 step 8 rollback path runs and the API
    /// surfaces `502 upstream_unavailable`.
    #[error("github upstream: {0}")]
    Upstream(String),
    /// The configured backend explicitly refuses to handle calls —
    /// the deployment hasn't wired a real GitHub writer yet. Used
    /// only by [`UnconfiguredIssueWriter`].
    #[error("issue write backend not configured")]
    Unconfigured,
}

impl IssueWriteError {
    fn into_api_error(self) -> ApiError {
        match self {
            // 422 — GitHub said no, the user's input is bad. Stable
            // code so the UI can switch on it.
            IssueWriteError::Validation(msg) => ApiError::BadRequest {
                code: "github_validation_failed",
                message: msg,
            },
            // 5xx / transport / unconfigured — surface as the same
            // upstream-unavailable shape; the rollback path has
            // already run so retries are safe.
            IssueWriteError::Upstream(msg) => ApiError::BadRequest {
                code: "upstream_unavailable",
                message: msg,
            },
            IssueWriteError::Unconfigured => ApiError::BadRequest {
                code: "upstream_unavailable",
                message: "issue write backend not configured".into(),
            },
        }
    }
}

/// Default [`IssueWriteBackend`] — refuses every call. Wired into
/// [`crate::state::AppState::new`] so deployments that forget to
/// wire a real backend fail loudly instead of silently bypassing
/// GitHub. The bin layer overrides with
/// [`AppState::with_issue_writer`][crate::state::AppState::with_issue_writer].
#[derive(Debug, Default, Clone, Copy)]
pub struct UnconfiguredIssueWriter;

#[async_trait]
impl IssueWriteBackend for UnconfiguredIssueWriter {
    async fn create_issue(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: Option<&str>,
    ) -> Result<i64, IssueWriteError> {
        Err(IssueWriteError::Unconfigured)
    }
    async fn update_issue(
        &self,
        _: &str,
        _: &str,
        _: i64,
        _: &IssuePatch,
    ) -> Result<Option<serde_json::Value>, IssueWriteError> {
        Err(IssueWriteError::Unconfigured)
    }
    async fn create_comment(
        &self,
        _: &str,
        _: &str,
        _: i64,
        _: &str,
    ) -> Result<(), IssueWriteError> {
        Err(IssueWriteError::Unconfigured)
    }
}

/// Production [`IssueWriteBackend`] backed by the dp-fetcher
/// octocrab client. Used by the bin layer in both PAT mode (the
/// classic / fine-grained token authorises the write) and the
/// per-org App-installation mode (a future stage will pick the
/// right per-org `Client` here).
///
/// The adapter is a thin shim: every method translates the
/// `IssuePatch` / argument set into the fetcher's
/// [`dp_fetcher::client::IssueRemotePatch`] / typed call, then maps
/// the fetcher's [`dp_fetcher::client::GhWriteError`] into
/// [`IssueWriteError`] one-to-one.
pub struct FetcherIssueWriter {
    client: Arc<dp_fetcher::client::Client>,
}

impl FetcherIssueWriter {
    /// Construct from a ready-to-use fetcher client. The bin layer
    /// already builds one for the read path (reconciler/backfill);
    /// the writer reuses the same handle so the local request
    /// budget covers writes too.
    pub fn new(client: Arc<dp_fetcher::client::Client>) -> Self {
        Self { client }
    }
}

impl std::fmt::Debug for FetcherIssueWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FetcherIssueWriter").finish_non_exhaustive()
    }
}

fn map_gh_write_err(e: dp_fetcher::client::GhWriteError) -> IssueWriteError {
    match e {
        dp_fetcher::client::GhWriteError::Validation(m) => IssueWriteError::Validation(m),
        dp_fetcher::client::GhWriteError::Upstream(m) => IssueWriteError::Upstream(m),
    }
}

#[async_trait]
impl IssueWriteBackend for FetcherIssueWriter {
    async fn create_issue(
        &self,
        owner_login: &str,
        repo_name: &str,
        title: &str,
        body: Option<&str>,
    ) -> Result<i64, IssueWriteError> {
        self.client
            .gh_create_issue(owner_login, repo_name, title, body)
            .await
            .map_err(map_gh_write_err)
    }

    async fn update_issue(
        &self,
        owner_login: &str,
        repo_name: &str,
        number: i64,
        patch: &IssuePatch,
    ) -> Result<Option<serde_json::Value>, IssueWriteError> {
        let remote = dp_fetcher::client::IssueRemotePatch {
            title: patch.title.clone(),
            body: patch.body.clone(),
            state: patch.state.clone(),
            labels: patch.labels.clone(),
            assignees: patch.assignees.clone(),
        };
        let payload = self
            .client
            .gh_update_issue(owner_login, repo_name, number, &remote)
            .await
            .map_err(map_gh_write_err)?;
        Ok(Some(payload))
    }

    async fn create_comment(
        &self,
        owner_login: &str,
        repo_name: &str,
        number: i64,
        body: &str,
    ) -> Result<(), IssueWriteError> {
        self.client
            .gh_create_comment(owner_login, repo_name, number, body)
            .await
            .map_err(map_gh_write_err)
    }

    async fn refresh_issue(
        &self,
        owner_login: &str,
        repo_name: &str,
        number: i64,
    ) -> Result<Option<serde_json::Value>, IssueWriteError> {
        let payload = self
            .client
            .gh_get_issue(owner_login, repo_name, number)
            .await
            .map_err(map_gh_write_err)?;
        Ok(Some(payload))
    }
}

// ---------------------------------------------------------------------------
// Wire DTOs
// ---------------------------------------------------------------------------

/// Body for `POST /issues`. The repo to mutate is named explicitly
/// (there is no canonical row yet to derive it from).
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CreateIssueRequest {
    /// Internal repo id the new issue belongs to.
    pub repo_id: Uuid,
    /// Issue title; required by GitHub.
    pub title: String,
    /// Optional body (Markdown).
    #[serde(default)]
    pub body: Option<String>,
}

/// Acknowledgement body for `POST /issues`. Returns the GitHub-side
/// number GitHub assigned (so the UI can deep-link immediately) and
/// echoes the actor's `repo_id` for symmetry with PATCH responses.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateIssueResponse {
    /// Echoed parent repo id.
    pub repo_id: Uuid,
    /// Repo-relative issue number GitHub assigned.
    pub number: i64,
}

/// Field-level patch the PATCH handler forwards to GitHub. Every
/// field is optional — only `Some(_)` lanes are sent to GitHub. The
/// caller is the source of truth on what to mutate; the handler
/// performs no field-vs-field consistency checks.
#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema)]
pub struct IssuePatch {
    /// New title (omitted when not changing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// New body (omitted when not changing; explicit `null` clears).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// New state — `"open"` or `"closed"`. The handler routes this
    /// to the `issue.close` / `issue.reopen` audit verbs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// Replacement label list. `None` leaves labels untouched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    /// Replacement assignee logins. `None` leaves assignees
    /// untouched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignees: Option<Vec<String>>,
}

/// Body for PATCH `/issues/{id}`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct PatchIssueRequest {
    /// CAS token — the `dp_issues.version` the UI rendered the form
    /// against. The §18.2 step 5 CAS misses if this is stale, and
    /// the handler returns `409 stale_local_version`.
    pub expected_version: i64,
    /// Field-level changes.
    #[serde(flatten)]
    pub patch: IssuePatch,
}

/// Body for POST `/issues/{id}/comments`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CreateCommentRequest {
    /// CAS token (same semantics as [`PatchIssueRequest`]).
    pub expected_version: i64,
    /// Comment body (Markdown).
    pub body: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /issues` — create a new GitHub issue.
///
/// No CAS (there is no local row yet). Sequence:
///
/// 1. Resolve `repo_id -> (Org, Repo)`.
/// 2. §18.2 step 4 install-permission check.
/// 3. Call the backend's `create_issue`.
/// 4. Record the `issue.create` audit row (target = `repo_id#number`).
///
/// The fetcher / webhook receiver materialises the `dp_issues` row
/// on the next pass.
#[utoipa::path(
    post,
    path = "/issues",
    request_body = CreateIssueRequest,
    responses(
        (status = 200, description = "Created GitHub-side; audit row written", body = CreateIssueResponse),
        (status = 403, description = "Writes not available for the target org"),
        (status = 400, description = "Validation failed at GitHub"),
    ),
    tag = "issues",
)]
pub async fn create_issue(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<CreateIssueRequest>,
) -> Result<Json<CreateIssueResponse>, ApiError> {
    let (org, repo) = resolve_repo(&*state.store, body.repo_id).await?;
    require_issues_write(&*state.store, &state.github_app, &org).await?;
    let number = state
        .issue_writer
        .create_issue(&org.login, &repo.name, &body.title, body.body.as_deref())
        .await
        .map_err(IssueWriteError::into_api_error)?;
    // Target = "{repo_id}#{number}" so the §11 transparency query
    // can correlate the audit row with the eventual dp_issues row.
    audit::record(
        &*state.store,
        principal.actor_user_id,
        audit::ISSUE_CREATE,
        format!("{}#{number}", body.repo_id),
    )
    .await?;
    Ok(Json(CreateIssueResponse {
        repo_id: body.repo_id,
        number,
    }))
}

/// `PATCH /issues/{id}` — update an existing GitHub issue. Routes
/// the `state` transition (open/closed) to the `issue.close` /
/// `issue.reopen` audit verbs; everything else is `issue.update`.
#[utoipa::path(
    patch,
    path = "/issues/{id}",
    params(("id" = Uuid, Path, description = "Issue id")),
    request_body = PatchIssueRequest,
    responses(
        (status = 200, description = "Mutation committed", body = IssueDto),
        (status = 403, description = "Writes not available for the target org"),
        (status = 409, description = "Stale local version — UI should reload"),
        (status = 400, description = "Validation failed at GitHub"),
    ),
    tag = "issues",
)]
pub async fn patch_issue(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchIssueRequest>,
) -> Result<Json<IssueDto>, ApiError> {
    let issue = state
        .store
        .get_issue(id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "issue_not_found",
            message: format!("no issue with id {id}"),
        })?;
    let (org, repo) = resolve_repo(&*state.store, issue.repo_id).await?;
    require_issues_write(&*state.store, &state.github_app, &org).await?;
    let op = patch_op(&body.patch);
    let slot = match acquire_issue_mutation_slot(
        &*state.store,
        principal.actor_user_id,
        issue.id,
        issue.repo_id,
        body.expected_version,
        op,
        json!({ "after": &body.patch }),
    )
    .await?
    {
        AcquireOutcome::Acquired(s) => s,
        AcquireOutcome::Stale { current_version } => {
            return Err(ApiError::StaleLocalVersion {
                issue_id: id,
                current_version,
            });
        }
    };
    match state
        .issue_writer
        .update_issue(&org.login, &repo.name, issue.number, &body.patch)
        .await
    {
        Ok(payload) => {
            commit_issue_mutation(&*state.store, &slot, None).await?;
            // Two-way sync: GitHub's PATCH response is the new
            // truth. Project it through the regular ingest path so
            // the local `dp_issues` row reflects the write before
            // the next webhook / reconciler tick — without this
            // the UI re-read below would return the *pre-write*
            // body / title / labels and the edit would appear lost.
            // Best-effort: a parse / upsert error here is logged
            // (the webhook will reconcile on the next tick) but
            // never fails the request the user just committed.
            if let Some(value) = payload {
                match dp_fetcher::worker::handlers::parse_issue_upsert(
                    issue.org_id,
                    issue.repo_id,
                    &value,
                ) {
                    Ok(upsert) => {
                        // Zero window: we already cleared
                        // pending_remote in commit_issue_mutation,
                        // so the §13.7 guard would not defer; pass
                        // a zero duration to make that explicit.
                        if let Err(e) = state
                            .store
                            .upsert_issue_from_github(&upsert, chrono::Duration::zero())
                            .await
                        {
                            tracing::warn!(
                                target: "dp_rest::issues_write",
                                issue_id = %issue.id,
                                error = %e,
                                "post-PATCH local upsert failed; row will reconcile on next webhook",
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "dp_rest::issues_write",
                            issue_id = %issue.id,
                            error = %e,
                            "post-PATCH parse of github payload failed; row will reconcile on next webhook",
                        );
                    }
                }
            }
        }
        Err(e) => {
            // Rollback before surfacing the error so the row is
            // released for the next attempt and the §13.7 buffer
            // is drained.
            rollback_issue_mutation(&*state.store, &slot, &e.to_string()).await?;
            return Err(e.into_api_error());
        }
    }
    // Re-read the row so the UI sees the post-commit version. The
    // store layer is authoritative; the dp_issues mutation itself
    // is reconciled on the next fetcher/webhook tick (§18.2 tail).
    let fresh = state
        .store
        .get_issue(id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "issue_not_found",
            message: format!("no issue with id {id}"),
        })?;
    let mut dto = IssueDto::from(fresh);
    crate::issues_read::attach_repo_slug_one(&*state.store, &mut dto).await?;
    Ok(Json(dto))
}

/// `POST /issues/{id}/comments` — append a comment. CAS-gated for
/// symmetry with PATCH so the UI's optimistic editor sees the same
/// stale-version surface.
///
/// After the GitHub call lands we do a best-effort single-issue
/// refetch (`gh_get_issue`) and project the payload through
/// `parse_issue_upsert`, so the local row's `comment_count` /
/// `updated_at` advance synchronously — without this the UI saw
/// the post-comment row stuck at the pre-write count until the
/// next webhook / reconciler tick (the bug the user reported).
/// The response carries the fresh `IssueDto` for the same reason
/// the PATCH handler does.
#[utoipa::path(
    post,
    path = "/issues/{id}/comments",
    params(("id" = Uuid, Path, description = "Issue id")),
    request_body = CreateCommentRequest,
    responses(
        (status = 200, description = "Comment posted; fresh issue row returned", body = IssueDto),
        (status = 403, description = "Writes not available for the target org"),
        (status = 409, description = "Stale local version — UI should reload"),
        (status = 400, description = "Validation failed at GitHub"),
    ),
    tag = "issues",
)]
pub async fn create_comment(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateCommentRequest>,
) -> Result<Json<IssueDto>, ApiError> {
    let issue = state
        .store
        .get_issue(id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "issue_not_found",
            message: format!("no issue with id {id}"),
        })?;
    let (org, repo) = resolve_repo(&*state.store, issue.repo_id).await?;
    require_issues_write(&*state.store, &state.github_app, &org).await?;
    let slot = match acquire_issue_mutation_slot(
        &*state.store,
        principal.actor_user_id,
        issue.id,
        issue.repo_id,
        body.expected_version,
        IssueMutationOp::Comment,
        json!({ "after": { "body": &body.body } }),
    )
    .await?
    {
        AcquireOutcome::Acquired(s) => s,
        AcquireOutcome::Stale { current_version } => {
            return Err(ApiError::StaleLocalVersion {
                issue_id: id,
                current_version,
            });
        }
    };
    match state
        .issue_writer
        .create_comment(&org.login, &repo.name, issue.number, &body.body)
        .await
    {
        Ok(()) => commit_issue_mutation(&*state.store, &slot, None).await?,
        Err(e) => {
            rollback_issue_mutation(&*state.store, &slot, &e.to_string()).await?;
            return Err(e.into_api_error());
        }
    }
    // Best-effort post-comment refresh: project the latest GitHub
    // payload through the regular ingest path so `comment_count` /
    // `updated_at` advance now instead of on the next webhook /
    // reconciler tick. Failures are logged and silently fall
    // through to the stored row — the user just succeeded at
    // commenting; never let a stale-read sub-step fail their
    // request.
    refresh_issue_best_effort(&state, &org.login, &repo.name, &issue).await;
    let fresh = state
        .store
        .get_issue(id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "issue_not_found",
            message: format!("no issue with id {id}"),
        })?;
    let mut dto = IssueDto::from(fresh);
    crate::issues_read::attach_repo_slug_one(&*state.store, &mut dto).await?;
    Ok(Json(dto))
}

/// `POST /issues/{id}/refresh` — fire-and-forget single-issue
/// resync the UI calls when an issue is opened (or when the user
/// hits a "refresh" affordance). Performs a `GET
/// /repos/{owner}/{repo}/issues/{number}` against GitHub, projects
/// the payload through `parse_issue_upsert`, and returns the post-
/// upsert row — so the frontend can swap its cached row for the
/// fresh one in a single round-trip.
///
/// Gated on `(issues, read)`: refreshing a row is a read of the
/// underlying issue (no GitHub mutation happens). The backend may
/// return `None` (the unconfigured stub, test fakes) in which case
/// we simply re-read the local row and return it — callers get a
/// 200 with the existing state, never a 503, so the lazy-refresh
/// effect on the frontend is safe to fire unconditionally.
#[utoipa::path(
    post,
    path = "/issues/{id}/refresh",
    params(("id" = Uuid, Path, description = "Issue id")),
    responses(
        (status = 200, description = "Refreshed (or fell back to local row when no backend is configured)", body = IssueDto),
        (status = 404, description = "No such issue"),
    ),
    tag = "issues",
)]
pub async fn refresh_issue(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<IssueDto>, ApiError> {
    let issue = state
        .store
        .get_issue(id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "issue_not_found",
            message: format!("no issue with id {id}"),
        })?;
    let (org, repo) = resolve_repo(&*state.store, issue.repo_id).await?;
    refresh_issue_best_effort(&state, &org.login, &repo.name, &issue).await;
    let fresh = state
        .store
        .get_issue(id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "issue_not_found",
            message: format!("no issue with id {id}"),
        })?;
    let mut dto = IssueDto::from(fresh);
    crate::issues_read::attach_repo_slug_one(&*state.store, &mut dto).await?;
    Ok(Json(dto))
}

/// Shared best-effort single-issue refetch. Used by the post-
/// comment refresh and by `POST /issues/{id}/refresh`. Never
/// returns an error: the caller has already succeeded at the
/// primary operation (or doesn't care about staleness blocking
/// the response), so a transient GitHub hiccup must not bubble.
async fn refresh_issue_best_effort(
    state: &AppState,
    owner_login: &str,
    repo_name: &str,
    issue: &dp_domain::issue::Issue,
) {
    let payload = match state
        .issue_writer
        .refresh_issue(owner_login, repo_name, issue.number)
        .await
    {
        Ok(Some(v)) => v,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!(
                target: "dp_rest::issues_write",
                issue_id = %issue.id,
                error = %e,
                "single-issue refresh failed; row will reconcile on next webhook",
            );
            return;
        }
    };
    match dp_fetcher::worker::handlers::parse_issue_upsert(
        issue.org_id,
        issue.repo_id,
        &payload,
    ) {
        Ok(upsert) => {
            if let Err(e) = state
                .store
                .upsert_issue_from_github(&upsert, chrono::Duration::zero())
                .await
            {
                tracing::warn!(
                    target: "dp_rest::issues_write",
                    issue_id = %issue.id,
                    error = %e,
                    "single-issue refresh upsert failed; row will reconcile on next webhook",
                );
            }
        }
        Err(e) => {
            tracing::warn!(
                target: "dp_rest::issues_write",
                issue_id = %issue.id,
                error = %e,
                "single-issue refresh parse failed; row will reconcile on next webhook",
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `repo_id -> (Org, Repo)`. Both rows must exist; either being
/// absent surfaces as `404 repo_not_resolved` — the §8 install-
/// permission check has no usable identity without them.
async fn resolve_repo(
    store: &dyn Store,
    repo_id: Uuid,
) -> Result<(dp_domain::org::Org, dp_domain::repo::Repo), ApiError> {
    let repo = store.get_repo(repo_id).await?.ok_or_else(|| ApiError::NotFound {
        code: "repo_not_found",
        message: format!("no repo with id {repo_id}"),
    })?;
    let org = store
        .get_org(repo.org_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "org_not_found",
            message: format!("no org with id {} (parent of repo {repo_id})", repo.org_id),
        })?;
    Ok((org, repo))
}

/// Route the [`IssuePatch`] to one of the four §8.5 audit verbs.
///
/// `state == "closed"` ⇒ `Close`, `state == "open"` ⇒ `Reopen`. Any
/// other patch — even when `state` is set to an unknown value — is
/// `Update`. The handler does not validate the state string here;
/// GitHub will return 422 and the rollback path runs.
fn patch_op(p: &IssuePatch) -> IssueMutationOp {
    match p.state.as_deref() {
        Some("closed") => IssueMutationOp::Close,
        Some("open") => IssueMutationOp::Reopen,
        _ => IssueMutationOp::Update,
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the issue-write router. Gated on `(issues, write)` so the
/// permission engine can toggle it independently of the read
/// surface.
pub fn issues_write_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new()
                .route("/issues", post(create_issue))
                .route("/issues/{id}", patch(patch_issue))
                .route("/issues/{id}/comments", post(create_comment)),
            "issues",
            "write",
        ))
        // `POST /issues/{id}/refresh` is a read trigger (no GitHub
        // mutation), so it's gated under `(issues, read)` so a
        // viewer can still hit the lazy-resync path that closes
        // the "open issue, see staleness" gap.
        .merge(with_permission(
            Router::new().route("/issues/{id}/refresh", post(refresh_issue)),
            "issues",
            "read",
        ))
        .with_state(inner)
}

// ---------------------------------------------------------------------------
// Tests — happy path, CAS miss, GitHub-failure rollback (+ §13.7
// buffer drain). The §8.2 / §8.5 primitives have their own tests in
// `crate::issues`; the tests here exercise the *handler* composition.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_permissions::GitHubAppConfig;
    use crate::audit::Principal;
    use crate::issues::AcquiredSlot;
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::extract::Request;
    use axum::http::{Method, StatusCode};
    use chrono::{DateTime, Duration, Utc};
    use dp_domain::app_install::{AppInstallPermissions, OrgAppInstall};
    use dp_domain::audit::AuditEntry;
    use dp_domain::event::{ActivityEvent, ActorRole, EventActor};
    use dp_domain::fetch::{FetchCursor, FetchRun, FetchRunKind, ResourceKind};
    use dp_domain::issue::{Issue, IssueState as DomainIssueState};
    use dp_domain::issue_mutation::{IssueMutation, IssueMutationResult};
    use dp_domain::membership::Membership;
    use dp_domain::org::Org;
    use dp_domain::repo::Repo;
    use dp_domain::store::{EventActorRow, PendingRemoteIssue, StoreError};
    use dp_domain::team::Team;
    use dp_domain::user::User;
    use dp_domain::webhook::WebhookDelivery;
    use dp_domain::window::Window;
    use std::collections::{HashMap, HashSet};
    use std::sync::Mutex;
    use tower::ServiceExt;

    // ---- Fake backend that records calls and is configurable -------

    #[derive(Default)]
    struct FakeBackend {
        calls: Mutex<Vec<String>>,
        fail_with: Mutex<Option<IssueWriteError>>,
        next_number: Mutex<i64>,
    }
    impl FakeBackend {
        fn new() -> Self {
            Self {
                next_number: Mutex::new(101),
                ..Self::default()
            }
        }
        fn fail_next(&self, e: IssueWriteError) {
            *self.fail_with.lock().unwrap() = Some(e);
        }
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }
    #[async_trait]
    impl IssueWriteBackend for FakeBackend {
        async fn create_issue(
            &self,
            owner: &str,
            repo: &str,
            title: &str,
            _: Option<&str>,
        ) -> Result<i64, IssueWriteError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("create:{owner}/{repo}:{title}"));
            if let Some(e) = self.fail_with.lock().unwrap().take() {
                return Err(e);
            }
            let mut n = self.next_number.lock().unwrap();
            *n += 1;
            Ok(*n)
        }
        async fn update_issue(
            &self,
            owner: &str,
            repo: &str,
            number: i64,
            _: &IssuePatch,
        ) -> Result<Option<serde_json::Value>, IssueWriteError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("update:{owner}/{repo}#{number}"));
            if let Some(e) = self.fail_with.lock().unwrap().take() {
                return Err(e);
            }
            Ok(None)
        }
        async fn create_comment(
            &self,
            owner: &str,
            repo: &str,
            number: i64,
            _: &str,
        ) -> Result<(), IssueWriteError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("comment:{owner}/{repo}#{number}"));
            if let Some(e) = self.fail_with.lock().unwrap().take() {
                return Err(e);
            }
            Ok(())
        }
    }

    // ---- FakeStore — minimal Store covering the surface the §8
    // write handler touches. Patterned on the FakeStore in
    // `crate::issues::tests`; trimmed to the methods this module
    // exercises. ---------------------------------------------------

    #[derive(Default)]
    struct FakeStore {
        inner: Mutex<FakeInner>,
    }
    #[derive(Default)]
    struct FakeInner {
        orgs: HashMap<Uuid, Org>,
        repos: HashMap<Uuid, Repo>,
        installs: HashMap<Uuid, OrgAppInstall>,
        issues_meta: HashMap<Uuid, (i64, bool, Option<DateTime<Utc>>, Option<Uuid>, Uuid)>,
        issues: HashMap<Uuid, Issue>,
        issue_index: HashMap<(Uuid, i64), Uuid>,
        repo_index: HashMap<i64, Uuid>,
        mutations: Vec<IssueMutation>,
        audit: Vec<AuditEntry>,
        buffered: HashMap<Uuid, Vec<WebhookDelivery>>,
        buffered_delivery_ids: HashSet<String>,
        applied_log: Vec<WebhookDelivery>,
    }

    #[async_trait]
    impl Store for FakeStore {
        async fn try_acquire_issue_pending_remote(
            &self,
            issue_id: Uuid,
            expected_version: i64,
            actor_user_id: Uuid,
        ) -> Result<Option<i64>, StoreError> {
            let mut g = self.inner.lock().unwrap();
            let row = g
                .issues_meta
                .get_mut(&issue_id)
                .ok_or_else(|| StoreError::NotFound {
                    entity: "issue",
                    id: issue_id.to_string(),
                })?;
            if row.0 != expected_version || row.1 {
                return Ok(None);
            }
            row.0 += 1;
            row.1 = true;
            row.2 = Some(Utc::now());
            row.3 = Some(actor_user_id);
            let v = row.0;
            if let Some(i) = g.issues.get_mut(&issue_id) {
                i.version = v;
            }
            Ok(Some(v))
        }
        async fn release_issue_pending_remote(
            &self,
            issue_id: Uuid,
            bump: bool,
        ) -> Result<i64, StoreError> {
            let mut g = self.inner.lock().unwrap();
            let row = g
                .issues_meta
                .get_mut(&issue_id)
                .ok_or_else(|| StoreError::NotFound {
                    entity: "issue",
                    id: issue_id.to_string(),
                })?;
            row.1 = false;
            row.2 = None;
            row.3 = None;
            if bump {
                row.0 += 1;
            }
            let v = row.0;
            if let Some(i) = g.issues.get_mut(&issue_id) {
                i.version = v;
            }
            Ok(v)
        }
        async fn get_issue_version(&self, issue_id: Uuid) -> Result<i64, StoreError> {
            self.inner
                .lock()
                .unwrap()
                .issues_meta
                .get(&issue_id)
                .map(|r| r.0)
                .ok_or_else(|| StoreError::NotFound {
                    entity: "issue",
                    id: issue_id.to_string(),
                })
        }
        async fn get_issue(&self, id: Uuid) -> Result<Option<Issue>, StoreError> {
            Ok(self.inner.lock().unwrap().issues.get(&id).cloned())
        }
        async fn get_repo(&self, id: Uuid) -> Result<Option<Repo>, StoreError> {
            Ok(self.inner.lock().unwrap().repos.get(&id).cloned())
        }
        async fn list_orgs(&self) -> Result<Vec<Org>, StoreError> {
            Ok(self.inner.lock().unwrap().orgs.values().cloned().collect())
        }
        async fn get_org_app_install(
            &self,
            org_id: Uuid,
        ) -> Result<Option<OrgAppInstall>, StoreError> {
            Ok(self.inner.lock().unwrap().installs.get(&org_id).cloned())
        }
        async fn list_issues_with_pending_remote_older_than(
            &self,
            cutoff: DateTime<Utc>,
        ) -> Result<Vec<PendingRemoteIssue>, StoreError> {
            let g = self.inner.lock().unwrap();
            Ok(g.issues_meta
                .iter()
                .filter(|(_, r)| r.1 && r.2.map(|t| t < cutoff).unwrap_or(false))
                .map(|(id, r)| PendingRemoteIssue {
                    issue_id: *id,
                    repo_id: r.4,
                    version: r.0,
                    actor_user_id: r.3.unwrap(),
                    pending_remote_at: r.2.unwrap(),
                })
                .collect())
        }
        async fn record_issue_mutation(
            &self,
            m: &IssueMutation,
        ) -> Result<IssueMutation, StoreError> {
            self.inner.lock().unwrap().mutations.push(m.clone());
            Ok(m.clone())
        }
        async fn update_issue_mutation_result(
            &self,
            id: Uuid,
            result: IssueMutationResult,
            delivery: Option<&str>,
            err: Option<&str>,
        ) -> Result<(), StoreError> {
            let mut g = self.inner.lock().unwrap();
            let m = g
                .mutations
                .iter_mut()
                .find(|m| m.id == id && matches!(m.result, IssueMutationResult::Pending))
                .ok_or_else(|| StoreError::NotFound {
                    entity: "dp_issue_mutations(pending)",
                    id: id.to_string(),
                })?;
            m.result = result;
            m.github_delivery_id = delivery.map(str::to_owned).or(m.github_delivery_id.clone());
            m.error = err.map(str::to_owned).or(m.error.clone());
            m.finished_at = Some(Utc::now());
            Ok(())
        }
        async fn list_pending_issue_mutations_older_than(
            &self,
            cutoff: DateTime<Utc>,
        ) -> Result<Vec<IssueMutation>, StoreError> {
            Ok(self
                .inner
                .lock()
                .unwrap()
                .mutations
                .iter()
                .filter(|m| {
                    matches!(m.result, IssueMutationResult::Pending) && m.created_at < cutoff
                })
                .cloned()
                .collect())
        }
        async fn record_audit_log(&self, e: &AuditEntry) -> Result<(), StoreError> {
            self.inner.lock().unwrap().audit.push(e.clone());
            Ok(())
        }
        async fn find_repo_id_by_github_id(
            &self,
            github_repo_id: i64,
        ) -> Result<Option<Uuid>, StoreError> {
            Ok(self
                .inner
                .lock()
                .unwrap()
                .repo_index
                .get(&github_repo_id)
                .copied())
        }
        async fn find_issue_id_by_repo_and_github_id(
            &self,
            repo_id: Uuid,
            github_issue_id: i64,
        ) -> Result<Option<Uuid>, StoreError> {
            Ok(self
                .inner
                .lock()
                .unwrap()
                .issue_index
                .get(&(repo_id, github_issue_id))
                .copied())
        }
        async fn is_issue_pending_remote_fresh(
            &self,
            issue_id: Uuid,
            timeout: Duration,
        ) -> Result<bool, StoreError> {
            let g = self.inner.lock().unwrap();
            let Some(r) = g.issues_meta.get(&issue_id) else { return Ok(false) };
            if !r.1 {
                return Ok(false);
            }
            let Some(at) = r.2 else { return Ok(false) };
            Ok(at >= Utc::now() - timeout)
        }
        async fn buffer_pending_remote_webhook(
            &self,
            issue_id: Uuid,
            delivery: &WebhookDelivery,
        ) -> Result<(), StoreError> {
            let mut g = self.inner.lock().unwrap();
            if !g.buffered_delivery_ids.insert(delivery.delivery_id.clone()) {
                return Err(StoreError::Conflict(format!(
                    "duplicate delivery_id {}",
                    delivery.delivery_id
                )));
            }
            g.buffered.entry(issue_id).or_default().push(delivery.clone());
            Ok(())
        }
        async fn take_buffered_webhooks_for_issue(
            &self,
            issue_id: Uuid,
        ) -> Result<Vec<WebhookDelivery>, StoreError> {
            let mut g = self.inner.lock().unwrap();
            let mut out = g.buffered.remove(&issue_id).unwrap_or_default();
            for d in &out {
                g.buffered_delivery_ids.remove(&d.delivery_id);
            }
            out.sort_by_key(|d| d.received_at);
            Ok(out)
        }
        async fn record_event(
            &self,
            e: &ActivityEvent,
        ) -> Result<ActivityEvent, StoreError> {
            self.inner.lock().unwrap().applied_log.push(WebhookDelivery {
                id: Uuid::new_v4(),
                delivery_id: e.external_id.clone(),
                event: format!("{:?}", e.kind),
                payload: serde_json::Value::Null,
                received_at: e.ts,
                processed_at: None,
                error: None,
            });
            Ok(e.clone())
        }

        // ---- Default-ish stubs for everything else --------------
        async fn upsert_user(&self, u: &User) -> Result<User, StoreError> {
            Ok(u.clone())
        }
        async fn get_user(&self, _: Uuid) -> Result<User, StoreError> {
            unimplemented!()
        }
        async fn get_user_by_github_id(&self, _: i64) -> Result<User, StoreError> {
            unimplemented!()
        }
        async fn list_users(&self) -> Result<Vec<User>, StoreError> {
            Ok(vec![])
        }
        async fn pseudonymise_user(&self, _: Uuid) -> Result<(), StoreError> {
            Ok(())
        }
        async fn upsert_org(&self, o: &Org) -> Result<Org, StoreError> {
            Ok(o.clone())
        }
        async fn upsert_team(&self, t: &Team) -> Result<Team, StoreError> {
            Ok(t.clone())
        }
        async fn upsert_repo(&self, r: &Repo) -> Result<Repo, StoreError> {
            Ok(r.clone())
        }
        async fn upsert_membership(&self, m: &Membership) -> Result<Membership, StoreError> {
            Ok(m.clone())
        }
        async fn list_memberships_for_user(
            &self,
            _: Uuid,
        ) -> Result<Vec<Membership>, StoreError> {
            Ok(vec![])
        }
        async fn set_home_org(
            &self,
            _: Uuid,
            _: Uuid,
            _: Option<Uuid>,
        ) -> Result<(), StoreError> {
            Ok(())
        }
        async fn add_event_actors(&self, _: &[EventActor]) -> Result<(), StoreError> {
            Ok(())
        }
        async fn list_event_actor_rows_in_window(
            &self,
            _: &Window,
            _: &[Uuid],
            _: &[Uuid],
            _: &[Uuid],
            _: &[ActorRole],
        ) -> Result<Vec<EventActorRow>, StoreError> {
            Ok(vec![])
        }
        async fn get_cursor(
            &self,
            _: Uuid,
            _: Option<Uuid>,
            _: ResourceKind,
        ) -> Result<FetchCursor, StoreError> {
            Err(StoreError::NotFound {
                entity: "fetch_cursor",
                id: String::new(),
            })
        }
        async fn put_cursor(&self, _: &FetchCursor) -> Result<(), StoreError> {
            Ok(())
        }
        async fn start_fetch_run(&self, _: FetchRunKind) -> Result<Uuid, StoreError> {
            Ok(Uuid::new_v4())
        }
        async fn finish_fetch_run(
            &self,
            _: Uuid,
            _: i64,
            _: i64,
            _: bool,
        ) -> Result<(), StoreError> {
            Ok(())
        }
        async fn list_recent_fetch_runs(
            &self,
            _: i64,
        ) -> Result<Vec<FetchRun>, StoreError> {
            Ok(vec![])
        }
        async fn data_as_of(&self) -> Result<dp_domain::freshness::DataAsOf, StoreError> {
            Ok(dp_domain::freshness::DataAsOf::default())
        }
        async fn enqueue_webhook(&self, _: &WebhookDelivery) -> Result<(), StoreError> {
            Ok(())
        }
        async fn claim_webhooks(&self, _: i64) -> Result<Vec<WebhookDelivery>, StoreError> {
            Ok(vec![])
        }
        async fn mark_webhook_processed(&self, _: Uuid) -> Result<(), StoreError> {
            Ok(())
        }
        async fn mark_webhook_failed(&self, _: Uuid, _: &str) -> Result<(), StoreError> {
            Ok(())
        }
    }

    // ---- Test rig ------------------------------------------------

    struct Rig {
        store: Arc<FakeStore>,
        backend: Arc<FakeBackend>,
        principal: Principal,
        org: Org,
        repo: Repo,
        issue: Issue,
    }

    fn build_rig() -> Rig {
        let store = Arc::new(FakeStore::default());
        let backend = Arc::new(FakeBackend::new());
        let actor = Uuid::new_v4();
        let org = Org {
            id: Uuid::new_v4(),
            github_id: 1,
            login: "acme".into(),
            name: Some("Acme".into()),
        };
        let repo = Repo {
            id: Uuid::new_v4(),
            org_id: org.id,
            github_id: 100,
            name: "widgets".into(),
        };
        let issue = Issue {
            id: Uuid::new_v4(),
            org_id: org.id,
            repo_id: repo.id,
            github_id: 9001,
            number: 42,
            title: "hello".into(),
            body: None,
            state: DomainIssueState::Open,
            labels: vec![],
            assignees: vec![],
            milestone: None,
            version: 7,
            github_node_id: None,
            updated_at: Utc::now(),
        };
        {
            let mut g = store.inner.lock().unwrap();
            g.orgs.insert(org.id, org.clone());
            g.repos.insert(repo.id, repo.clone());
            g.installs.insert(
                org.id,
                OrgAppInstall {
                    org_id: org.id,
                    installation_id: 555,
                    permissions: AppInstallPermissions { issues_write: true },
                    observed_at: Utc::now(),
                },
            );
            g.issues_meta
                .insert(issue.id, (issue.version, false, None, None, repo.id));
            g.issues.insert(issue.id, issue.clone());
            g.repo_index.insert(repo.github_id, repo.id);
            g.issue_index
                .insert((repo.id, issue.github_id), issue.id);
        }
        Rig {
            store,
            backend,
            principal: Principal { actor_user_id: actor },
            org,
            repo,
            issue,
        }
    }

    fn app(rig: &Rig) -> Router {
        let cfg = GitHubAppConfig {
            request_issues_write: true,
            slug: Some("dev-pulse-app".into()),
            ..GitHubAppConfig::default()
        };
        let state = AppState::new(rig.store.clone())
            .with_github_app(Arc::new(cfg))
            .with_issue_writer(rig.backend.clone());
        // Skip the (issues, write) permission gate — that's a layer
        // applied at the dp-server composition seam. Mounting the
        // bare router here keeps the unit tests focused on the §8.2
        // composition shape.
        Router::new()
            .route("/issues", post(create_issue))
            .route("/issues/{id}", patch(patch_issue))
            .route("/issues/{id}/comments", post(create_comment))
            .layer(axum::Extension(rig.principal.clone()))
            .with_state(state)
    }

    async fn send(app: Router, req: Request) -> (StatusCode, Vec<u8>) {
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        (status, bytes.to_vec())
    }

    // ---- Tests --------------------------------------------------

    #[tokio::test]
    async fn patch_happy_path_commits_and_audits() {
        let rig = build_rig();
        let body = serde_json::json!({
            "expected_version": 7,
            "title": "renamed",
        });
        let req = Request::builder()
            .method(Method::PATCH)
            .uri(format!("/issues/{}", rig.issue.id))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let (status, _) = send(app(&rig), req).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            rig.backend.calls(),
            vec![format!("update:{}/{}#42", rig.org.login, rig.repo.name)]
        );
        let g = rig.store.inner.lock().unwrap();
        // CAS bumped to 8; commit did not bump again.
        assert_eq!(g.issues_meta[&rig.issue.id].0, 8);
        // Audit row exists with `issue.update`.
        assert!(g
            .audit
            .iter()
            .any(|a| a.action == audit::ISSUE_UPDATE));
        // Mutation row committed.
        assert!(matches!(
            g.mutations[0].result,
            IssueMutationResult::Committed
        ));
    }

    #[tokio::test]
    async fn patch_cas_miss_returns_stale_local_version_409() {
        let rig = build_rig();
        let body = serde_json::json!({
            "expected_version": 3, // stale
            "title": "renamed",
        });
        let req = Request::builder()
            .method(Method::PATCH)
            .uri(format!("/issues/{}", rig.issue.id))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let (status, body) = send(app(&rig), req).await;
        assert_eq!(status, StatusCode::CONFLICT);
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["code"], "stale_local_version");
        assert_eq!(parsed["current_version"], 7);
        assert_eq!(parsed["issue_id"], rig.issue.id.to_string());
        // Backend was not called.
        assert!(rig.backend.calls().is_empty());
    }

    #[tokio::test]
    async fn patch_github_failure_rolls_back_and_drains_buffer() {
        use dp_fetcher::reconciler::guard::{apply_or_defer_delivery, GuardOutcome};
        let rig = build_rig();
        // Pre-arm a webhook that arrives mid-flight: simulate the
        // §13.7 deferral by acquiring the slot manually, then
        // running the guard, then dropping back into the handler
        // path. Simpler: have the backend fail, and after the
        // rollback verify the buffer has been drained for an
        // already-buffered delivery.
        //
        // Step A — acquire the pending_remote flag via a dummy
        // call, then deliver a webhook; the guard buffers it.
        rig.backend.fail_next(IssueWriteError::Upstream(
            "github 503 unavailable".into(),
        ));

        // Buffer a synthetic delivery against the issue *while*
        // pending_remote will be true mid-handler. We approximate by
        // pre-buffering after manually setting pending_remote.
        {
            // Mark pending_remote = true so the guard defers, then
            // run apply_or_defer_delivery. After the guard, clear it
            // so the handler can take the CAS path itself.
            rig.store
                .try_acquire_issue_pending_remote(
                    rig.issue.id,
                    rig.issue.version,
                    rig.principal.actor_user_id,
                )
                .await
                .unwrap();
            let webhook = WebhookDelivery {
                id: Uuid::new_v4(),
                delivery_id: "d-mid".into(),
                event: "issues".into(),
                payload: serde_json::json!({
                    "action": "closed",
                    "sender": { "id": 99, "login": "bot" },
                    "repository": {
                        "id": rig.repo.github_id,
                        "name": rig.repo.name,
                        "owner": { "id": 1, "login": rig.org.login }
                    },
                    "issue": {
                        "id": rig.issue.github_id,
                        "number": rig.issue.number,
                        "node_id": "I_test",
                        "title": rig.issue.title,
                        "state": "closed",
                        "created_at": "2024-01-01T00:00:00Z",
                        "updated_at": "2024-01-02T00:00:00Z",
                        "closed_at": "2024-01-02T00:00:00Z",
                        "user": { "id": 7, "login": "alice" },
                        "assignees": []
                    }
                }),
                received_at: Utc::now(),
                processed_at: None,
                error: None,
            };
            match apply_or_defer_delivery(
                &*rig.store,
                &webhook,
                Duration::seconds(60),
            )
            .await
            .unwrap()
            {
                GuardOutcome::Deferred { .. } => {}
                _ => panic!("expected webhook to be deferred"),
            }
            // Release the manual pending_remote so the handler's
            // CAS can succeed. Bump=false ⇒ version stays at 8.
            rig.store
                .release_issue_pending_remote(rig.issue.id, false)
                .await
                .unwrap();
        }
        // Buffer holds the delivery.
        assert_eq!(
            rig.store
                .inner
                .lock()
                .unwrap()
                .buffered
                .get(&rig.issue.id)
                .map(Vec::len),
            Some(1)
        );

        // Now drive the handler. The CAS uses the post-manual
        // version (8) as expected. The backend errors; rollback
        // runs, bumps version to 9, and drains the buffer.
        let body = serde_json::json!({
            "expected_version": 8,
            "title": "renamed",
        });
        let req = Request::builder()
            .method(Method::PATCH)
            .uri(format!("/issues/{}", rig.issue.id))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let (status, _) = send(app(&rig), req).await;
        // Backend returned 5xx ⇒ surface as 400 upstream_unavailable
        // (per the explicit mapping in IssueWriteError::into_api_error).
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let g = rig.store.inner.lock().unwrap();
        // Version: 7 (initial) → 8 (manual pre-buffer CAS) → 8
        // (manual release w/o bump) → 9 (handler CAS) → 10
        // (rollback bump). The §13.7 buffer-drain test deliberately
        // takes the slot once to deflect the webhook before letting
        // the handler do its own CAS.
        assert_eq!(g.issues_meta[&rig.issue.id].0, 10);
        // Mutation row marked Failed with the GitHub error.
        assert!(matches!(
            g.mutations[0].result,
            IssueMutationResult::Failed
        ));
        assert!(g.mutations[0]
            .error
            .as_deref()
            .unwrap_or("")
            .contains("503"));
        // §13.7 buffer was drained on rollback.
        assert!(
            g.buffered.get(&rig.issue.id).map_or(true, Vec::is_empty),
            "buffer should be empty after rollback drain"
        );
        // The buffered delivery flowed through apply_delivery.
        assert!(!g.applied_log.is_empty());
        // Audit row exists.
        assert!(g
            .audit
            .iter()
            .any(|a| a.action == audit::ISSUE_UPDATE));
    }

    #[tokio::test]
    async fn create_issue_happy_path() {
        let rig = build_rig();
        let body = serde_json::json!({
            "repo_id": rig.repo.id,
            "title": "new issue",
        });
        let req = Request::builder()
            .method(Method::POST)
            .uri("/issues")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let (status, body) = send(app(&rig), req).await;
        assert_eq!(status, StatusCode::OK);
        let parsed: CreateIssueResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.repo_id, rig.repo.id);
        assert_eq!(parsed.number, 102);
        let g = rig.store.inner.lock().unwrap();
        assert!(g
            .audit
            .iter()
            .any(|a| a.action == audit::ISSUE_CREATE));
    }

    #[tokio::test]
    async fn create_comment_happy_path() {
        let rig = build_rig();
        let body = serde_json::json!({
            "expected_version": 7,
            "body": "ping",
        });
        let req = Request::builder()
            .method(Method::POST)
            .uri(format!("/issues/{}/comments", rig.issue.id))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let (status, _) = send(app(&rig), req).await;
        // §13.7 lazy resync: comment endpoint now returns 200 +
        // fresh `IssueDto` so the UI doesn't have to issue a
        // follow-up `GET /issues/{id}` to learn the new
        // `comment_count` / `updated_at`.
        assert_eq!(status, StatusCode::OK);
        let g = rig.store.inner.lock().unwrap();
        assert!(g
            .audit
            .iter()
            .any(|a| a.action == audit::ISSUE_COMMENT));
        assert!(matches!(
            g.mutations[0].result,
            IssueMutationResult::Committed
        ));
    }

    // Suppress unused-import warnings on items kept for symmetry
    // with the broader §8 surface tests.
    #[allow(dead_code)]
    fn _unused(_: AcquiredSlot) {}
}
