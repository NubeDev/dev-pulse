//! Master-data parties — [`Manufacturer`], [`Supplier`], [`Customer`]
//! (`DOCS/ideas/product-manufacturing.md` §5.1).
//!
//! Three column-for-column identical tables (customers add
//! `account_ref`). Kept as three structs rather than one polymorphic
//! `party` because their lifecycles diverge later (customers gain
//! RMAs; suppliers gain a BOM). Same read/upsert split + CAS as
//! [`crate::project::Project`].

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Shared contact / audit fields for a master-data party row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manufacturer {
    /// Primary key.
    pub id: Uuid,
    /// Parent org.
    pub org_id: Uuid,
    /// Display name (case-insensitively unique per org while active).
    pub name: String,
    /// Optional primary contact name.
    pub contact_name: Option<String>,
    /// Optional email.
    pub email: Option<String>,
    /// Optional phone.
    pub phone: Option<String>,
    /// Optional address.
    pub address: Option<String>,
    /// Optional website.
    pub website: Option<String>,
    /// Optional markdown notes.
    pub notes: Option<String>,
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

/// A supplier (scaffold only; no consumers yet — §2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Supplier {
    /// Primary key.
    pub id: Uuid,
    /// Parent org.
    pub org_id: Uuid,
    /// Display name.
    pub name: String,
    /// Optional primary contact name.
    pub contact_name: Option<String>,
    /// Optional email.
    pub email: Option<String>,
    /// Optional phone.
    pub phone: Option<String>,
    /// Optional address.
    pub address: Option<String>,
    /// Optional website.
    pub website: Option<String>,
    /// Optional markdown notes.
    pub notes: Option<String>,
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

/// A customer (ships-to / raises RMAs). Adds `account_ref`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Customer {
    /// Primary key.
    pub id: Uuid,
    /// Parent org.
    pub org_id: Uuid,
    /// Display name.
    pub name: String,
    /// Optional primary contact name.
    pub contact_name: Option<String>,
    /// Optional email.
    pub email: Option<String>,
    /// Optional phone.
    pub phone: Option<String>,
    /// Optional address.
    pub address: Option<String>,
    /// Optional website.
    pub website: Option<String>,
    /// Optional markdown notes.
    pub notes: Option<String>,
    /// Optional external CRM/ERP id.
    pub account_ref: Option<String>,
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

/// Mutable payload for manufacturer create / update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManufacturerUpsert {
    /// Parent org.
    pub org_id: Uuid,
    /// Display name.
    pub name: String,
    /// Optional contact name.
    pub contact_name: Option<String>,
    /// Optional email.
    pub email: Option<String>,
    /// Optional phone.
    pub phone: Option<String>,
    /// Optional address.
    pub address: Option<String>,
    /// Optional website.
    pub website: Option<String>,
    /// Optional markdown notes.
    pub notes: Option<String>,
    /// Author (create only).
    pub created_by: Option<Uuid>,
}

/// Mutable payload for supplier create / update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupplierUpsert {
    /// Parent org.
    pub org_id: Uuid,
    /// Display name.
    pub name: String,
    /// Optional contact name.
    pub contact_name: Option<String>,
    /// Optional email.
    pub email: Option<String>,
    /// Optional phone.
    pub phone: Option<String>,
    /// Optional address.
    pub address: Option<String>,
    /// Optional website.
    pub website: Option<String>,
    /// Optional markdown notes.
    pub notes: Option<String>,
    /// Author (create only).
    pub created_by: Option<Uuid>,
}

/// Mutable payload for customer create / update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomerUpsert {
    /// Parent org.
    pub org_id: Uuid,
    /// Display name.
    pub name: String,
    /// Optional contact name.
    pub contact_name: Option<String>,
    /// Optional email.
    pub email: Option<String>,
    /// Optional phone.
    pub phone: Option<String>,
    /// Optional address.
    pub address: Option<String>,
    /// Optional website.
    pub website: Option<String>,
    /// Optional markdown notes.
    pub notes: Option<String>,
    /// Optional external CRM/ERP id.
    pub account_ref: Option<String>,
    /// Author (create only).
    pub created_by: Option<Uuid>,
}

/// List / filter parameters shared by the three party lists.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyListFilter {
    /// Restrict to one org.
    pub org_id: Option<Uuid>,
    /// Case-insensitive substring on name.
    pub q: Option<String>,
    /// Include archived rows when true.
    pub include_archived: bool,
    /// Page size.
    pub limit: i64,
    /// Page offset.
    pub offset: i64,
}
