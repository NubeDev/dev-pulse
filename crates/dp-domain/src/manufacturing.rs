//! Manufacturing runs + serialised units
//! (`DOCS/ideas/product-manufacturing.md` §5.4 / §6).
//!
//! A [`ManufacturingRun`] is a production batch of one product with
//! planned/built/pass/fail counters and a per-run serial allocator
//! (`next_serial_seq`). A [`ProductUnit`] is one physical, serialised
//! instance. Serial allocation reserves a contiguous block atomically
//! (§6) — never via the run's `version` CAS.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Manufacturing run lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Planned but not started.
    Planned,
    /// Currently being built.
    InProgress,
    /// Finished.
    Completed,
    /// Abandoned.
    Cancelled,
}

impl RunStatus {
    /// Wire / SQL form.
    pub const fn as_str(self) -> &'static str {
        match self {
            RunStatus::Planned => "planned",
            RunStatus::InProgress => "in_progress",
            RunStatus::Completed => "completed",
            RunStatus::Cancelled => "cancelled",
        }
    }
    /// Parse the SQL / JSON form.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "planned" => Some(Self::Planned),
            "in_progress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// Serialised-unit lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnitStatus {
    /// Built, not yet tested.
    Built,
    /// EOL-tested.
    Tested,
    /// Shipped to a customer.
    Shipped,
    /// Returned (RMA).
    Returned,
    /// Scrapped.
    Scrapped,
}

impl UnitStatus {
    /// Wire / SQL form.
    pub const fn as_str(self) -> &'static str {
        match self {
            UnitStatus::Built => "built",
            UnitStatus::Tested => "tested",
            UnitStatus::Shipped => "shipped",
            UnitStatus::Returned => "returned",
            UnitStatus::Scrapped => "scrapped",
        }
    }
    /// Parse the SQL / JSON form.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "built" => Some(Self::Built),
            "tested" => Some(Self::Tested),
            "shipped" => Some(Self::Shipped),
            "returned" => Some(Self::Returned),
            "scrapped" => Some(Self::Scrapped),
            _ => None,
        }
    }
}

/// One row in `dp_manufacturing_runs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManufacturingRun {
    /// Primary key.
    pub id: Uuid,
    /// Parent org.
    pub org_id: Uuid,
    /// Parent product.
    pub product_id: Uuid,
    /// Builder (overrides the product's manufacturer for this run).
    pub manufacturer_id: Option<Uuid>,
    /// Batch / lot code, e.g. `R2026-014`.
    pub run_code: String,
    /// Lifecycle status.
    pub status: RunStatus,
    /// Planned quantity.
    pub qty_planned: i32,
    /// Distinct units built.
    pub qty_built: i32,
    /// Units whose latest EOL outcome is pass (§5.4).
    pub qty_passed: i32,
    /// Units whose latest EOL outcome is fail (§5.4).
    pub qty_failed: i32,
    /// Next serial sequence number (allocator; §6).
    pub next_serial_seq: i32,
    /// When the build started.
    pub started_at: Option<DateTime<Utc>>,
    /// When the build completed.
    pub completed_at: Option<DateTime<Utc>>,
    /// Markdown notes.
    pub notes: Option<String>,
    /// Creator.
    pub created_by: Option<Uuid>,
    /// When created.
    pub created_at: DateTime<Utc>,
    /// When last mutated.
    pub updated_at: DateTime<Utc>,
    /// §8.2 CAS counter (NOT bumped by serial allocation, §6).
    pub version: i64,
}

/// Mutable payload for run create / update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunUpsert {
    /// Parent org.
    pub org_id: Uuid,
    /// Parent product.
    pub product_id: Uuid,
    /// Optional builder for this run.
    pub manufacturer_id: Option<Uuid>,
    /// Batch / lot code.
    pub run_code: String,
    /// Lifecycle status.
    pub status: RunStatus,
    /// Planned quantity.
    pub qty_planned: i32,
    /// When the build started.
    pub started_at: Option<DateTime<Utc>>,
    /// When the build completed.
    pub completed_at: Option<DateTime<Utc>>,
    /// Markdown notes.
    pub notes: Option<String>,
    /// Author (create only).
    pub created_by: Option<Uuid>,
}

/// One row in `dp_product_units`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductUnit {
    /// Primary key (the stable QR payload; §6).
    pub id: Uuid,
    /// Parent org.
    pub org_id: Uuid,
    /// Parent product.
    pub product_id: Uuid,
    /// Parent run (nullable for hand-entered units).
    pub run_id: Option<Uuid>,
    /// Serial number (unique per org).
    pub serial_number: String,
    /// Lifecycle status.
    pub status: UnitStatus,
    /// Shipped-to customer.
    pub customer_id: Option<Uuid>,
    /// When built.
    pub built_at: Option<DateTime<Utc>>,
    /// When shipped.
    pub shipped_at: Option<DateTime<Utc>>,
    /// When created.
    pub created_at: DateTime<Utc>,
    /// When last mutated.
    pub updated_at: DateTime<Utc>,
    /// §8.2 CAS counter.
    pub version: i64,
}

/// Patch payload for a unit (status / customer / ship). All fields are
/// optional new values; the handler overlays them on the current row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitUpsert {
    /// New status.
    pub status: UnitStatus,
    /// New shipped-to customer (None ⇒ unset).
    pub customer_id: Option<Uuid>,
    /// When built.
    pub built_at: Option<DateTime<Utc>>,
    /// When shipped.
    pub shipped_at: Option<DateTime<Utc>>,
}

/// Outcome of a serial-block allocation (`allocate_units`): the units
/// created and the reserved `[first_seq, first_seq + n)` range (§6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitAllocation {
    /// The freshly created units (in serial order).
    pub units: Vec<ProductUnit>,
    /// First sequence number reserved.
    pub first_seq: i32,
    /// Number of units allocated.
    pub count: i32,
}

/// Cap on `N` per allocation request (§6). Larger requests must chunk.
pub const MAX_UNIT_ALLOC: i32 = 1000;
