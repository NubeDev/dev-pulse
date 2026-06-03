//! Product document uploads (`DOCS/ideas/product-manufacturing.md`
//! §5.2). Mirrors `ExecSummaryDocument` (0045) verbatim — an opaque
//! `blob_ref` plus metadata. Append-only; no version/CAS.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Opaque blob handle round-tripped as JSON; never inspected here.
pub use crate::project_exec_summary::BlobRefJson;

/// One row in `dp_product_documents`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductDocument {
    /// Primary key.
    pub id: Uuid,
    /// Parent product.
    pub product_id: Uuid,
    /// Opaque BlobRef payload.
    pub blob_ref: BlobRefJson,
    /// Display title.
    pub title: String,
    /// Optional doc type, e.g. `datasheet`, `bom`, `cert`.
    pub doc_type: Option<String>,
    /// Optional notes.
    pub notes: Option<String>,
    /// Free-text uploader label (§7.1), not an app-user uuid.
    pub uploaded_by: Option<String>,
    /// When created.
    pub created_at: DateTime<Utc>,
}
