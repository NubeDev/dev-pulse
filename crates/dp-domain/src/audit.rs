//! [`AuditEntry`] — one row in the `dp_audit_log` table (SCOPE §9).
//!
//! Every protected handler in `dp-rest` writes one of these per call
//! through [`Store::record_audit_log`](crate::Store::record_audit_log).
//! The `action` vocabulary is pinned in `dp-rest::audit` (Phase 4
//! D4.4); this domain type stays vocabulary-free so a future surface
//! (MCP, CLI) can write rows with its own verbs without a
//! `dp-domain` change.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single audit-trail row. Mirrors the columns of `dp_audit_log`
/// (Phase 1 migration `0001_init.sql`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Primary key (caller-assigned so writers can correlate the
    /// row with downstream effects without a round-trip).
    pub id: Uuid,
    /// The principal that performed the action. Stable across
    /// pseudonymisation (§0.5).
    pub actor_user_id: Uuid,
    /// Pinned verb (e.g. `"home_org.set"`, `"report.read"`).
    pub action: String,
    /// Free-form identifier of what the action operated on. Common
    /// shapes: `"user:<uuid>"`, `"org:<uuid>"`, `"report:/reports/user/<uuid>"`.
    pub target: String,
    /// When the row was written.
    pub at: DateTime<Utc>,
}
