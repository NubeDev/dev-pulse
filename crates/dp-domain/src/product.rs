//! [`Product`] — a model/SKU the org designs or sells
//! (`DOCS/ideas/product-manufacturing.md` §5.2).
//!
//! A product carries a unique-per-org model number, an owning
//! manufacturer, a lifecycle status, and the serial-number generation
//! config (§6). It links N—N to projects and is the parent of
//! manufacturing runs, units, manuals, documents and returns.
//!
//! Follows the `Project` / `ProjectUpsert` read/upsert split and the
//! §8.2 CAS contract: PATCH / archive carry an `expected_version`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Product lifecycle state. Mirrors the `dp_products.status` text
/// column constrained by the migration CHECK to one of these values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProductStatus {
    /// Being defined; not yet in production.
    Draft,
    /// Actively manufactured / sold. Default for new products.
    Active,
    /// End-of-life: no longer built, history retained.
    Eol,
    /// Hidden from default views; name freed for reuse.
    Archived,
}

impl ProductStatus {
    /// Wire form used by the SQL column and the JSON envelope.
    pub const fn as_str(self) -> &'static str {
        match self {
            ProductStatus::Draft => "draft",
            ProductStatus::Active => "active",
            ProductStatus::Eol => "eol",
            ProductStatus::Archived => "archived",
        }
    }

    /// Parse the SQL / JSON form. Unknown values map to `None`.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(Self::Draft),
            "active" => Some(Self::Active),
            "eol" => Some(Self::Eol),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

/// Who makes the product — an in-house Nube iO product or a re-badged
/// OEM product. Distinct from [`Product::manufacturer_id`], which
/// records *which* manufacturer builds it; this is the in-house/OEM
/// split used to colour-code the catalogue. Mirrors the
/// `dp_products.kind` text column constrained by the migration CHECK.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductKind {
    /// In-house Nube iO product. Default for new + backfilled rows.
    NubeIo,
    /// Third-party / re-badged OEM product.
    Oem,
}

impl ProductKind {
    /// Wire form used by the SQL column and the JSON envelope.
    pub const fn as_str(self) -> &'static str {
        match self {
            ProductKind::NubeIo => "nube_io",
            ProductKind::Oem => "oem",
        }
    }

    /// Parse the SQL / JSON form. Unknown values map to `None`.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "nube_io" => Some(Self::NubeIo),
            "oem" => Some(Self::Oem),
            _ => None,
        }
    }
}

impl Default for ProductKind {
    fn default() -> Self {
        Self::NubeIo
    }
}

/// One row in `dp_products`. Read shape for the products hub / detail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Product {
    /// Primary key.
    pub id: Uuid,
    /// Parent org.
    pub org_id: Uuid,
    /// Human-readable name.
    pub name: String,
    /// Model / SKU number. Case-insensitively unique per org (active).
    pub model_number: String,
    /// Optional markdown description.
    pub description: Option<String>,
    /// Owning manufacturer (nullable; can be overridden per run).
    pub manufacturer_id: Option<Uuid>,
    /// Lifecycle state.
    pub status: ProductStatus,
    /// In-house Nube iO vs OEM (feedback #1).
    pub kind: ProductKind,
    /// Serial prefix, e.g. `NB`.
    pub serial_prefix: Option<String>,
    /// Serial template, e.g. `{prefix}-{run_code}-{seq:05}` (§6).
    pub serial_format: Option<String>,
    /// Soft-delete marker.
    pub archived_at: Option<DateTime<Utc>>,
    /// Creator (immutable; `ON DELETE SET NULL`).
    pub created_by: Option<Uuid>,
    /// When the row was first written.
    pub created_at: DateTime<Utc>,
    /// When the row last mutated.
    pub updated_at: DateTime<Utc>,
    /// §8.2 CAS counter.
    pub version: i64,
}

/// Mutable payload for create / update. The store fills `id`,
/// `version`, timestamps and `archived_at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductUpsert {
    /// Parent org.
    pub org_id: Uuid,
    /// Product name.
    pub name: String,
    /// Model number.
    pub model_number: String,
    /// Optional description.
    pub description: Option<String>,
    /// Optional owning manufacturer.
    pub manufacturer_id: Option<Uuid>,
    /// Lifecycle state.
    pub status: ProductStatus,
    /// In-house Nube iO vs OEM (feedback #1).
    pub kind: ProductKind,
    /// Optional serial prefix.
    pub serial_prefix: Option<String>,
    /// Optional serial template.
    pub serial_format: Option<String>,
    /// Author (stored in `created_by` on create; ignored on update).
    pub created_by: Option<Uuid>,
}

/// List / filter parameters for the products hub.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductListFilter {
    /// Restrict to one org.
    pub org_id: Option<Uuid>,
    /// Restrict to one status.
    pub status: Option<ProductStatus>,
    /// Case-insensitive substring on name or model number.
    pub q: Option<String>,
    /// Page size.
    pub limit: i64,
    /// Page offset.
    pub offset: i64,
}

/// A product↔project link row (read shape).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductProjectLink {
    /// Link id.
    pub id: Uuid,
    /// Product side.
    pub product_id: Uuid,
    /// Project side.
    pub project_id: Uuid,
    /// Who linked it.
    pub linked_by: Option<Uuid>,
    /// When linked.
    pub linked_at: DateTime<Utc>,
}
