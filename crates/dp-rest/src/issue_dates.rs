//! `PATCH /issues/{id}/dates` — local-first start / due date upsert
//! plus the §3.10 best-effort GraphQL mirror enqueue.
//!
//! Shape (§3.10):
//!
//! 1. Resolve `issue_id -> Issue -> Repo -> Org`.
//! 2. `(issues, write)` install-permission check.
//! 3. Synchronous local `UPSERT` into `dp_issue_dates`. Any
//!    failure here surfaces as 4xx / 5xx and the response carries
//!    no mirror promise.
//! 4. For every `dp_project_board_links` row attached to the
//!    issue's project, enqueue a `mirror_dates` task into
//!    `dp_projectv2_mirror_tasks` AND
//!    spawn a best-effort backend call (`addProjectV2ItemById`
//!    then `updateProjectV2ItemFieldValue`). Failures land on
//!    `dp_issue_dates.mirror_error` and never block the response.
//!
//! No CAS. Dates are sparse, local-first, and the slice deliberately
//! decouples the date editor from the §8 mutation path so a
//! mirror-only failure (network blip, GitHub 5xx) cannot strand the
//! user's local edit.
//!
//! Projects v2 *pull-back* (read GitHub Projects state into
//! dev-pulse) is a slice-3 deferral — the `pull_back` task kind is
//! reserved in the migration and the [`ProjectV2MirrorBackend`]
//! trait is shaped so the slice-3 worker can grow alongside it
//! without churning this handler.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    extract::{Extension, Path, State},
    response::Json,
    routing::{get, patch},
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::board_link::BoardItemMirrorOutcome;
use dp_domain::issue_dates::{IssueDates, ProjectV2MirrorTaskKind, RepoProjectLink};

use crate::app_permissions::require_issues_write;
use crate::audit::{self, Principal};
use crate::error::ApiError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Mirror backend — the GraphQL seam
// ---------------------------------------------------------------------------

/// Outcome of one `addProjectV2ItemById` + `updateProjectV2ItemFieldValue`
/// round-trip. Returned by the backend so the store layer can stamp
/// the canonical Projects v2 *item* node id on success.
#[derive(Debug, Clone)]
pub struct MirrorDatesOk {
    /// The Projects v2 item node id GitHub assigned (or echoed
    /// back on the second-and-later edit). Persisted to
    /// `dp_issue_dates.mirror_node_id` so the next mirror reuses
    /// the same card.
    pub item_node_id: String,
}

/// Errors a [`ProjectV2MirrorBackend`] may surface. Verbatim error
/// text is round-tripped to `dp_issue_dates.mirror_error` so the
/// UI can render it next to the date pills.
#[derive(Debug, thiserror::Error)]
pub enum MirrorError {
    /// GitHub GraphQL returned an error (4xx or `errors[]`).
    /// `0` carries the verbatim message safe to surface.
    #[error("github graphql: {0}")]
    GraphQl(String),
    /// Transport / 5xx failure.
    #[error("github transport: {0}")]
    Transport(String),
    /// The deployment hasn't wired a real mirror backend yet.
    /// [`UnconfiguredProjectV2Mirror`] returns this; the handler
    /// treats it as "no-op, do not even enqueue".
    #[error("projects v2 mirror not configured")]
    Unconfigured,
}

/// The GraphQL seam every `PATCH /issues/{id}/dates` mirror call
/// flows through. Production binaries wire an octocrab-graphql
/// implementation from the bin layer; tests pass a fake. Hot rule:
/// **never blocks the local save** — the handler spawns this and
/// records the outcome out-of-band.
#[async_trait]
pub trait ProjectV2MirrorBackend: Send + Sync + 'static {
    /// `addProjectV2ItemById(projectId, contentId = issue_node_id)`
    /// followed by `updateProjectV2ItemFieldValue` for each of
    /// `start_field_node_id` / `due_field_node_id` (skipping
    /// fields the project does not define). The backend reuses
    /// an existing item id when `existing_item_node_id` is
    /// `Some`; otherwise it issues the add and surfaces the
    /// fresh id.
    async fn mirror_dates(
        &self,
        link: &RepoProjectLink,
        issue_node_id: &IssueNodeIdRef,
        existing_item_node_id: Option<&str>,
        start_at: Option<DateTime<Utc>>,
        due_at: Option<DateTime<Utc>>,
    ) -> Result<MirrorDatesOk, MirrorError>;
}

/// Default backend — refuses every call. Bin layer overrides with
/// [`AppState::with_projectv2_mirror`][crate::state::AppState::with_projectv2_mirror].
/// Refusing here means "no enqueue, no spawn" — the handler simply
/// skips the mirror entirely so test deployments don't grow
/// `mirror_error` rows for a feature they never opted into.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnconfiguredProjectV2Mirror;

#[async_trait]
impl ProjectV2MirrorBackend for UnconfiguredProjectV2Mirror {
    async fn mirror_dates(
        &self,
        _: &RepoProjectLink,
        _: &IssueNodeIdRef,
        _: Option<&str>,
        _: Option<DateTime<Utc>>,
        _: Option<DateTime<Utc>>,
    ) -> Result<MirrorDatesOk, MirrorError> {
        Err(MirrorError::Unconfigured)
    }
}

/// Production [`ProjectV2MirrorBackend`] backed by the dp-fetcher
/// octocrab client. Sits in parallel with
/// [`crate::issues_write::FetcherIssueWriter`] — same dependency
/// shape (a budget-shared [`dp_fetcher::client::Client`]), same
/// thin error-mapping discipline.
///
/// The adapter owns an `Arc<dyn Store>` so it can:
///
///   * Resolve `(repo_id) -> (org.login, repo.name)` when an
///     issue row pre-dates the 0021 `github_node_id` migration
///     and needs a lazy `repository.issue(number)` lookup.
///   * Stamp the resolved node id back via
///     [`Store::set_issue_github_node_id`][dp_domain::store::Store::set_issue_github_node_id]
///     so subsequent mirrors skip the lookup.
///
/// Errors from the fetcher are mapped one-to-one into
/// [`MirrorError`], with one extra discrimination: a `Validation`
/// carrying the verbatim GitHub "Resource not accessible by
/// personal access token" / `FORBIDDEN` text is reshaped into a
/// `GraphQl` error prefixed with a clear remediation hint so the
/// `dp_issue_dates.mirror_error` column the UI surfaces tells the
/// operator exactly what to fix.
pub struct OctocrabProjectV2Mirror {
    client: Arc<dp_fetcher::client::Client>,
    store: Arc<dyn dp_domain::store::Store>,
}

impl OctocrabProjectV2Mirror {
    /// Construct from a ready-to-use fetcher client and a store
    /// handle. The bin layer already builds the fetcher client
    /// for [`crate::issues_write::FetcherIssueWriter`]; the
    /// mirror reuses the same handle so the local request budget
    /// covers GraphQL mutations too.
    pub fn new(
        client: Arc<dp_fetcher::client::Client>,
        store: Arc<dyn dp_domain::store::Store>,
    ) -> Self {
        Self { client, store }
    }
}

impl std::fmt::Debug for OctocrabProjectV2Mirror {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OctocrabProjectV2Mirror").finish_non_exhaustive()
    }
}

/// Translate a [`dp_fetcher::client::GhWriteError`] into a
/// [`MirrorError`], with PAT-scope FORBIDDEN replies surfaced as
/// a clear remediation hint. The text rendered here lands on
/// `dp_issue_dates.mirror_error` and is shown in the date editor.
fn map_mirror_err(e: dp_fetcher::client::GhWriteError) -> MirrorError {
    use dp_fetcher::client::GhWriteError as G;
    match e {
        G::Validation(msg) => {
            // GitHub returns `FORBIDDEN` / "Resource not
            // accessible by personal access token" when the
            // operator's classic PAT lacks the `project` scope or
            // when a fine-grained token has no Projects v2
            // permission. The operator can fix this in 30s on
            // github.com so call it out explicitly.
            let lower = msg.to_lowercase();
            if lower.contains("forbidden")
                || lower.contains("resource not accessible")
                || lower.contains("not accessible by personal access token")
            {
                MirrorError::GraphQl(format!(
                    "PAT lacks 'project' scope (or fine-grained \
                     'Projects: Read and Write'): {msg}"
                ))
            } else {
                MirrorError::GraphQl(msg)
            }
        }
        G::Upstream(msg) => MirrorError::Transport(msg),
    }
}

#[async_trait]
impl ProjectV2MirrorBackend for OctocrabProjectV2Mirror {
    async fn mirror_dates(
        &self,
        link: &RepoProjectLink,
        issue_node_id: &IssueNodeIdRef,
        existing_item_node_id: Option<&str>,
        start_at: Option<DateTime<Utc>>,
        due_at: Option<DateTime<Utc>>,
    ) -> Result<MirrorDatesOk, MirrorError> {
        // Step 1 — resolve the GitHub content node id. Cached on
        // `dp_issues.github_node_id` for post-0021 rows; lazy
        // GraphQL lookup + cache-back for older rows.
        let content_node_id: String = match issue_node_id {
            IssueNodeIdRef::Known { node_id } => node_id.clone(),
            IssueNodeIdRef::Unresolved {
                issue_id,
                repo_id,
                number,
            } => {
                let repo = self
                    .store
                    .get_repo(*repo_id)
                    .await
                    .map_err(|e| MirrorError::Transport(e.to_string()))?
                    .ok_or_else(|| {
                        MirrorError::GraphQl(format!(
                            "repo {repo_id} vanished before lazy node-id resolve"
                        ))
                    })?;
                let org = self
                    .store
                    .get_org(repo.org_id)
                    .await
                    .map_err(|e| MirrorError::Transport(e.to_string()))?
                    .ok_or_else(|| {
                        MirrorError::GraphQl(format!(
                            "org {} vanished before lazy node-id resolve",
                            repo.org_id
                        ))
                    })?;
                let resolved = self
                    .client
                    .gh_resolve_issue_node_id(&org.login, &repo.name, *number)
                    .await
                    .map_err(map_mirror_err)?;
                // Cache-back: stamp the resolved id so the next
                // mirror skips the lookup. Best-effort — a write
                // failure here is logged and swallowed so the
                // mirror still proceeds.
                if let Err(e) = self
                    .store
                    .set_issue_github_node_id(*issue_id, &resolved)
                    .await
                {
                    tracing::warn!(
                        target: "dp_rest::issue_dates",
                        error = %e,
                        issue_id = %issue_id,
                        "set_issue_github_node_id cache-back failed",
                    );
                }
                resolved
            }
        };

        // Step 2 — resolve the Projects v2 *item* id. Reuse the
        // existing card when we already mirrored this issue;
        // otherwise issue `addProjectV2ItemById`. The mutation is
        // idempotent on the GitHub side but we still record the
        // returned id so we skip the call on the next edit.
        let item_node_id = match existing_item_node_id {
            Some(id) => id.to_string(),
            None => self
                .client
                .gh_projectv2_add_item(&link.project_node_id, &content_node_id)
                .await
                .map_err(map_mirror_err)?,
        };

        // Step 3 — push the date fields. The handler passes
        // `None` to clear; per §3.10 we forward the null so a
        // local clear lands on Projects v2 instead of leaving a
        // stale value. Each lane is independent: a project that
        // does not configure a start field skips that mutation.
        if let Some(field) = link.start_field_node_id.as_deref() {
            self.client
                .gh_projectv2_update_date_field(
                    &link.project_node_id,
                    &item_node_id,
                    field,
                    start_at,
                )
                .await
                .map_err(map_mirror_err)?;
        }
        if let Some(field) = link.due_field_node_id.as_deref() {
            self.client
                .gh_projectv2_update_date_field(
                    &link.project_node_id,
                    &item_node_id,
                    field,
                    due_at,
                )
                .await
                .map_err(map_mirror_err)?;
        }

        Ok(MirrorDatesOk {
            item_node_id,
        })
    }
}

// ---------------------------------------------------------------------------
// Wire DTOs
// ---------------------------------------------------------------------------

/// Body for `PATCH /issues/{id}/dates`. Both fields are optional —
/// `null` (or omitted) clears that side. The schema CHECK guards
/// `start_at <= due_at`; the handler surfaces violations as
/// `400 invalid_date_window`.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct PatchIssueDatesRequest {
    /// New start instant (inclusive). Omit / `null` to clear.
    #[serde(default)]
    pub start_at: Option<DateTime<Utc>>,
    /// New due instant (inclusive). Omit / `null` to clear.
    #[serde(default)]
    pub due_at: Option<DateTime<Utc>>,
}

/// Response body — the canonical post-upsert row. Mirrors
/// [`IssueDates`] one-for-one so the UI can render the freshly
/// stamped `updated_at` plus the most recent mirror outcome
/// (`mirror_synced_at` / `mirror_error`) without a second read.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IssueDatesDto {
    /// Echoed `dp_issues.id`.
    pub issue_id: Uuid,
    /// Start instant, or `null`.
    pub start_at: Option<DateTime<Utc>>,
    /// Due instant, or `null`.
    pub due_at: Option<DateTime<Utc>>,
    /// Projects v2 item node id, when mirroring has ever
    /// succeeded for this row.
    pub mirror_node_id: Option<String>,
    /// Wall-clock the most recent mirror attempt succeeded;
    /// `null` until the first success.
    pub mirror_synced_at: Option<DateTime<Utc>>,
    /// Verbatim error from the most recent failed mirror attempt;
    /// `null` when the latest attempt succeeded or no attempt has
    /// run yet (e.g. the repo has no project link).
    pub mirror_error: Option<String>,
    /// `updated_at` on the local row.
    pub updated_at: DateTime<Utc>,
}

impl From<IssueDates> for IssueDatesDto {
    fn from(d: IssueDates) -> Self {
        Self {
            issue_id: d.issue_id,
            start_at: d.start_at,
            due_at: d.due_at,
            mirror_node_id: d.mirror_node_id,
            mirror_synced_at: d.mirror_synced_at,
            mirror_error: d.mirror_error,
            updated_at: d.updated_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `PATCH /issues/{id}/dates` — see module docs for the four-step
/// shape. Authorisation: `(issues, write)`.
#[utoipa::path(
    patch,
    path = "/issues/{id}/dates",
    params(("id" = Uuid, Path, description = "Issue id")),
    request_body = PatchIssueDatesRequest,
    responses(
        (status = 200, description = "Local upsert committed; mirror best-effort", body = IssueDatesDto),
        (status = 400, description = "start_at must be <= due_at"),
        (status = 403, description = "Writes not available for the target org"),
        (status = 404, description = "No such issue"),
    ),
    tag = "issues",
)]
pub async fn patch_issue_dates(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchIssueDatesRequest>,
) -> Result<Json<IssueDatesDto>, ApiError> {
    // Pre-validate the window so the schema CHECK only catches
    // races. Cheap and lets us surface a stable code regardless of
    // backend.
    if let (Some(s), Some(d)) = (body.start_at, body.due_at) {
        if s > d {
            return Err(ApiError::BadRequest {
                code: "invalid_date_window",
                message: "start_at must be <= due_at".into(),
            });
        }
    }

    let issue = state
        .store
        .get_issue(id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "issue_not_found",
            message: format!("no issue with id {id}"),
        })?;
    let repo = state
        .store
        .get_repo(issue.repo_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "repo_not_found",
            message: format!("no repo with id {}", issue.repo_id),
        })?;
    let org = state
        .store
        .get_org(repo.org_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "org_not_found",
            message: format!("no org with id {}", repo.org_id),
        })?;
    // SCOPE.md §4.1.1 — local-only issues have no GitHub-side
    // card, so the install-write check and the Projects v2 mirror
    // fan-out below are both skipped. We still upsert into
    // `dp_issue_dates` so the local timeline / chart surfaces
    // work uniformly across the two lanes.
    let is_local_issue = issue.is_local;
    if !is_local_issue {
        require_issues_write(&*state.store, &state.github_app, &org).await?;
    }

    // §3.10 step 3 — synchronous local upsert. Schema CHECK
    // violations surface as Invalid (cheap pre-check above usually
    // catches them first).
    let dates = state
        .store
        .upsert_issue_dates(id, body.start_at, body.due_at)
        .await
        .map_err(|e| match e {
            dp_domain::store::StoreError::Invalid(msg) => ApiError::BadRequest {
                code: "invalid_date_window",
                message: msg,
            },
            other => ApiError::from(other),
        })?;

    audit::record(
        &*state.store,
        principal.actor_user_id,
        audit::ISSUE_DATES_UPDATE,
        format!("{}", id),
    )
    .await?;

    // §7.4 step 4 — best-effort mirror fan-out across every board
    // the issue's project links. Slice-B rewire of the §3.10 path:
    // dev-pulse no longer keeps a per-repo board link; the mirror
    // resolves the (single, per §4 `UNIQUE (issue_id)`) project
    // for the issue and spawns one mirror round-trip per linked
    // board. Per-board outcomes are recorded via
    // [`Store::record_board_item_result`], which transactionally
    // rolls success / failure up to the aggregate
    // `dp_project_board_links.last_mirror_at` /
    // `last_mirror_error` columns the §6.3 row surfaces.
    //
    // Local-first invariant unchanged: the local `dp_issue_dates`
    // upsert has already committed and the response carries the
    // canonical row; mirror failures land in the per-link
    // aggregate, never on the synchronous response.
    if !is_local_issue {
        if let Some(project) = state.store.get_project_for_issue(id).await.ok().flatten() {
        let links = state
            .store
            .list_board_links(project.id)
            .await
            .unwrap_or_default();
        for link in links {
            // Enqueue an outbox row per link — durable record the
            // mirror should run, even if the spawned task is
            // dropped. Best-effort: enqueue failure is logged and
            // swallowed so the local save remains the source of
            // truth.
            if let Err(e) = state
                .store
                .enqueue_projectv2_mirror_task(
                    id,
                    repo.id,
                    ProjectV2MirrorTaskKind::MirrorDates,
                    json!({
                        "start_at": body.start_at,
                        "due_at":   body.due_at,
                        "link_id":  link.id,
                    }),
                )
                .await
            {
                tracing::warn!(error = %e, issue_id = %id, link_id = %link.id,
                    "enqueue projectv2 mirror task failed");
            }

            // Resolve the per-(link, issue) existing item id so the
            // mirror updates the same Projects v2 card on every
            // subsequent edit (no duplicate cards).
            let existing_item = state
                .store
                .get_board_item(link.id, id)
                .await
                .ok()
                .flatten()
                .map(|i| i.item_node_id);
            // The backend trait still takes a §3.10-shaped
            // `RepoProjectLink` because the underlying GraphQL call
            // only cares about `(project_node_id, start_field,
            // due_field)`. Synthesise one per linked board — the
            // `repo_id` field is meaningless on this code path and
            // the backend never reads it.
            let synthetic = RepoProjectLink {
                repo_id: repo.id,
                project_node_id: link.github_board_node_id.clone(),
                start_field_node_id: link.start_field_node_id.clone(),
                due_field_node_id: link.due_field_node_id.clone(),
            };
            let store = state.store.clone();
            let backend = state.projectv2_mirror.clone();
            let issue_node_id = issue_node_id(&issue);
            let start = body.start_at;
            let due = body.due_at;
            let link_id = link.id;
            tokio::spawn(async move {
                match backend
                    .mirror_dates(
                        &synthetic,
                        &issue_node_id,
                        existing_item.as_deref(),
                        start,
                        due,
                    )
                    .await
                {
                    Ok(ok) => {
                        if let Err(e) = store
                            .record_board_item_result(
                                link_id,
                                id,
                                BoardItemMirrorOutcome::Success {
                                    item_node_id: &ok.item_node_id,
                                },
                            )
                            .await
                        {
                            tracing::warn!(error = %e, issue_id = %id, link_id = %link_id,
                                "record board item success failed");
                        }
                    }
                    Err(MirrorError::Unconfigured) => {
                        // Backend declined — treat as if the mirror
                        // never ran. Don't surface a stale error
                        // on the per-link aggregate.
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if let Err(se) = store
                            .record_board_item_result(
                                link_id,
                                id,
                                BoardItemMirrorOutcome::Failure { error: &msg },
                            )
                            .await
                        {
                            tracing::warn!(error = %se, issue_id = %id, link_id = %link_id,
                                "record board item failure failed");
                        }
                    }
                }
            });
        }
    }
    } // end of `if !is_local_issue` mirror block
    Ok(Json(IssueDatesDto::from(dates)))
}

/// `GET /issues/{id}/dates` — read the local `dp_issue_dates` row
/// for an issue. Returns `200 { issue_id, start_at: null, due_at:
/// null, … }` with all nullable fields set when no row exists, so
/// the frontend can render the picker uniformly. Authorisation:
/// the same `(issues, read)` pair the rest of the read surface uses
/// — read-only and per-org, the picker is also visible to viewers
/// who cannot write.
#[utoipa::path(
    get,
    path = "/issues/{id}/dates",
    params(("id" = Uuid, Path, description = "Issue id")),
    responses(
        (status = 200, description = "Local dates row (zero-valued when unset)", body = IssueDatesDto),
        (status = 404, description = "No such issue"),
    ),
    tag = "issues",
)]
pub async fn get_issue_dates(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<IssueDatesDto>, ApiError> {
    let issue = state
        .store
        .get_issue(id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "issue_not_found",
            message: format!("no issue with id {id}"),
        })?;
    match state.store.get_issue_dates(id).await? {
        Some(d) => Ok(Json(IssueDatesDto::from(d))),
        None => Ok(Json(IssueDatesDto {
            issue_id: issue.id,
            start_at: None,
            due_at: None,
            mirror_node_id: None,
            mirror_synced_at: None,
            mirror_error: None,
            updated_at: issue.updated_at,
        })),
    }
}

/// The Projects v2 GraphQL surface needs an *issue* node id
/// (`I_...`), not the numeric `dp_issues.github_id`. Two paths:
///
///   1. The 0021 migration adds `dp_issues.github_node_id` and
///      the fetcher populates it on every webhook / backfill
///      ingest, so issues sighted after the migration deploy
///      have the id locally. We hand it to the mirror adapter
///      verbatim.
///   2. Rows that pre-date the migration carry `None` here.
///      The mirror adapter falls back to a one-shot
///      `repository.issue(number)` GraphQL lookup and stamps the
///      result back via
///      [`Store::set_issue_github_node_id`][dp_domain::store::Store::set_issue_github_node_id],
///      so the lazy path is taken at most once per row.
///
/// Returns either the cached node id or the `(repo_id, number)`
/// pair the adapter uses to resolve it. The handler does *not*
/// resolve the repo identity here — the adapter owns the
/// `Store` handle it needs for `get_repo` and for stamping the
/// resolved id.
fn issue_node_id(i: &dp_domain::issue::Issue) -> IssueNodeIdRef {
    match i.github_node_id.as_deref() {
        Some(node) => IssueNodeIdRef::Known {
            node_id: node.to_string(),
        },
        None => IssueNodeIdRef::Unresolved {
            issue_id: i.id,
            repo_id: i.repo_id,
            number: i.number,
        },
    }
}

/// What the handler hands to the mirror backend so it can either
/// use the cached GitHub node id directly or resolve it lazily.
/// Public so production backends in the bin layer can match on
/// it; the in-process fakes the tests use ignore the
/// `Unresolved` arm (their fixture rows always carry a node id).
#[derive(Debug, Clone)]
pub enum IssueNodeIdRef {
    /// Cached on `dp_issues.github_node_id`; the adapter passes
    /// it straight through to `addProjectV2ItemById`.
    Known {
        /// GitHub GraphQL node id (`I_...`).
        node_id: String,
    },
    /// Row pre-dates migration 0021. The adapter resolves the
    /// id via `repository(owner, name) { issue(number) { id } }`
    /// and stamps it back so subsequent mirrors are free.
    Unresolved {
        /// `dp_issues.id` — the adapter uses this as the key
        /// for `set_issue_github_node_id` after a successful
        /// resolve.
        issue_id: Uuid,
        /// `dp_issues.repo_id` — the adapter joins back to
        /// `dp_repos`/`dp_orgs` for the GraphQL `owner` / `name`.
        repo_id: Uuid,
        /// `dp_issues.number` for the `issue(number:)` lookup.
        number: i64,
    },
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the issue-dates router. Gated on `(issues, write)` — same
/// pair as the §8 mutation surface; dates are a write op even
/// though the GitHub mirror is best-effort.
pub fn issue_dates_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        // Read surface — gated on (issues, read) so viewers can see
        // existing start/due dates on the §3.10 picker even without
        // write access. The PATCH below keeps the (issues, write)
        // gate intact.
        .merge(with_permission(
            Router::new().route("/issues/{id}/dates", get(get_issue_dates)),
            "issues",
            "read",
        ))
        .merge(with_permission(
            Router::new().route("/issues/{id}/dates", patch(patch_issue_dates)),
            "issues",
            "write",
        ))
        .with_state(inner)
}

// ---------------------------------------------------------------------------
// Tests — local upsert happy path, invalid window, unconfigured
// mirror is silent. The mirror best-effort spawn is exercised via
// a fake backend that records calls; we await it through a oneshot
// so the test is deterministic.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_permissions::GitHubAppConfig;
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::extract::Request;
    use axum::http::{Method, StatusCode};
    use chrono::Duration;
    use dp_domain::app_install::{AppInstallPermissions, OrgAppInstall};
    use dp_domain::audit::AuditEntry;
    use dp_domain::event::ActivityEvent;
    use dp_domain::issue::{Issue, IssueState as DomainIssueState};
    use dp_domain::issue_dates::ProjectV2MirrorTask;
    use dp_domain::org::Org;
    use dp_domain::repo::Repo;
    use dp_domain::board_link::{BoardItem, BoardLink};
    use dp_domain::project::{Project, ProjectStatus};
    use dp_domain::store::{IssueDatesMirrorOutcome, Store, StoreError};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tokio::sync::oneshot;
    use tower::ServiceExt;

    #[derive(Default)]
    struct FakeStore {
        inner: Mutex<FakeInner>,
    }
    #[derive(Default)]
    struct FakeInner {
        orgs: HashMap<Uuid, Org>,
        repos: HashMap<Uuid, Repo>,
        issues: HashMap<Uuid, Issue>,
        installs: HashMap<Uuid, OrgAppInstall>,
        dates: HashMap<Uuid, IssueDates>,
        tasks: Vec<ProjectV2MirrorTask>,
        audit: Vec<AuditEntry>,
        mirror_results: Vec<(Uuid, String)>, // (issue, "ok:NODE" | "err:MSG")
        projects: HashMap<Uuid, Project>,
        project_for_issue: HashMap<Uuid, Uuid>, // issue -> project
        board_links: HashMap<Uuid, BoardLink>,  // id -> link
        board_items: HashMap<(Uuid, Uuid), BoardItem>, // (link, issue) -> item
        // (link_id, issue_id, "ok:NODE" | "err:MSG")
        board_results: Vec<(Uuid, Uuid, String)>,
    }

    #[async_trait]
    impl Store for FakeStore {
        async fn get_issue(&self, id: Uuid) -> Result<Option<Issue>, StoreError> {
            Ok(self.inner.lock().unwrap().issues.get(&id).cloned())
        }
        async fn get_repo(&self, id: Uuid) -> Result<Option<Repo>, StoreError> {
            Ok(self.inner.lock().unwrap().repos.get(&id).cloned())
        }
        async fn get_org(&self, id: Uuid) -> Result<Option<Org>, StoreError> {
            Ok(self.inner.lock().unwrap().orgs.get(&id).cloned())
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
        async fn upsert_issue_dates(
            &self,
            issue_id: Uuid,
            start_at: Option<DateTime<Utc>>,
            due_at: Option<DateTime<Utc>>,
        ) -> Result<IssueDates, StoreError> {
            if let (Some(s), Some(d)) = (start_at, due_at) {
                if s > d {
                    return Err(StoreError::Invalid("start > due".into()));
                }
            }
            let mut g = self.inner.lock().unwrap();
            let prev = g.dates.get(&issue_id).cloned();
            let row = IssueDates {
                issue_id,
                start_at,
                due_at,
                mirror_node_id: prev.as_ref().and_then(|p| p.mirror_node_id.clone()),
                mirror_synced_at: prev.as_ref().and_then(|p| p.mirror_synced_at),
                mirror_error: prev.as_ref().and_then(|p| p.mirror_error.clone()),
                updated_at: Utc::now(),
            };
            g.dates.insert(issue_id, row.clone());
            Ok(row)
        }
        async fn record_issue_dates_mirror_result(
            &self,
            issue_id: Uuid,
            outcome: IssueDatesMirrorOutcome<'_>,
        ) -> Result<(), StoreError> {
            let mut g = self.inner.lock().unwrap();
            let label = match outcome {
                IssueDatesMirrorOutcome::Success { node_id } => {
                    if let Some(d) = g.dates.get_mut(&issue_id) {
                        d.mirror_node_id = Some(node_id.to_string());
                        d.mirror_synced_at = Some(Utc::now());
                        d.mirror_error = None;
                    }
                    format!("ok:{node_id}")
                }
                IssueDatesMirrorOutcome::Failure { error } => {
                    if let Some(d) = g.dates.get_mut(&issue_id) {
                        d.mirror_error = Some(error.to_string());
                    }
                    format!("err:{error}")
                }
            };
            g.mirror_results.push((issue_id, label));
            Ok(())
        }
        async fn enqueue_projectv2_mirror_task(
            &self,
            issue_id: Uuid,
            repo_id: Uuid,
            kind: ProjectV2MirrorTaskKind,
            payload: serde_json::Value,
        ) -> Result<(), StoreError> {
            self.inner.lock().unwrap().tasks.push(ProjectV2MirrorTask {
                id: Uuid::new_v4(),
                issue_id,
                repo_id,
                kind,
                payload,
                attempts: 0,
                last_error: None,
                enqueued_at: Utc::now(),
                processed_at: None,
            });
            Ok(())
        }
        async fn record_audit_log(&self, e: &AuditEntry) -> Result<(), StoreError> {
            self.inner.lock().unwrap().audit.push(e.clone());
            Ok(())
        }
        async fn get_project_for_issue(
            &self,
            issue_id: Uuid,
        ) -> Result<Option<Project>, StoreError> {
            let g = self.inner.lock().unwrap();
            Ok(g.project_for_issue
                .get(&issue_id)
                .and_then(|pid| g.projects.get(pid).cloned()))
        }
        async fn list_board_links(
            &self,
            project_id: Uuid,
        ) -> Result<Vec<BoardLink>, StoreError> {
            Ok(self
                .inner
                .lock()
                .unwrap()
                .board_links
                .values()
                .filter(|l| l.project_id == project_id)
                .cloned()
                .collect())
        }
        async fn get_board_item(
            &self,
            link_id: Uuid,
            issue_id: Uuid,
        ) -> Result<Option<BoardItem>, StoreError> {
            Ok(self
                .inner
                .lock()
                .unwrap()
                .board_items
                .get(&(link_id, issue_id))
                .cloned())
        }
        async fn record_board_item_result(
            &self,
            link_id: Uuid,
            issue_id: Uuid,
            outcome: BoardItemMirrorOutcome<'_>,
        ) -> Result<(), StoreError> {
            let mut g = self.inner.lock().unwrap();
            let label = match outcome {
                BoardItemMirrorOutcome::Success { item_node_id } => {
                    g.board_items.insert(
                        (link_id, issue_id),
                        BoardItem {
                            link_id,
                            issue_id,
                            item_node_id: item_node_id.to_string(),
                            last_synced_at: Some(Utc::now()),
                            last_error: None,
                            created_at: Utc::now(),
                            updated_at: Utc::now(),
                        },
                    );
                    if let Some(link) = g.board_links.get_mut(&link_id) {
                        link.last_mirror_at = Some(Utc::now());
                        link.last_mirror_error = None;
                    }
                    format!("ok:{item_node_id}")
                }
                BoardItemMirrorOutcome::Failure { error } => {
                    if let Some(link) = g.board_links.get_mut(&link_id) {
                        link.last_mirror_error = Some(error.to_string());
                    }
                    format!("err:{error}")
                }
            };
            g.board_results.push((link_id, issue_id, label));
            Ok(())
        }
        // Minimal stubs ------------------------------------------------
        async fn upsert_user(&self, u: &dp_domain::user::User) -> Result<dp_domain::user::User, StoreError> {
            Ok(u.clone())
        }
        async fn get_user(&self, _: Uuid) -> Result<dp_domain::user::User, StoreError> {
            unimplemented!()
        }
        async fn get_user_by_github_id(&self, _: i64) -> Result<dp_domain::user::User, StoreError> {
            unimplemented!()
        }
        async fn list_users(&self) -> Result<Vec<dp_domain::user::User>, StoreError> {
            Ok(vec![])
        }
        async fn pseudonymise_user(&self, _: Uuid) -> Result<(), StoreError> {
            Ok(())
        }
        async fn upsert_org(&self, o: &Org) -> Result<Org, StoreError> {
            Ok(o.clone())
        }
        async fn upsert_team(&self, t: &dp_domain::team::Team) -> Result<dp_domain::team::Team, StoreError> {
            Ok(t.clone())
        }
        async fn upsert_repo(&self, r: &Repo) -> Result<Repo, StoreError> {
            Ok(r.clone())
        }
        async fn upsert_membership(
            &self,
            m: &dp_domain::membership::Membership,
        ) -> Result<dp_domain::membership::Membership, StoreError> {
            Ok(m.clone())
        }
        async fn list_memberships_for_user(
            &self,
            _: Uuid,
        ) -> Result<Vec<dp_domain::membership::Membership>, StoreError> {
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
        async fn add_event_actors(
            &self,
            _: &[dp_domain::event::EventActor],
        ) -> Result<(), StoreError> {
            Ok(())
        }
        async fn list_event_actor_rows_in_window(
            &self,
            _: &dp_domain::window::Window,
            _: &[Uuid],
            _: &[Uuid],
            _: &[Uuid],
            _: &[dp_domain::event::ActorRole],
        ) -> Result<Vec<dp_domain::store::EventActorRow>, StoreError> {
            Ok(vec![])
        }
        async fn get_cursor(
            &self,
            _: Uuid,
            _: Option<Uuid>,
            _: dp_domain::fetch::ResourceKind,
        ) -> Result<dp_domain::fetch::FetchCursor, StoreError> {
            Err(StoreError::NotFound {
                entity: "fetch_cursor",
                id: String::new(),
            })
        }
        async fn put_cursor(&self, _: &dp_domain::fetch::FetchCursor) -> Result<(), StoreError> {
            Ok(())
        }
        async fn start_fetch_run(
            &self,
            _: dp_domain::fetch::FetchRunKind,
        ) -> Result<Uuid, StoreError> {
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
        ) -> Result<Vec<dp_domain::fetch::FetchRun>, StoreError> {
            Ok(vec![])
        }
        async fn data_as_of(
            &self,
        ) -> Result<dp_domain::freshness::DataAsOf, StoreError> {
            Ok(dp_domain::freshness::DataAsOf::default())
        }
        async fn enqueue_webhook(
            &self,
            _: &dp_domain::webhook::WebhookDelivery,
        ) -> Result<(), StoreError> {
            Ok(())
        }
        async fn claim_webhooks(
            &self,
            _: i64,
        ) -> Result<Vec<dp_domain::webhook::WebhookDelivery>, StoreError> {
            Ok(vec![])
        }
        async fn mark_webhook_processed(&self, _: Uuid) -> Result<(), StoreError> {
            Ok(())
        }
        async fn mark_webhook_failed(&self, _: Uuid, _: &str) -> Result<(), StoreError> {
            Ok(())
        }
        async fn record_event(
            &self,
            e: &ActivityEvent,
        ) -> Result<ActivityEvent, StoreError> {
            Ok(e.clone())
        }
    }

    /// Backend that records calls and signals via a oneshot so the
    /// test can `.await` mirror completion deterministically.
    struct RecordingBackend {
        outcome: Mutex<Result<MirrorDatesOk, MirrorError>>,
        signal: Mutex<Option<oneshot::Sender<()>>>,
        seen: Mutex<Option<(String, Option<String>, Option<DateTime<Utc>>, Option<DateTime<Utc>>)>>,
    }
    impl RecordingBackend {
        fn ok(node: &str) -> (Arc<Self>, oneshot::Receiver<()>) {
            let (tx, rx) = oneshot::channel();
            (
                Arc::new(Self {
                    outcome: Mutex::new(Ok(MirrorDatesOk {
                        item_node_id: node.to_string(),
                    })),
                    signal: Mutex::new(Some(tx)),
                    seen: Mutex::new(None),
                }),
                rx,
            )
        }
        fn err(msg: &str) -> (Arc<Self>, oneshot::Receiver<()>) {
            let (tx, rx) = oneshot::channel();
            (
                Arc::new(Self {
                    outcome: Mutex::new(Err(MirrorError::GraphQl(msg.to_string()))),
                    signal: Mutex::new(Some(tx)),
                    seen: Mutex::new(None),
                }),
                rx,
            )
        }
    }
    #[async_trait]
    impl ProjectV2MirrorBackend for RecordingBackend {
        async fn mirror_dates(
            &self,
            _link: &RepoProjectLink,
            issue_node_id: &IssueNodeIdRef,
            existing_item_node_id: Option<&str>,
            start_at: Option<DateTime<Utc>>,
            due_at: Option<DateTime<Utc>>,
        ) -> Result<MirrorDatesOk, MirrorError> {
            let seen_node = match issue_node_id {
                IssueNodeIdRef::Known { node_id } => node_id.clone(),
                IssueNodeIdRef::Unresolved { issue_id, .. } => {
                    format!("unresolved:{issue_id}")
                }
            };
            *self.seen.lock().unwrap() = Some((
                seen_node,
                existing_item_node_id.map(str::to_string),
                start_at,
                due_at,
            ));
            let out = match &*self.outcome.lock().unwrap() {
                Ok(v) => Ok(v.clone()),
                Err(MirrorError::GraphQl(s)) => Err(MirrorError::GraphQl(s.clone())),
                Err(MirrorError::Transport(s)) => Err(MirrorError::Transport(s.clone())),
                Err(MirrorError::Unconfigured) => Err(MirrorError::Unconfigured),
            };
            if let Some(tx) = self.signal.lock().unwrap().take() {
                let _ = tx.send(());
            }
            out
        }
    }

    struct Rig {
        store: Arc<FakeStore>,
        principal: Principal,
        org: Org,
        repo: Repo,
        issue: Issue,
    }

    fn build_rig() -> Rig {
        let store = Arc::new(FakeStore::default());
        let actor = Uuid::new_v4();
        let org = Org {
            id: Uuid::new_v4(),
            github_id: 1,
            login: "acme".into(),
            name: None,
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
            version: 1,
            github_node_id: Some("I_kwTEST".into()),
            updated_at: Utc::now(),
            is_local: false,
        };
        {
            let mut g = store.inner.lock().unwrap();
            g.orgs.insert(org.id, org.clone());
            g.repos.insert(repo.id, repo.clone());
            g.issues.insert(issue.id, issue.clone());
            g.installs.insert(
                org.id,
                OrgAppInstall {
                    org_id: org.id,
                    installation_id: 555,
                    permissions: AppInstallPermissions { issues_write: true },
                    observed_at: Utc::now(),
                },
            );
        }
        Rig {
            store,
            principal: Principal { actor_user_id: actor },
            org,
            repo,
            issue,
        }
    }

    fn app(rig: &Rig, mirror: Arc<dyn ProjectV2MirrorBackend>) -> Router {
        let cfg = GitHubAppConfig {
            request_issues_write: true,
            slug: Some("dev-pulse-app".into()),
            ..GitHubAppConfig::default()
        };
        let state = AppState::new(rig.store.clone())
            .with_github_app(Arc::new(cfg))
            .with_projectv2_mirror(mirror);
        Router::new()
            .route("/issues/{id}/dates", patch(patch_issue_dates))
            .layer(axum::Extension(rig.principal))
            .with_state(state)
    }

    async fn send(app: Router, req: Request) -> (StatusCode, Vec<u8>) {
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        (status, bytes.to_vec())
    }

    fn patch_req(id: Uuid, body: serde_json::Value) -> Request {
        Request::builder()
            .method(Method::PATCH)
            .uri(format!("/issues/{id}/dates"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn local_upsert_no_link_no_mirror() {
        let rig = build_rig();
        let mirror = Arc::new(UnconfiguredProjectV2Mirror);
        let now = Utc::now();
        let body = json!({
            "start_at": now,
            "due_at":   now + Duration::days(3),
        });
        let (status, bytes) = send(app(&rig, mirror), patch_req(rig.issue.id, body)).await;
        assert_eq!(status, StatusCode::OK);
        let dto: IssueDatesDto = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(dto.issue_id, rig.issue.id);
        assert!(dto.start_at.is_some() && dto.due_at.is_some());
        let g = rig.store.inner.lock().unwrap();
        assert!(g.tasks.is_empty(), "no link ⇒ no outbox row");
        assert!(g.mirror_results.is_empty(), "no link ⇒ no mirror");
        assert!(g.audit.iter().any(|a| a.action == audit::ISSUE_DATES_UPDATE));
    }

    #[tokio::test]
    async fn invalid_window_rejected() {
        let rig = build_rig();
        let mirror = Arc::new(UnconfiguredProjectV2Mirror);
        let now = Utc::now();
        let body = json!({
            "start_at": now + Duration::days(5),
            "due_at":   now,
        });
        let (status, bytes) = send(app(&rig, mirror), patch_req(rig.issue.id, body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["code"], "invalid_date_window");
    }

    /// Seed a project owning `issue` plus a single board link. The
    /// §7.4 mirror fan-out reads through `get_project_for_issue` →
    /// `list_board_links` so a successful test path needs both
    /// rows wired up.
    fn seed_project_with_link(rig: &Rig) -> (Uuid, Uuid) {
        let project_id = Uuid::new_v4();
        let link_id = Uuid::new_v4();
        let mut g = rig.store.inner.lock().unwrap();
        g.projects.insert(
            project_id,
            Project {
                id: project_id,
                org_id: rig.org.id,
                name: "P".into(),
                description: None,
                lead_user_id: None,
                status: ProjectStatus::Active,
                start_at: None,
                due_at: None,
                issue_count: 1,
                closed_issue_count: 0,
                created_by: Some(rig.principal.actor_user_id),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                version: 1,
                primary_milestone_id: None,
            },
        );
        g.project_for_issue.insert(rig.issue.id, project_id);
        g.board_links.insert(
            link_id,
            BoardLink {
                id: link_id,
                project_id,
                github_board_node_id: "PVT_kw".into(),
                github_board_title: Some("Rubix Roadmap".into()),
                github_board_url: Some("https://github.com/orgs/acme/projects/12".into()),
                github_board_cached_at: Some(Utc::now()),
                start_field_node_id: Some("PVF_start".into()),
                due_field_node_id: Some("PVF_due".into()),
                status_field_node_id: None,
                last_mirror_at: None,
                last_mirror_error: None,
                created_by: Some(rig.principal.actor_user_id),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        );
        (project_id, link_id)
    }

    #[tokio::test]
    async fn mirror_success_writes_item_id_and_clears_link_error() {
        let rig = build_rig();
        let (_project_id, link_id) = seed_project_with_link(&rig);
        let (backend, done) = RecordingBackend::ok("PVTI_item_42");
        let now = Utc::now();
        let body = json!({ "start_at": now, "due_at": now + Duration::days(1) });
        let (status, _) = send(
            app(&rig, backend.clone() as Arc<dyn ProjectV2MirrorBackend>),
            patch_req(rig.issue.id, body),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        done.await.unwrap();
        // Yield so the spawned task's post-await store call runs.
        tokio::task::yield_now().await;
        let g = rig.store.inner.lock().unwrap();
        assert_eq!(g.tasks.len(), 1, "one outbox row per linked board");
        assert_eq!(g.tasks[0].kind, ProjectV2MirrorTaskKind::MirrorDates);
        assert_eq!(g.board_results.len(), 1);
        let (link, issue, label) = &g.board_results[0];
        assert_eq!(*link, link_id);
        assert_eq!(*issue, rig.issue.id);
        assert!(label.starts_with("ok:PVTI_item_42"));
        let item = g
            .board_items
            .get(&(link_id, rig.issue.id))
            .expect("board item persisted");
        assert_eq!(item.item_node_id, "PVTI_item_42");
        // Aggregate last_mirror_* rolled up on the link row.
        let link_row = &g.board_links[&link_id];
        assert!(link_row.last_mirror_at.is_some());
        assert!(link_row.last_mirror_error.is_none());
    }

    #[tokio::test]
    async fn mirror_failure_records_per_link_error_does_not_fail_response() {
        let rig = build_rig();
        let (_project_id, link_id) = seed_project_with_link(&rig);
        let (backend, done) = RecordingBackend::err("field not found");
        let body = json!({ "due_at": Utc::now() });
        let (status, _) = send(
            app(&rig, backend.clone() as Arc<dyn ProjectV2MirrorBackend>),
            patch_req(rig.issue.id, body),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "local save must succeed");
        done.await.unwrap();
        tokio::task::yield_now().await;
        let g = rig.store.inner.lock().unwrap();
        assert!(g.board_results[0].2.contains("field not found"));
        let link_row = &g.board_links[&link_id];
        assert!(link_row
            .last_mirror_error
            .as_deref()
            .unwrap_or("")
            .contains("field not found"));
    }

    // Silence unused-field warnings on the rig fields used purely
    // to keep the type stable across future tests.
    #[allow(dead_code)]
    fn _touch(r: &Rig) {
        let _ = (&r.org, &r.repo);
    }
}
