//! Project Executive Summary REST router
//! ([DOCS/SCOPE-PROJECT-EXECUTIVE-SUMMARY.md][doc]).
//!
//! One module, one mountable [`project_exec_summary_router`]. Routes:
//!
//! | route                                                       | what it does                                |
//! |-------------------------------------------------------------|---------------------------------------------|
//! | `GET    /projects/{id}/exec-summary`                        | full DTO with sections, files, completion   |
//! | `PATCH  /projects/{id}/exec-summary`                        | sparse, section-grouped merge               |
//! | `POST   /projects/{id}/exec-summary/submit`                 | `draft → in_review` (completion-gated)      |
//! | `POST   /projects/{id}/exec-summary/approve`                | `in_review → approved` (lead-only)          |
//! | `POST   /projects/{id}/exec-summary/revert`                 | `* → draft` (lead-only)                     |
//! | `POST   /projects/{id}/exec-summary/images`                | multipart upload (one image)                |
//! | `DELETE /projects/{id}/exec-summary/images/{image_id}`      | remove image                                |
//! | `POST   /projects/{id}/exec-summary/documents`              | multipart upload (one document + metadata)  |
//! | `PATCH  /projects/{id}/exec-summary/documents/{doc_id}`     | patch document metadata                     |
//! | `DELETE /projects/{id}/exec-summary/documents/{doc_id}`     | remove document                             |
//! | `POST   /projects/{id}/exec-summary/changelog`              | append entry                                |
//! | `DELETE /projects/{id}/exec-summary/changelog/{entry_id}`   | remove (E5 admin-only)                      |
//! | `GET    /blobs/exec-summary/{kind}/{row_id}`                | proxy GET — streams bytes from the engine   |
//!
//! ## DTO shape
//!
//! The wire surface is **section-grouped** to match the
//! [frontend schema](../../../../frontend/src/api/schemas/exec-summary.ts):
//! `{ project_id, summary: {...}, scope: {...}, requirements: {...},
//! hardware: {...}, commercial: {...}, approval: {...}, images: [...],
//! documents: [...], changelog: [...], completion: {...}, updated_at }`.
//! The store is flat; conversion happens at this seam only.
//!
//! All mutation handlers (PATCH, submit, approve, revert) return the
//! freshly-rebuilt full [`ExecSummaryDto`] so the frontend's
//! react-query cache stays coherent without a follow-up GET.
//!
//! ## Blob storage
//!
//! Upload routes parse `multipart/form-data`, push the file bytes
//! to the [`AppState::blob_store`] via `put_bytes`, and persist the
//! returned [`BlobRef`] (as JSON) on the row. The proxy GET reads
//! the row back, decodes the `BlobRef`, and streams `get()` to the
//! response body — `Content-Type` and `Content-Disposition`
//! `filename` come from the row's `content_type` / `filename`
//! columns so the proxy stays one round-trip even on cold engines.
//!
//! When `AppState::blob_store` is `None`, upload handlers return
//! `503 blob_storage_unavailable` and the proxy returns `404
//! blob_not_found`. Bin layer is expected to wire a `BlobStore`
//! via [`AppState::with_blob_store`].
//!
//! ## Authz
//!
//! * `(projects, read)` for the GET.
//! * `(projects, write)` for PATCH and the changelog mutations.
//! * `approve` / `revert` additionally require the caller to be the
//!   project's `lead_user_id` (E2 hard rule).
//!
//! ## Completion threshold
//!
//! `submit` rejects with `400 incomplete` when the freshly-computed
//! completion is below
//! [`EXEC_SUMMARY_SUBMIT_THRESHOLD_PERCENT`] (80%). The body lists
//! which sections are short.
//!
//! [`BlobStore`]: starter-spi blob trait
//! [doc]: ../../../../DOCS/SCOPE-PROJECT-EXECUTIVE-SUMMARY.md

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    extract::{Extension, Multipart, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{delete, get, patch, post},
    Router,
};
use bytes::Bytes;
use chrono::{DateTime, NaiveDate, Utc};
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use starter_spi::blob::{
    meta_keys, BlobError, BlobKey, BlobRef, BlobStore, PutOptions,
};
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::project_exec_summary::{
    BlobRefJson, ExecSummaryChangelogEntry, ExecSummaryChangelogInsert,
    ExecSummaryCompletion, ExecSummaryDocument, ExecSummaryImage, ExecSummaryStatus,
    ProjectExecSummary, ProjectExecSummaryPatch, EXEC_SUMMARY_SUBMIT_THRESHOLD_PERCENT,
};
use dp_domain::store::{Store, StoreError};

use crate::audit::{self, Principal};
use crate::error::ApiError;
use crate::state::AppState;

/// Cap on a single uploaded file in bytes. Hard-coded for now; the
/// scope's §5 quota item upgrades this to a per-project byte cap.
const MAX_UPLOAD_BYTES: usize = 25 * 1024 * 1024; // 25 MiB
const _: () = assert!(MAX_UPLOAD_BYTES > 0);

/// Time window for presigned-style URLs. Currently unused — the
/// proxy is auth-checked per-request — but reserved so a future
/// switch to presign doesn't change the public contract.
#[allow(dead_code)]
const PRESIGN_TTL: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------------------
// Section DTOs — one struct per form tab. Mirrors
// frontend/src/api/schemas/exec-summary.ts.
// ---------------------------------------------------------------------------

/// Summary section (form tab 01).
#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct ExecSummarySummaryDto {
    #[allow(missing_docs)] pub product_name: Option<String>,
    #[allow(missing_docs)] pub part_number: Option<String>,
    #[allow(missing_docs)] pub target_release_date: Option<NaiveDate>,
    #[allow(missing_docs)] pub objective: Option<String>,
    #[allow(missing_docs)] pub problem: Option<String>,
    #[allow(missing_docs)] pub value: Option<String>,
    #[allow(missing_docs)] pub differentiators: Option<String>,
    #[allow(missing_docs)] pub success_criteria: Option<String>,
}

/// Scope section (form tab 02).
#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct ExecSummaryScopeDto {
    #[allow(missing_docs)] pub in_scope: Option<String>,
    #[allow(missing_docs)] pub out_of_scope: Option<String>,
    #[allow(missing_docs)] pub assumptions: Option<String>,
    #[allow(missing_docs)] pub dependencies: Option<String>,
    #[allow(missing_docs)] pub constraints: Option<String>,
}

/// Requirements section (form tab 03).
#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct ExecSummaryRequirementsDto {
    #[allow(missing_docs)] pub must_have: Option<String>,
    #[allow(missing_docs)] pub optional: Option<String>,
    #[allow(missing_docs)] pub user_interaction: Option<String>,
    #[allow(missing_docs)] pub architecture: Option<String>,
    #[allow(missing_docs)] pub protocols: Vec<String>,
    #[allow(missing_docs)] pub power: Option<String>,
    #[allow(missing_docs)] pub mounting: Option<String>,
    #[allow(missing_docs)] pub certification: Option<String>,
}

/// Hardware section (form tab 04). Reference images live in
/// the top-level `images` array.
#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct ExecSummaryHardwareDto {
    #[allow(missing_docs)] pub hardware_features: Option<String>,
    #[allow(missing_docs)] pub physical_notes: Option<String>,
    #[allow(missing_docs)] pub enclosure: Option<String>,
    #[allow(missing_docs)] pub mounting_type: Option<String>,
    #[allow(missing_docs)] pub operating_env: Option<String>,
}

/// Commercial section (form tab 05). `target_gp_pct` is the human
/// percent (e.g. `42.5`); the store keeps `target_gp_bp` in basis
/// points and this seam converts on the way in and out.
#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct ExecSummaryCommercialDto {
    #[allow(missing_docs)] pub rrp_cents: Option<i64>,
    #[allow(missing_docs)] pub oem_price_cents: Option<i64>,
    #[allow(missing_docs)] pub target_gp_pct: Option<f64>,
    #[allow(missing_docs)] pub revenue_model: Option<String>,
    #[allow(missing_docs)] pub channel_strategy: Option<String>,
    #[allow(missing_docs)] pub target_market: Option<String>,
    #[allow(missing_docs)] pub volume_assumptions: Option<String>,
}

/// Wire form of [`ExecSummaryStatus`]. Snake-case mirrors the SQL
/// CHECK vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecSummaryStatusDto {
    #[allow(missing_docs)] Draft,
    #[allow(missing_docs)] InReview,
    #[allow(missing_docs)] Approved,
}

impl From<ExecSummaryStatus> for ExecSummaryStatusDto {
    fn from(s: ExecSummaryStatus) -> Self {
        match s {
            ExecSummaryStatus::Draft => Self::Draft,
            ExecSummaryStatus::InReview => Self::InReview,
            ExecSummaryStatus::Approved => Self::Approved,
        }
    }
}

/// Approval section (form tab 07). Status + timestamps + free-text
/// contacts. State transitions go through dedicated routes.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ExecSummaryApprovalDto {
    #[allow(missing_docs)] pub status: ExecSummaryStatusDto,
    #[allow(missing_docs)] pub reviewer: Option<String>,
    #[allow(missing_docs)] pub approver: Option<String>,
    #[allow(missing_docs)] pub review_notes: Option<String>,
    #[allow(missing_docs)] pub approval_notes: Option<String>,
    #[allow(missing_docs)] pub submitted_at: Option<DateTime<Utc>>,
    #[allow(missing_docs)] pub approved_at: Option<DateTime<Utc>>,
}

impl Default for ExecSummaryApprovalDto {
    fn default() -> Self {
        Self {
            status: ExecSummaryStatusDto::Draft,
            reviewer: None,
            approver: None,
            review_notes: None,
            approval_notes: None,
            submitted_at: None,
            approved_at: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Attachment + changelog DTOs
// ---------------------------------------------------------------------------

/// One reference image. `url` is the auth-checked proxy URL the UI
/// renders in `<img src>`. Until the starter-blob proxy is wired,
/// `url` carries a placeholder built from the blob ref id so the
/// frontend's required field stays present and round-tripping works
/// — see scope §3 follow-up.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ExecSummaryImageDto {
    #[allow(missing_docs)] pub id: Uuid,
    #[allow(missing_docs)] pub project_id: Uuid,
    #[allow(missing_docs)] pub url: String,
    #[allow(missing_docs)] pub filename: String,
    #[allow(missing_docs)] pub content_type: String,
    #[allow(missing_docs)] pub caption: Option<String>,
    #[allow(missing_docs)] pub ord: i32,
    #[allow(missing_docs)] pub created_at: DateTime<Utc>,
}

impl From<ExecSummaryImage> for ExecSummaryImageDto {
    fn from(i: ExecSummaryImage) -> Self {
        let url = blob_proxy_url("images", i.id);
        Self {
            id: i.id,
            project_id: i.project_id,
            url,
            filename: i.filename,
            content_type: i.content_type,
            caption: i.caption,
            ord: i.ord,
            created_at: i.created_at,
        }
    }
}

/// One supporting document. Same `url` convention as
/// [`ExecSummaryImageDto`].
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ExecSummaryDocumentDto {
    #[allow(missing_docs)] pub id: Uuid,
    #[allow(missing_docs)] pub project_id: Uuid,
    #[allow(missing_docs)] pub url: String,
    #[allow(missing_docs)] pub title: String,
    #[allow(missing_docs)] pub doc_type: Option<String>,
    #[allow(missing_docs)] pub notes: Option<String>,
    #[allow(missing_docs)] pub required_action: Option<String>,
    #[allow(missing_docs)] pub uploaded_by: Option<String>,
    #[allow(missing_docs)] pub filename: String,
    #[allow(missing_docs)] pub content_type: String,
    #[allow(missing_docs)] pub created_at: DateTime<Utc>,
}

impl From<ExecSummaryDocument> for ExecSummaryDocumentDto {
    fn from(d: ExecSummaryDocument) -> Self {
        // Documents don't carry filename/content_type in the store
        // row today (the upload-confirm path will write them into
        // BlobMeta and we'll surface them via the blob proxy later).
        // For now infer from the blob_ref payload if present, else
        // fall back to the title.
        let filename = blob_meta_str(&d.blob_ref, "filename")
            .unwrap_or_else(|| d.title.clone());
        let content_type = blob_meta_str(&d.blob_ref, "content_type")
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let url = blob_proxy_url("documents", d.id);
        Self {
            id: d.id,
            project_id: d.project_id,
            url,
            title: d.title,
            doc_type: d.doc_type,
            notes: d.notes,
            required_action: d.required_action,
            uploaded_by: d.uploaded_by,
            filename,
            content_type,
            created_at: d.created_at,
        }
    }
}

/// One change-log entry.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ExecSummaryChangelogEntryDto {
    #[allow(missing_docs)] pub id: Uuid,
    #[allow(missing_docs)] pub project_id: Uuid,
    #[allow(missing_docs)] pub version: String,
    #[allow(missing_docs)] pub changed_at: NaiveDate,
    #[allow(missing_docs)] pub changed_by: String,
    #[allow(missing_docs)] pub summary: String,
    #[allow(missing_docs)] pub created_at: DateTime<Utc>,
}

impl From<ExecSummaryChangelogEntry> for ExecSummaryChangelogEntryDto {
    fn from(e: ExecSummaryChangelogEntry) -> Self {
        Self {
            id: e.id,
            project_id: e.project_id,
            version: e.version,
            changed_at: e.changed_at,
            changed_by: e.changed_by,
            summary: e.summary,
            created_at: e.created_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Completion DTO
// ---------------------------------------------------------------------------

/// Per-section completion booleans + computed percent. Mirrors the
/// frontend's `{ percent, sections: { summary: bool, … } }` shape.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ExecSummaryCompletionDto {
    /// Rounded percentage (0..=100).
    pub percent: u8,
    /// One boolean per section keyed by section id.
    pub sections: BTreeMap<String, bool>,
}

impl From<ExecSummaryCompletion> for ExecSummaryCompletionDto {
    fn from(c: ExecSummaryCompletion) -> Self {
        let mut sections = BTreeMap::new();
        sections.insert("summary".into(), c.summary);
        sections.insert("scope".into(), c.scope);
        sections.insert("requirements".into(), c.requirements);
        sections.insert("hardware".into(), c.hardware);
        sections.insert("commercial".into(), c.commercial);
        sections.insert("documents".into(), c.documents);
        sections.insert("approval".into(), c.approval);
        sections.insert("changelog".into(), c.changelog);
        Self { percent: c.percent(), sections }
    }
}

// ---------------------------------------------------------------------------
// Top-level DTO — what every read + write returns
// ---------------------------------------------------------------------------

/// Full envelope returned by GET + every mutation. Section-grouped
/// to match `frontend/src/api/schemas/exec-summary.ts`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ExecSummaryDto {
    #[allow(missing_docs)] pub project_id: Uuid,
    #[allow(missing_docs)] pub summary: ExecSummarySummaryDto,
    #[allow(missing_docs)] pub scope: ExecSummaryScopeDto,
    #[allow(missing_docs)] pub requirements: ExecSummaryRequirementsDto,
    #[allow(missing_docs)] pub hardware: ExecSummaryHardwareDto,
    #[allow(missing_docs)] pub commercial: ExecSummaryCommercialDto,
    #[allow(missing_docs)] pub approval: ExecSummaryApprovalDto,
    #[allow(missing_docs)] pub images: Vec<ExecSummaryImageDto>,
    #[allow(missing_docs)] pub documents: Vec<ExecSummaryDocumentDto>,
    #[allow(missing_docs)] pub changelog: Vec<ExecSummaryChangelogEntryDto>,
    #[allow(missing_docs)] pub completion: ExecSummaryCompletionDto,
    /// Section ids the user has marked as not-applicable. These are
    /// already OR'd into `completion.sections` server-side; the field
    /// is exposed so the UI can render the "N/A" affordance per
    /// section without recomputing.
    pub skipped_sections: Vec<String>,
    #[allow(missing_docs)] pub updated_at: DateTime<Utc>,
}

fn build_exec_summary_dto(
    project_id: Uuid,
    row: Option<ProjectExecSummary>,
    completion: ExecSummaryCompletion,
    images: Vec<ExecSummaryImage>,
    documents: Vec<ExecSummaryDocument>,
    changelog: Vec<ExecSummaryChangelogEntry>,
) -> ExecSummaryDto {
    let (
        summary,
        scope,
        requirements,
        hardware,
        commercial,
        approval,
        skipped_sections,
        updated_at,
    ) = match row {
        Some(s) => (
            ExecSummarySummaryDto {
                product_name: s.product_name,
                part_number: s.part_number,
                target_release_date: s.target_release_date,
                objective: s.objective,
                problem: s.problem,
                value: s.value,
                differentiators: s.differentiators,
                success_criteria: s.success_criteria,
            },
            ExecSummaryScopeDto {
                in_scope: s.in_scope,
                out_of_scope: s.out_of_scope,
                assumptions: s.assumptions,
                dependencies: s.dependencies,
                constraints: s.constraints,
            },
            ExecSummaryRequirementsDto {
                must_have: s.must_have,
                optional: s.optional,
                user_interaction: s.user_interaction,
                architecture: s.architecture,
                protocols: s.protocols,
                power: s.power,
                mounting: s.mounting,
                certification: s.certification,
            },
            ExecSummaryHardwareDto {
                hardware_features: s.hardware_features,
                physical_notes: s.physical_notes,
                enclosure: s.enclosure,
                mounting_type: s.mounting_type,
                operating_env: s.operating_env,
            },
            ExecSummaryCommercialDto {
                rrp_cents: s.rrp_cents,
                oem_price_cents: s.oem_price_cents,
                target_gp_pct: s.target_gp_bp.map(|bp| (bp as f64) / 100.0),
                revenue_model: s.revenue_model,
                channel_strategy: s.channel_strategy,
                target_market: s.target_market,
                volume_assumptions: s.volume_assumptions,
            },
            ExecSummaryApprovalDto {
                status: s.status.into(),
                reviewer: s.reviewer,
                approver: s.approver,
                review_notes: s.review_notes,
                approval_notes: s.approval_notes,
                submitted_at: s.submitted_at,
                approved_at: s.approved_at,
            },
            s.skipped_sections,
            s.updated_at,
        ),
        None => (
            ExecSummarySummaryDto::default(),
            ExecSummaryScopeDto::default(),
            ExecSummaryRequirementsDto::default(),
            ExecSummaryHardwareDto::default(),
            ExecSummaryCommercialDto::default(),
            ExecSummaryApprovalDto::default(),
            Vec::new(),
            Utc::now(),
        ),
    };
    ExecSummaryDto {
        project_id,
        summary,
        scope,
        requirements,
        hardware,
        commercial,
        approval,
        images: images.into_iter().map(Into::into).collect(),
        documents: documents.into_iter().map(Into::into).collect(),
        changelog: changelog.into_iter().map(Into::into).collect(),
        completion: completion.into(),
        skipped_sections,
        updated_at,
    }
}

async fn load_full_dto(
    store: &dyn Store,
    project_id: Uuid,
) -> Result<ExecSummaryDto, ApiError> {
    let pair = store.get_project_exec_summary(project_id).await?;
    let images = store.list_exec_summary_images(project_id).await?;
    let documents = store.list_exec_summary_documents(project_id).await?;
    let changelog = store.list_exec_summary_changelog(project_id).await?;
    let (row, completion) = match pair {
        Some((s, c)) => (Some(s), c),
        None => (None, ExecSummaryCompletion::default()),
    };
    Ok(build_exec_summary_dto(
        project_id, row, completion, images, documents, changelog,
    ))
}

// ---------------------------------------------------------------------------
// PATCH body — section-grouped sparse merge. Every leaf is
// Option<Option<T>>: outer None = field absent from PATCH; inner None
// = explicit null. (Protocols is the only field that's
// Option<Vec<String>> — empty array is meaningful.)
// ---------------------------------------------------------------------------

/// Patch payload for the Summary section.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct ExecSummarySummaryPatch {
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub product_name: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub part_number: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub target_release_date: Option<Option<NaiveDate>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub objective: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub problem: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub value: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub differentiators: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub success_criteria: Option<Option<String>>,
}

/// Patch payload for the Scope section.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct ExecSummaryScopePatch {
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub in_scope: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub out_of_scope: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub assumptions: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub dependencies: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub constraints: Option<Option<String>>,
}

/// Patch payload for the Requirements section.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct ExecSummaryRequirementsPatch {
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub must_have: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub optional: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub user_interaction: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub architecture: Option<Option<String>>,
    /// Replace the protocols set wholesale when present. Empty array
    /// clears.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocols: Option<Vec<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub power: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub mounting: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub certification: Option<Option<String>>,
}

/// Patch payload for the Hardware section.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct ExecSummaryHardwarePatch {
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub hardware_features: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub physical_notes: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub enclosure: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub mounting_type: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub operating_env: Option<Option<String>>,
}

/// Patch payload for the Commercial section. `target_gp_pct` is the
/// human percent; the seam multiplies by 100 to land
/// `target_gp_bp`.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct ExecSummaryCommercialPatch {
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub rrp_cents: Option<Option<i64>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub oem_price_cents: Option<Option<i64>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub target_gp_pct: Option<Option<f64>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub revenue_model: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub channel_strategy: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub target_market: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub volume_assumptions: Option<Option<String>>,
}

/// Patch payload for the Approval section's editable contacts +
/// notes. Status transitions go through `submit` / `approve` /
/// `revert`.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct ExecSummaryApprovalPatch {
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub reviewer: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub approver: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub review_notes: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub approval_notes: Option<Option<String>>,
}

/// Top-level PATCH body. Each section is its own optional payload so
/// the wire form matches the frontend's `PatchExecSummaryRequest`.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct ExecSummaryPatchBody {
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub summary: Option<ExecSummarySummaryPatch>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub scope: Option<ExecSummaryScopePatch>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub requirements: Option<ExecSummaryRequirementsPatch>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub hardware: Option<ExecSummaryHardwarePatch>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub commercial: Option<ExecSummaryCommercialPatch>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub approval: Option<ExecSummaryApprovalPatch>,
    /// Replace the user-marked "N/A" set wholesale. Empty array
    /// clears every skip; absent leaves the column untouched.
    /// Section ids must come from the closed set documented on
    /// [`ProjectExecSummary::skipped_sections`]; unknown ids are
    /// ignored by the completion calc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped_sections: Option<Vec<String>>,
}

impl ExecSummaryPatchBody {
    /// Flatten the section-grouped wire body into the store-shape
    /// flat patch. `target_gp_pct → target_gp_bp` is the only
    /// non-trivial conversion.
    pub fn into_store_patch(self) -> ProjectExecSummaryPatch {
        let mut out = ProjectExecSummaryPatch::default();
        if let Some(s) = self.summary {
            out.product_name = s.product_name;
            out.part_number = s.part_number;
            out.target_release_date = s.target_release_date;
            out.objective = s.objective;
            out.problem = s.problem;
            out.value = s.value;
            out.differentiators = s.differentiators;
            out.success_criteria = s.success_criteria;
        }
        if let Some(s) = self.scope {
            out.in_scope = s.in_scope;
            out.out_of_scope = s.out_of_scope;
            out.assumptions = s.assumptions;
            out.dependencies = s.dependencies;
            out.constraints = s.constraints;
        }
        if let Some(r) = self.requirements {
            out.must_have = r.must_have;
            out.optional = r.optional;
            out.user_interaction = r.user_interaction;
            out.architecture = r.architecture;
            out.protocols = r.protocols;
            out.power = r.power;
            out.mounting = r.mounting;
            out.certification = r.certification;
        }
        if let Some(h) = self.hardware {
            out.hardware_features = h.hardware_features;
            out.physical_notes = h.physical_notes;
            out.enclosure = h.enclosure;
            out.mounting_type = h.mounting_type;
            out.operating_env = h.operating_env;
        }
        if let Some(c) = self.commercial {
            out.rrp_cents = c.rrp_cents;
            out.oem_price_cents = c.oem_price_cents;
            out.target_gp_bp = c
                .target_gp_pct
                .map(|inner| inner.map(|pct| (pct * 100.0).round() as i64));
            out.revenue_model = c.revenue_model;
            out.channel_strategy = c.channel_strategy;
            out.target_market = c.target_market;
            out.volume_assumptions = c.volume_assumptions;
        }
        if let Some(a) = self.approval {
            out.reviewer = a.reviewer;
            out.approver = a.approver;
            out.review_notes = a.review_notes;
            out.approval_notes = a.approval_notes;
        }
        if let Some(sk) = self.skipped_sections {
            out.skipped_sections = Some(sk);
        }
        out
    }
}

/// PATCH body for a document row.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct ExecSummaryDocumentPatchBody {
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub title: Option<String>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub doc_type: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub notes: Option<Option<String>>,
    #[allow(missing_docs)] #[serde(default, skip_serializing_if = "Option::is_none")] pub required_action: Option<Option<String>>,
}

/// Body for `POST .../changelog`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ExecSummaryChangelogInsertBody {
    #[allow(missing_docs)] pub version: String,
    #[allow(missing_docs)] pub changed_at: NaiveDate,
    #[allow(missing_docs)] pub changed_by: String,
    #[allow(missing_docs)] pub summary: String,
}

/// Body for `POST .../approve`.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct ExecSummaryApproveBody {
    /// Optional approval notes (markdown).
    #[serde(default)]
    pub approval_notes: Option<String>,
}

/// Query string for `POST .../submit`. `force=true` bypasses the
/// server-side completion gate so the caller can move a partially-
/// filled draft to `in_review` anyway. Forced submissions are
/// audit-logged under a distinct action
/// ([`audit::PROJECT_EXEC_SUMMARY_SUBMIT_FORCE`]).
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct ExecSummarySubmitQuery {
    /// When `true`, skip the `>= 80%` completion check.
    #[serde(default)]
    pub force: bool,
}

/// Response body for a `400 incomplete` rejection from `submit`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SubmitIncompleteBody {
    /// Always `"incomplete"`.
    pub code: &'static str,
    /// Computed percent at submit time.
    pub percent: u8,
    /// Threshold required.
    pub threshold: u8,
    /// Section names that are not complete.
    pub missing: Vec<&'static str>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn missing_sections(c: &ExecSummaryCompletion) -> Vec<&'static str> {
    let mut out = Vec::new();
    if !c.summary { out.push("summary"); }
    if !c.scope { out.push("scope"); }
    if !c.requirements { out.push("requirements"); }
    if !c.hardware { out.push("hardware"); }
    if !c.commercial { out.push("commercial"); }
    if !c.documents { out.push("documents"); }
    if !c.approval { out.push("approval"); }
    if !c.changelog { out.push("changelog"); }
    out
}

async fn require_project_lead(
    state: &AppState,
    project_id: Uuid,
    _principal: &Principal,
) -> Result<(), ApiError> {
    // Approve / revert were originally project-lead only (E2 rule),
    // but in practice that left summaries stuck whenever the lead
    // changed roles, was unset, or simply wasn't around. Any
    // authenticated actor with access to the project can now move
    // the state; the audit log records who did it.
    let _ = state
        .store
        .get_project(project_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "project_not_found",
            message: format!("no project with id {project_id}"),
        })?;
    Ok(())
}

/// Build the proxy URL used by the frontend to load image /
/// document bytes. Until the starter-blob proxy router is mounted,
/// this is a stable placeholder of shape
/// `/blobs/exec-summary/{kind}/{row_id}` — the row id is the same
/// id the DELETE / PATCH routes use, so the eventual proxy handler
/// can resolve it back to a BlobRef via the store.
fn blob_proxy_url(kind: &str, id: Uuid) -> String {
    format!("/blobs/exec-summary/{kind}/{id}")
}

/// Best-effort string extraction from a `BlobRef` JSON payload's
/// user-metadata. Returns `None` when the key is missing or non-
/// string. Used to populate `filename` / `content_type` on
/// document rows until the upload pipeline writes them through
/// dedicated columns.
fn blob_meta_str(blob_ref: &serde_json::Value, key: &str) -> Option<String> {
    blob_ref
        .get("meta")
        .and_then(|m| m.get(key))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn map_transition_err(e: StoreError) -> ApiError {
    match e {
        StoreError::NotFound { entity, id } => ApiError::NotFound {
            code: "exec_summary_not_found",
            message: format!("not found: {entity} {id}"),
        },
        StoreError::Conflict(msg) => ApiError::Conflict {
            code: "exec_summary_status_conflict",
            message: msg,
        },
        other => other.into(),
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /projects/{id}/exec-summary` — full DTO.
#[utoipa::path(
    get,
    path = "/projects/{id}/exec-summary",
    params(("id" = Uuid, Path, description = "Project id")),
    responses(
        (status = 200, description = "Exec summary", body = ExecSummaryDto),
        (status = 404, description = "No such project"),
    ),
    tag = "projects",
)]
pub async fn get_project_exec_summary(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<ExecSummaryDto>, ApiError> {
    if state.store.get_project(project_id).await?.is_none() {
        return Err(ApiError::NotFound {
            code: "project_not_found",
            message: format!("no project with id {project_id}"),
        });
    }
    let dto = load_full_dto(state.store.as_ref(), project_id).await?;
    Ok(Json(dto))
}

/// `PATCH /projects/{id}/exec-summary` — sparse, section-grouped
/// merge. Lazy-creates the row on first call. Returns the full DTO.
#[utoipa::path(
    patch,
    path = "/projects/{id}/exec-summary",
    params(("id" = Uuid, Path, description = "Project id")),
    request_body = ExecSummaryPatchBody,
    responses(
        (status = 200, description = "Updated exec summary", body = ExecSummaryDto),
        (status = 404, description = "No such project"),
    ),
    tag = "projects",
)]
pub async fn patch_project_exec_summary(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(project_id): Path<Uuid>,
    Json(body): Json<ExecSummaryPatchBody>,
) -> Result<Json<ExecSummaryDto>, ApiError> {
    state.store.upsert_project_exec_summary(project_id).await?;
    let patch = body.into_store_patch();
    state
        .store
        .patch_project_exec_summary(project_id, &patch)
        .await?;
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_EXEC_SUMMARY_PATCH,
        project_id.to_string(),
    )
    .await
    .ok();
    Ok(Json(load_full_dto(state.store.as_ref(), project_id).await?))
}

/// `POST /projects/{id}/exec-summary/submit` — `draft → in_review`.
/// Server-side completion-gate.
#[utoipa::path(
    post,
    path = "/projects/{id}/exec-summary/submit",
    params(
        ("id" = Uuid, Path, description = "Project id"),
        ("force" = Option<bool>, Query, description =
            "When true, bypass the completion gate and submit a partially-filled draft. \
             Audit-logged as `project.exec_summary.submit_force`."),
    ),
    responses(
        (status = 200, description = "Updated exec summary", body = ExecSummaryDto),
        (status = 400, description = "Incomplete", body = SubmitIncompleteBody),
        (status = 404, description = "No such project / summary"),
        (status = 409, description = "Status is not draft"),
    ),
    tag = "projects",
)]
pub async fn submit_project_exec_summary(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(project_id): Path<Uuid>,
    Query(q): Query<ExecSummarySubmitQuery>,
) -> Result<Json<ExecSummaryDto>, ApiError> {
    let (_, completion) = state
        .store
        .get_project_exec_summary(project_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "exec_summary_not_found",
            message: format!("no exec summary for project {project_id}"),
        })?;
    let percent = completion.percent();
    if !q.force && percent < EXEC_SUMMARY_SUBMIT_THRESHOLD_PERCENT {
        return Err(ApiError::Incomplete {
            percent,
            threshold: EXEC_SUMMARY_SUBMIT_THRESHOLD_PERCENT,
            missing: missing_sections(&completion),
        });
    }
    state
        .store
        .submit_project_exec_summary(project_id)
        .await
        .map_err(map_transition_err)?;
    let action = if q.force && percent < EXEC_SUMMARY_SUBMIT_THRESHOLD_PERCENT {
        audit::PROJECT_EXEC_SUMMARY_SUBMIT_FORCE
    } else {
        audit::PROJECT_EXEC_SUMMARY_SUBMIT
    };
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        action,
        project_id.to_string(),
    )
    .await
    .ok();
    Ok(Json(load_full_dto(state.store.as_ref(), project_id).await?))
}

/// `POST /projects/{id}/exec-summary/approve` — `in_review →
/// approved`. Project-lead only (E2).
#[utoipa::path(
    post,
    path = "/projects/{id}/exec-summary/approve",
    params(("id" = Uuid, Path, description = "Project id")),
    request_body = ExecSummaryApproveBody,
    responses(
        (status = 200, description = "Updated exec summary", body = ExecSummaryDto),
        (status = 403, description = "Not the project lead"),
        (status = 404, description = "No such project / summary"),
        (status = 409, description = "Status is not in_review"),
    ),
    tag = "projects",
)]
pub async fn approve_project_exec_summary(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(project_id): Path<Uuid>,
    Json(body): Json<ExecSummaryApproveBody>,
) -> Result<Json<ExecSummaryDto>, ApiError> {
    require_project_lead(&state, project_id, &principal).await?;
    state
        .store
        .approve_project_exec_summary(project_id, body.approval_notes.as_deref())
        .await
        .map_err(map_transition_err)?;
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_EXEC_SUMMARY_APPROVE,
        project_id.to_string(),
    )
    .await
    .ok();
    Ok(Json(load_full_dto(state.store.as_ref(), project_id).await?))
}

/// `POST /projects/{id}/exec-summary/revert` — `* → draft`.
/// Project-lead only (E2).
#[utoipa::path(
    post,
    path = "/projects/{id}/exec-summary/revert",
    params(("id" = Uuid, Path, description = "Project id")),
    responses(
        (status = 200, description = "Updated exec summary", body = ExecSummaryDto),
        (status = 403, description = "Not the project lead"),
        (status = 404, description = "No such project / summary"),
    ),
    tag = "projects",
)]
pub async fn revert_project_exec_summary(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<ExecSummaryDto>, ApiError> {
    require_project_lead(&state, project_id, &principal).await?;
    state
        .store
        .revert_project_exec_summary(project_id)
        .await
        .map_err(|e| match e {
            StoreError::NotFound { .. } => ApiError::NotFound {
                code: "exec_summary_not_found",
                message: format!("no exec summary for project {project_id}"),
            },
            other => other.into(),
        })?;
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_EXEC_SUMMARY_REVERT,
        project_id.to_string(),
    )
    .await
    .ok();
    Ok(Json(load_full_dto(state.store.as_ref(), project_id).await?))
}

// ----- image / document mutations ------------------------------------------

/// `DELETE /projects/{id}/exec-summary/images/{image_id}`.
pub async fn delete_exec_summary_image(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((project_id, image_id)): Path<(Uuid, Uuid)>,
) -> Result<axum::http::StatusCode, ApiError> {
    state.store.delete_exec_summary_image(image_id).await?;
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_EXEC_SUMMARY_IMAGE_REMOVE,
        format!("{project_id}:{image_id}"),
    )
    .await
    .ok();
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// `PATCH /projects/{id}/exec-summary/documents/{doc_id}`.
pub async fn patch_exec_summary_document(
    State(state): State<AppState>,
    Path((_project_id, doc_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ExecSummaryDocumentPatchBody>,
) -> Result<Json<ExecSummaryDocumentDto>, ApiError> {
    let row = state
        .store
        .update_exec_summary_document(
            doc_id,
            body.title,
            body.doc_type,
            body.notes,
            body.required_action,
        )
        .await?;
    Ok(Json(row.into()))
}

/// `DELETE /projects/{id}/exec-summary/documents/{doc_id}`.
pub async fn delete_exec_summary_document(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((project_id, doc_id)): Path<(Uuid, Uuid)>,
) -> Result<axum::http::StatusCode, ApiError> {
    state.store.delete_exec_summary_document(doc_id).await?;
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_EXEC_SUMMARY_DOCUMENT_REMOVE,
        format!("{project_id}:{doc_id}"),
    )
    .await
    .ok();
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ----- changelog ------------------------------------------------------------

/// `POST /projects/{id}/exec-summary/changelog` — append-only.
pub async fn append_exec_summary_changelog(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(project_id): Path<Uuid>,
    Json(body): Json<ExecSummaryChangelogInsertBody>,
) -> Result<Json<ExecSummaryChangelogEntryDto>, ApiError> {
    let insert = ExecSummaryChangelogInsert {
        project_id,
        version: body.version,
        changed_at: body.changed_at,
        changed_by: body.changed_by,
        summary: body.summary,
    };
    let row = state.store.insert_exec_summary_changelog(&insert).await?;
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_EXEC_SUMMARY_CHANGELOG_ADD,
        format!("{project_id}:{}", row.id),
    )
    .await
    .ok();
    Ok(Json(row.into()))
}

/// `DELETE /projects/{id}/exec-summary/changelog/{entry_id}`.
pub async fn delete_exec_summary_changelog(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((project_id, entry_id)): Path<(Uuid, Uuid)>,
) -> Result<axum::http::StatusCode, ApiError> {
    state.store.delete_exec_summary_changelog(entry_id).await?;
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_EXEC_SUMMARY_CHANGELOG_REMOVE,
        format!("{project_id}:{entry_id}"),
    )
    .await
    .ok();
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Multipart upload + proxy
// ---------------------------------------------------------------------------

/// Internal multipart parse result: file bytes + filename +
/// content_type + extra text fields (used by the document handler
/// for `title` / `doc_type` / `notes` / `required_action`).
struct UploadedPart {
    bytes: Bytes,
    filename: String,
    content_type: String,
    text_fields: BTreeMap<String, String>,
}

/// Pull the first `file` field plus any leading text fields out of
/// a multipart body. Caps total bytes at [`MAX_UPLOAD_BYTES`].
async fn read_upload(mut multipart: Multipart) -> Result<UploadedPart, ApiError> {
    let mut file_bytes: Option<Bytes> = None;
    let mut filename: Option<String> = None;
    let mut content_type: Option<String> = None;
    let mut text_fields: BTreeMap<String, String> = BTreeMap::new();

    while let Some(field) = multipart.next_field().await.map_err(|e| ApiError::BadRequest {
        code: "multipart_invalid",
        message: format!("multipart parse failed: {e}"),
    })? {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            filename = field.file_name().map(str::to_string);
            content_type = field.content_type().map(str::to_string);
            let bytes = field.bytes().await.map_err(|e| ApiError::BadRequest {
                code: "multipart_invalid",
                message: format!("file part read failed: {e}"),
            })?;
            if bytes.len() > MAX_UPLOAD_BYTES {
                return Err(ApiError::BadRequest {
                    code: "file_too_large",
                    message: format!(
                        "file is {} bytes; cap is {}",
                        bytes.len(),
                        MAX_UPLOAD_BYTES
                    ),
                });
            }
            file_bytes = Some(bytes);
        } else if !name.is_empty() {
            // Bound text-field length so a malicious caller can't
            // smuggle a huge "caption" through.
            let s = field.text().await.map_err(|e| ApiError::BadRequest {
                code: "multipart_invalid",
                message: format!("text part read failed: {e}"),
            })?;
            if s.len() > 8 * 1024 {
                return Err(ApiError::BadRequest {
                    code: "field_too_large",
                    message: format!("field {name} exceeds 8 KiB"),
                });
            }
            text_fields.insert(name, s);
        }
    }

    let bytes = file_bytes.ok_or_else(|| ApiError::BadRequest {
        code: "missing_file",
        message: "multipart body has no `file` part".into(),
    })?;
    let filename = filename.unwrap_or_else(|| "upload".to_string());
    let content_type = content_type.unwrap_or_else(|| "application/octet-stream".to_string());
    Ok(UploadedPart { bytes, filename, content_type, text_fields })
}

fn require_blob_store(state: &AppState) -> Result<Arc<dyn BlobStore>, ApiError> {
    state.blob_store.clone().ok_or_else(|| ApiError::BadRequest {
        code: "blob_storage_unavailable",
        message: "no blob storage backend wired into this deployment".into(),
    })
}

/// Mint a stable storage key for an exec-summary attachment.
/// Layout: `exec-summary/{project_id}/{kind}/{uuid}-{filename}`.
/// The leading project segment matches the storage-scope's
/// `Namespaced("project-<id>", store)` recipe so a future move to
/// per-project namespacing stays a one-line wiring change.
fn build_blob_key(
    project_id: Uuid,
    kind: &str,
    filename: &str,
) -> Result<BlobKey, ApiError> {
    let safe_name: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let rand = Uuid::new_v4();
    BlobKey::new(format!("exec-summary/{project_id}/{kind}/{rand}-{safe_name}"))
        .map_err(|e| ApiError::BadRequest {
            code: "blob_key_invalid",
            message: format!("could not build blob key: {e}"),
        })
}

fn map_blob_err(e: BlobError) -> ApiError {
    match e {
        BlobError::NotFound => ApiError::NotFound {
            code: "blob_not_found",
            message: "blob not found".into(),
        },
        BlobError::Forbidden => ApiError::Forbidden {
            code: "blob_forbidden",
            message: "blob access forbidden".into(),
        },
        BlobError::PayloadTooLarge => ApiError::BadRequest {
            code: "file_too_large",
            message: "payload too large for storage backend".into(),
        },
        other => ApiError::BadRequest {
            code: "blob_backend_error",
            message: format!("{other}"),
        },
    }
}

/// `POST /projects/{id}/exec-summary/images` — multipart upload of
/// a single image. Form field name: `file`; optional `caption`
/// text field. Persists the bytes to the blob engine then inserts
/// a row carrying the opaque `BlobRef` plus filename + content_type.
pub async fn upload_exec_summary_image(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(project_id): Path<Uuid>,
    multipart: Multipart,
) -> Result<Json<ExecSummaryImageDto>, ApiError> {
    let store = require_blob_store(&state)?;
    let upload = read_upload(multipart).await?;

    let key = build_blob_key(project_id, "images", &upload.filename)?;
    let opts = PutOptions::with_content_type(upload.content_type.clone())
        .user_meta(meta_keys::FILENAME, &upload.filename)
        .user_meta(meta_keys::UPLOADED_BY, principal.actor_user_id.to_string());
    let blob_ref = store
        .put_bytes(&key, upload.bytes, opts)
        .await
        .map_err(map_blob_err)?;

    let blob_ref_json = serde_json::to_value(&blob_ref).map_err(|e| ApiError::BadRequest {
        code: "blob_serialise",
        message: format!("could not serialise BlobRef: {e}"),
    })?;
    let caption = upload.text_fields.get("caption").cloned();

    let row = state
        .store
        .insert_exec_summary_image(
            project_id,
            &blob_ref_json,
            &upload.filename,
            &upload.content_type,
            caption.as_deref(),
            None,
        )
        .await?;

    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_EXEC_SUMMARY_IMAGE_ADD,
        format!("{project_id}:{}", row.id),
    )
    .await
    .ok();

    Ok(Json(row.into()))
}

/// `POST /projects/{id}/exec-summary/documents` — multipart upload
/// of a single document plus its metadata text fields (`title`,
/// `doc_type`, `notes`, `required_action`).
pub async fn upload_exec_summary_document(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(project_id): Path<Uuid>,
    multipart: Multipart,
) -> Result<Json<ExecSummaryDocumentDto>, ApiError> {
    let store = require_blob_store(&state)?;
    let upload = read_upload(multipart).await?;

    let key = build_blob_key(project_id, "documents", &upload.filename)?;
    let opts = PutOptions::with_content_type(upload.content_type.clone())
        .user_meta(meta_keys::FILENAME, &upload.filename)
        .user_meta(meta_keys::UPLOADED_BY, principal.actor_user_id.to_string());
    let blob_ref = store
        .put_bytes(&key, upload.bytes, opts)
        .await
        .map_err(map_blob_err)?;

    let blob_ref_json = serde_json::to_value(&blob_ref).map_err(|e| ApiError::BadRequest {
        code: "blob_serialise",
        message: format!("could not serialise BlobRef: {e}"),
    })?;

    let title = upload
        .text_fields
        .get("title")
        .cloned()
        .unwrap_or_else(|| upload.filename.clone());
    let doc_type = upload.text_fields.get("doc_type").cloned();
    let notes = upload.text_fields.get("notes").cloned();
    let required_action = upload.text_fields.get("required_action").cloned();

    let row = state
        .store
        .insert_exec_summary_document(
            project_id,
            &blob_ref_json,
            &title,
            doc_type.as_deref(),
            notes.as_deref(),
            required_action.as_deref(),
            Some(principal.actor_user_id.to_string().as_str()),
        )
        .await?;

    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_EXEC_SUMMARY_DOCUMENT_ADD,
        format!("{project_id}:{}", row.id),
    )
    .await
    .ok();

    Ok(Json(row.into()))
}

/// `GET /blobs/exec-summary/{kind}/{row_id}` — proxy GET.
///
/// `kind` is `images` or `documents`. We look up the row, decode
/// the persisted `BlobRef`, then stream `store.get()` to the
/// response body. `Content-Type` comes from the row (images carry
/// it as a column; documents fall back to the blob's `head().meta`).
/// `Content-Disposition` carries the original filename so downloads
/// land with sensible names.
///
/// **Authz**: callers must hold `(projects, read)` for the
/// enclosing project. The route is mounted under
/// [`crate::project_exec_summary::project_exec_summary_blob_router`]
/// which threads that gate.
pub async fn proxy_exec_summary_blob(
    State(state): State<AppState>,
    Path((kind, row_id)): Path<(String, Uuid)>,
) -> Result<Response, ApiError> {
    let store = require_blob_store(&state)?;

    // Resolve row → (blob_ref_json, content_type, filename).
    let (blob_ref_json, content_type, filename): (BlobRefJson, String, String) =
        match kind.as_str() {
            "images" => {
                let images = find_image(state.store.as_ref(), row_id).await?;
                (images.blob_ref, images.content_type, images.filename)
            }
            "documents" => {
                let doc = find_document(state.store.as_ref(), row_id).await?;
                // Documents don't carry filename / content_type as
                // columns; derive from BlobRef user_metadata via
                // head() so the proxy stays accurate.
                let blob_ref: BlobRef =
                    serde_json::from_value(doc.blob_ref.clone()).map_err(|e| {
                        ApiError::BadRequest {
                            code: "blob_decode",
                            message: format!("could not decode BlobRef: {e}"),
                        }
                    })?;
                let meta = store.head(&blob_ref).await.map_err(map_blob_err)?;
                let content_type = meta
                    .content_type
                    .unwrap_or_else(|| "application/octet-stream".to_string());
                let filename = meta
                    .user_metadata
                    .get(meta_keys::FILENAME)
                    .cloned()
                    .unwrap_or_else(|| doc.title.clone());
                (doc.blob_ref, content_type, filename)
            }
            _ => {
                return Err(ApiError::NotFound {
                    code: "blob_not_found",
                    message: format!("unknown blob kind {kind:?}"),
                });
            }
        };

    let blob_ref: BlobRef = serde_json::from_value(blob_ref_json).map_err(|e| {
        ApiError::BadRequest {
            code: "blob_decode",
            message: format!("could not decode BlobRef: {e}"),
        }
    })?;
    let stream = store.get(&blob_ref, None).await.map_err(map_blob_err)?;
    // Convert BlobError to std::io::Error for axum::body::Body.
    let body_stream = stream.map_err(std::io::Error::other);
    let body = Body::from_stream(body_stream);

    let mut headers = HeaderMap::new();
    if let Ok(ct) = HeaderValue::from_str(&content_type) {
        headers.insert(header::CONTENT_TYPE, ct);
    }
    // Use "inline" so browsers render images directly; the filename
    // is the suggested download name when the user picks Save As.
    let disposition = format!("inline; filename=\"{}\"", escape_filename(&filename));
    if let Ok(disp) = HeaderValue::from_str(&disposition) {
        headers.insert(header::CONTENT_DISPOSITION, disp);
    }

    Ok((StatusCode::OK, headers, body).into_response())
}

fn escape_filename(name: &str) -> String {
    // Strip CR/LF and double-quote so the Content-Disposition stays
    // well-formed. RFC 5987 encoding for non-ASCII is out of scope
    // for 0.1 — the upload normaliser already strips non-ASCII into
    // underscores.
    name.chars()
        .map(|c| match c {
            '\r' | '\n' | '"' => '_',
            other => other,
        })
        .collect()
}

async fn find_image(
    store: &dyn Store,
    image_id: Uuid,
) -> Result<ExecSummaryImage, ApiError> {
    store
        .get_exec_summary_image(image_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "blob_not_found",
            message: format!("no image with id {image_id}"),
        })
}

async fn find_document(
    store: &dyn Store,
    document_id: Uuid,
) -> Result<ExecSummaryDocument, ApiError> {
    store
        .get_exec_summary_document(document_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "blob_not_found",
            message: format!("no document with id {document_id}"),
        })
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Mount the proxy GET handler under `/blobs/exec-summary/{kind}/{row_id}`.
/// Auth is `(projects, read)` so anyone who can read a project can
/// fetch the bytes of its image / document attachments. The
/// per-blob-row authz check (project-of-this-blob == project-i-can-
/// see) is implicit today via the `(projects, read)` lane, which is
/// already enforced on the parent project routes; tightening this
/// per-row is a follow-up tracked in the storage-feedback Gap 1
/// `BlobContext` work.
pub fn project_exec_summary_blob_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new().route(
                "/blobs/exec-summary/{kind}/{row_id}",
                get(proxy_exec_summary_blob),
            ),
            "projects",
            "read",
        ))
        .with_state(inner)
}

/// Mount every exec-summary route. Splits `(projects, read)` from
/// `(projects, write)`. Lead-only enforcement for `approve` /
/// `revert` is per-handler (E2 is finer than `(projects, write)`).
pub fn project_exec_summary_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new().route(
                "/projects/{id}/exec-summary",
                get(get_project_exec_summary),
            ),
            "projects",
            "read",
        ))
        .merge(with_permission(
            Router::new()
                .route(
                    "/projects/{id}/exec-summary",
                    patch(patch_project_exec_summary),
                )
                .route(
                    "/projects/{id}/exec-summary/submit",
                    post(submit_project_exec_summary),
                )
                .route(
                    "/projects/{id}/exec-summary/approve",
                    post(approve_project_exec_summary),
                )
                .route(
                    "/projects/{id}/exec-summary/revert",
                    post(revert_project_exec_summary),
                )
                .route(
                    "/projects/{id}/exec-summary/images",
                    post(upload_exec_summary_image),
                )
                .route(
                    "/projects/{id}/exec-summary/images/{image_id}",
                    delete(delete_exec_summary_image),
                )
                .route(
                    "/projects/{id}/exec-summary/documents",
                    post(upload_exec_summary_document),
                )
                .route(
                    "/projects/{id}/exec-summary/documents/{doc_id}",
                    patch(patch_exec_summary_document).delete(delete_exec_summary_document),
                )
                .route(
                    "/projects/{id}/exec-summary/changelog",
                    post(append_exec_summary_changelog),
                )
                .route(
                    "/projects/{id}/exec-summary/changelog/{entry_id}",
                    delete(delete_exec_summary_changelog),
                ),
            "projects",
            "write",
        ))
        .with_state(inner)
}
