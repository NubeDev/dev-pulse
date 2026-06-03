//! Returns / RMA (`DOCS/ideas/product-manufacturing.md` §5.5).
//!
//! An [`Rma`] is a return authorisation for a product (optionally a
//! serialised [`crate::manufacturing::ProductUnit`]). It carries a
//! lifecycle status, warranty flag, and free-text reason / diagnosis /
//! resolution notes plus received / resolved timestamps. The row is a
//! mutable top-level entity → §8.2 `version` CAS.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// RMA lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RmaStatus {
    /// Opened, not yet received.
    Open,
    /// Goods received.
    Received,
    /// Diagnosed.
    Diagnosed,
    /// Repaired.
    Repaired,
    /// Replaced.
    Replaced,
    /// Rejected (not covered / not faulty).
    Rejected,
    /// Closed.
    Closed,
}

impl RmaStatus {
    /// Wire / SQL form.
    pub const fn as_str(self) -> &'static str {
        match self {
            RmaStatus::Open => "open",
            RmaStatus::Received => "received",
            RmaStatus::Diagnosed => "diagnosed",
            RmaStatus::Repaired => "repaired",
            RmaStatus::Replaced => "replaced",
            RmaStatus::Rejected => "rejected",
            RmaStatus::Closed => "closed",
        }
    }
    /// Parse the SQL / JSON form.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "open" => Some(Self::Open),
            "received" => Some(Self::Received),
            "diagnosed" => Some(Self::Diagnosed),
            "repaired" => Some(Self::Repaired),
            "replaced" => Some(Self::Replaced),
            "rejected" => Some(Self::Rejected),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }
}

/// One row in `dp_rma_returns`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rma {
    /// Primary key.
    pub id: Uuid,
    /// Parent org.
    pub org_id: Uuid,
    /// Optional serialised unit under return.
    pub unit_id: Option<Uuid>,
    /// Parent product.
    pub product_id: Uuid,
    /// Optional customer.
    pub customer_id: Option<Uuid>,
    /// RMA number (unique per org, case-insensitive).
    pub rma_number: String,
    /// Whether the return is covered by warranty.
    pub under_warranty: bool,
    /// Lifecycle status.
    pub status: RmaStatus,
    /// Customer-reported reason.
    pub reason: Option<String>,
    /// Diagnosis notes.
    pub diagnosis: Option<String>,
    /// Resolution notes.
    pub resolution: Option<String>,
    /// When goods were received.
    pub received_at: Option<DateTime<Utc>>,
    /// When resolved.
    pub resolved_at: Option<DateTime<Utc>>,
    /// Creator.
    pub created_by: Option<Uuid>,
    /// When created.
    pub created_at: DateTime<Utc>,
    /// When last mutated.
    pub updated_at: DateTime<Utc>,
    /// §8.2 CAS counter.
    pub version: i64,
}

/// Create payload for an RMA.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RmaCreate {
    /// Parent org.
    pub org_id: Uuid,
    /// Parent product.
    pub product_id: Uuid,
    /// Optional serialised unit.
    pub unit_id: Option<Uuid>,
    /// Optional customer.
    pub customer_id: Option<Uuid>,
    /// RMA number.
    pub rma_number: String,
    /// Warranty flag.
    pub under_warranty: bool,
    /// Initial status (defaults to open at the REST layer).
    pub status: RmaStatus,
    /// Customer-reported reason.
    pub reason: Option<String>,
    /// Author.
    pub created_by: Option<Uuid>,
}

/// Patch payload for an RMA (full upsert of editable fields + CAS).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RmaUpdate {
    /// Optional serialised unit.
    pub unit_id: Option<Uuid>,
    /// Optional customer.
    pub customer_id: Option<Uuid>,
    /// Warranty flag.
    pub under_warranty: bool,
    /// Lifecycle status.
    pub status: RmaStatus,
    /// Customer-reported reason.
    pub reason: Option<String>,
    /// Diagnosis notes.
    pub diagnosis: Option<String>,
    /// Resolution notes.
    pub resolution: Option<String>,
    /// When goods were received.
    pub received_at: Option<DateTime<Utc>>,
    /// When resolved.
    pub resolved_at: Option<DateTime<Utc>>,
}

/// List filter for RMAs. Each `Some(_)` narrows the result set; all
/// applied filters intersect.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RmaFilter {
    /// Scope to an org.
    pub org_id: Option<Uuid>,
    /// Scope to a status.
    pub status: Option<RmaStatus>,
    /// Scope to a product.
    pub product_id: Option<Uuid>,
    /// Scope to a customer.
    pub customer_id: Option<Uuid>,
    /// Scope to a unit.
    pub unit_id: Option<Uuid>,
}
