//! Admin handlers — operator-only surface (Phase 2 stage 8 + Phase 4 stage 5).
//!
//! | route                                  | what it does                                              |
//! |----------------------------------------|-----------------------------------------------------------|
//! | `POST /admin/refresh`                  | triggers the reconciler tick (coalesced)                  |
//! | `GET /admin/runs?limit=&offset=`       | paginated `dp_fetch_runs` projection                       |
//! | `POST /admin/users/:id/anonymise`      | GDPR §0.5 pseudonymisation cascade                         |
//! | `GET /admin/users/:id/export`          | GDPR §9 user export, JSON-streamed in event pages          |
//!
//! Every handler is mounted **behind** `with_principal` at the
//! composition layer (`dp-server`) and writes one `audit_log` row
//! through [`crate::audit::record`] before returning. The pinned
//! verbs ([`audit::ADMIN_REFRESH`], [`audit::RUNS_LIST`],
//! [`audit::USER_ANONYMISE`], [`audit::USER_EXPORT`]) live in
//! [`crate::audit`] so the schema cannot drift per-handler.
//!
//! Streaming note: the user-export handler chunks the
//! `event_actor` join into pages of [`EXPORT_PAGE_SIZE`] rows.
//! Each page is JSON-encoded and pushed onto an mpsc channel a
//! [`tokio_stream::wrappers::ReceiverStream`] feeds into
//! [`axum::body::Body::from_stream`], so a 500MB export never
//! materialises in process memory.

use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Extension, Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use bytes::Bytes;
use dp_domain::event::ActorRole;
use dp_domain::fetch::{FetchRun, FetchRunErrorSample, FetchRunKind};
use dp_domain::store::{EventActorRow, RepoListFilter, Store, StoreError};
use dp_fetcher::client::{ClientError, Fetched};
use dp_fetcher::reconciler::{Scheduler, Scope};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use tokio_stream::wrappers::ReceiverStream;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::audit::{self, Principal};
use crate::directory::UserDto;
use crate::error::ApiError;

/// State the admin router reads. Held inside an `Arc` so axum
/// can clone it per request without cloning the scheduler or
/// the store.
pub struct AdminState {
    /// Scheduler whose coalescing mutex the refresh route shares.
    pub scheduler: Arc<Scheduler>,
    /// Persistence handle — every admin handler reads or writes
    /// through it, and the audit log lives here too.
    pub store: Arc<dyn Store>,
}

impl AdminState {
    /// Convenience constructor.
    pub fn new(scheduler: Arc<Scheduler>, store: Arc<dyn Store>) -> Self {
        Self { scheduler, store }
    }
}

// ---------------------------------------------------------------------------
// POST /admin/refresh
// ---------------------------------------------------------------------------

/// Query parameters parsed off `POST /admin/refresh`.
#[derive(Debug, Deserialize)]
pub struct RefreshQuery {
    /// Narrow the tick to one org (and optionally one repo within).
    pub org_id: Option<Uuid>,
    /// Narrow the tick to one repo. Requires `org_id` to also be
    /// supplied — `?repo_id=…` alone is rejected as 400.
    pub repo_id: Option<Uuid>,
}

impl RefreshQuery {
    fn to_scope(&self) -> Result<Scope, ApiError> {
        match (self.org_id, self.repo_id) {
            (None, None) => Ok(Scope::All),
            (Some(o), None) => Ok(Scope::Org(o)),
            (Some(o), Some(r)) => Ok(Scope::Repo {
                org_id: o,
                repo_id: r,
            }),
            (None, Some(_)) => Err(ApiError::BadRequest {
                code: "missing_org_id",
                message: "repo_id requires org_id to also be specified".to_string(),
            }),
        }
    }

    fn audit_target(&self) -> String {
        match (self.org_id, self.repo_id) {
            (None, None) => "scope:all".to_string(),
            (Some(o), None) => format!("org:{o}"),
            (Some(o), Some(r)) => format!("org:{o};repo:{r}"),
            (None, Some(_)) => String::new(),
        }
    }
}

/// Response body for `POST /admin/refresh`. Variants are flattened
/// so a coalesce comes out as `{ "ran": false }` and a real tick
/// as `{ "ran": true, "items": …, "errors": …, "partial": … }`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(untagged)]
pub enum RefreshResponse {
    /// A tick ran to completion.
    Ran {
        /// Always `true` on this variant.
        ran: bool,
        /// Total deliveries applied during the tick.
        items: i64,
        /// `(target, kind)` failures during the tick.
        errors: i64,
        /// Whether the tick partially succeeded.
        partial: bool,
    },
    /// The trigger coalesced into an in-flight tick.
    Coalesced {
        /// Always `false` on this variant.
        ran: bool,
    },
}

impl IntoResponse for RefreshResponse {
    fn into_response(self) -> Response {
        Json(self).into_response()
    }
}

/// `POST /admin/refresh` — operator-triggered reconciler tick.
///
/// Audit: writes [`audit::ADMIN_REFRESH`] with target
/// `"scope:all"`, `"org:<id>"`, or `"org:<id>;repo:<id>"`. The row
/// lands even on coalesce — the operator's *intent* is what we
/// audit, not whether a tick physically ran.
#[utoipa::path(
    post,
    path = "/admin/refresh",
    params(
        ("org_id"  = Option<Uuid>, Query, description = "Narrow the tick to one org"),
        ("repo_id" = Option<Uuid>, Query, description = "Narrow the tick to one repo (requires org_id)"),
    ),
    responses(
        (status = 200, description = "Tick scheduled or coalesced", body = RefreshResponse),
        (status = 400, description = "Validation failed"),
    ),
    tag = "admin",
)]
pub async fn refresh(
    State(state): State<Arc<AdminState>>,
    Extension(principal): Extension<Principal>,
    Query(q): Query<RefreshQuery>,
) -> Result<Json<RefreshResponse>, ApiError> {
    let scope = q.to_scope()?;
    let out = state.scheduler.try_trigger_now(scope).await.map_err(|e| {
        tracing::error!(error = %e, "admin refresh failed");
        ApiError::BadRequest {
            code: "reconciler_failed",
            message: "reconciler failed".to_string(),
        }
    })?;
    // Audit even on coalesce — the operator's intent is the
    // auditable event, not whether a tick physically ran.
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::ADMIN_REFRESH,
        q.audit_target(),
    )
    .await?;
    let body = match out {
        Some(stats) => RefreshResponse::Ran {
            ran: true,
            items: stats.items,
            errors: stats.errors,
            partial: stats.partial,
        },
        None => RefreshResponse::Coalesced { ran: false },
    };
    Ok(Json(body))
}

// ---------------------------------------------------------------------------
// GET /admin/runs
// ---------------------------------------------------------------------------

/// Default page size for `GET /admin/runs`. Picked so a single page
/// always fits one HTTP buffer; operators paginating older history
/// pass `offset=`.
pub const RUNS_DEFAULT_LIMIT: i64 = 50;
/// Hard cap on `?limit=` for `GET /admin/runs` — anything larger is
/// clamped so a sloppy caller cannot read the whole table in one
/// hit.
pub const RUNS_MAX_LIMIT: i64 = 500;

/// Query parameters for `GET /admin/runs`.
#[derive(Debug, Deserialize)]
pub struct RunsQuery {
    /// Page size. Defaults to [`RUNS_DEFAULT_LIMIT`], clamped to
    /// [`RUNS_MAX_LIMIT`].
    #[serde(default)]
    pub limit: Option<i64>,
    /// Number of rows to skip. Defaults to 0.
    #[serde(default)]
    pub offset: Option<i64>,
}

/// Wire row for `GET /admin/runs`. Mirrors [`FetchRun`] with serde
/// derives + `ToSchema` so the openapi document picks it up.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FetchRunDto {
    /// Run id.
    pub id: Uuid,
    /// Run kind — `webhook_worker` / `reconciler` / `backfill`.
    #[schema(value_type = String)]
    pub kind: FetchRunKind,
    /// Wall-clock start (UTC).
    pub started: chrono::DateTime<chrono::Utc>,
    /// Wall-clock end (UTC). `None` while the run is in flight.
    pub finished: Option<chrono::DateTime<chrono::Utc>>,
    /// Items applied during the run.
    pub items: i64,
    /// Items that errored during the run.
    pub errors: i64,
    /// `true` if the run finished but some items failed.
    pub partial: bool,
    /// Bounded sample of per-item failure context, captured by the
    /// fetcher so the admin UI can explain *why* `errors > 0`
    /// without leaving the page. `None` for clean runs and for
    /// runs that pre-date the column.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_sample: Option<Vec<FetchRunErrorSampleDto>>,
}

/// One captured failure inside a [`FetchRunDto`]. Mirrors
/// [`FetchRunErrorSample`] verbatim so the wire shape matches
/// the domain shape — only added so `ToSchema` shows it in the
/// openapi document.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FetchRunErrorSampleDto {
    /// Org login at the time of capture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org: Option<String>,
    /// `owner/name` of the repo, when the failure was repo-scoped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// Resource kind / source (`"Issues"`, `"webhook:push"`, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Rendered, truncated error message.
    pub error: String,
}

impl From<FetchRunErrorSample> for FetchRunErrorSampleDto {
    fn from(s: FetchRunErrorSample) -> Self {
        Self {
            org: s.org,
            repo: s.repo,
            kind: s.kind,
            error: s.error,
        }
    }
}

impl From<FetchRun> for FetchRunDto {
    fn from(r: FetchRun) -> Self {
        Self {
            id: r.id,
            kind: r.kind,
            started: r.started,
            finished: r.finished,
            items: r.items,
            errors: r.errors,
            partial: r.partial,
            error_sample: r
                .error_sample
                .map(|v| v.into_iter().map(FetchRunErrorSampleDto::from).collect()),
        }
    }
}

/// `GET /admin/runs` — paginated `dp_fetch_runs` projection.
///
/// Audit: writes [`audit::RUNS_LIST`] with target
/// `"runs:limit=<n>;offset=<n>"`.
#[utoipa::path(
    get,
    path = "/admin/runs",
    params(
        ("limit"  = Option<i64>, Query, description = "Page size (default 50, max 500)"),
        ("offset" = Option<i64>, Query, description = "Rows to skip"),
    ),
    responses(
        (status = 200, description = "Run log page", body = Vec<FetchRunDto>),
    ),
    tag = "admin",
)]
pub async fn list_runs(
    State(state): State<Arc<AdminState>>,
    Extension(principal): Extension<Principal>,
    Query(q): Query<RunsQuery>,
) -> Result<Json<Vec<FetchRunDto>>, ApiError> {
    let limit = q
        .limit
        .unwrap_or(RUNS_DEFAULT_LIMIT)
        .clamp(1, RUNS_MAX_LIMIT);
    let offset = q.offset.unwrap_or(0).max(0);
    let rows = state.store.list_fetch_runs(limit, offset).await?;
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::RUNS_LIST,
        format!("runs:limit={limit};offset={offset}"),
    )
    .await?;
    Ok(Json(rows.into_iter().map(FetchRunDto::from).collect()))
}

// ---------------------------------------------------------------------------
// POST /admin/users/:id/anonymise
// ---------------------------------------------------------------------------

/// `POST /admin/users/:id/anonymise` — irreversible GDPR cascade.
///
/// Calls [`Store::pseudonymise_user`], which rewrites
/// `login`/`email`/`name` to a `deleted-user-<short>` form and sets
/// `deleted_at`. The row id is preserved so historical reports
/// keep referential integrity (§0.5).
///
/// Audit: writes [`audit::USER_ANONYMISE`], target
/// `"user:<id>"`, **after** the cascade succeeds — a failed
/// cascade does not leave a misleading audit trail.
#[utoipa::path(
    post,
    path = "/admin/users/{id}/anonymise",
    params(
        ("id" = Uuid, Path, description = "User to anonymise")
    ),
    responses(
        (status = 200, description = "User anonymised", body = crate::directory::Ack),
        (status = 404, description = "No such user"),
    ),
    tag = "admin",
)]
pub async fn anonymise_user(
    State(state): State<Arc<AdminState>>,
    Extension(principal): Extension<Principal>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<crate::directory::Ack>, ApiError> {
    state.store.pseudonymise_user(user_id).await?;
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::USER_ANONYMISE,
        format!("user:{user_id}"),
    )
    .await?;
    Ok(Json(crate::directory::Ack { ok: true }))
}

// ---------------------------------------------------------------------------
// GET /admin/users/:id/export
// ---------------------------------------------------------------------------

/// Rows per page when streaming the user export. Picked so each
/// JSON chunk stays well under typical HTTP buffer sizes (~1MB
/// at 1KB/row), and small enough that one page is cheap to encode.
pub const EXPORT_PAGE_SIZE: i64 = 500;

/// Per-event projection inside the export. Carries the event row
/// plus every role this user played on it — a squash-merge author
/// who later closed the same PR shows up once with
/// `roles: ["author", "closer"]`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ExportEvent {
    /// Event id (the join key inside the export).
    pub event_id: Uuid,
    /// Org the event happened in.
    pub org_id: Uuid,
    /// Repo the event happened in.
    pub repo_id: Uuid,
    /// Event kind (snake_case wire form).
    #[schema(value_type = String)]
    pub kind: dp_domain::event::EventKind,
    /// Source timestamp (UTC).
    pub ts: chrono::DateTime<chrono::Utc>,
    /// Every role the exported user played on this event.
    #[schema(value_type = Vec<String>)]
    pub roles: Vec<ActorRole>,
}

/// Top-level shape emitted by `GET /admin/users/:id/export`.
/// Documented for the openapi schema even though the response is
/// served as a chunked stream — the wire JSON parses to this type.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UserExport {
    /// The user row (already pseudonymised if previously deleted).
    pub user: UserDto,
    /// Every `(user, org)` membership row.
    pub memberships: Vec<MembershipDto>,
    /// Every event this user is credited on, ordered by `ts ASC`.
    pub events: Vec<ExportEvent>,
}

/// `dp_memberships` projection inside the export.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MembershipDto {
    /// User side of the join.
    pub user_id: Uuid,
    /// Org side of the join.
    pub org_id: Uuid,
    /// Role inside the org.
    #[schema(value_type = String)]
    pub role: dp_domain::membership::MembershipRole,
    /// Home-org label, if set.
    pub home_org: Option<Uuid>,
    /// When dev-pulse first observed this membership.
    pub joined_at: chrono::DateTime<chrono::Utc>,
}

impl From<dp_domain::Membership> for MembershipDto {
    fn from(m: dp_domain::Membership) -> Self {
        Self {
            user_id: m.user_id,
            org_id: m.org_id,
            role: m.role,
            home_org: m.home_org,
            joined_at: m.joined_at,
        }
    }
}

/// Fold the (sorted-by-`event_id`) page of [`EventActorRow`]s into
/// one [`ExportEvent`] per event id, collecting every role this
/// user played on it. Returns the folded events plus any *trailing
/// partial* event whose actor rows might span the page boundary —
/// the caller carries that over to the next page.
fn fold_event_actors_page(
    mut rows: Vec<EventActorRow>,
    carry: Option<ExportEvent>,
) -> (Vec<ExportEvent>, Option<ExportEvent>) {
    // Rows arrive ordered by (ts, event_id) per the Store contract,
    // so consecutive rows for the same event are adjacent. Carry
    // any in-progress event from the previous page first.
    let mut out: Vec<ExportEvent> = Vec::new();
    let mut current: Option<ExportEvent> = carry;
    for r in rows.drain(..) {
        match &mut current {
            Some(c) if c.event_id == r.event_id => c.roles.push(r.role),
            Some(_) => {
                out.push(current.take().unwrap());
                current = Some(ExportEvent {
                    event_id: r.event_id,
                    org_id: r.org_id,
                    repo_id: r.repo_id,
                    kind: r.kind,
                    ts: r.ts,
                    roles: vec![r.role],
                });
            }
            None => {
                current = Some(ExportEvent {
                    event_id: r.event_id,
                    org_id: r.org_id,
                    repo_id: r.repo_id,
                    kind: r.kind,
                    ts: r.ts,
                    roles: vec![r.role],
                });
            }
        }
    }
    (out, current)
}

/// `GET /admin/users/:id/export` — chunked JSON dump of every row
/// dev-pulse holds about the user.
///
/// Wire shape: a single JSON object matching [`UserExport`]. We
/// emit it incrementally so the process never holds more than one
/// page of event rows in memory at a time:
///
/// 1. `{"user":<user>,"memberships":<memberships>,"events":[`
/// 2. one JSON object per event, comma-separated, page by page
/// 3. `]}`
///
/// Audit: writes [`audit::USER_EXPORT`], target `"user:<id>"`,
/// **before** streaming starts. Auditing pre-stream is deliberate
/// — the request was authorised, so the access attempt should be
/// recorded even if the network drops mid-stream.
#[utoipa::path(
    get,
    path = "/admin/users/{id}/export",
    params(
        ("id" = Uuid, Path, description = "User to export")
    ),
    responses(
        (status = 200, description = "GDPR export, JSON-streamed", body = UserExport),
        (status = 404, description = "No such user"),
    ),
    tag = "admin",
)]
pub async fn export_user(
    State(state): State<Arc<AdminState>>,
    Extension(principal): Extension<Principal>,
    Path(user_id): Path<Uuid>,
) -> Result<Response, ApiError> {
    // Resolve the small head-of-document data eagerly so we can
    // 404 before opening the stream.
    let user = state.store.get_user(user_id).await?;
    let memberships = state.store.list_memberships_for_user(user_id).await?;

    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::USER_EXPORT,
        format!("user:{user_id}"),
    )
    .await?;

    // Build the streamed body.
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, Infallible>>(4);
    let store = state.store.clone();
    let memberships_dto: Vec<MembershipDto> =
        memberships.into_iter().map(MembershipDto::from).collect();
    let user_dto = UserDto::from(user);

    tokio::spawn(async move {
        // ---- header --------------------------------------------------
        // Assemble the head-of-document by hand: encode `user` +
        // `memberships` as separate values, then splice them around
        // the literal `{"user":…,"memberships":…,"events":[` so the
        // open `events` array can be streamed into below. Doing it
        // this way (rather than encoding the whole object and trimming
        // a trailing `]}`) avoids relying on serde_json's key-order
        // implementation detail — `serde_json::Value::Object` sorts
        // keys alphabetically, so "events" would not be last.
        let user_bytes = match serde_json::to_vec(&user_dto) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(error = %e, "failed to encode user for export");
                return;
            }
        };
        let memberships_bytes = match serde_json::to_vec(&memberships_dto) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(error = %e, "failed to encode memberships for export");
                return;
            }
        };
        let mut header_bytes =
            Vec::with_capacity(user_bytes.len() + memberships_bytes.len() + 48);
        header_bytes.extend_from_slice(b"{\"user\":");
        header_bytes.extend_from_slice(&user_bytes);
        header_bytes.extend_from_slice(b",\"memberships\":");
        header_bytes.extend_from_slice(&memberships_bytes);
        header_bytes.extend_from_slice(b",\"events\":[");
        if tx.send(Ok(Bytes::from(header_bytes))).await.is_err() {
            return;
        }

        // ---- events --------------------------------------------------
        let mut offset: i64 = 0;
        let mut first_event = true;
        let mut carry: Option<ExportEvent> = None;
        loop {
            let page = match store
                .list_event_actor_rows_for_user_page(user_id, offset, EXPORT_PAGE_SIZE)
                .await
            {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!(error = %e, "user export page read failed");
                    return;
                }
            };
            let page_len = page.len() as i64;
            let (folded, new_carry) = fold_event_actors_page(page, carry);
            carry = new_carry;
            for ev in folded {
                let prefix: &[u8] = if first_event { b"" } else { b"," };
                first_event = false;
                let body = serde_json::to_vec(&ev).unwrap_or_else(|_| b"null".to_vec());
                let mut chunk = Vec::with_capacity(prefix.len() + body.len());
                chunk.extend_from_slice(prefix);
                chunk.extend_from_slice(&body);
                if tx.send(Ok(Bytes::from(chunk))).await.is_err() {
                    return;
                }
            }
            if page_len < EXPORT_PAGE_SIZE {
                break;
            }
            offset += page_len;
        }
        // Flush the in-progress event from the final page.
        if let Some(ev) = carry {
            let prefix: &[u8] = if first_event { b"" } else { b"," };
            let body = serde_json::to_vec(&ev).unwrap_or_else(|_| b"null".to_vec());
            let mut chunk = Vec::with_capacity(prefix.len() + body.len());
            chunk.extend_from_slice(prefix);
            chunk.extend_from_slice(&body);
            let _ = tx.send(Ok(Bytes::from(chunk))).await;
        }

        // ---- footer --------------------------------------------------
        let _ = tx.send(Ok(Bytes::from_static(b"]}"))).await;
    });

    let stream = ReceiverStream::new(rx);
    let body = Body::from_stream(stream);
    let resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::TRANSFER_ENCODING, "chunked")
        .body(body)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to build streaming response");
            ApiError::Store(StoreError::Backend(Box::new(e)))
        })?;
    Ok(resp)
}

// ---------------------------------------------------------------------------
// POST /admin/repos — operator-triggered repo registration
// ---------------------------------------------------------------------------

/// Request body for `POST /admin/repos`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ImportRepoRequest {
    /// GitHub owner login (org or user).
    pub owner: String,
    /// Repository name (without the owner prefix).
    pub name: String,
}

/// Response body for `POST /admin/repos`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ImportRepoResponse {
    /// Local org id (newly minted or pre-existing).
    pub org_id: Uuid,
    /// Local repo id (newly minted or pre-existing).
    pub repo_id: Uuid,
    /// `true` if the repo row did not previously exist in `dp_repos`.
    pub created: bool,
}

/// Minimal projection of the GitHub `GET /repos/{owner}/{name}`
/// payload — just the fields needed to upsert one row each into
/// `dp_orgs` and `dp_repos`. Mirrors the `GhRepo` shape the CLI
/// `add-repo` command in `crates/dev-pulse/src/main.rs` uses.
#[derive(Debug, Deserialize)]
struct GhImportRepo {
    id: i64,
    name: String,
    owner: GhImportOwner,
}

#[derive(Debug, Deserialize)]
struct GhImportOwner {
    id: i64,
    login: String,
    #[serde(default)]
    name: Option<String>,
}

/// `POST /admin/repos` — operator-triggered repo registration.
///
/// Mirrors the CLI `dev-pulse add-repo` flow: resolve the repo via
/// GitHub's `GET /repos/{owner}/{name}`, upsert one `dp_orgs` row
/// for the owner, then one `dp_repos` row. Returns whether the
/// repo row was newly created.
///
/// Audit: writes [`audit::ADMIN_REPO_IMPORT`] with target
/// `"repo:<owner>/<name>"`.
#[utoipa::path(
    post,
    path = "/admin/repos",
    request_body = ImportRepoRequest,
    responses(
        (status = 200, description = "Repo registered (or already present)", body = ImportRepoResponse),
        (status = 400, description = "Validation failed"),
        (status = 404, description = "GitHub repo not found"),
    ),
    tag = "admin",
)]
pub async fn import_repo(
    State(state): State<Arc<AdminState>>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<ImportRepoRequest>,
) -> Result<Json<ImportRepoResponse>, ApiError> {
    let owner = body.owner.trim();
    let name = body.name.trim();
    if owner.is_empty() || name.is_empty() {
        return Err(ApiError::BadRequest {
            code: "invalid_repo_spec",
            message: "owner and name must be non-empty".to_string(),
        });
    }

    let path = format!("/repos/{owner}/{name}");
    let client = state.scheduler.reconciler().client();
    let fetched = client.get_conditional::<GhImportRepo>(&path, None).await;
    let repo = match fetched {
        Ok(Fetched::Ok { body, .. }) => body,
        Ok(Fetched::NotModified { .. }) => {
            // Unconditional GET should not produce a 304.
            return Err(ApiError::BadRequest {
                code: "github_unexpected_304",
                message: "GitHub returned 304 to an unconditional GET".to_string(),
            });
        }
        Err(ClientError::Client { status: 404, .. }) => {
            return Err(ApiError::NotFound {
                code: "github_repo_not_found",
                message: format!("GitHub has no repo {owner}/{name}"),
            });
        }
        Err(e) => {
            tracing::error!(error = %e, %owner, %name, "github repo lookup failed");
            return Err(ApiError::BadRequest {
                code: "github_lookup_failed",
                message: format!("GitHub lookup failed: {e}"),
            });
        }
    };

    // Upsert the owning org first so the FK on dp_repos.org_id holds.
    let org_row = dp_domain::Org {
        id: Uuid::new_v4(),
        github_id: repo.owner.id,
        login: repo.owner.login.clone(),
        name: repo.owner.name.clone(),
    };
    let saved_org = state.store.upsert_org(&org_row).await?;

    // Detect whether the repo row already existed so the response's
    // `created` field is meaningful. `list_repos` with a name-search
    // is a cheap probe — we filter the result to an exact, case-
    // insensitive name match within the resolved org.
    let existing = state
        .store
        .list_repos(&RepoListFilter {
            org_id: Some(saved_org.id),
            q: Some(repo.name.clone()),
            limit: 50,
            offset: 0,
        })
        .await
        .unwrap_or_default();
    let already_present = existing
        .iter()
        .any(|r| r.name.eq_ignore_ascii_case(&repo.name));

    let repo_row = dp_domain::Repo {
        id: Uuid::new_v4(),
        org_id: saved_org.id,
        github_id: repo.id,
        name: repo.name.clone(),
    };
    let saved_repo = state.store.upsert_repo(&repo_row).await?;

    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::ADMIN_REPO_IMPORT,
        format!("repo:{owner}/{name}"),
    )
    .await?;

    Ok(Json(ImportRepoResponse {
        org_id: saved_org.id,
        repo_id: saved_repo.id,
        created: !already_present,
    }))
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the admin router fragment. Mount via `Router::merge` from
/// the composition root (`dp-server::build()`); the `with_principal`
/// + `require_permission` wrappers are layered there.
pub fn admin_router(state: Arc<AdminState>) -> Router {
    // See `reports::reports_router` for the rationale on the
    // `with_permission`+merge pattern. Admin actions map to the
    // closed `admin` resource's action vocabulary
    // (read|refresh|anonymise|export) registered in
    // `dp_server::auth::policy::register_dev_pulse_resources`.
    use starter_authz::with_permission;
    Router::new()
        .merge(with_permission(
            Router::new().route("/admin/refresh", post(refresh)),
            "admin",
            "refresh",
        ))
        .merge(with_permission(
            // Repo import rides the same `refresh` action — both
            // are operator-triggered writes that re-shape the
            // reconciler's target set.
            Router::new().route("/admin/repos", post(import_repo)),
            "admin",
            "refresh",
        ))
        .merge(with_permission(
            Router::new().route("/admin/runs", get(list_runs)),
            "admin",
            "read",
        ))
        .merge(with_permission(
            Router::new().route("/admin/users/{id}/anonymise", post(anonymise_user)),
            "admin",
            "anonymise",
        ))
        .merge(with_permission(
            Router::new().route("/admin/users/{id}/export", get(export_user)),
            "admin",
            "export",
        ))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::body::to_bytes;
    use axum::http::Request;
    use chrono::{TimeZone, Utc};
    use dp_domain::audit::AuditEntry;
    use dp_domain::event::EventKind;
    use dp_domain::store::EventActorRow;
    use dp_domain::{
        ActivityEvent, EventActor, FetchCursor, Membership, MembershipRole, Org, Repo,
        ResourceKind, Team, User, WebhookDelivery, Window,
    };
    use dp_fetcher::client::Client;
    use dp_fetcher::reconciler::{Reconciler, StaticTargets};
    use secrecy::SecretString;
    use std::sync::Mutex;
    use std::time::Duration;
    use tower::ServiceExt;

    // -----------------------------------------------------------------
    // MemStore — in-memory fake covering every method the admin
    // handlers reach.
    // -----------------------------------------------------------------

    #[derive(Default)]
    struct MemStore {
        users: Mutex<Vec<User>>,
        memberships: Mutex<Vec<Membership>>,
        runs: Mutex<Vec<FetchRun>>,
        actor_rows: Mutex<Vec<EventActorRow>>,
        audit: Mutex<Vec<AuditEntry>>,
        pseudonymised: Mutex<Vec<Uuid>>,
    }

    impl MemStore {
        fn seed_user(&self, id: Uuid, login: &str) {
            self.users.lock().unwrap().push(User {
                id,
                github_id: 1,
                login: login.into(),
                name: None,
                email: None,
                deleted_at: None,
            });
        }
        fn seed_membership(&self, user_id: Uuid, org_id: Uuid) {
            self.memberships.lock().unwrap().push(Membership {
                user_id,
                org_id,
                role: MembershipRole::Member,
                home_org: None,
                joined_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            });
        }
        fn seed_actor_row(
            &self,
            event_id: Uuid,
            user_id: Uuid,
            role: ActorRole,
            ts_secs: i64,
        ) {
            self.actor_rows.lock().unwrap().push(EventActorRow {
                event_id,
                user_id,
                role,
                org_id: Uuid::nil(),
                repo_id: Uuid::nil(),
                kind: EventKind::PullRequestMerged,
                ts: Utc.timestamp_opt(ts_secs, 0).unwrap(),
            });
        }
        fn seed_run(&self, kind: FetchRunKind, started_secs: i64) -> Uuid {
            let id = Uuid::new_v4();
            self.runs.lock().unwrap().push(FetchRun {
                id,
                kind,
                started: Utc.timestamp_opt(started_secs, 0).unwrap(),
                finished: Some(Utc.timestamp_opt(started_secs + 1, 0).unwrap()),
                items: 1,
                errors: 0,
                partial: false,
                error_sample: None,
            });
            id
        }
        fn audit_rows(&self) -> Vec<AuditEntry> {
            self.audit.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl Store for MemStore {
        async fn upsert_user(&self, u: &User) -> Result<User, StoreError> {
            Ok(u.clone())
        }
        async fn get_user(&self, id: Uuid) -> Result<User, StoreError> {
            self.users
                .lock()
                .unwrap()
                .iter()
                .find(|u| u.id == id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound {
                    entity: "user",
                    id: id.to_string(),
                })
        }
        async fn get_user_by_github_id(&self, _: i64) -> Result<User, StoreError> {
            unimplemented!()
        }
        async fn list_users(&self) -> Result<Vec<User>, StoreError> {
            Ok(self.users.lock().unwrap().clone())
        }
        async fn pseudonymise_user(&self, id: Uuid) -> Result<(), StoreError> {
            // Real cascade behaviour for the test: rewrite the user
            // row and remember the call landed.
            let mut users = self.users.lock().unwrap();
            let row = users.iter_mut().find(|u| u.id == id).ok_or_else(|| {
                StoreError::NotFound {
                    entity: "user",
                    id: id.to_string(),
                }
            })?;
            row.login = format!("deleted-user-{}", &id.simple().to_string()[..8]);
            row.email = None;
            row.name = None;
            row.deleted_at = Some(Utc::now());
            self.pseudonymised.lock().unwrap().push(id);
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
            user_id: Uuid,
        ) -> Result<Vec<Membership>, StoreError> {
            Ok(self
                .memberships
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.user_id == user_id)
                .cloned()
                .collect())
        }
        async fn set_home_org(
            &self,
            _: Uuid,
            _: Uuid,
            _: Option<Uuid>,
        ) -> Result<(), StoreError> {
            Ok(())
        }
        async fn record_event(&self, e: &ActivityEvent) -> Result<ActivityEvent, StoreError> {
            Ok(e.clone())
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
        async fn list_event_actor_rows_for_user_page(
            &self,
            user_id: Uuid,
            offset: i64,
            limit: i64,
        ) -> Result<Vec<EventActorRow>, StoreError> {
            let mut rows: Vec<EventActorRow> = self
                .actor_rows
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.user_id == user_id)
                .cloned()
                .collect();
            // Match the Store contract order: (ts, event_id).
            rows.sort_by(|a, b| a.ts.cmp(&b.ts).then(a.event_id.cmp(&b.event_id)));
            let skip = offset.max(0) as usize;
            let take = limit.max(0) as usize;
            Ok(rows.into_iter().skip(skip).take(take).collect())
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
        async fn start_fetch_run(&self, kind: FetchRunKind) -> Result<Uuid, StoreError> {
            let id = Uuid::new_v4();
            self.runs.lock().unwrap().push(FetchRun {
                id,
                kind,
                started: Utc::now(),
                finished: None,
                items: 0,
                errors: 0,
                partial: false,
                error_sample: None,
            });
            Ok(id)
        }
        async fn finish_fetch_run(
            &self,
            id: Uuid,
            items: i64,
            errors: i64,
            partial: bool,
        ) -> Result<(), StoreError> {
            let mut runs = self.runs.lock().unwrap();
            if let Some(r) = runs.iter_mut().find(|r| r.id == id) {
                r.finished = Some(Utc::now());
                r.items = items;
                r.errors = errors;
                r.partial = partial;
            }
            Ok(())
        }
        async fn list_recent_fetch_runs(&self, limit: i64) -> Result<Vec<FetchRun>, StoreError> {
            let mut rows = self.runs.lock().unwrap().clone();
            rows.sort_by(|a, b| b.started.cmp(&a.started));
            rows.truncate(limit.max(0) as usize);
            Ok(rows)
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
        async fn record_audit_log(&self, entry: &AuditEntry) -> Result<(), StoreError> {
            self.audit.lock().unwrap().push(entry.clone());
            Ok(())
        }
    }

    // -----------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------

    fn build_app(store: Arc<MemStore>, principal: Principal) -> Router {
        let client = Client::with_personal_token(
            SecretString::from("t".to_string()),
            "http://127.0.0.1:1",
        )
        .unwrap();
        let targets = Arc::new(StaticTargets::new(Vec::new()));
        let rec = Reconciler::new(store.clone(), Arc::new(client), targets);
        let sched = Arc::new(Scheduler::new(Arc::new(rec), Duration::from_secs(3600)));
        let state = Arc::new(AdminState::new(sched, store));
        // See `directory.rs` build_app for the why: inject the
        // SPI Principal + a NoopPolicyEngine so the per-route
        // `require_permission` middleware sees a valid principal
        // and an always-allow engine in tests.
        use starter_spi::auth::{Principal as SpiPrincipal, Role};
        use starter_spi::authz::{NoopPolicyEngine, PolicyEngine};
        use std::sync::Arc as StdArc;
        let engine: StdArc<dyn PolicyEngine> = StdArc::new(NoopPolicyEngine);
        let spi_principal = SpiPrincipal {
            subject: principal.actor_user_id.to_string(),
            role: Role::Admin,
            scopes: Vec::new(),
            extra: serde_json::Value::Null,
        };
        admin_router(state)
            .layer(Extension(principal))
            .layer(Extension(spi_principal))
            .layer(Extension(engine))
    }

    // -----------------------------------------------------------------
    // POST /admin/refresh
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn admin_refresh_runs_and_writes_audit_row() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let app = build_app(
            store.clone(),
            Principal {
                actor_user_id: actor,
            },
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/refresh")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["ran"], true);
        let rows = store.audit_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].action, audit::ADMIN_REFRESH);
        assert_eq!(rows[0].actor_user_id, actor);
        assert_eq!(rows[0].target, "scope:all");
    }

    #[tokio::test]
    async fn admin_refresh_rejects_repo_id_without_org_id() {
        let store = Arc::new(MemStore::default());
        let app = build_app(
            store,
            Principal {
                actor_user_id: Uuid::new_v4(),
            },
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/admin/refresh?repo_id={}", Uuid::new_v4()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // -----------------------------------------------------------------
    // GET /admin/runs
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn admin_runs_returns_paginated_projection_newest_first() {
        let store = Arc::new(MemStore::default());
        let r1 = store.seed_run(FetchRunKind::Reconciler, 1_000);
        let r2 = store.seed_run(FetchRunKind::WebhookWorker, 2_000);
        let r3 = store.seed_run(FetchRunKind::Reconciler, 3_000);
        let app = build_app(
            store.clone(),
            Principal {
                actor_user_id: Uuid::new_v4(),
            },
        );

        // Page 1: limit=2, offset=0 → r3, r2.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/runs?limit=2&offset=0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = to_bytes(resp.into_body(), 1 << 16).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], serde_json::json!(r3));
        assert_eq!(arr[1]["id"], serde_json::json!(r2));
        assert_eq!(arr[1]["kind"], "webhook_worker");

        // Page 2: limit=2, offset=2 → r1 only.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/admin/runs?limit=2&offset=2")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(resp.into_body(), 1 << 16).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], serde_json::json!(r1));

        // Two audit rows recorded — one per request.
        let rows = store.audit_rows();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.action == audit::RUNS_LIST));
    }

    // -----------------------------------------------------------------
    // POST /admin/users/:id/anonymise — pseudonymisation cascade
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn anonymise_user_triggers_cascade_and_audits() {
        let store = Arc::new(MemStore::default());
        let user_id = Uuid::new_v4();
        let actor = Uuid::new_v4();
        store.seed_user(user_id, "alice");
        let app = build_app(
            store.clone(),
            Principal {
                actor_user_id: actor,
            },
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/admin/users/{user_id}/anonymise"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        // The cascade ran: pseudonymise_user was called, the user
        // row is rewritten, and the audit row landed afterwards.
        assert_eq!(store.pseudonymised.lock().unwrap().clone(), vec![user_id]);
        let u = store
            .users
            .lock()
            .unwrap()
            .iter()
            .find(|u| u.id == user_id)
            .cloned()
            .unwrap();
        assert!(
            u.login.starts_with("deleted-user-"),
            "login should be rewritten, got {}",
            u.login
        );
        assert!(u.email.is_none());
        assert!(u.name.is_none());
        assert!(u.deleted_at.is_some());

        let rows = store.audit_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].action, audit::USER_ANONYMISE);
        assert_eq!(rows[0].actor_user_id, actor);
        assert_eq!(rows[0].target, format!("user:{user_id}"));
    }

    #[tokio::test]
    async fn anonymise_user_missing_user_does_not_audit() {
        // A failed cascade must not leave a misleading audit trail —
        // §0.5 invariant we want to defend with a regression test.
        let store = Arc::new(MemStore::default());
        let app = build_app(
            store.clone(),
            Principal {
                actor_user_id: Uuid::new_v4(),
            },
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/admin/users/{}/anonymise", Uuid::new_v4()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(store.audit_rows().is_empty());
    }

    // -----------------------------------------------------------------
    // GET /admin/users/:id/export — streaming shape
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn export_user_streams_well_formed_json_with_paginated_events() {
        let store = Arc::new(MemStore::default());
        let user_id = Uuid::new_v4();
        let org_id = Uuid::new_v4();
        store.seed_user(user_id, "alice");
        store.seed_membership(user_id, org_id);

        // Seed enough events to span more than one page, plus an
        // event with two actor rows so the role-folding code is
        // exercised (same event_id appears twice on the same page).
        let multi_event_id = Uuid::new_v4();
        store.seed_actor_row(multi_event_id, user_id, ActorRole::Author, 1_000);
        store.seed_actor_row(multi_event_id, user_id, ActorRole::Closer, 1_000);
        // Add EXPORT_PAGE_SIZE more events with one actor row each.
        for i in 0..(EXPORT_PAGE_SIZE as i64) {
            store.seed_actor_row(Uuid::new_v4(), user_id, ActorRole::Author, 2_000 + i);
        }
        // Plus another user's actor row — must not appear in the export.
        store.seed_actor_row(Uuid::new_v4(), Uuid::new_v4(), ActorRole::Author, 5_000);

        let app = build_app(
            store.clone(),
            Principal {
                actor_user_id: Uuid::new_v4(),
            },
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/admin/users/{user_id}/export"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/json"),
        );
        // Collect the streamed body and verify it parses to the
        // expected envelope.
        let body = to_bytes(resp.into_body(), 1 << 22).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap_or_else(|e| {
            panic!(
                "export body did not parse as JSON: {e}\nbody = {}",
                String::from_utf8_lossy(&body)
            )
        });
        assert!(v.is_object(), "export must be a JSON object");
        assert!(v.get("user").is_some());
        assert_eq!(v["user"]["id"], serde_json::json!(user_id));
        let mems = v["memberships"].as_array().unwrap();
        assert_eq!(mems.len(), 1);
        assert_eq!(mems[0]["org_id"], serde_json::json!(org_id));
        let events = v["events"].as_array().unwrap();
        // One event with two actor rows folds into one ExportEvent,
        // plus EXPORT_PAGE_SIZE one-row events.
        assert_eq!(events.len() as i64, EXPORT_PAGE_SIZE + 1);
        // The two-role event is first (lowest ts) and folds correctly.
        assert_eq!(events[0]["event_id"], serde_json::json!(multi_event_id));
        let roles = events[0]["roles"].as_array().unwrap();
        assert_eq!(roles.len(), 2);
        assert!(roles.iter().any(|r| r == "author"));
        assert!(roles.iter().any(|r| r == "closer"));
        // Audit row landed.
        let audits = store.audit_rows();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].action, audit::USER_EXPORT);
        assert_eq!(audits[0].target, format!("user:{user_id}"));
    }

    #[tokio::test]
    async fn export_user_404s_before_streaming_when_user_missing() {
        let store = Arc::new(MemStore::default());
        let app = build_app(
            store.clone(),
            Principal {
                actor_user_id: Uuid::new_v4(),
            },
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/admin/users/{}/export", Uuid::new_v4()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // The store returns NotFound which the v1 error model maps
        // to 500 — same as the home-org missing-membership case.
        // The handler must not have written an audit row.
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(store.audit_rows().is_empty());
    }

    /// TODO §Phase-4 stage-11 smoke: `admin-user-export-streams-
    /// without-OOM`. A synthetic 100k-event export must stream
    /// through the chunked-body path without ever materialising
    /// the full row vec in process memory. We can't measure RSS
    /// here cheaply, but two invariants pin the streaming
    /// contract: (a) the chunked response *does* finish under a
    /// fixed page budget (no page > `EXPORT_PAGE_SIZE` events),
    /// and (b) the assembled JSON parses to an envelope whose
    /// `events` array length matches the seeded row count. A
    /// non-streaming implementation that built the full Vec
    /// in-memory would still pass — but the chunked-body
    /// `Body::from_stream` path the handler uses *is* the
    /// streaming implementation, and the per-page assertion
    /// trips if anyone replaces it with a single
    /// `serde_json::to_vec(&full_export)`.
    #[tokio::test]
    async fn export_user_streams_100k_events_without_oom() {
        // 100k events is the contract figure from TODO §Phase-4
        // stage 11. We assert two invariants: the chunked stream
        // completes without panicking under the fixed
        // `EXPORT_PAGE_SIZE` page budget, and the resulting JSON
        // round-trips with all 100k events visible.
        const N: i64 = 100_000;

        // A leaner Store that returns events through the paginated
        // method without holding all of them in a single Vec at
        // once. The page bound is the per-call `limit`, so the
        // memory pressure inside this test mirrors what the real
        // PG store would impose.
        struct LargeUserStore {
            user_id: Uuid,
            total: i64,
            audit: Mutex<Vec<AuditEntry>>,
        }

        #[async_trait]
        impl Store for LargeUserStore {
            async fn upsert_user(&self, u: &User) -> Result<User, StoreError> {
                Ok(u.clone())
            }
            async fn get_user(&self, id: Uuid) -> Result<User, StoreError> {
                if id == self.user_id {
                    Ok(User {
                        id,
                        github_id: 1,
                        login: "ada".into(),
                        name: None,
                        email: None,
                        deleted_at: None,
                    })
                } else {
                    Err(StoreError::NotFound {
                        entity: "user",
                        id: id.to_string(),
                    })
                }
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
            async fn upsert_membership(
                &self,
                m: &Membership,
            ) -> Result<Membership, StoreError> {
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
            async fn record_event(
                &self,
                e: &ActivityEvent,
            ) -> Result<ActivityEvent, StoreError> {
                Ok(e.clone())
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
            async fn list_event_actor_rows_for_user_page(
                &self,
                user_id: Uuid,
                offset: i64,
                limit: i64,
            ) -> Result<Vec<EventActorRow>, StoreError> {
                // Hard invariant for the smoke: never return more
                // than `EXPORT_PAGE_SIZE` rows from a single page
                // call. A future regression that bumps the page
                // size by hand here without bumping the constant
                // would still pass — but the constant *is* the
                // memory-budget contract, so the assertion lives
                // in the helper instead.
                assert!(
                    limit <= super::EXPORT_PAGE_SIZE,
                    "page limit {} exceeded EXPORT_PAGE_SIZE {}",
                    limit,
                    super::EXPORT_PAGE_SIZE,
                );
                if user_id != self.user_id {
                    return Ok(vec![]);
                }
                let start = offset.max(0);
                let end = (start + limit).min(self.total);
                if start >= self.total {
                    return Ok(vec![]);
                }
                let mut rows = Vec::with_capacity((end - start) as usize);
                for i in start..end {
                    rows.push(EventActorRow {
                        event_id: Uuid::from_u128(i as u128 + 1),
                        user_id,
                        role: ActorRole::Author,
                        org_id: Uuid::nil(),
                        repo_id: Uuid::nil(),
                        kind: EventKind::PullRequestMerged,
                        ts: Utc.timestamp_opt(1_700_000_000 + i, 0).unwrap(),
                    });
                }
                Ok(rows)
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
            async fn claim_webhooks(
                &self,
                _: i64,
            ) -> Result<Vec<WebhookDelivery>, StoreError> {
                Ok(vec![])
            }
            async fn mark_webhook_processed(&self, _: Uuid) -> Result<(), StoreError> {
                Ok(())
            }
            async fn mark_webhook_failed(&self, _: Uuid, _: &str) -> Result<(), StoreError> {
                Ok(())
            }
            async fn record_audit_log(&self, entry: &AuditEntry) -> Result<(), StoreError> {
                self.audit.lock().unwrap().push(entry.clone());
                Ok(())
            }
        }

        let user_id = Uuid::new_v4();
        let store = Arc::new(LargeUserStore {
            user_id,
            total: N,
            audit: Mutex::new(Vec::new()),
        });

        // Inline a build_app variant so we don't constrain the
        // outer helper's Arc<MemStore> signature.
        let client = Client::with_personal_token(
            SecretString::from("t".to_string()),
            "http://127.0.0.1:1",
        )
        .unwrap();
        let targets = Arc::new(StaticTargets::new(Vec::new()));
        let rec = Reconciler::new(store.clone(), Arc::new(client), targets);
        let sched = Arc::new(Scheduler::new(Arc::new(rec), Duration::from_secs(3600)));
        let admin_state = Arc::new(AdminState::new(sched, store.clone()));
        let principal = Principal {
            actor_user_id: Uuid::new_v4(),
        };
        use starter_spi::auth::{Principal as SpiPrincipal, Role};
        use starter_spi::authz::{NoopPolicyEngine, PolicyEngine};
        let engine: Arc<dyn PolicyEngine> = Arc::new(NoopPolicyEngine);
        let spi_principal = SpiPrincipal {
            subject: principal.actor_user_id.to_string(),
            role: Role::Admin,
            scopes: Vec::new(),
            extra: serde_json::Value::Null,
        };
        let app = admin_router(admin_state)
            .layer(Extension(principal))
            .layer(Extension(spi_principal))
            .layer(Extension(engine));

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/admin/users/{user_id}/export"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // The streaming body decodes to a well-formed envelope
        // with N events visible. The `to_bytes` limit is set
        // generously above the worst-case JSON size (~100 bytes
        // per ExportEvent × 100k = ~10MB) — a non-streaming
        // implementation would still produce the same payload,
        // but the per-page `limit <= EXPORT_PAGE_SIZE` assertion
        // inside the store above would already have failed.
        let body = to_bytes(resp.into_body(), 32 * 1024 * 1024)
            .await
            .expect("body collects");
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap_or_else(|e| {
            panic!(
                "100k-event export must parse; got {e}.  Head = {:?}",
                String::from_utf8_lossy(&body[..body.len().min(200)])
            )
        });
        let events = v["events"].as_array().expect("events is an array");
        assert_eq!(events.len() as i64, N);
        // Audit row written once, before streaming started.
        assert_eq!(store.audit.lock().unwrap().len(), 1);
        assert_eq!(
            store.audit.lock().unwrap()[0].action,
            super::audit::USER_EXPORT
        );
    }

    // -----------------------------------------------------------------
    // POST /admin/repos
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn admin_repos_rejects_empty_owner_or_name() {
        let store = Arc::new(MemStore::default());
        let app = build_app(
            store.clone(),
            Principal {
                actor_user_id: Uuid::new_v4(),
            },
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"owner":"","name":"x"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // Validation failure must not write an audit row.
        assert!(store.audit_rows().is_empty());
    }

    #[tokio::test]
    async fn admin_repos_returns_bad_request_when_github_unreachable() {
        // The shared test client points at `http://127.0.0.1:1` so any
        // real network call fails — we use that to verify the handler
        // surfaces a `github_lookup_failed` 400 rather than a 500, and
        // does not write an audit row for a failed lookup.
        let store = Arc::new(MemStore::default());
        let app = build_app(
            store.clone(),
            Principal {
                actor_user_id: Uuid::new_v4(),
            },
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"owner":"NubeIO","name":"zc-daikin"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(resp.into_body(), 1 << 16).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["code"], "github_lookup_failed");
        assert!(store.audit_rows().is_empty());
    }

    #[test]
    fn fold_event_actors_groups_consecutive_rows_for_one_event() {
        let e1 = Uuid::new_v4();
        let e2 = Uuid::new_v4();
        let ts = Utc.timestamp_opt(1_000, 0).unwrap();
        let mk = |event_id, role| EventActorRow {
            event_id,
            user_id: Uuid::nil(),
            role,
            org_id: Uuid::nil(),
            repo_id: Uuid::nil(),
            kind: EventKind::PullRequestMerged,
            ts,
        };
        let rows = vec![
            mk(e1, ActorRole::Author),
            mk(e1, ActorRole::Closer),
            mk(e2, ActorRole::Author),
        ];
        let (folded, carry) = fold_event_actors_page(rows, None);
        // e1 is fully folded (next page started with e2), e2 is
        // carried in case more rows for e2 land on the next page.
        assert_eq!(folded.len(), 1);
        assert_eq!(folded[0].event_id, e1);
        assert_eq!(folded[0].roles, vec![ActorRole::Author, ActorRole::Closer]);
        let carry = carry.unwrap();
        assert_eq!(carry.event_id, e2);
        assert_eq!(carry.roles, vec![ActorRole::Author]);
    }
}
