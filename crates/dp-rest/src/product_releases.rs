//! Per-product software & firmware release history REST
//! (DOCS/ideas/product-manufacturing.md §5.x). Gated by
//! `(manufacturing, read|write)`.
//!
//! Releases are nested children of a product. Per §8, every child route
//! resolves the release up to its parent product and rejects (404) a
//! release whose `product_id` does not match the path id.

use std::sync::Arc;

use axum::{
    extract::{Extension, Path, Query, State},
    response::Json,
    routing::{get, patch},
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::product_release::{
    ProductRelease, ProductReleaseCreate, ProductReleaseUpdate, ReleaseKind, ReleaseLink,
};
use dp_domain::store::StoreError;

use crate::audit::{self, Principal};
use crate::error::ApiError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

/// Wire form of [`ReleaseKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseKindDto {
    /// Software release.
    Software,
    /// Firmware release.
    Firmware,
}

impl From<ReleaseKind> for ReleaseKindDto {
    fn from(k: ReleaseKind) -> Self {
        match k {
            ReleaseKind::Software => Self::Software,
            ReleaseKind::Firmware => Self::Firmware,
        }
    }
}
impl From<ReleaseKindDto> for ReleaseKind {
    fn from(k: ReleaseKindDto) -> Self {
        match k {
            ReleaseKindDto::Software => Self::Software,
            ReleaseKindDto::Firmware => Self::Firmware,
        }
    }
}

/// A labelled build / download link on a release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ReleaseLinkDto {
    /// Human label, e.g. `Firmware binary (.bin)`.
    pub label: String,
    /// The URL.
    pub url: String,
}
impl From<ReleaseLink> for ReleaseLinkDto {
    fn from(l: ReleaseLink) -> Self {
        Self { label: l.label, url: l.url }
    }
}

/// One product release on the wire.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProductReleaseDto {
    /// Primary key.
    pub id: Uuid,
    /// Parent org.
    pub org_id: Uuid,
    /// Parent product.
    pub product_id: Uuid,
    /// Software or firmware.
    pub kind: ReleaseKindDto,
    /// Major version component.
    pub major: i32,
    /// Minor version component.
    pub minor: i32,
    /// Derived `v{major}.{minor}` label for convenience.
    pub version_label: String,
    /// Optional release notes.
    pub release_notes: Option<String>,
    /// Optional release date.
    pub released_at: Option<DateTime<Utc>>,
    /// Build / download links.
    pub links: Vec<ReleaseLinkDto>,
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

impl From<ProductRelease> for ProductReleaseDto {
    fn from(r: ProductRelease) -> Self {
        let version_label = format!("v{}.{}", r.major, r.minor);
        Self {
            id: r.id,
            org_id: r.org_id,
            product_id: r.product_id,
            kind: r.kind.into(),
            major: r.major,
            minor: r.minor,
            version_label,
            release_notes: r.release_notes,
            released_at: r.released_at,
            links: r.links.into_iter().map(ReleaseLinkDto::from).collect(),
            archived_at: r.archived_at,
            created_by: r.created_by,
            version: r.version,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Query params for `GET /products/{id}/releases`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListReleasesQuery {
    /// Restrict to one kind.
    #[serde(default)]
    pub kind: Option<ReleaseKindDto>,
}

/// Create body for `POST /products/{id}/releases`.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct CreateReleaseRequest {
    /// Software or firmware.
    pub kind: ReleaseKindDto,
    /// Major version component.
    pub major: i32,
    /// Minor version component.
    pub minor: i32,
    /// Optional release notes.
    #[serde(default)]
    pub release_notes: Option<String>,
    /// Optional release date.
    #[serde(default)]
    pub released_at: Option<DateTime<Utc>>,
    /// Build / download links.
    #[serde(default)]
    pub links: Vec<ReleaseLinkDto>,
}

/// Patch body for `PATCH /products/{id}/releases/{rid}` (CAS).
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct PatchReleaseRequest {
    /// Observed version for §8.2 CAS.
    pub expected_version: i64,
    /// Software or firmware.
    pub kind: ReleaseKindDto,
    /// Major version component.
    pub major: i32,
    /// Minor version component.
    pub minor: i32,
    /// Optional release notes.
    #[serde(default)]
    pub release_notes: Option<String>,
    /// Optional release date.
    #[serde(default)]
    pub released_at: Option<DateTime<Utc>>,
    /// Build / download links.
    #[serde(default)]
    pub links: Vec<ReleaseLinkDto>,
}

/// Body for `DELETE /products/{id}/releases/{rid}` (archive).
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct ArchiveReleaseRequest {
    /// Observed version for §8.2 CAS.
    pub expected_version: i64,
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Max links per release (a sane cap on the JSON array).
const MAX_RELEASE_LINKS: usize = 30;

/// Validate + normalise build links: trim, drop blank rows, require a
/// non-empty `http(s)` URL, default the label to the URL, cap the count.
fn to_domain_links(links: Vec<ReleaseLinkDto>) -> Result<Vec<ReleaseLink>, ApiError> {
    let mut out = Vec::new();
    for l in links {
        let url = l.url.trim().to_string();
        if url.is_empty() {
            continue; // skip blank rows from the form
        }
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(ApiError::BadRequest {
                code: "release_link_invalid",
                message: format!("link URL must start with http:// or https:// — got {url:?}"),
            });
        }
        let label = {
            let t = l.label.trim();
            if t.is_empty() { url.clone() } else { t.to_string() }
        };
        out.push(ReleaseLink { label, url });
    }
    if out.len() > MAX_RELEASE_LINKS {
        return Err(ApiError::BadRequest {
            code: "release_links_too_many",
            message: format!("at most {MAX_RELEASE_LINKS} links per release"),
        });
    }
    Ok(out)
}

fn validate_version(major: i32, minor: i32) -> Result<(), ApiError> {
    if major < 0 || minor < 0 {
        return Err(ApiError::BadRequest {
            code: "release_version_invalid",
            message: "major and minor must be non-negative".into(),
        });
    }
    Ok(())
}

fn map_cas(id: Uuid, err: StoreError) -> ApiError {
    match err {
        StoreError::NotFound { .. } => ApiError::NotFound {
            code: "release_not_found",
            message: format!("no product release with id {id}"),
        },
        StoreError::Conflict(msg) => ApiError::Conflict {
            code: "release_conflict",
            message: msg,
        },
        StoreError::Invalid(msg) => ApiError::BadRequest {
            code: "release_invalid",
            message: msg,
        },
        e => e.into(),
    }
}

/// Resolve a release and enforce §8: it must belong to the path product.
async fn release_under_product(
    state: &AppState,
    product_id: Uuid,
    release_id: Uuid,
) -> Result<ProductRelease, ApiError> {
    let r = state.store.get_product_release(release_id).await?.ok_or(ApiError::NotFound {
        code: "release_not_found",
        message: "no release with that id".into(),
    })?;
    if r.product_id != product_id {
        return Err(ApiError::NotFound {
            code: "release_not_found",
            message: "release does not belong to this product".into(),
        });
    }
    Ok(r)
}

// ---------------------------------------------------------------------------
// handlers
// ---------------------------------------------------------------------------

/// `GET /products/{id}/releases` — non-archived releases, optional kind.
#[utoipa::path(get, path = "/products/{id}/releases", params(("id" = Uuid, Path)),
    responses((status = 200, body = [ProductReleaseDto])), tag = "manufacturing")]
pub async fn list_releases(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<ListReleasesQuery>,
) -> Result<Json<Vec<ProductReleaseDto>>, ApiError> {
    let rows = state.store.list_product_releases(id, q.kind.map(Into::into)).await?;
    Ok(Json(rows.into_iter().map(ProductReleaseDto::from).collect()))
}

/// `POST /products/{id}/releases`.
#[utoipa::path(post, path = "/products/{id}/releases", params(("id" = Uuid, Path)),
    request_body = CreateReleaseRequest,
    responses((status = 200, body = ProductReleaseDto), (status = 404), (status = 409)), tag = "manufacturing")]
pub async fn create_release(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateReleaseRequest>,
) -> Result<Json<ProductReleaseDto>, ApiError> {
    validate_version(body.major, body.minor)?;
    // §8: org_id is taken from the parent product (also enforces it exists).
    let product = state.store.get_product(id).await?.ok_or(ApiError::NotFound {
        code: "product_not_found",
        message: "no product with that id".into(),
    })?;
    let c = ProductReleaseCreate {
        org_id: product.org_id,
        product_id: id,
        kind: body.kind.into(),
        major: body.major,
        minor: body.minor,
        release_notes: body.release_notes,
        released_at: body.released_at,
        links: to_domain_links(body.links)?,
        created_by: Some(principal.actor_user_id),
    };
    let row = state.store.create_product_release(&c).await.map_err(|e| map_cas(id, e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::PRODUCT_RELEASE_CREATE, format!("{id}:{}", row.id)).await.ok();
    Ok(Json(row.into()))
}

/// `PATCH /products/{id}/releases/{rid}` — CAS update.
#[utoipa::path(patch, path = "/products/{id}/releases/{rid}",
    params(("id" = Uuid, Path), ("rid" = Uuid, Path)),
    request_body = PatchReleaseRequest,
    responses((status = 200, body = ProductReleaseDto), (status = 404), (status = 409)), tag = "manufacturing")]
pub async fn patch_release(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((id, rid)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchReleaseRequest>,
) -> Result<Json<ProductReleaseDto>, ApiError> {
    validate_version(body.major, body.minor)?;
    // §8: verify the release belongs to the path product before updating.
    release_under_product(&state, id, rid).await?;
    let u = ProductReleaseUpdate {
        kind: body.kind.into(),
        major: body.major,
        minor: body.minor,
        release_notes: body.release_notes,
        released_at: body.released_at,
        links: to_domain_links(body.links)?,
    };
    let row = state.store.update_product_release(rid, body.expected_version, &u).await.map_err(|e| map_cas(rid, e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::PRODUCT_RELEASE_UPDATE, format!("{id}:{rid}")).await.ok();
    Ok(Json(row.into()))
}

/// `DELETE /products/{id}/releases/{rid}` — archive (CAS, idempotent).
#[utoipa::path(delete, path = "/products/{id}/releases/{rid}",
    params(("id" = Uuid, Path), ("rid" = Uuid, Path)),
    request_body = ArchiveReleaseRequest,
    responses((status = 200, body = ProductReleaseDto), (status = 404), (status = 409)), tag = "manufacturing")]
pub async fn archive_release(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((id, rid)): Path<(Uuid, Uuid)>,
    Json(body): Json<ArchiveReleaseRequest>,
) -> Result<Json<ProductReleaseDto>, ApiError> {
    // §8: verify the release belongs to the path product first.
    release_under_product(&state, id, rid).await?;
    let row = state.store.archive_product_release(rid, body.expected_version).await.map_err(|e| map_cas(rid, e))?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::PRODUCT_RELEASE_UPDATE, format!("{id}:{rid}")).await.ok();
    Ok(Json(row.into()))
}

// ---------------------------------------------------------------------------
// router
// ---------------------------------------------------------------------------

/// Build the releases router, gated by `(manufacturing, read|write)`.
pub fn product_releases_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new()
                .route("/products/{id}/releases", get(list_releases)),
            "manufacturing",
            "read",
        ))
        .merge(with_permission(
            Router::new()
                .route("/products/{id}/releases", axum::routing::post(create_release))
                .route("/products/{id}/releases/{rid}", patch(patch_release).delete(archive_release)),
            "manufacturing",
            "write",
        ))
        .with_state(inner)
}
