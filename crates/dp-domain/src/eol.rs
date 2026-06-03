//! End-Of-Line test reports + run-level sign-off summary
//! (`DOCS/ideas/product-manufacturing.md` §5.4 + LOCKED DECISION #3).
//!
//! Per-unit [`EolTestReport`]s are the source of truth (one pass/fail
//! per test; re-tests allowed; current = latest). [`RunEolSummary`] is
//! a point-in-time operator sign-off snapshot per run.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Opaque blob handle for an optional raw-log upload.
pub use crate::project_exec_summary::BlobRefJson;

/// EOL test outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EolResult {
    /// Passed.
    Pass,
    /// Failed.
    Fail,
}

impl EolResult {
    /// Wire / SQL form.
    pub const fn as_str(self) -> &'static str {
        match self {
            EolResult::Pass => "pass",
            EolResult::Fail => "fail",
        }
    }
    /// Parse the SQL / JSON form.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pass" => Some(Self::Pass),
            "fail" => Some(Self::Fail),
            _ => None,
        }
    }
}

/// One row in `dp_eol_test_reports` (append-only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EolTestReport {
    /// Primary key.
    pub id: Uuid,
    /// Parent unit.
    pub unit_id: Uuid,
    /// Pass / fail.
    pub result: EolResult,
    /// Test rig / bench id.
    pub station: Option<String>,
    /// Firmware under test.
    pub firmware: Option<String>,
    /// Structured measurements (opaque JSON).
    pub measurements: serde_json::Value,
    /// Optional raw-log blob handle.
    pub log_blob_ref: Option<BlobRefJson>,
    /// Notes.
    pub notes: Option<String>,
    /// Free-text station operator (§7.1).
    pub tested_by: Option<String>,
    /// When tested.
    pub tested_at: DateTime<Utc>,
    /// When created.
    pub created_at: DateTime<Utc>,
}

/// Create payload for an EOL report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EolTestUpsert {
    /// Pass / fail.
    pub result: EolResult,
    /// Test rig / bench id.
    pub station: Option<String>,
    /// Firmware under test.
    pub firmware: Option<String>,
    /// Structured measurements.
    pub measurements: serde_json::Value,
    /// Optional raw-log blob handle.
    pub log_blob_ref: Option<BlobRefJson>,
    /// Notes.
    pub notes: Option<String>,
    /// Free-text station operator.
    pub tested_by: Option<String>,
}

/// One row in `dp_run_eol_summary` — the run-level sign-off snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunEolSummary {
    /// Parent run (also the PK).
    pub run_id: Uuid,
    /// Built-count snapshot at sign-off.
    pub built_count: i32,
    /// Pass-count snapshot.
    pub pass_count: i32,
    /// Fail-count snapshot.
    pub fail_count: i32,
    /// Markdown notes.
    pub notes_md: Option<String>,
    /// Operator who signed off.
    pub signed_by: Option<Uuid>,
    /// When signed off.
    pub signed_at: Option<DateTime<Utc>>,
    /// When created.
    pub created_at: DateTime<Utc>,
    /// When last mutated.
    pub updated_at: DateTime<Utc>,
    /// §8.2 CAS counter.
    pub version: i64,
}

/// Create / update payload for a run EOL sign-off. The counts are
/// snapshotted from the run's current counters by the store; the
/// caller supplies notes + sign-off intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunEolSummaryUpsert {
    /// Markdown notes.
    pub notes_md: Option<String>,
    /// When true, stamp `signed_by`/`signed_at`.
    pub sign_off: bool,
    /// Operator (the caller).
    pub signed_by: Option<Uuid>,
}
