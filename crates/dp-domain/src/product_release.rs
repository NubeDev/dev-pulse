//! Per-product software & firmware release history
//! (`DOCS/ideas/product-manufacturing.md` §5.x).
//!
//! Each [`ProductRelease`] records a `major.minor` version of a
//! product's software or firmware, with optional release notes and a
//! release date. Rows are CAS-mutable (PATCH / archive carry an
//! `expected_version`) and archive is a soft-delete that frees the
//! `(product, kind, major, minor)` slot for reuse.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Which kind of release a row records. Mirrors the
/// `dp_product_releases.kind` text column constrained by the migration
/// CHECK to one of these values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseKind {
    /// A software release.
    Software,
    /// A firmware release.
    Firmware,
}

impl ReleaseKind {
    /// Wire form used by the SQL column and the JSON envelope.
    pub const fn as_str(self) -> &'static str {
        match self {
            ReleaseKind::Software => "software",
            ReleaseKind::Firmware => "firmware",
        }
    }

    /// Parse the SQL / JSON form. Unknown values map to `None`.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "software" => Some(Self::Software),
            "firmware" => Some(Self::Firmware),
            _ => None,
        }
    }
}

/// A labelled link attached to a release — e.g. a build artifact /
/// download URL, a release page, or a changelog. Stored as a JSON
/// array in `dp_product_releases.links`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseLink {
    /// Human label, e.g. `Firmware binary (.bin)` or `Release page`.
    pub label: String,
    /// The URL.
    pub url: String,
}

/// One row in `dp_product_releases`. Read shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductRelease {
    /// Primary key.
    pub id: Uuid,
    /// Parent org.
    pub org_id: Uuid,
    /// Parent product.
    pub product_id: Uuid,
    /// Software or firmware.
    pub kind: ReleaseKind,
    /// Major version component.
    pub major: i32,
    /// Minor version component.
    pub minor: i32,
    /// Optional release notes (markdown).
    pub release_notes: Option<String>,
    /// Optional release date.
    pub released_at: Option<DateTime<Utc>>,
    /// Build / download links attached to this release.
    pub links: Vec<ReleaseLink>,
    /// Soft-delete marker.
    pub archived_at: Option<DateTime<Utc>>,
    /// Creator.
    pub created_by: Option<Uuid>,
    /// When created.
    pub created_at: DateTime<Utc>,
    /// When last mutated.
    pub updated_at: DateTime<Utc>,
    /// §8.2 CAS counter.
    pub version: i64,
}

/// Create payload for a new release. The store fills `id`, `version`,
/// timestamps and `archived_at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductReleaseCreate {
    /// Parent org.
    pub org_id: Uuid,
    /// Parent product.
    pub product_id: Uuid,
    /// Software or firmware.
    pub kind: ReleaseKind,
    /// Major version component.
    pub major: i32,
    /// Minor version component.
    pub minor: i32,
    /// Optional release notes.
    pub release_notes: Option<String>,
    /// Optional release date.
    pub released_at: Option<DateTime<Utc>>,
    /// Build / download links.
    pub links: Vec<ReleaseLink>,
    /// Author (create only).
    pub created_by: Option<Uuid>,
}

/// Mutable payload for `update_product_release` (CAS).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductReleaseUpdate {
    /// Software or firmware.
    pub kind: ReleaseKind,
    /// Major version component.
    pub major: i32,
    /// Minor version component.
    pub minor: i32,
    /// Optional release notes.
    pub release_notes: Option<String>,
    /// Optional release date.
    pub released_at: Option<DateTime<Utc>>,
    /// Build / download links.
    pub links: Vec<ReleaseLink>,
}
