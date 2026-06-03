//! Product manuals + revisions REST (DOCS/ideas/product-manufacturing.md
//! §5.3 / §7.3). Gated by `(manufacturing, read|write)`.

use std::sync::Arc;

use axum::{
    extract::{Extension, Path, State},
    response::Json,
    routing::{get, post},
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::product_manual::{
    ManualRevision, ManualUpsert, ProductManual, RevisionStatus, RevisionUpsert,
};
use dp_domain::store::StoreError;

use crate::audit::{self, Principal};
use crate::error::ApiError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

/// Wire form of [`RevisionStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum RevisionStatusDto {
    /// Draft.
    Draft,
    /// Published.
    Published,
    /// Superseded.
    Superseded,
}

impl From<RevisionStatus> for RevisionStatusDto {
    fn from(s: RevisionStatus) -> Self {
        match s {
            RevisionStatus::Draft => Self::Draft,
            RevisionStatus::Published => Self::Published,
            RevisionStatus::Superseded => Self::Superseded,
        }
    }
}

/// A manual container on the wire.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ManualDto {
    /// Primary key.
    pub id: Uuid,
    /// Parent product.
    pub product_id: Uuid,
    /// Title.
    pub title: String,
    /// Creator.
    pub created_by: Option<Uuid>,
    /// §8.2 CAS counter.
    pub version: i64,
    /// When created.
    pub created_at: DateTime<Utc>,
    /// When last mutated.
    pub updated_at: DateTime<Utc>,
}

impl From<ProductManual> for ManualDto {
    fn from(m: ProductManual) -> Self {
        Self {
            id: m.id,
            product_id: m.product_id,
            title: m.title,
            created_by: m.created_by,
            version: m.version,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

/// A manual revision on the wire.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ManualRevisionDto {
    /// Primary key.
    pub id: Uuid,
    /// Parent manual.
    pub manual_id: Uuid,
    /// Free-form revision string.
    pub revision: String,
    /// Status.
    pub status: RevisionStatusDto,
    /// Markdown body.
    pub body_md: String,
    /// Optional "what changed" note.
    pub change_note: Option<String>,
    /// Author.
    pub authored_by: Option<Uuid>,
    /// When created.
    pub created_at: DateTime<Utc>,
}

impl From<ManualRevision> for ManualRevisionDto {
    fn from(r: ManualRevision) -> Self {
        Self {
            id: r.id,
            manual_id: r.manual_id,
            revision: r.revision,
            status: r.status.into(),
            body_md: r.body_md,
            change_note: r.change_note,
            authored_by: r.authored_by,
            created_at: r.created_at,
        }
    }
}

/// Create body for a manual container.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct CreateManualRequest {
    /// Title.
    pub title: String,
}

/// Create body for a new revision.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct CreateRevisionRequest {
    /// Free-form revision string.
    pub revision: String,
    /// Markdown body.
    pub body_md: String,
    /// Optional "what changed" note.
    #[serde(default)]
    pub change_note: Option<String>,
}

// ---------------------------------------------------------------------------
// handlers
// ---------------------------------------------------------------------------

async fn ensure_product(state: &AppState, product_id: Uuid) -> Result<(), ApiError> {
    state.store.get_product(product_id).await?.ok_or(ApiError::NotFound {
        code: "product_not_found",
        message: "no product with that id".into(),
    })?;
    Ok(())
}

/// `GET /products/{id}/manuals`.
#[utoipa::path(get, path = "/products/{id}/manuals", params(("id" = Uuid, Path)),
    responses((status = 200, body = [ManualDto])), tag = "manufacturing")]
pub async fn list_manuals(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<ManualDto>>, ApiError> {
    let rows = state.store.list_product_manuals(id).await?;
    Ok(Json(rows.into_iter().map(ManualDto::from).collect()))
}

/// `POST /products/{id}/manuals`.
#[utoipa::path(post, path = "/products/{id}/manuals", params(("id" = Uuid, Path)),
    request_body = CreateManualRequest,
    responses((status = 200, body = ManualDto), (status = 404)), tag = "manufacturing")]
pub async fn create_manual(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateManualRequest>,
) -> Result<Json<ManualDto>, ApiError> {
    if body.title.trim().is_empty() {
        return Err(ApiError::BadRequest { code: "manual_title_required", message: "title must be non-empty".into() });
    }
    ensure_product(&state, id).await?;
    let u = ManualUpsert {
        product_id: id,
        title: body.title.trim().to_string(),
        created_by: Some(principal.actor_user_id),
    };
    let row = state.store.create_product_manual(&u).await?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::MANUAL_CREATE, format!("{id}:{}", row.id)).await.ok();
    Ok(Json(row.into()))
}

/// `GET /products/{id}/manuals/{manual_id}/revisions`.
#[utoipa::path(get, path = "/products/{id}/manuals/{manual_id}/revisions",
    params(("id" = Uuid, Path), ("manual_id" = Uuid, Path)),
    responses((status = 200, body = [ManualRevisionDto])), tag = "manufacturing")]
pub async fn list_revisions(
    State(state): State<AppState>,
    Path((_id, manual_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<ManualRevisionDto>>, ApiError> {
    let rows = state.store.list_manual_revisions(manual_id).await?;
    Ok(Json(rows.into_iter().map(ManualRevisionDto::from).collect()))
}

/// `POST /products/{id}/manuals/{manual_id}/revisions` — new revision.
#[utoipa::path(post, path = "/products/{id}/manuals/{manual_id}/revisions",
    params(("id" = Uuid, Path), ("manual_id" = Uuid, Path)),
    request_body = CreateRevisionRequest,
    responses((status = 200, body = ManualRevisionDto), (status = 404), (status = 409)), tag = "manufacturing")]
pub async fn create_revision(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((id, manual_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<CreateRevisionRequest>,
) -> Result<Json<ManualRevisionDto>, ApiError> {
    if body.revision.trim().is_empty() {
        return Err(ApiError::BadRequest { code: "revision_required", message: "revision string must be non-empty".into() });
    }
    let manual = state.store.get_product_manual(manual_id).await?.ok_or(ApiError::NotFound {
        code: "manual_not_found",
        message: "no manual with that id".into(),
    })?;
    // §8: the manual must belong to the path product.
    if manual.product_id != id {
        return Err(ApiError::NotFound { code: "manual_not_found", message: "manual does not belong to this product".into() });
    }
    let u = RevisionUpsert {
        revision: body.revision.trim().to_string(),
        body_md: body.body_md,
        change_note: body.change_note,
        authored_by: Some(principal.actor_user_id),
    };
    let row = state.store.create_manual_revision(manual_id, &u).await.map_err(|e| match e {
        StoreError::Conflict(msg) => ApiError::Conflict { code: "revision_string_taken", message: msg },
        other => other.into(),
    })?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::MANUAL_REVISION_ADD, format!("{}:{}", manual.id, row.id)).await.ok();
    Ok(Json(row.into()))
}

/// `POST /products/{id}/manuals/{manual_id}/revisions/{rev_id}/publish`.
#[utoipa::path(post, path = "/products/{id}/manuals/{manual_id}/revisions/{rev_id}/publish",
    params(("id" = Uuid, Path), ("manual_id" = Uuid, Path), ("rev_id" = Uuid, Path)),
    responses((status = 200, body = ManualRevisionDto), (status = 404)), tag = "manufacturing")]
pub async fn publish_revision(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((id, manual_id, rev_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<ManualRevisionDto>, ApiError> {
    // §8: the manual must belong to the path product.
    let manual = state.store.get_product_manual(manual_id).await?.ok_or(ApiError::NotFound {
        code: "manual_not_found",
        message: "no manual with that id".into(),
    })?;
    if manual.product_id != id {
        return Err(ApiError::NotFound { code: "manual_not_found", message: "manual does not belong to this product".into() });
    }
    let row = state.store.publish_manual_revision(manual_id, rev_id).await.map_err(|e| match e {
        StoreError::NotFound { .. } => ApiError::NotFound {
            code: "revision_not_found",
            message: "no such revision on this manual".into(),
        },
        other => other.into(),
    })?;
    audit::record(state.store.as_ref(), principal.actor_user_id, audit::MANUAL_REVISION_PUBLISH, format!("{manual_id}:{rev_id}")).await.ok();
    Ok(Json(row.into()))
}

// ---------------------------------------------------------------------------
// router
// ---------------------------------------------------------------------------

/// Build the manuals router, gated by `(manufacturing, read|write)`.
pub fn product_manuals_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new()
                .route("/products/{id}/manuals", get(list_manuals))
                .route("/products/{id}/manuals/{manual_id}/revisions", get(list_revisions)),
            "manufacturing",
            "read",
        ))
        .merge(with_permission(
            Router::new()
                .route("/products/{id}/manuals", post(create_manual))
                .route("/products/{id}/manuals/{manual_id}/revisions", post(create_revision))
                .route(
                    "/products/{id}/manuals/{manual_id}/revisions/{rev_id}/publish",
                    post(publish_revision),
                ),
            "manufacturing",
            "write",
        ))
        .with_state(inner)
}
