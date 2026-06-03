//! Returns / RMA REST (DOCS/ideas/product-manufacturing.md §5.5).
//!
//! All routes are gated by `(manufacturing, read|write)` — RMA is part
//! of the manufacturing surface, so it shares the resource. Create
//! validates product / unit parentage in the store; the unique
//! `(org, rma_number)` index surfaces as a 409.

use std::sync::Arc;

use axum::{
    extract::{Extension, Path, Query, State},
    response::Json,
    routing::{get, patch, post},
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::rma::{Rma, RmaCreate, RmaFilter, RmaStatus, RmaUpdate};
use dp_domain::store::StoreError;

use crate::audit::{self, Principal};
use crate::error::ApiError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

/// Wire form of [`RmaStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RmaStatusDto {
    /// Open.
    Open,
    /// Received.
    Received,
    /// Diagnosed.
    Diagnosed,
    /// Repaired.
    Repaired,
    /// Replaced.
    Replaced,
    /// Rejected.
    Rejected,
    /// Closed.
    Closed,
}
impl From<RmaStatus> for RmaStatusDto {
    fn from(s: RmaStatus) -> Self {
        match s {
            RmaStatus::Open => Self::Open,
            RmaStatus::Received => Self::Received,
            RmaStatus::Diagnosed => Self::Diagnosed,
            RmaStatus::Repaired => Self::Repaired,
            RmaStatus::Replaced => Self::Replaced,
            RmaStatus::Rejected => Self::Rejected,
            RmaStatus::Closed => Self::Closed,
        }
    }
}
impl From<RmaStatusDto> for RmaStatus {
    fn from(s: RmaStatusDto) -> Self {
        match s {
            RmaStatusDto::Open => Self::Open,
            RmaStatusDto::Received => Self::Received,
            RmaStatusDto::Diagnosed => Self::Diagnosed,
            RmaStatusDto::Repaired => Self::Repaired,
            RmaStatusDto::Replaced => Self::Replaced,
            RmaStatusDto::Rejected => Self::Rejected,
            RmaStatusDto::Closed => Self::Closed,
        }
    }
}

/// An RMA on the wire.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RmaDto {
    /// Primary key.
    pub id: Uuid,
    /// Parent org.
    pub org_id: Uuid,
    /// Optional serialised unit.
    pub unit_id: Option<Uuid>,
    /// Parent product.
    pub product_id: Uuid,
    /// Optional customer.
    pub customer_id: Option<Uuid>,
    /// RMA number.
    pub rma_number: String,
    /// Warranty flag.
    pub under_warranty: bool,
    /// Status.
    pub status: RmaStatusDto,
    /// Customer-reported reason.
    pub reason: Option<String>,
    /// Diagnosis notes.
    pub diagnosis: Option<String>,
    /// Resolution notes.
    pub resolution: Option<String>,
    /// When received.
    pub received_at: Option<DateTime<Utc>>,
    /// When resolved.
    pub resolved_at: Option<DateTime<Utc>>,
    /// §8.2 CAS counter.
    pub version: i64,
    /// When created.
    pub created_at: DateTime<Utc>,
    /// When last mutated.
    pub updated_at: DateTime<Utc>,
}
impl From<Rma> for RmaDto {
    fn from(r: Rma) -> Self {
        Self {
            id: r.id,
            org_id: r.org_id,
            unit_id: r.unit_id,
            product_id: r.product_id,
            customer_id: r.customer_id,
            rma_number: r.rma_number,
            under_warranty: r.under_warranty,
            status: r.status.into(),
            reason: r.reason,
            diagnosis: r.diagnosis,
            resolution: r.resolution,
            received_at: r.received_at,
            resolved_at: r.resolved_at,
            version: r.version,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

// ---- request bodies / query -----------------------------------------

/// Query for `GET /rma` — each present field narrows the result set.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct ListRmaQuery {
    /// Scope to an org.
    #[serde(default)]
    pub org_id: Option<Uuid>,
    /// Scope to a status.
    #[serde(default)]
    pub status: Option<RmaStatusDto>,
    /// Scope to a product.
    #[serde(default)]
    pub product_id: Option<Uuid>,
    /// Scope to a customer.
    #[serde(default)]
    pub customer_id: Option<Uuid>,
    /// Scope to a unit.
    #[serde(default)]
    pub unit_id: Option<Uuid>,
}

/// Create body for `POST /rma`. Status defaults to `open`.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct CreateRmaRequest {
    /// Parent org.
    pub org_id: Uuid,
    /// Parent product.
    pub product_id: Uuid,
    /// Optional serialised unit.
    #[serde(default)]
    pub unit_id: Option<Uuid>,
    /// Optional customer.
    #[serde(default)]
    pub customer_id: Option<Uuid>,
    /// RMA number.
    pub rma_number: String,
    /// Warranty flag (defaults false).
    #[serde(default)]
    pub under_warranty: bool,
    /// Customer-reported reason.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Patch body for `PATCH /rma/{id}` (full upsert of editable fields + CAS).
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct PatchRmaRequest {
    /// Observed version.
    pub expected_version: i64,
    /// Optional serialised unit.
    #[serde(default)]
    pub unit_id: Option<Uuid>,
    /// Optional customer.
    #[serde(default)]
    pub customer_id: Option<Uuid>,
    /// Warranty flag.
    pub under_warranty: bool,
    /// Status.
    pub status: RmaStatusDto,
    /// Customer-reported reason.
    #[serde(default)]
    pub reason: Option<String>,
    /// Diagnosis notes.
    #[serde(default)]
    pub diagnosis: Option<String>,
    /// Resolution notes.
    #[serde(default)]
    pub resolution: Option<String>,
    /// When received.
    #[serde(default)]
    pub received_at: Option<DateTime<Utc>>,
    /// When resolved.
    #[serde(default)]
    pub resolved_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn map_cas(entity: &'static str, id: Uuid, err: StoreError) -> ApiError {
    match err {
        StoreError::NotFound { .. } => ApiError::NotFound {
            code: "rma_not_found",
            message: format!("no {entity} with id {id}"),
        },
        StoreError::Conflict(msg) => ApiError::Conflict { code: "stale_version", message: msg },
        StoreError::Invalid(msg) => ApiError::BadRequest { code: "rma_invalid", message: msg },
        e => e.into(),
    }
}

// ---------------------------------------------------------------------------
// handlers
// ---------------------------------------------------------------------------

/// `GET /rma`.
#[utoipa::path(get, path = "/rma",
    params(
        ("org_id" = Option<Uuid>, Query),
        ("status" = Option<RmaStatusDto>, Query),
        ("product_id" = Option<Uuid>, Query),
        ("customer_id" = Option<Uuid>, Query),
        ("unit_id" = Option<Uuid>, Query),
    ),
    responses((status = 200, body = [RmaDto])), tag = "manufacturing")]
pub async fn list_rma(
    State(state): State<AppState>,
    Query(q): Query<ListRmaQuery>,
) -> Result<Json<Vec<RmaDto>>, ApiError> {
    let filter = RmaFilter {
        org_id: q.org_id,
        status: q.status.map(Into::into),
        product_id: q.product_id,
        customer_id: q.customer_id,
        unit_id: q.unit_id,
    };
    let rows = state.store.list_rma(&filter).await?;
    Ok(Json(rows.into_iter().map(RmaDto::from).collect()))
}

/// `GET /rma/{id}`.
#[utoipa::path(get, path = "/rma/{id}", params(("id" = Uuid, Path)),
    responses((status = 200, body = RmaDto), (status = 404)), tag = "manufacturing")]
pub async fn get_rma(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RmaDto>, ApiError> {
    let row = state.store.get_rma(id).await?.ok_or(ApiError::NotFound {
        code: "rma_not_found",
        message: "no rma with that id".into(),
    })?;
    Ok(Json(row.into()))
}

/// `POST /rma`.
#[utoipa::path(post, path = "/rma",
    request_body = CreateRmaRequest,
    responses((status = 200, body = RmaDto), (status = 400), (status = 404), (status = 409)), tag = "manufacturing")]
pub async fn create_rma(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<CreateRmaRequest>,
) -> Result<Json<RmaDto>, ApiError> {
    if body.rma_number.trim().is_empty() {
        return Err(ApiError::BadRequest { code: "rma_number_required", message: "rma_number must be non-empty".into() });
    }
    let c = RmaCreate {
        org_id: body.org_id,
        product_id: body.product_id,
        unit_id: body.unit_id,
        customer_id: body.customer_id,
        rma_number: body.rma_number.trim().to_string(),
        under_warranty: body.under_warranty,
        status: RmaStatus::Open,
        reason: body.reason,
        created_by: Some(principal.actor_user_id),
    };
    let row = state.store.create_rma(&c).await.map_err(|e| map_cas("rma", Uuid::nil(), e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::RMA_CREATE, row.id.to_string()).await.ok();
    Ok(Json(row.into()))
}

/// `PATCH /rma/{id}`.
#[utoipa::path(patch, path = "/rma/{id}", params(("id" = Uuid, Path)),
    request_body = PatchRmaRequest,
    responses((status = 200, body = RmaDto), (status = 404), (status = 409)), tag = "manufacturing")]
pub async fn patch_rma(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchRmaRequest>,
) -> Result<Json<RmaDto>, ApiError> {
    let u = RmaUpdate {
        unit_id: body.unit_id,
        customer_id: body.customer_id,
        under_warranty: body.under_warranty,
        status: body.status.into(),
        reason: body.reason,
        diagnosis: body.diagnosis,
        resolution: body.resolution,
        received_at: body.received_at,
        resolved_at: body.resolved_at,
    };
    let row = state.store.update_rma(id, body.expected_version, &u).await.map_err(|e| map_cas("rma", id, e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::RMA_UPDATE, id.to_string()).await.ok();
    Ok(Json(row.into()))
}

// ---------------------------------------------------------------------------
// router
// ---------------------------------------------------------------------------

/// Authenticated RMA router, gated by `(manufacturing, read|write)`.
pub fn rma_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new()
                .route("/rma", get(list_rma))
                .route("/rma/{id}", get(get_rma)),
            "manufacturing",
            "read",
        ))
        .merge(with_permission(
            Router::new()
                .route("/rma", post(create_rma))
                .route("/rma/{id}", patch(patch_rma)),
            "manufacturing",
            "write",
        ))
        .with_state(inner)
}
