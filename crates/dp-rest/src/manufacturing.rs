//! Manufacturing REST — runs, serialised units, serial allocation,
//! EOL reports + run sign-off, the authenticated `qr.svg` label
//! endpoint, and the TOKEN-GATED PUBLIC unit landing `/u/{id}`
//! (DOCS/ideas/product-manufacturing.md §6 / §7 + LOCKED DECISIONS 1–3).
//!
//! Everything except the public landing is gated by
//! `(manufacturing, read|write)`. The public landing authenticates via
//! an HMAC token (`t=HMAC-SHA256(secret, unit_id)`) instead, returns a
//! lean read-only view, and 404s on a missing/invalid token.

use std::sync::Arc;

use axum::{
    extract::{Extension, Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Json, Response},
    routing::{get, patch, post},
    Router,
};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::eol::{EolResult, EolTestUpsert, RunEolSummary, RunEolSummaryUpsert};
use dp_domain::manufacturing::{
    ManufacturingRun, ProductUnit, RunStatus, RunUpsert, UnitStatus, UnitUpsert, MAX_UNIT_ALLOC,
};
use dp_domain::store::StoreError;

use crate::audit::{self, Principal};
use crate::error::ApiError;
use crate::state::AppState;

type HmacSha256 = Hmac<Sha256>;

// ---------------------------------------------------------------------------
// QR token helpers (§6, LOCKED DECISION #2)
// ---------------------------------------------------------------------------

/// `HMAC-SHA256(secret, unit_id)` as lowercase hex. No expiry so
/// printed labels never break.
fn unit_token(secret: &str, unit_id: Uuid) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(unit_id.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Constant-time verify of a hex token against `unit_id`.
fn verify_unit_token(secret: &str, unit_id: Uuid, token: &str) -> bool {
    let Ok(bytes) = hex::decode(token) else { return false };
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(unit_id.as_bytes());
    mac.verify_slice(&bytes).is_ok()
}

/// Compose the public QR URL `{base_url}/u/{id}?t=<token>` when both a
/// base URL and a secret are configured; otherwise `None`.
fn unit_qr_url(state: &AppState, unit_id: Uuid) -> Option<String> {
    let base = state.public_base_url.as_deref()?;
    let secret = state.manufacturing_qr_secret.as_deref()?;
    let token = unit_token(secret, unit_id);
    let base = base.trim_end_matches('/');
    Some(format!("{base}/u/{unit_id}?t={token}"))
}

// ---------------------------------------------------------------------------
// DTOs — runs
// ---------------------------------------------------------------------------

/// Wire form of [`RunStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStatusDto {
    /// Planned.
    Planned,
    /// In progress.
    InProgress,
    /// Completed.
    Completed,
    /// Cancelled.
    Cancelled,
}
impl From<RunStatus> for RunStatusDto {
    fn from(s: RunStatus) -> Self {
        match s {
            RunStatus::Planned => Self::Planned,
            RunStatus::InProgress => Self::InProgress,
            RunStatus::Completed => Self::Completed,
            RunStatus::Cancelled => Self::Cancelled,
        }
    }
}
impl From<RunStatusDto> for RunStatus {
    fn from(s: RunStatusDto) -> Self {
        match s {
            RunStatusDto::Planned => Self::Planned,
            RunStatusDto::InProgress => Self::InProgress,
            RunStatusDto::Completed => Self::Completed,
            RunStatusDto::Cancelled => Self::Cancelled,
        }
    }
}

/// A manufacturing run on the wire.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RunDto {
    /// Primary key.
    pub id: Uuid,
    /// Parent org.
    pub org_id: Uuid,
    /// Parent product.
    pub product_id: Uuid,
    /// Builder.
    pub manufacturer_id: Option<Uuid>,
    /// Batch / lot code.
    pub run_code: String,
    /// Status.
    pub status: RunStatusDto,
    /// Planned quantity.
    pub qty_planned: i32,
    /// Built quantity.
    pub qty_built: i32,
    /// Passed (latest-outcome) quantity.
    pub qty_passed: i32,
    /// Failed (latest-outcome) quantity.
    pub qty_failed: i32,
    /// Next serial sequence number.
    pub next_serial_seq: i32,
    /// Started.
    pub started_at: Option<DateTime<Utc>>,
    /// Completed.
    pub completed_at: Option<DateTime<Utc>>,
    /// Markdown notes.
    pub notes: Option<String>,
    /// §8.2 CAS counter.
    pub version: i64,
    /// When created.
    pub created_at: DateTime<Utc>,
    /// When last mutated.
    pub updated_at: DateTime<Utc>,
}
impl From<ManufacturingRun> for RunDto {
    fn from(r: ManufacturingRun) -> Self {
        Self {
            id: r.id,
            org_id: r.org_id,
            product_id: r.product_id,
            manufacturer_id: r.manufacturer_id,
            run_code: r.run_code,
            status: r.status.into(),
            qty_planned: r.qty_planned,
            qty_built: r.qty_built,
            qty_passed: r.qty_passed,
            qty_failed: r.qty_failed,
            next_serial_seq: r.next_serial_seq,
            started_at: r.started_at,
            completed_at: r.completed_at,
            notes: r.notes,
            version: r.version,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Wire form of [`UnitStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum UnitStatusDto {
    /// Built.
    Built,
    /// Tested.
    Tested,
    /// Shipped.
    Shipped,
    /// Returned.
    Returned,
    /// Scrapped.
    Scrapped,
}
impl From<UnitStatus> for UnitStatusDto {
    fn from(s: UnitStatus) -> Self {
        match s {
            UnitStatus::Built => Self::Built,
            UnitStatus::Tested => Self::Tested,
            UnitStatus::Shipped => Self::Shipped,
            UnitStatus::Returned => Self::Returned,
            UnitStatus::Scrapped => Self::Scrapped,
        }
    }
}
impl From<UnitStatusDto> for UnitStatus {
    fn from(s: UnitStatusDto) -> Self {
        match s {
            UnitStatusDto::Built => Self::Built,
            UnitStatusDto::Tested => Self::Tested,
            UnitStatusDto::Shipped => Self::Shipped,
            UnitStatusDto::Returned => Self::Returned,
            UnitStatusDto::Scrapped => Self::Scrapped,
        }
    }
}

/// A serialised unit on the wire. `qr_url` is the composed public QR
/// payload (`{base_url}/u/{id}?t=<token>`) — `None` when no base URL /
/// secret is configured. The client renders the on-screen QR from it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UnitDto {
    /// Primary key (the stable QR payload).
    pub id: Uuid,
    /// Parent org.
    pub org_id: Uuid,
    /// Parent product.
    pub product_id: Uuid,
    /// Parent run.
    pub run_id: Option<Uuid>,
    /// Serial number.
    pub serial_number: String,
    /// Status.
    pub status: UnitStatusDto,
    /// Shipped-to customer.
    pub customer_id: Option<Uuid>,
    /// When built.
    pub built_at: Option<DateTime<Utc>>,
    /// When shipped.
    pub shipped_at: Option<DateTime<Utc>>,
    /// Composed public QR URL (client renders the QR from this).
    pub qr_url: Option<String>,
    /// §8.2 CAS counter.
    pub version: i64,
    /// When created.
    pub created_at: DateTime<Utc>,
    /// When last mutated.
    pub updated_at: DateTime<Utc>,
}

fn to_unit_dto(state: &AppState, u: ProductUnit) -> UnitDto {
    let qr_url = unit_qr_url(state, u.id);
    UnitDto {
        id: u.id,
        org_id: u.org_id,
        product_id: u.product_id,
        run_id: u.run_id,
        serial_number: u.serial_number,
        status: u.status.into(),
        customer_id: u.customer_id,
        built_at: u.built_at,
        shipped_at: u.shipped_at,
        qr_url,
        version: u.version,
        created_at: u.created_at,
        updated_at: u.updated_at,
    }
}

/// Allocation response: the new units + the reserved serial range.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UnitAllocationDto {
    /// Newly created units.
    pub units: Vec<UnitDto>,
    /// First reserved sequence number.
    pub first_seq: i32,
    /// Count allocated.
    pub count: i32,
}

/// Wire form of [`EolResult`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum EolResultDto {
    /// Pass.
    Pass,
    /// Fail.
    Fail,
}
impl From<EolResult> for EolResultDto {
    fn from(r: EolResult) -> Self {
        match r {
            EolResult::Pass => Self::Pass,
            EolResult::Fail => Self::Fail,
        }
    }
}
impl From<EolResultDto> for EolResult {
    fn from(r: EolResultDto) -> Self {
        match r {
            EolResultDto::Pass => Self::Pass,
            EolResultDto::Fail => Self::Fail,
        }
    }
}

/// An EOL report on the wire.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EolReportDto {
    /// Primary key.
    pub id: Uuid,
    /// Parent unit.
    pub unit_id: Uuid,
    /// Pass / fail.
    pub result: EolResultDto,
    /// Test rig / bench id.
    pub station: Option<String>,
    /// Firmware under test.
    pub firmware: Option<String>,
    /// Structured measurements (opaque JSON).
    pub measurements: serde_json::Value,
    /// Notes.
    pub notes: Option<String>,
    /// Free-text station operator.
    pub tested_by: Option<String>,
    /// When tested.
    pub tested_at: DateTime<Utc>,
}
impl From<dp_domain::eol::EolTestReport> for EolReportDto {
    fn from(r: dp_domain::eol::EolTestReport) -> Self {
        Self {
            id: r.id,
            unit_id: r.unit_id,
            result: r.result.into(),
            station: r.station,
            firmware: r.firmware,
            measurements: r.measurements,
            notes: r.notes,
            tested_by: r.tested_by,
            tested_at: r.tested_at,
        }
    }
}

/// A run EOL sign-off summary on the wire.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RunEolSummaryDto {
    /// Parent run.
    pub run_id: Uuid,
    /// Built snapshot.
    pub built_count: i32,
    /// Pass snapshot.
    pub pass_count: i32,
    /// Fail snapshot.
    pub fail_count: i32,
    /// Markdown notes.
    pub notes_md: Option<String>,
    /// Operator who signed off.
    pub signed_by: Option<Uuid>,
    /// When signed off.
    pub signed_at: Option<DateTime<Utc>>,
    /// §8.2 CAS counter.
    pub version: i64,
}
impl From<RunEolSummary> for RunEolSummaryDto {
    fn from(s: RunEolSummary) -> Self {
        Self {
            run_id: s.run_id,
            built_count: s.built_count,
            pass_count: s.pass_count,
            fail_count: s.fail_count,
            notes_md: s.notes_md,
            signed_by: s.signed_by,
            signed_at: s.signed_at,
            version: s.version,
        }
    }
}

// ---- request bodies -------------------------------------------------

/// Create body for `POST /products/{id}/runs`.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct CreateRunRequest {
    /// Parent org.
    pub org_id: Uuid,
    /// Optional builder.
    #[serde(default)]
    pub manufacturer_id: Option<Uuid>,
    /// Batch / lot code.
    pub run_code: String,
    /// Optional status; defaults to `planned`.
    #[serde(default)]
    pub status: Option<RunStatusDto>,
    /// Planned quantity.
    #[serde(default)]
    pub qty_planned: i32,
    /// Markdown notes.
    #[serde(default)]
    pub notes: Option<String>,
}

/// Patch body for `PATCH /runs/{id}` (full upsert of editable fields + CAS).
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct PatchRunRequest {
    /// Observed version.
    pub expected_version: i64,
    /// Optional builder.
    #[serde(default)]
    pub manufacturer_id: Option<Uuid>,
    /// Batch / lot code.
    pub run_code: String,
    /// Status.
    pub status: RunStatusDto,
    /// Planned quantity.
    pub qty_planned: i32,
    /// Started.
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
    /// Completed.
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
    /// Markdown notes.
    #[serde(default)]
    pub notes: Option<String>,
}

/// Body for `POST /runs/{id}/units` — allocate N serialised units.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct AllocateUnitsRequest {
    /// Number of units to allocate (1..=1000, §6).
    pub count: i32,
}

/// Patch body for `PATCH /units/{id}`.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct PatchUnitRequest {
    /// Observed version.
    pub expected_version: i64,
    /// New status.
    pub status: UnitStatusDto,
    /// Shipped-to customer (null clears).
    #[serde(default)]
    pub customer_id: Option<Uuid>,
    /// When built.
    #[serde(default)]
    pub built_at: Option<DateTime<Utc>>,
    /// When shipped.
    #[serde(default)]
    pub shipped_at: Option<DateTime<Utc>>,
}

/// Body for `POST /units/{id}/eol`.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct RecordEolRequest {
    /// Pass / fail.
    pub result: EolResultDto,
    /// Test rig / bench id.
    #[serde(default)]
    pub station: Option<String>,
    /// Firmware under test.
    #[serde(default)]
    pub firmware: Option<String>,
    /// Structured measurements.
    #[serde(default)]
    pub measurements: Option<serde_json::Value>,
    /// Notes.
    #[serde(default)]
    pub notes: Option<String>,
    /// Free-text station operator.
    #[serde(default)]
    pub tested_by: Option<String>,
}

/// Body for `POST /runs/{id}/eol-summary` (sign-off).
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct RunEolSummaryRequest {
    /// Markdown notes.
    #[serde(default)]
    pub notes_md: Option<String>,
    /// When true, stamp the operator sign-off.
    #[serde(default)]
    pub sign_off: bool,
}

/// Lean public unit landing payload (`/u/{id}`, token-gated).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PublicUnitDto {
    /// Serial number.
    pub serial_number: String,
    /// Model number (from the product).
    pub model_number: String,
    /// Product name.
    pub product_name: String,
    /// Unit status.
    pub status: UnitStatusDto,
    /// Published manual titles + revision strings.
    pub manuals: Vec<PublicManualRef>,
}

/// A published-manual reference on the public landing.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PublicManualRef {
    /// Manual title.
    pub title: String,
    /// Published revision string.
    pub revision: String,
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn map_cas(entity: &'static str, id: Uuid, err: StoreError) -> ApiError {
    match err {
        StoreError::NotFound { .. } => ApiError::NotFound {
            code: "manufacturing_not_found",
            message: format!("no {entity} with id {id}"),
        },
        StoreError::Conflict(msg) => ApiError::Conflict { code: "stale_version", message: msg },
        StoreError::Invalid(msg) => ApiError::BadRequest { code: "manufacturing_invalid", message: msg },
        e => e.into(),
    }
}

// ---------------------------------------------------------------------------
// run handlers
// ---------------------------------------------------------------------------

/// `GET /products/{id}/runs`.
#[utoipa::path(get, path = "/products/{id}/runs", params(("id" = Uuid, Path)),
    responses((status = 200, body = [RunDto])), tag = "manufacturing")]
pub async fn list_runs(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<RunDto>>, ApiError> {
    let rows = state.store.list_runs(id).await?;
    Ok(Json(rows.into_iter().map(RunDto::from).collect()))
}

/// `POST /products/{id}/runs`.
#[utoipa::path(post, path = "/products/{id}/runs", params(("id" = Uuid, Path)),
    request_body = CreateRunRequest,
    responses((status = 200, body = RunDto), (status = 409)), tag = "manufacturing")]
pub async fn create_run(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateRunRequest>,
) -> Result<Json<RunDto>, ApiError> {
    if body.run_code.trim().is_empty() {
        return Err(ApiError::BadRequest { code: "run_code_required", message: "run_code must be non-empty".into() });
    }
    if body.qty_planned < 0 {
        return Err(ApiError::BadRequest { code: "qty_planned_invalid", message: "qty_planned must be >= 0".into() });
    }
    // The product must exist (and we use its id as the run parent).
    state.store.get_product(id).await?.ok_or(ApiError::NotFound {
        code: "product_not_found",
        message: "no product with that id".into(),
    })?;
    let u = RunUpsert {
        org_id: body.org_id,
        product_id: id,
        manufacturer_id: body.manufacturer_id,
        run_code: body.run_code.trim().to_string(),
        status: body.status.map(Into::into).unwrap_or(RunStatus::Planned),
        qty_planned: body.qty_planned,
        started_at: None,
        completed_at: None,
        notes: body.notes,
        created_by: Some(principal.actor_user_id),
    };
    let row = state.store.create_run(&u).await.map_err(|e| map_cas("run", Uuid::nil(), e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::RUN_CREATE, format!("{id}:{}", row.id)).await.ok();
    Ok(Json(row.into()))
}

/// `GET /runs/{run_id}`.
#[utoipa::path(get, path = "/runs/{run_id}", params(("run_id" = Uuid, Path)),
    responses((status = 200, body = RunDto), (status = 404)), tag = "manufacturing")]
pub async fn get_run(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<RunDto>, ApiError> {
    let row = state.store.get_run(run_id).await?.ok_or(ApiError::NotFound {
        code: "run_not_found",
        message: "no run with that id".into(),
    })?;
    Ok(Json(row.into()))
}

/// `PATCH /runs/{run_id}`.
#[utoipa::path(patch, path = "/runs/{run_id}", params(("run_id" = Uuid, Path)),
    request_body = PatchRunRequest,
    responses((status = 200, body = RunDto), (status = 404), (status = 409)), tag = "manufacturing")]
pub async fn patch_run(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(run_id): Path<Uuid>,
    Json(body): Json<PatchRunRequest>,
) -> Result<Json<RunDto>, ApiError> {
    if body.run_code.trim().is_empty() {
        return Err(ApiError::BadRequest { code: "run_code_required", message: "run_code must be non-empty".into() });
    }
    // Preserve org_id / product_id from the existing row (immutable).
    let cur = state.store.get_run(run_id).await?.ok_or(ApiError::NotFound {
        code: "run_not_found",
        message: "no run with that id".into(),
    })?;
    let u = RunUpsert {
        org_id: cur.org_id,
        product_id: cur.product_id,
        manufacturer_id: body.manufacturer_id,
        run_code: body.run_code.trim().to_string(),
        status: body.status.into(),
        qty_planned: body.qty_planned,
        started_at: body.started_at,
        completed_at: body.completed_at,
        notes: body.notes,
        created_by: None,
    };
    let row = state.store.update_run(run_id, body.expected_version, &u).await.map_err(|e| map_cas("run", run_id, e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::RUN_UPDATE, run_id.to_string()).await.ok();
    Ok(Json(row.into()))
}

// ---------------------------------------------------------------------------
// unit handlers
// ---------------------------------------------------------------------------

/// `GET /runs/{run_id}/units`.
#[utoipa::path(get, path = "/runs/{run_id}/units", params(("run_id" = Uuid, Path)),
    responses((status = 200, body = [UnitDto])), tag = "manufacturing")]
pub async fn list_run_units(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<Vec<UnitDto>>, ApiError> {
    let rows = state.store.list_run_units(run_id).await?;
    Ok(Json(rows.into_iter().map(|u| to_unit_dto(&state, u)).collect()))
}

/// `POST /runs/{run_id}/units` — allocate N serialised units (§6).
#[utoipa::path(post, path = "/runs/{run_id}/units", params(("run_id" = Uuid, Path)),
    request_body = AllocateUnitsRequest,
    responses((status = 200, body = UnitAllocationDto), (status = 400), (status = 404)), tag = "manufacturing")]
pub async fn allocate_units(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(run_id): Path<Uuid>,
    Json(body): Json<AllocateUnitsRequest>,
) -> Result<Json<UnitAllocationDto>, ApiError> {
    if body.count < 1 || body.count > MAX_UNIT_ALLOC {
        return Err(ApiError::BadRequest {
            code: "alloc_count_invalid",
            message: format!("count must be 1..={MAX_UNIT_ALLOC}"),
        });
    }
    let alloc = state.store.allocate_units(run_id, body.count).await.map_err(|e| map_cas("run", run_id, e))?;
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::RUN_ALLOCATE_UNITS,
        format!("{run_id}:+{}", alloc.count),
    )
    .await
    .ok();
    Ok(Json(UnitAllocationDto {
        units: alloc.units.into_iter().map(|u| to_unit_dto(&state, u)).collect(),
        first_seq: alloc.first_seq,
        count: alloc.count,
    }))
}

/// `GET /units/{unit_id}`.
#[utoipa::path(get, path = "/units/{unit_id}", params(("unit_id" = Uuid, Path)),
    responses((status = 200, body = UnitDto), (status = 404)), tag = "manufacturing")]
pub async fn get_unit(
    State(state): State<AppState>,
    Path(unit_id): Path<Uuid>,
) -> Result<Json<UnitDto>, ApiError> {
    let row = state.store.get_unit(unit_id).await?.ok_or(ApiError::NotFound {
        code: "unit_not_found",
        message: "no unit with that id".into(),
    })?;
    Ok(Json(to_unit_dto(&state, row)))
}

/// `PATCH /units/{unit_id}`.
#[utoipa::path(patch, path = "/units/{unit_id}", params(("unit_id" = Uuid, Path)),
    request_body = PatchUnitRequest,
    responses((status = 200, body = UnitDto), (status = 404), (status = 409)), tag = "manufacturing")]
pub async fn patch_unit(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(unit_id): Path<Uuid>,
    Json(body): Json<PatchUnitRequest>,
) -> Result<Json<UnitDto>, ApiError> {
    let u = UnitUpsert {
        status: body.status.into(),
        customer_id: body.customer_id,
        built_at: body.built_at,
        shipped_at: body.shipped_at,
    };
    let row = state.store.update_unit(unit_id, body.expected_version, &u).await.map_err(|e| map_cas("unit", unit_id, e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::UNIT_UPDATE, unit_id.to_string()).await.ok();
    Ok(Json(to_unit_dto(&state, row)))
}

/// `GET /units/{unit_id}/qr.svg` — crisp SVG QR for printable labels
/// (§6). Composes the same `{base_url}/u/{id}?t=<token>` payload.
#[utoipa::path(get, path = "/units/{unit_id}/qr.svg", params(("unit_id" = Uuid, Path)),
    responses((status = 200, description = "SVG QR code"), (status = 404), (status = 503)), tag = "manufacturing")]
pub async fn unit_qr_svg(
    State(state): State<AppState>,
    Path(unit_id): Path<Uuid>,
) -> Result<Response, ApiError> {
    // Confirm the unit exists before emitting a label.
    state.store.get_unit(unit_id).await?.ok_or(ApiError::NotFound {
        code: "unit_not_found",
        message: "no unit with that id".into(),
    })?;
    let url = unit_qr_url(&state, unit_id).ok_or(ApiError::BadRequest {
        code: "qr_unavailable",
        message: "QR is unavailable: base_url and MANUFACTURING_QR_SECRET must be configured".into(),
    })?;
    let code = qrcode::QrCode::new(url.as_bytes()).map_err(|e| ApiError::BadRequest {
        code: "qr_encode_failed",
        message: format!("could not encode QR: {e}"),
    })?;
    let svg = code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(220, 220)
        .quiet_zone(true)
        .build();
    let mut resp = (StatusCode::OK, svg).into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml; charset=utf-8"),
    );
    Ok(resp)
}

// ---------------------------------------------------------------------------
// EOL handlers
// ---------------------------------------------------------------------------

/// `GET /units/{unit_id}/eol`.
#[utoipa::path(get, path = "/units/{unit_id}/eol", params(("unit_id" = Uuid, Path)),
    responses((status = 200, body = [EolReportDto])), tag = "manufacturing")]
pub async fn list_unit_eol(
    State(state): State<AppState>,
    Path(unit_id): Path<Uuid>,
) -> Result<Json<Vec<EolReportDto>>, ApiError> {
    let rows = state.store.list_unit_eol_reports(unit_id).await?;
    Ok(Json(rows.into_iter().map(EolReportDto::from).collect()))
}

/// `POST /units/{unit_id}/eol` — record a pass/fail report (maintains
/// the run's re-test-safe counters, §5.4).
#[utoipa::path(post, path = "/units/{unit_id}/eol", params(("unit_id" = Uuid, Path)),
    request_body = RecordEolRequest,
    responses((status = 200, body = EolReportDto), (status = 404)), tag = "manufacturing")]
pub async fn record_eol(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(unit_id): Path<Uuid>,
    Json(body): Json<RecordEolRequest>,
) -> Result<Json<EolReportDto>, ApiError> {
    let u = EolTestUpsert {
        result: body.result.into(),
        station: body.station,
        firmware: body.firmware,
        measurements: body.measurements.unwrap_or_else(|| serde_json::json!({})),
        log_blob_ref: None,
        notes: body.notes,
        tested_by: body.tested_by,
    };
    let row = state.store.record_eol_report(unit_id, &u).await.map_err(|e| map_cas("unit", unit_id, e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::EOL_RECORD, format!("{unit_id}:{}", row.id)).await.ok();
    Ok(Json(row.into()))
}

/// `GET /runs/{run_id}/eol-summary`.
#[utoipa::path(get, path = "/runs/{run_id}/eol-summary", params(("run_id" = Uuid, Path)),
    responses((status = 200, body = RunEolSummaryDto), (status = 404)), tag = "manufacturing")]
pub async fn get_run_eol_summary(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<RunEolSummaryDto>, ApiError> {
    let row = state.store.get_run_eol_summary(run_id).await?.ok_or(ApiError::NotFound {
        code: "run_eol_summary_not_found",
        message: "no sign-off summary for that run yet".into(),
    })?;
    Ok(Json(row.into()))
}

/// `POST /runs/{run_id}/eol-summary` — upsert/sign-off (snapshots the
/// run counters; stamps the operator when `sign_off`).
#[utoipa::path(post, path = "/runs/{run_id}/eol-summary", params(("run_id" = Uuid, Path)),
    request_body = RunEolSummaryRequest,
    responses((status = 200, body = RunEolSummaryDto), (status = 404)), tag = "manufacturing")]
pub async fn upsert_run_eol_summary(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(run_id): Path<Uuid>,
    Json(body): Json<RunEolSummaryRequest>,
) -> Result<Json<RunEolSummaryDto>, ApiError> {
    let u = RunEolSummaryUpsert {
        notes_md: body.notes_md,
        sign_off: body.sign_off,
        signed_by: Some(principal.actor_user_id),
    };
    let row = state.store.upsert_run_eol_summary(run_id, &u).await.map_err(|e| map_cas("run", run_id, e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::RUN_EOL_SIGN_OFF, run_id.to_string()).await.ok();
    Ok(Json(row.into()))
}

// ---------------------------------------------------------------------------
// PUBLIC token-gated unit landing — `/u/{id}` (LOCKED DECISION #2)
// ---------------------------------------------------------------------------

/// Query for the public landing — carries the HMAC token.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PublicLandingQuery {
    /// HMAC token `t=HMAC-SHA256(secret, unit_id)`.
    #[serde(default)]
    pub t: Option<String>,
}

/// Resolve the lean public payload for a unit, enforcing the token.
/// Returns `None` (→ 404) for missing/invalid token or unknown unit so
/// the route never leaks existence.
async fn resolve_public_unit(
    state: &AppState,
    unit_id: Uuid,
    token: Option<&str>,
) -> Option<PublicUnitDto> {
    let secret = state.manufacturing_qr_secret.as_deref()?;
    let token = token?;
    if !verify_unit_token(secret, unit_id, token) {
        return None;
    }
    let unit = state.store.get_unit(unit_id).await.ok()??;
    let product = state.store.get_product(unit.product_id).await.ok()??;
    let manuals = state
        .store
        .list_published_manuals_for_product(unit.product_id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(m, r)| PublicManualRef { title: m.title, revision: r.revision })
        .collect();
    Some(PublicUnitDto {
        serial_number: unit.serial_number,
        model_number: product.model_number,
        product_name: product.name,
        status: unit.status.into(),
        manuals,
    })
}

fn public_not_found() -> ApiError {
    // Uniform 404 for missing/invalid token OR unknown unit.
    ApiError::NotFound { code: "not_found", message: "not found".into() }
}

/// `GET /u/{id}` — PUBLIC, token-gated lean unit landing. Returns a
/// self-contained HTML page (phone-scan friendly), or JSON when the
/// caller sends `Accept: application/json`. Mounted OUTSIDE the auth
/// middleware. Missing/invalid token → 404.
pub async fn public_unit_landing(
    State(state): State<AppState>,
    Path(unit_id): Path<Uuid>,
    Query(q): Query<PublicLandingQuery>,
    headers: axum::http::HeaderMap,
) -> Result<Response, ApiError> {
    let dto = resolve_public_unit(&state, unit_id, q.t.as_deref())
        .await
        .ok_or_else(public_not_found)?;
    let wants_json = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|a| a.contains("application/json"))
        .unwrap_or(false);
    if wants_json {
        return Ok(Json(dto).into_response());
    }
    Ok(Html(render_landing_html(&dto)).into_response())
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn render_landing_html(d: &PublicUnitDto) -> String {
    let manuals = if d.manuals.is_empty() {
        "<p class=\"muted\">No published manuals.</p>".to_string()
    } else {
        let items: String = d
            .manuals
            .iter()
            .map(|m| format!("<li>{} <span class=\"muted\">(rev {})</span></li>", esc(&m.title), esc(&m.revision)))
            .collect();
        format!("<ul>{items}</ul>")
    };
    let status = format!("{:?}", d.status).to_lowercase();
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<title>{model} · {serial}</title>
<style>
 body{{font-family:system-ui,-apple-system,Segoe UI,Roboto,sans-serif;margin:0;background:#0b0c10;color:#e8e8ea}}
 .card{{max-width:520px;margin:32px auto;padding:24px;background:#15171c;border-radius:14px;box-shadow:0 6px 24px rgba(0,0,0,.4)}}
 h1{{font-size:1.3rem;margin:0 0 4px}} .sn{{font-family:ui-monospace,monospace;font-size:1.05rem}}
 .row{{display:flex;justify-content:space-between;padding:8px 0;border-bottom:1px solid #23262e}}
 .label{{color:#8b8f9a}} .muted{{color:#8b8f9a}} .badge{{padding:2px 10px;border-radius:999px;background:#26303f;font-size:.85rem}}
 ul{{padding-left:18px}} li{{margin:4px 0}}
</style></head><body><div class="card">
 <h1>{name}</h1>
 <div class="row"><span class="label">Model</span><span>{model}</span></div>
 <div class="row"><span class="label">Serial</span><span class="sn">{serial}</span></div>
 <div class="row"><span class="label">Status</span><span class="badge">{status}</span></div>
 <h2 style="font-size:1rem;margin:18px 0 6px">Manuals</h2>
 {manuals}
 <p class="muted" style="margin-top:18px;font-size:.8rem">Scanned product label.</p>
</div></body></html>"#,
        name = esc(&d.product_name),
        model = esc(&d.model_number),
        serial = esc(&d.serial_number),
        status = esc(&status),
        manuals = manuals,
    )
}

// ---------------------------------------------------------------------------
// routers
// ---------------------------------------------------------------------------

/// Authenticated manufacturing router, gated by `(manufacturing, read|write)`.
pub fn manufacturing_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new()
                .route("/products/{id}/runs", get(list_runs))
                .route("/runs/{run_id}", get(get_run))
                .route("/runs/{run_id}/units", get(list_run_units))
                .route("/runs/{run_id}/eol-summary", get(get_run_eol_summary))
                .route("/units/{unit_id}", get(get_unit))
                .route("/units/{unit_id}/qr.svg", get(unit_qr_svg))
                .route("/units/{unit_id}/eol", get(list_unit_eol)),
            "manufacturing",
            "read",
        ))
        .merge(with_permission(
            Router::new()
                .route("/products/{id}/runs", post(create_run))
                .route("/runs/{run_id}", patch(patch_run))
                .route("/runs/{run_id}/units", post(allocate_units))
                .route("/runs/{run_id}/eol-summary", post(upsert_run_eol_summary))
                .route("/units/{unit_id}", patch(patch_unit))
                .route("/units/{unit_id}/eol", post(record_eol)),
            "manufacturing",
            "write",
        ))
        .with_state(inner)
}

/// PUBLIC router for the token-gated unit landing. Mounted OUTSIDE the
/// `with_principal` auth layer (LOCKED DECISION #2) — auth is the HMAC
/// token, not a session.
pub fn manufacturing_public_router(state: Arc<AppState>) -> Router {
    let inner: AppState = (*state).clone();
    Router::new()
        .route("/u/{id}", get(public_unit_landing))
        .with_state(inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_round_trips_and_rejects_tampering() {
        let secret = "test-secret";
        let id = Uuid::from_u128(0x1234_5678_9abc_def0_1234_5678_9abc_def0);
        let tok = unit_token(secret, id);
        assert!(verify_unit_token(secret, id, &tok));
        // wrong id
        assert!(!verify_unit_token(secret, Uuid::nil(), &tok));
        // wrong secret
        assert!(!verify_unit_token("other", id, &tok));
        // tampered token
        assert!(!verify_unit_token(secret, id, "deadbeef"));
        assert!(!verify_unit_token(secret, id, "not-hex"));
    }
}
