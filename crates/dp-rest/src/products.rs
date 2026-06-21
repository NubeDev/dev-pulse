//! Products REST CRUD + project links + document uploads
//! (DOCS/ideas/product-manufacturing.md §5.2 / §7.3).
//!
//! Gated by `(manufacturing, read|write)`. Document upload reuses the
//! exec-summary blob precedent verbatim (multipart `read_upload`,
//! 25 MiB cap, `BlobStore::put_bytes`, opaque `blob_ref` jsonb,
//! proxy `GET /blobs/product/...`).

use std::sync::Arc;

use axum::{
    extract::{Extension, Multipart, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, patch, post},
    Router,
};
use axum::body::Body;
use chrono::{DateTime, Utc};
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use starter_spi::blob::{meta_keys, BlobKey, BlobRef, PutOptions};
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::product::{Product, ProductKind, ProductListFilter, ProductStatus, ProductUpsert};
use dp_domain::store::{StoreError, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT};

use crate::audit::{self, Principal};
use crate::error::ApiError;
use crate::project_exec_summary::{map_blob_err, read_upload, require_blob_store};
use crate::projects::ProjectDto;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

/// Wire form of [`ProductStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProductStatusDto {
    /// Being defined.
    Draft,
    /// Active.
    Active,
    /// End-of-life.
    Eol,
    /// Archived.
    Archived,
}

impl From<ProductStatus> for ProductStatusDto {
    fn from(s: ProductStatus) -> Self {
        match s {
            ProductStatus::Draft => Self::Draft,
            ProductStatus::Active => Self::Active,
            ProductStatus::Eol => Self::Eol,
            ProductStatus::Archived => Self::Archived,
        }
    }
}
impl From<ProductStatusDto> for ProductStatus {
    fn from(s: ProductStatusDto) -> Self {
        match s {
            ProductStatusDto::Draft => Self::Draft,
            ProductStatusDto::Active => Self::Active,
            ProductStatusDto::Eol => Self::Eol,
            ProductStatusDto::Archived => Self::Archived,
        }
    }
}

/// Wire form of [`ProductKind`] — in-house Nube iO vs OEM (feedback #1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProductKindDto {
    /// In-house Nube iO product.
    NubeIo,
    /// Third-party / re-badged OEM product.
    Oem,
}

impl From<ProductKind> for ProductKindDto {
    fn from(k: ProductKind) -> Self {
        match k {
            ProductKind::NubeIo => Self::NubeIo,
            ProductKind::Oem => Self::Oem,
        }
    }
}
impl From<ProductKindDto> for ProductKind {
    fn from(k: ProductKindDto) -> Self {
        match k {
            ProductKindDto::NubeIo => Self::NubeIo,
            ProductKindDto::Oem => Self::Oem,
        }
    }
}

/// One product row on the wire.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProductDto {
    /// Primary key.
    pub id: Uuid,
    /// Parent org.
    pub org_id: Uuid,
    /// Name.
    pub name: String,
    /// Model number.
    pub model_number: String,
    /// Optional markdown description.
    pub description: Option<String>,
    /// Owning manufacturer.
    pub manufacturer_id: Option<Uuid>,
    /// Lifecycle status.
    pub status: ProductStatusDto,
    /// In-house Nube iO vs OEM (feedback #1).
    pub kind: ProductKindDto,
    /// Serial prefix.
    pub serial_prefix: Option<String>,
    /// Serial template.
    pub serial_format: Option<String>,
    /// Soft-delete marker.
    pub archived_at: Option<DateTime<Utc>>,
    /// Creator.
    pub created_by: Option<Uuid>,
    /// §8.2 CAS counter.
    pub version: i64,
    /// When created.
    pub created_at: DateTime<Utc>,
    /// When last mutated.
    pub updated_at: DateTime<Utc>,
}

impl From<Product> for ProductDto {
    fn from(p: Product) -> Self {
        Self {
            id: p.id,
            org_id: p.org_id,
            name: p.name,
            model_number: p.model_number,
            description: p.description,
            manufacturer_id: p.manufacturer_id,
            status: p.status.into(),
            kind: p.kind.into(),
            serial_prefix: p.serial_prefix,
            serial_format: p.serial_format,
            archived_at: p.archived_at,
            created_by: p.created_by,
            version: p.version,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

/// Paginated product list envelope.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProductListResponse {
    /// Rows on this page.
    pub rows: Vec<ProductDto>,
    /// Total matching the filter.
    pub total: i64,
    /// Echoed limit.
    pub limit: i64,
    /// Echoed offset.
    pub offset: i64,
}

/// A product document on the wire (adds a download `url`).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProductDocumentDto {
    /// Primary key.
    pub id: Uuid,
    /// Parent product.
    pub product_id: Uuid,
    /// Proxy download URL.
    pub url: String,
    /// Display title.
    pub title: String,
    /// Optional doc type.
    pub doc_type: Option<String>,
    /// Optional notes.
    pub notes: Option<String>,
    /// Free-text uploader label.
    pub uploaded_by: Option<String>,
    /// When created.
    pub created_at: DateTime<Utc>,
}

impl From<dp_domain::product_doc::ProductDocument> for ProductDocumentDto {
    fn from(d: dp_domain::product_doc::ProductDocument) -> Self {
        Self {
            url: format!("/blobs/product/documents/{}", d.id),
            id: d.id,
            product_id: d.product_id,
            title: d.title,
            doc_type: d.doc_type,
            notes: d.notes,
            uploaded_by: d.uploaded_by,
            created_at: d.created_at,
        }
    }
}

/// Query params for `GET /products`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListProductsQuery {
    /// Restrict to one org.
    #[serde(default)]
    pub org_id: Option<Uuid>,
    /// Restrict to one status.
    #[serde(default)]
    pub status: Option<ProductStatusDto>,
    /// Substring on name or model number.
    #[serde(default)]
    pub q: Option<String>,
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

/// Create body for `POST /products`.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct CreateProductRequest {
    /// Parent org.
    pub org_id: Uuid,
    /// Name.
    pub name: String,
    /// Model number.
    pub model_number: String,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
    /// Optional owning manufacturer.
    #[serde(default)]
    pub manufacturer_id: Option<Uuid>,
    /// Optional status; defaults to `active`.
    #[serde(default)]
    pub status: Option<ProductStatusDto>,
    /// Optional kind; defaults to `nube_io`.
    #[serde(default)]
    pub kind: Option<ProductKindDto>,
    /// Optional serial prefix.
    #[serde(default)]
    pub serial_prefix: Option<String>,
    /// Optional serial template.
    #[serde(default)]
    pub serial_format: Option<String>,
}

/// Patch body for `PATCH /products/{id}` (full upsert + CAS).
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct PatchProductRequest {
    /// Observed version for §8.2 CAS.
    pub expected_version: i64,
    /// Name.
    pub name: String,
    /// Model number.
    pub model_number: String,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
    /// Optional owning manufacturer.
    #[serde(default)]
    pub manufacturer_id: Option<Uuid>,
    /// Status.
    pub status: ProductStatusDto,
    /// Kind — in-house Nube iO vs OEM (feedback #1).
    pub kind: ProductKindDto,
    /// Optional serial prefix.
    #[serde(default)]
    pub serial_prefix: Option<String>,
    /// Optional serial template.
    #[serde(default)]
    pub serial_format: Option<String>,
}

/// Body for `DELETE /products/{id}` (archive).
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct ArchiveProductRequest {
    /// Observed version for §8.2 CAS.
    pub expected_version: i64,
}

/// Body for linking a project (optional; link can also be path-only).
#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema)]
pub struct LinkProjectRequest {}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn resolve_pagination(limit: Option<i64>, offset: Option<i64>) -> (i64, i64) {
    (
        limit.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, MAX_LIST_LIMIT),
        offset.unwrap_or(0).max(0),
    )
}

fn validate_required(label: &'static str, code: &'static str, v: &str) -> Result<(), ApiError> {
    if v.trim().is_empty() {
        return Err(ApiError::BadRequest { code, message: format!("{label} must be non-empty") });
    }
    Ok(())
}

/// Validate a `serial_format` template (§6): only the whitelisted
/// tokens `{prefix}`, `{run_code}`, `{seq}` / `{seq:NN}`, and a
/// mandatory `{seq}` token (else units collide on the serial index).
pub fn validate_serial_format(fmt: &str) -> Result<(), ApiError> {
    let mut rest = fmt;
    let mut has_seq = false;
    while let Some(start) = rest.find('{') {
        let end = rest[start..].find('}').map(|e| start + e).ok_or(ApiError::BadRequest {
            code: "serial_format_invalid",
            message: "unbalanced '{' in serial_format".into(),
        })?;
        let token = &rest[start + 1..end];
        let ok = token == "prefix"
            || token == "run_code"
            || token == "seq"
            || token
                .strip_prefix("seq:")
                .map(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
                .unwrap_or(false);
        if !ok {
            return Err(ApiError::BadRequest {
                code: "serial_format_invalid",
                message: format!("unknown serial_format token: {{{token}}}"),
            });
        }
        if token == "seq" || token.starts_with("seq:") {
            has_seq = true;
        }
        rest = &rest[end + 1..];
    }
    if rest.contains('}') {
        return Err(ApiError::BadRequest {
            code: "serial_format_invalid",
            message: "unbalanced '}' in serial_format".into(),
        });
    }
    if !has_seq {
        return Err(ApiError::BadRequest {
            code: "serial_format_invalid",
            message: "serial_format must contain a {seq} token".into(),
        });
    }
    Ok(())
}

fn map_cas(id: Uuid, err: StoreError) -> ApiError {
    match err {
        StoreError::NotFound { .. } => ApiError::NotFound {
            code: "product_not_found",
            message: format!("no product with id {id}"),
        },
        StoreError::Conflict(msg) => ApiError::Conflict {
            code: "stale_product_version",
            message: msg,
        },
        StoreError::Invalid(msg) => ApiError::BadRequest {
            code: "product_invalid",
            message: msg,
        },
        e => e.into(),
    }
}

fn build_product_blob_key(product_id: Uuid, filename: &str) -> Result<BlobKey, ApiError> {
    let safe: String = filename
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .collect();
    let rand = Uuid::new_v4();
    BlobKey::new(format!("product/{product_id}/documents/{rand}-{safe}")).map_err(|e| {
        ApiError::BadRequest { code: "blob_key_invalid", message: format!("could not build blob key: {e}") }
    })
}

// ---------------------------------------------------------------------------
// product handlers
// ---------------------------------------------------------------------------

/// `GET /products` — filtered, paginated list.
#[utoipa::path(get, path = "/products",
    responses((status = 200, body = ProductListResponse)), tag = "manufacturing")]
pub async fn list_products(
    State(state): State<AppState>,
    Query(q): Query<ListProductsQuery>,
) -> Result<Json<ProductListResponse>, ApiError> {
    let (limit, offset) = resolve_pagination(q.limit, q.offset);
    let filter = ProductListFilter {
        org_id: q.org_id,
        status: q.status.map(Into::into),
        q: q.q.clone(),
        limit,
        offset,
    };
    let total = state.store.count_products(&filter).await?;
    if matches!(q.count_only, Some(n) if n != 0) {
        return Ok(Json(ProductListResponse { rows: vec![], total, limit: 0, offset }));
    }
    let rows = state.store.list_products(&filter).await?;
    Ok(Json(ProductListResponse {
        rows: rows.into_iter().map(ProductDto::from).collect(),
        total,
        limit,
        offset,
    }))
}

/// `GET /products/{id}`.
#[utoipa::path(get, path = "/products/{id}", params(("id" = Uuid, Path)),
    responses((status = 200, body = ProductDto), (status = 404)), tag = "manufacturing")]
pub async fn get_product(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ProductDto>, ApiError> {
    let row = state.store.get_product(id).await?.ok_or(ApiError::NotFound {
        code: "product_not_found",
        message: "no product with that id".into(),
    })?;
    Ok(Json(row.into()))
}

/// `POST /products`.
#[utoipa::path(post, path = "/products", request_body = CreateProductRequest,
    responses((status = 200, body = ProductDto), (status = 409)), tag = "manufacturing")]
pub async fn create_product(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<CreateProductRequest>,
) -> Result<Json<ProductDto>, ApiError> {
    validate_required("product name", "product_name_required", &body.name)?;
    validate_required("model number", "model_number_required", &body.model_number)?;
    if let Some(fmt) = body.serial_format.as_deref() {
        validate_serial_format(fmt)?;
    }
    let u = ProductUpsert {
        org_id: body.org_id,
        name: body.name.trim().to_string(),
        model_number: body.model_number.trim().to_string(),
        description: body.description,
        manufacturer_id: body.manufacturer_id,
        status: body.status.map(Into::into).unwrap_or(ProductStatus::Active),
        kind: body.kind.map(Into::into).unwrap_or(ProductKind::NubeIo),
        serial_prefix: body.serial_prefix,
        serial_format: body.serial_format,
        created_by: Some(principal.actor_user_id),
    };
    let row = state.store.create_product(&u).await.map_err(|e| map_cas(Uuid::nil(), e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::PRODUCT_CREATE, row.id.to_string()).await.ok();
    Ok(Json(row.into()))
}

/// `PATCH /products/{id}`.
#[utoipa::path(patch, path = "/products/{id}", params(("id" = Uuid, Path)),
    request_body = PatchProductRequest,
    responses((status = 200, body = ProductDto), (status = 404), (status = 409)), tag = "manufacturing")]
pub async fn patch_product(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchProductRequest>,
) -> Result<Json<ProductDto>, ApiError> {
    validate_required("product name", "product_name_required", &body.name)?;
    validate_required("model number", "model_number_required", &body.model_number)?;
    if let Some(fmt) = body.serial_format.as_deref() {
        validate_serial_format(fmt)?;
    }
    let u = ProductUpsert {
        org_id: Uuid::nil(),
        name: body.name.trim().to_string(),
        model_number: body.model_number.trim().to_string(),
        description: body.description,
        manufacturer_id: body.manufacturer_id,
        status: body.status.into(),
        kind: body.kind.into(),
        serial_prefix: body.serial_prefix,
        serial_format: body.serial_format,
        created_by: None,
    };
    let row = state.store.update_product(id, body.expected_version, &u).await.map_err(|e| map_cas(id, e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::PRODUCT_UPDATE, id.to_string()).await.ok();
    Ok(Json(row.into()))
}

/// `DELETE /products/{id}` — archive (idempotent, CAS-gated).
#[utoipa::path(delete, path = "/products/{id}", params(("id" = Uuid, Path)),
    request_body = ArchiveProductRequest,
    responses((status = 200, body = ProductDto), (status = 404), (status = 409)), tag = "manufacturing")]
pub async fn archive_product(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(body): Json<ArchiveProductRequest>,
) -> Result<Json<ProductDto>, ApiError> {
    let row = state.store.archive_product(id, body.expected_version).await.map_err(|e| map_cas(id, e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::PRODUCT_ARCHIVE, id.to_string()).await.ok();
    Ok(Json(row.into()))
}

// ---------------------------------------------------------------------------
// project links
// ---------------------------------------------------------------------------

/// `GET /products/{id}/projects` — projects linked to a product.
#[utoipa::path(get, path = "/products/{id}/projects", params(("id" = Uuid, Path)),
    responses((status = 200, body = [ProjectDto])), tag = "manufacturing")]
pub async fn list_product_projects(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<ProjectDto>>, ApiError> {
    let rows = state.store.list_product_projects(id).await?;
    Ok(Json(rows.into_iter().map(ProjectDto::from).collect()))
}

/// `GET /projects/{id}/products` — reverse view (products in project).
#[utoipa::path(get, path = "/projects/{id}/products", params(("id" = Uuid, Path)),
    responses((status = 200, body = [ProductDto])), tag = "manufacturing")]
pub async fn list_project_products(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<ProductDto>>, ApiError> {
    let rows = state.store.list_project_products(id).await?;
    Ok(Json(rows.into_iter().map(ProductDto::from).collect()))
}

/// `POST /products/{id}/projects/{project_id}` — link (idempotent).
#[utoipa::path(post, path = "/products/{id}/projects/{project_id}",
    params(("id" = Uuid, Path), ("project_id" = Uuid, Path)),
    responses((status = 200)), tag = "manufacturing")]
pub async fn link_product_project(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((id, project_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    state.store.link_product_project(id, project_id, Some(principal.actor_user_id)).await?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::PRODUCT_LINK_PROJECT, format!("{id}:{project_id}")).await.ok();
    Ok(StatusCode::OK)
}

/// `DELETE /products/{id}/projects/{project_id}` — unlink.
#[utoipa::path(delete, path = "/products/{id}/projects/{project_id}",
    params(("id" = Uuid, Path), ("project_id" = Uuid, Path)),
    responses((status = 200)), tag = "manufacturing")]
pub async fn unlink_product_project(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((id, project_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    state.store.unlink_product_project(id, project_id).await?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::PRODUCT_UNLINK_PROJECT, format!("{id}:{project_id}")).await.ok();
    Ok(StatusCode::OK)
}

// ---------------------------------------------------------------------------
// documents
// ---------------------------------------------------------------------------

/// `GET /products/{id}/documents`.
#[utoipa::path(get, path = "/products/{id}/documents", params(("id" = Uuid, Path)),
    responses((status = 200, body = [ProductDocumentDto])), tag = "manufacturing")]
pub async fn list_product_documents(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<ProductDocumentDto>>, ApiError> {
    let rows = state.store.list_product_documents(id).await?;
    Ok(Json(rows.into_iter().map(ProductDocumentDto::from).collect()))
}

/// `POST /products/{id}/documents` — multipart upload (`file` +
/// optional `title`/`doc_type`/`notes`).
#[utoipa::path(post, path = "/products/{id}/documents", params(("id" = Uuid, Path)),
    responses((status = 200, body = ProductDocumentDto)), tag = "manufacturing")]
pub async fn upload_product_document(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    multipart: Multipart,
) -> Result<Json<ProductDocumentDto>, ApiError> {
    let store = require_blob_store(&state)?;
    let upload = read_upload(multipart).await?;
    let key = build_product_blob_key(id, &upload.filename)?;
    let opts = PutOptions::with_content_type(upload.content_type.clone())
        .user_meta(meta_keys::FILENAME, &upload.filename)
        .user_meta(meta_keys::UPLOADED_BY, principal.actor_user_id.to_string());
    let blob_ref = store.put_bytes(&key, upload.bytes, opts).await.map_err(map_blob_err)?;
    let blob_ref_json = serde_json::to_value(&blob_ref).map_err(|e| ApiError::BadRequest {
        code: "blob_serialise",
        message: format!("could not serialise BlobRef: {e}"),
    })?;
    let title = upload.text_fields.get("title").cloned().unwrap_or_else(|| upload.filename.clone());
    let doc_type = upload.text_fields.get("doc_type").cloned();
    let notes = upload.text_fields.get("notes").cloned();
    let row = state
        .store
        .insert_product_document(
            id,
            &blob_ref_json,
            &title,
            doc_type.as_deref(),
            notes.as_deref(),
            Some(principal.actor_user_id.to_string().as_str()),
        )
        .await?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::PRODUCT_DOCUMENT_ADD, format!("{id}:{}", row.id)).await.ok();
    Ok(Json(row.into()))
}

/// `DELETE /products/{id}/documents/{doc_id}`.
#[utoipa::path(delete, path = "/products/{id}/documents/{doc_id}",
    params(("id" = Uuid, Path), ("doc_id" = Uuid, Path)),
    responses((status = 200)), tag = "manufacturing")]
pub async fn delete_product_document(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((id, doc_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    // §8: resolve the child up to its parent — never act on a child id
    // in isolation. The document must belong to the path product.
    let doc = state.store.get_product_document(doc_id).await?.ok_or(ApiError::NotFound {
        code: "document_not_found",
        message: "no such document".into(),
    })?;
    if doc.product_id != id {
        return Err(ApiError::NotFound {
            code: "document_not_found",
            message: "document does not belong to this product".into(),
        });
    }
    state.store.delete_product_document(doc_id).await?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::PRODUCT_DOCUMENT_REMOVE, format!("{id}:{doc_id}")).await.ok();
    Ok(StatusCode::OK)
}

/// `GET /blobs/product/{kind}/{row_id}` — proxy download.
#[utoipa::path(get, path = "/blobs/product/{kind}/{row_id}",
    params(("kind" = String, Path), ("row_id" = Uuid, Path)),
    responses((status = 200), (status = 404)), tag = "manufacturing")]
pub async fn proxy_product_blob(
    State(state): State<AppState>,
    Path((kind, row_id)): Path<(String, Uuid)>,
) -> Result<Response, ApiError> {
    let store = require_blob_store(&state)?;
    if kind != "documents" {
        return Err(ApiError::NotFound { code: "blob_not_found", message: format!("unknown blob kind {kind:?}") });
    }
    let doc = state.store.get_product_document(row_id).await?.ok_or(ApiError::NotFound {
        code: "blob_not_found",
        message: "no such document".into(),
    })?;
    let blob_ref: BlobRef = serde_json::from_value(doc.blob_ref.clone()).map_err(|e| ApiError::BadRequest {
        code: "blob_decode",
        message: format!("could not decode BlobRef: {e}"),
    })?;
    let meta = store.head(&blob_ref).await.map_err(map_blob_err)?;
    let content_type = meta.content_type.unwrap_or_else(|| "application/octet-stream".to_string());
    let filename = meta
        .user_metadata
        .get(meta_keys::FILENAME)
        .cloned()
        .unwrap_or_else(|| doc.title.clone());
    let stream = store.get(&blob_ref, None).await.map_err(map_blob_err)?;
    let body = Body::from_stream(stream.map_err(std::io::Error::other));
    let mut headers = HeaderMap::new();
    if let Ok(ct) = HeaderValue::from_str(&content_type) {
        headers.insert(header::CONTENT_TYPE, ct);
    }
    let disposition = format!("inline; filename=\"{}\"", crate::project_exec_summary::escape_filename(&filename));
    if let Ok(disp) = HeaderValue::from_str(&disposition) {
        headers.insert(header::CONTENT_DISPOSITION, disp);
    }
    Ok((StatusCode::OK, headers, body).into_response())
}

// ---------------------------------------------------------------------------
// router
// ---------------------------------------------------------------------------

/// Build the products router, gated by `(manufacturing, read|write)`.
pub fn products_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new()
                .route("/products", get(list_products))
                .route("/products/{id}", get(get_product))
                .route("/products/{id}/projects", get(list_product_projects))
                .route("/projects/{id}/products", get(list_project_products))
                .route("/products/{id}/documents", get(list_product_documents))
                .route("/blobs/product/{kind}/{row_id}", get(proxy_product_blob)),
            "manufacturing",
            "read",
        ))
        .merge(with_permission(
            Router::new()
                .route("/products", post(create_product))
                .route("/products/{id}", patch(patch_product).delete(archive_product))
                .route(
                    "/products/{id}/projects/{project_id}",
                    post(link_product_project).delete(unlink_product_project),
                )
                .route("/products/{id}/documents", post(upload_product_document))
                .route("/products/{id}/documents/{doc_id}", axum::routing::delete(delete_product_document)),
            "manufacturing",
            "write",
        ))
        .with_state(inner)
}
