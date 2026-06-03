//! Master-data parties REST CRUD — customers, manufacturers,
//! suppliers (DOCS/ideas/product-manufacturing.md §5.1 / §7.3).
//!
//! Three near-identical CRUD surfaces, all gated by the
//! `(manufacturing, read|write)` permission pair. PATCH takes a full
//! upsert body (name required; other fields are the new desired value,
//! `null`/omitted ⇒ cleared) plus an `expected_version` for §8.2 CAS.
//! Archive is idempotent.

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

use dp_domain::party::{
    Customer, CustomerUpsert, Manufacturer, ManufacturerUpsert, PartyListFilter, Supplier,
    SupplierUpsert,
};
use dp_domain::store::{StoreError, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT};

use crate::audit::{self, Principal};
use crate::error::ApiError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

/// Wire form of a manufacturer / supplier row (identical shape).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PartyDto {
    /// Primary key.
    pub id: Uuid,
    /// Parent org.
    pub org_id: Uuid,
    /// Display name.
    pub name: String,
    /// Optional contact name.
    pub contact_name: Option<String>,
    /// Optional email.
    pub email: Option<String>,
    /// Optional phone.
    pub phone: Option<String>,
    /// Optional address.
    pub address: Option<String>,
    /// Optional website.
    pub website: Option<String>,
    /// Optional markdown notes.
    pub notes: Option<String>,
    /// Soft-delete marker.
    pub archived_at: Option<DateTime<Utc>>,
    /// §8.2 CAS counter.
    pub version: i64,
    /// When created.
    pub created_at: DateTime<Utc>,
    /// When last mutated.
    pub updated_at: DateTime<Utc>,
}

impl From<Manufacturer> for PartyDto {
    fn from(m: Manufacturer) -> Self {
        Self {
            id: m.id,
            org_id: m.org_id,
            name: m.name,
            contact_name: m.contact_name,
            email: m.email,
            phone: m.phone,
            address: m.address,
            website: m.website,
            notes: m.notes,
            archived_at: m.archived_at,
            version: m.version,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

impl From<Supplier> for PartyDto {
    fn from(s: Supplier) -> Self {
        Self {
            id: s.id,
            org_id: s.org_id,
            name: s.name,
            contact_name: s.contact_name,
            email: s.email,
            phone: s.phone,
            address: s.address,
            website: s.website,
            notes: s.notes,
            archived_at: s.archived_at,
            version: s.version,
            created_at: s.created_at,
            updated_at: s.updated_at,
        }
    }
}

/// Wire form of a customer (adds `account_ref`).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CustomerDto {
    /// Primary key.
    pub id: Uuid,
    /// Parent org.
    pub org_id: Uuid,
    /// Display name.
    pub name: String,
    /// Optional contact name.
    pub contact_name: Option<String>,
    /// Optional email.
    pub email: Option<String>,
    /// Optional phone.
    pub phone: Option<String>,
    /// Optional address.
    pub address: Option<String>,
    /// Optional website.
    pub website: Option<String>,
    /// Optional markdown notes.
    pub notes: Option<String>,
    /// Optional external CRM/ERP id.
    pub account_ref: Option<String>,
    /// Soft-delete marker.
    pub archived_at: Option<DateTime<Utc>>,
    /// §8.2 CAS counter.
    pub version: i64,
    /// When created.
    pub created_at: DateTime<Utc>,
    /// When last mutated.
    pub updated_at: DateTime<Utc>,
}

impl From<Customer> for CustomerDto {
    fn from(c: Customer) -> Self {
        Self {
            id: c.id,
            org_id: c.org_id,
            name: c.name,
            contact_name: c.contact_name,
            email: c.email,
            phone: c.phone,
            address: c.address,
            website: c.website,
            notes: c.notes,
            account_ref: c.account_ref,
            archived_at: c.archived_at,
            version: c.version,
            created_at: c.created_at,
            updated_at: c.updated_at,
        }
    }
}

/// Paginated party list envelope.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PartyListResponse {
    /// Rows on this page.
    pub rows: Vec<PartyDto>,
    /// Total matching the filter.
    pub total: i64,
    /// Echoed limit.
    pub limit: i64,
    /// Echoed offset.
    pub offset: i64,
}

/// Paginated customer list envelope.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CustomerListResponse {
    /// Rows on this page.
    pub rows: Vec<CustomerDto>,
    /// Total matching the filter.
    pub total: i64,
    /// Echoed limit.
    pub limit: i64,
    /// Echoed offset.
    pub offset: i64,
}

/// Query params for the three party lists.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListPartiesQuery {
    /// Restrict to one org.
    #[serde(default)]
    pub org_id: Option<Uuid>,
    /// Case-insensitive substring on name.
    #[serde(default)]
    pub q: Option<String>,
    /// Include archived rows.
    #[serde(default)]
    pub include_archived: Option<bool>,
    /// Page size.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Page offset.
    #[serde(default)]
    pub offset: Option<i64>,
    /// Count-only mode.
    #[serde(default)]
    pub count_only: Option<u8>,
}

/// Create body for a manufacturer / supplier.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct CreatePartyRequest {
    /// Parent org.
    pub org_id: Uuid,
    /// Display name.
    pub name: String,
    /// Optional contact name.
    #[serde(default)]
    pub contact_name: Option<String>,
    /// Optional email.
    #[serde(default)]
    pub email: Option<String>,
    /// Optional phone.
    #[serde(default)]
    pub phone: Option<String>,
    /// Optional address.
    #[serde(default)]
    pub address: Option<String>,
    /// Optional website.
    #[serde(default)]
    pub website: Option<String>,
    /// Optional markdown notes.
    #[serde(default)]
    pub notes: Option<String>,
}

/// Patch body for a manufacturer / supplier (full upsert + CAS).
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct PatchPartyRequest {
    /// Observed version for §8.2 CAS.
    pub expected_version: i64,
    /// Display name.
    pub name: String,
    /// Optional contact name (null clears).
    #[serde(default)]
    pub contact_name: Option<String>,
    /// Optional email.
    #[serde(default)]
    pub email: Option<String>,
    /// Optional phone.
    #[serde(default)]
    pub phone: Option<String>,
    /// Optional address.
    #[serde(default)]
    pub address: Option<String>,
    /// Optional website.
    #[serde(default)]
    pub website: Option<String>,
    /// Optional markdown notes.
    #[serde(default)]
    pub notes: Option<String>,
}

/// Create body for a customer (adds `account_ref`).
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct CreateCustomerRequest {
    /// Parent org.
    pub org_id: Uuid,
    /// Display name.
    pub name: String,
    /// Optional contact name.
    #[serde(default)]
    pub contact_name: Option<String>,
    /// Optional email.
    #[serde(default)]
    pub email: Option<String>,
    /// Optional phone.
    #[serde(default)]
    pub phone: Option<String>,
    /// Optional address.
    #[serde(default)]
    pub address: Option<String>,
    /// Optional website.
    #[serde(default)]
    pub website: Option<String>,
    /// Optional markdown notes.
    #[serde(default)]
    pub notes: Option<String>,
    /// Optional external CRM/ERP id.
    #[serde(default)]
    pub account_ref: Option<String>,
}

/// Patch body for a customer (full upsert + CAS).
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct PatchCustomerRequest {
    /// Observed version for §8.2 CAS.
    pub expected_version: i64,
    /// Display name.
    pub name: String,
    /// Optional contact name.
    #[serde(default)]
    pub contact_name: Option<String>,
    /// Optional email.
    #[serde(default)]
    pub email: Option<String>,
    /// Optional phone.
    #[serde(default)]
    pub phone: Option<String>,
    /// Optional address.
    #[serde(default)]
    pub address: Option<String>,
    /// Optional website.
    #[serde(default)]
    pub website: Option<String>,
    /// Optional markdown notes.
    #[serde(default)]
    pub notes: Option<String>,
    /// Optional external CRM/ERP id.
    #[serde(default)]
    pub account_ref: Option<String>,
}

/// Body for the archive routes — CAS-gated, idempotent.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct ArchivePartyRequest {
    /// Observed version for §8.2 CAS.
    pub expected_version: i64,
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn resolve_pagination(limit: Option<i64>, offset: Option<i64>) -> (i64, i64) {
    (
        limit.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, MAX_LIST_LIMIT),
        offset.unwrap_or(0).max(0),
    )
}

fn validate_name(name: &str) -> Result<(), ApiError> {
    let t = name.trim();
    if t.is_empty() {
        return Err(ApiError::BadRequest {
            code: "party_name_required",
            message: "name must be non-empty".into(),
        });
    }
    if t.len() > 200 {
        return Err(ApiError::BadRequest {
            code: "party_name_too_long",
            message: "name must be 200 characters or fewer".into(),
        });
    }
    Ok(())
}

fn map_cas(entity: &'static str, id: Uuid, err: StoreError) -> ApiError {
    match err {
        StoreError::NotFound { .. } => ApiError::NotFound {
            code: "party_not_found",
            message: format!("no {entity} with id {id}"),
        },
        StoreError::Conflict(msg) => ApiError::Conflict {
            code: "stale_party_version",
            message: msg,
        },
        StoreError::Invalid(msg) => ApiError::BadRequest {
            code: "party_invalid",
            message: msg,
        },
        e => e.into(),
    }
}

fn filter_from(q: &ListPartiesQuery) -> PartyListFilter {
    let (limit, offset) = resolve_pagination(q.limit, q.offset);
    PartyListFilter {
        org_id: q.org_id,
        q: q.q.clone(),
        include_archived: q.include_archived.unwrap_or(false),
        limit,
        offset,
    }
}

// ---------------------------------------------------------------------------
// manufacturers
// ---------------------------------------------------------------------------

/// `GET /manufacturers` — filtered, paginated list.
#[utoipa::path(get, path = "/manufacturers",
    responses((status = 200, body = PartyListResponse)), tag = "manufacturing")]
pub async fn list_manufacturers(
    State(state): State<AppState>,
    Query(q): Query<ListPartiesQuery>,
) -> Result<Json<PartyListResponse>, ApiError> {
    let filter = filter_from(&q);
    let total = state.store.count_manufacturers(&filter).await?;
    if matches!(q.count_only, Some(n) if n != 0) {
        return Ok(Json(PartyListResponse { rows: vec![], total, limit: 0, offset: filter.offset }));
    }
    let rows = state.store.list_manufacturers(&filter).await?;
    Ok(Json(PartyListResponse {
        rows: rows.into_iter().map(PartyDto::from).collect(),
        total,
        limit: filter.limit,
        offset: filter.offset,
    }))
}

/// `GET /manufacturers/{id}`.
#[utoipa::path(get, path = "/manufacturers/{id}",
    params(("id" = Uuid, Path)),
    responses((status = 200, body = PartyDto), (status = 404)), tag = "manufacturing")]
pub async fn get_manufacturer(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<PartyDto>, ApiError> {
    let row = state.store.get_manufacturer(id).await?.ok_or(ApiError::NotFound {
        code: "party_not_found",
        message: "no manufacturer with that id".into(),
    })?;
    Ok(Json(row.into()))
}

/// `POST /manufacturers`.
#[utoipa::path(post, path = "/manufacturers", request_body = CreatePartyRequest,
    responses((status = 200, body = PartyDto), (status = 409)), tag = "manufacturing")]
pub async fn create_manufacturer(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<CreatePartyRequest>,
) -> Result<Json<PartyDto>, ApiError> {
    validate_name(&body.name)?;
    let u = ManufacturerUpsert {
        org_id: body.org_id,
        name: body.name.trim().to_string(),
        contact_name: body.contact_name,
        email: body.email,
        phone: body.phone,
        address: body.address,
        website: body.website,
        notes: body.notes,
        created_by: Some(principal.actor_user_id),
    };
    let row = state.store.create_manufacturer(&u).await.map_err(|e| map_cas("manufacturer", Uuid::nil(), e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::PARTY_CREATE, format!("manufacturer:{}", row.id)).await.ok();
    Ok(Json(row.into()))
}

/// `PATCH /manufacturers/{id}`.
#[utoipa::path(patch, path = "/manufacturers/{id}", params(("id" = Uuid, Path)),
    request_body = PatchPartyRequest,
    responses((status = 200, body = PartyDto), (status = 404), (status = 409)), tag = "manufacturing")]
pub async fn patch_manufacturer(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchPartyRequest>,
) -> Result<Json<PartyDto>, ApiError> {
    validate_name(&body.name)?;
    let u = ManufacturerUpsert {
        org_id: Uuid::nil(), // org_id is immutable; store ignores it on update
        name: body.name.trim().to_string(),
        contact_name: body.contact_name,
        email: body.email,
        phone: body.phone,
        address: body.address,
        website: body.website,
        notes: body.notes,
        created_by: None,
    };
    let row = state.store.update_manufacturer(id, body.expected_version, &u).await.map_err(|e| map_cas("manufacturer", id, e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::PARTY_UPDATE, format!("manufacturer:{id}")).await.ok();
    Ok(Json(row.into()))
}

/// `DELETE /manufacturers/{id}` — archive (idempotent, CAS-gated).
#[utoipa::path(delete, path = "/manufacturers/{id}", params(("id" = Uuid, Path)),
    request_body = ArchivePartyRequest,
    responses((status = 200, body = PartyDto), (status = 404), (status = 409)), tag = "manufacturing")]
pub async fn archive_manufacturer(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(body): Json<ArchivePartyRequest>,
) -> Result<Json<PartyDto>, ApiError> {
    let row = state.store.archive_manufacturer(id, body.expected_version).await.map_err(|e| map_cas("manufacturer", id, e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::PARTY_ARCHIVE, format!("manufacturer:{id}")).await.ok();
    Ok(Json(row.into()))
}

// ---------------------------------------------------------------------------
// suppliers
// ---------------------------------------------------------------------------

/// `GET /suppliers`.
#[utoipa::path(get, path = "/suppliers",
    responses((status = 200, body = PartyListResponse)), tag = "manufacturing")]
pub async fn list_suppliers(
    State(state): State<AppState>,
    Query(q): Query<ListPartiesQuery>,
) -> Result<Json<PartyListResponse>, ApiError> {
    let filter = filter_from(&q);
    let total = state.store.count_suppliers(&filter).await?;
    if matches!(q.count_only, Some(n) if n != 0) {
        return Ok(Json(PartyListResponse { rows: vec![], total, limit: 0, offset: filter.offset }));
    }
    let rows = state.store.list_suppliers(&filter).await?;
    Ok(Json(PartyListResponse {
        rows: rows.into_iter().map(PartyDto::from).collect(),
        total,
        limit: filter.limit,
        offset: filter.offset,
    }))
}

/// `GET /suppliers/{id}`.
#[utoipa::path(get, path = "/suppliers/{id}", params(("id" = Uuid, Path)),
    responses((status = 200, body = PartyDto), (status = 404)), tag = "manufacturing")]
pub async fn get_supplier(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<PartyDto>, ApiError> {
    let row = state.store.get_supplier(id).await?.ok_or(ApiError::NotFound {
        code: "party_not_found",
        message: "no supplier with that id".into(),
    })?;
    Ok(Json(row.into()))
}

/// `POST /suppliers`.
#[utoipa::path(post, path = "/suppliers", request_body = CreatePartyRequest,
    responses((status = 200, body = PartyDto), (status = 409)), tag = "manufacturing")]
pub async fn create_supplier(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<CreatePartyRequest>,
) -> Result<Json<PartyDto>, ApiError> {
    validate_name(&body.name)?;
    let u = SupplierUpsert {
        org_id: body.org_id,
        name: body.name.trim().to_string(),
        contact_name: body.contact_name,
        email: body.email,
        phone: body.phone,
        address: body.address,
        website: body.website,
        notes: body.notes,
        created_by: Some(principal.actor_user_id),
    };
    let row = state.store.create_supplier(&u).await.map_err(|e| map_cas("supplier", Uuid::nil(), e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::PARTY_CREATE, format!("supplier:{}", row.id)).await.ok();
    Ok(Json(row.into()))
}

/// `PATCH /suppliers/{id}`.
#[utoipa::path(patch, path = "/suppliers/{id}", params(("id" = Uuid, Path)),
    request_body = PatchPartyRequest,
    responses((status = 200, body = PartyDto), (status = 404), (status = 409)), tag = "manufacturing")]
pub async fn patch_supplier(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchPartyRequest>,
) -> Result<Json<PartyDto>, ApiError> {
    validate_name(&body.name)?;
    let u = SupplierUpsert {
        org_id: Uuid::nil(),
        name: body.name.trim().to_string(),
        contact_name: body.contact_name,
        email: body.email,
        phone: body.phone,
        address: body.address,
        website: body.website,
        notes: body.notes,
        created_by: None,
    };
    let row = state.store.update_supplier(id, body.expected_version, &u).await.map_err(|e| map_cas("supplier", id, e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::PARTY_UPDATE, format!("supplier:{id}")).await.ok();
    Ok(Json(row.into()))
}

/// `DELETE /suppliers/{id}` — archive.
#[utoipa::path(delete, path = "/suppliers/{id}", params(("id" = Uuid, Path)),
    request_body = ArchivePartyRequest,
    responses((status = 200, body = PartyDto), (status = 404), (status = 409)), tag = "manufacturing")]
pub async fn archive_supplier(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(body): Json<ArchivePartyRequest>,
) -> Result<Json<PartyDto>, ApiError> {
    let row = state.store.archive_supplier(id, body.expected_version).await.map_err(|e| map_cas("supplier", id, e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::PARTY_ARCHIVE, format!("supplier:{id}")).await.ok();
    Ok(Json(row.into()))
}

// ---------------------------------------------------------------------------
// customers
// ---------------------------------------------------------------------------

/// `GET /customers`.
#[utoipa::path(get, path = "/customers",
    responses((status = 200, body = CustomerListResponse)), tag = "manufacturing")]
pub async fn list_customers(
    State(state): State<AppState>,
    Query(q): Query<ListPartiesQuery>,
) -> Result<Json<CustomerListResponse>, ApiError> {
    let filter = filter_from(&q);
    let total = state.store.count_customers(&filter).await?;
    if matches!(q.count_only, Some(n) if n != 0) {
        return Ok(Json(CustomerListResponse { rows: vec![], total, limit: 0, offset: filter.offset }));
    }
    let rows = state.store.list_customers(&filter).await?;
    Ok(Json(CustomerListResponse {
        rows: rows.into_iter().map(CustomerDto::from).collect(),
        total,
        limit: filter.limit,
        offset: filter.offset,
    }))
}

/// `GET /customers/{id}`.
#[utoipa::path(get, path = "/customers/{id}", params(("id" = Uuid, Path)),
    responses((status = 200, body = CustomerDto), (status = 404)), tag = "manufacturing")]
pub async fn get_customer(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<CustomerDto>, ApiError> {
    let row = state.store.get_customer(id).await?.ok_or(ApiError::NotFound {
        code: "party_not_found",
        message: "no customer with that id".into(),
    })?;
    Ok(Json(row.into()))
}

/// `POST /customers`.
#[utoipa::path(post, path = "/customers", request_body = CreateCustomerRequest,
    responses((status = 200, body = CustomerDto), (status = 409)), tag = "manufacturing")]
pub async fn create_customer(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<CreateCustomerRequest>,
) -> Result<Json<CustomerDto>, ApiError> {
    validate_name(&body.name)?;
    let u = CustomerUpsert {
        org_id: body.org_id,
        name: body.name.trim().to_string(),
        contact_name: body.contact_name,
        email: body.email,
        phone: body.phone,
        address: body.address,
        website: body.website,
        notes: body.notes,
        account_ref: body.account_ref,
        created_by: Some(principal.actor_user_id),
    };
    let row = state.store.create_customer(&u).await.map_err(|e| map_cas("customer", Uuid::nil(), e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::PARTY_CREATE, format!("customer:{}", row.id)).await.ok();
    Ok(Json(row.into()))
}

/// `PATCH /customers/{id}`.
#[utoipa::path(patch, path = "/customers/{id}", params(("id" = Uuid, Path)),
    request_body = PatchCustomerRequest,
    responses((status = 200, body = CustomerDto), (status = 404), (status = 409)), tag = "manufacturing")]
pub async fn patch_customer(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchCustomerRequest>,
) -> Result<Json<CustomerDto>, ApiError> {
    validate_name(&body.name)?;
    let u = CustomerUpsert {
        org_id: Uuid::nil(),
        name: body.name.trim().to_string(),
        contact_name: body.contact_name,
        email: body.email,
        phone: body.phone,
        address: body.address,
        website: body.website,
        notes: body.notes,
        account_ref: body.account_ref,
        created_by: None,
    };
    let row = state.store.update_customer(id, body.expected_version, &u).await.map_err(|e| map_cas("customer", id, e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::PARTY_UPDATE, format!("customer:{id}")).await.ok();
    Ok(Json(row.into()))
}

/// `DELETE /customers/{id}` — archive.
#[utoipa::path(delete, path = "/customers/{id}", params(("id" = Uuid, Path)),
    request_body = ArchivePartyRequest,
    responses((status = 200, body = CustomerDto), (status = 404), (status = 409)), tag = "manufacturing")]
pub async fn archive_customer(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(body): Json<ArchivePartyRequest>,
) -> Result<Json<CustomerDto>, ApiError> {
    let row = state.store.archive_customer(id, body.expected_version).await.map_err(|e| map_cas("customer", id, e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::PARTY_ARCHIVE, format!("customer:{id}")).await.ok();
    Ok(Json(row.into()))
}

// ---------------------------------------------------------------------------
// router
// ---------------------------------------------------------------------------

/// Build the parties router, gated by `(manufacturing, read|write)`.
pub fn parties_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new()
                .route("/manufacturers", get(list_manufacturers))
                .route("/manufacturers/{id}", get(get_manufacturer))
                .route("/suppliers", get(list_suppliers))
                .route("/suppliers/{id}", get(get_supplier))
                .route("/customers", get(list_customers))
                .route("/customers/{id}", get(get_customer)),
            "manufacturing",
            "read",
        ))
        .merge(with_permission(
            Router::new()
                .route("/manufacturers", post(create_manufacturer))
                .route("/manufacturers/{id}", patch(patch_manufacturer).delete(archive_manufacturer))
                .route("/suppliers", post(create_supplier))
                .route("/suppliers/{id}", patch(patch_supplier).delete(archive_supplier))
                .route("/customers", post(create_customer))
                .route("/customers/{id}", patch(patch_customer).delete(archive_customer)),
            "manufacturing",
            "write",
        ))
        .with_state(inner)
}
