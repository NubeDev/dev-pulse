//! User manuals + revisions (`DOCS/ideas/product-manufacturing.md`
//! §5.3).
//!
//! A [`ProductManual`] is a named container (CAS-mutable); each save
//! creates an immutable [`ManualRevision`] with a free-form revision
//! string. At most one revision per manual is `published`; publishing
//! a new one supersedes the prior published revision in one tx.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Lifecycle of a manual revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RevisionStatus {
    /// Work in progress; not visible on the public landing.
    Draft,
    /// The current published revision (at most one per manual).
    Published,
    /// A formerly-published revision, retained for history.
    Superseded,
}

impl RevisionStatus {
    /// Wire form used by the SQL column and the JSON envelope.
    pub const fn as_str(self) -> &'static str {
        match self {
            RevisionStatus::Draft => "draft",
            RevisionStatus::Published => "published",
            RevisionStatus::Superseded => "superseded",
        }
    }

    /// Parse the SQL / JSON form. Unknown values map to `None`.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(Self::Draft),
            "published" => Some(Self::Published),
            "superseded" => Some(Self::Superseded),
            _ => None,
        }
    }
}

/// One row in `dp_product_manuals`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductManual {
    /// Primary key.
    pub id: Uuid,
    /// Parent product.
    pub product_id: Uuid,
    /// Manual title, e.g. `Installation Guide`.
    pub title: String,
    /// Creator.
    pub created_by: Option<Uuid>,
    /// When created.
    pub created_at: DateTime<Utc>,
    /// When last mutated.
    pub updated_at: DateTime<Utc>,
    /// §8.2 CAS counter.
    pub version: i64,
}

/// One row in `dp_product_manual_revisions` (append-only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualRevision {
    /// Primary key.
    pub id: Uuid,
    /// Parent manual.
    pub manual_id: Uuid,
    /// Free-form revision string, e.g. `A`, `1.0`.
    pub revision: String,
    /// Lifecycle status.
    pub status: RevisionStatus,
    /// Markdown body.
    pub body_md: String,
    /// Optional "what changed" note.
    pub change_note: Option<String>,
    /// Author.
    pub authored_by: Option<Uuid>,
    /// When created.
    pub created_at: DateTime<Utc>,
}

/// Create payload for a new manual container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualUpsert {
    /// Parent product.
    pub product_id: Uuid,
    /// Manual title.
    pub title: String,
    /// Author (create only).
    pub created_by: Option<Uuid>,
}

/// Create payload for a new revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionUpsert {
    /// Free-form revision string.
    pub revision: String,
    /// Markdown body.
    pub body_md: String,
    /// Optional "what changed" note.
    pub change_note: Option<String>,
    /// Author.
    pub authored_by: Option<Uuid>,
}
