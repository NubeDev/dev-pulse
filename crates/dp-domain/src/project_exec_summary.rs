//! Project Executive Summary entity types.
//!
//! Mirrors the form captured in
//! [DOCS/SCOPE-PROJECT-EXECUTIVE-SUMMARY.md][doc]. One
//! [`ProjectExecSummary`] per [`crate::project::Project`], lazily
//! materialised on first edit. Supporting blob attachments
//! ([`ExecSummaryImage`], [`ExecSummaryDocument`]) carry an opaque
//! [`BlobRefJson`] handle — the bytes themselves are owned by the
//! starter `BlobStore` the server is configured with.
//!
//! ## Boundary
//!
//! Per §0.6 this crate stays free of `starter_*` imports. The blob
//! handle is therefore typed as `serde_json::Value` here and converted
//! to / from `starter_spi::blob::BlobRef` at the edges (the REST and
//! store crates). The shape is opaque per the storage scope's B2 —
//! domain code never inspects the inner fields.
//!
//! [doc]: https://internal/dev-pulse/DOCS/SCOPE-PROJECT-EXECUTIVE-SUMMARY.md

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Opaque serde round-trip of `starter_spi::blob::BlobRef`. dp-domain
/// never reads the fields; consumers (rest, store) round-trip it
/// verbatim into / out of the `jsonb` column.
pub type BlobRefJson = serde_json::Value;

/// Approval state machine for an exec summary.
///
/// Transitions (enforced in [`crate::store::Store`]):
///
/// * `Draft → InReview` via submit (requires completion ≥ threshold).
/// * `InReview → Approved` via approve (requires project lead).
/// * Any → `Draft` via revert.
///
/// See the scope doc §3.4 for the full rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecSummaryStatus {
    /// Editable, not yet submitted.
    Draft,
    /// Submitted; awaiting approval.
    InReview,
    /// Locked, signed off by the project lead.
    Approved,
}

impl ExecSummaryStatus {
    /// SQL `CHECK` vocabulary in
    /// [`0045_project_exec_summary.sql`](../../../dp-store-pg/migrations/dp/0045_project_exec_summary.sql).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::InReview => "in_review",
            Self::Approved => "approved",
        }
    }
}

/// The one-to-one exec summary row for a project.
///
/// All long-text fields are markdown bodies; consumers render with the
/// shared `@uiw/react-md-editor` widget. `protocols` is the closed-enum
/// multi-select from the Requirements section (BACnet MS/TP, BACnet IP,
/// Modbus RTU, Modbus TCP, MQTT, LoRa, LoRaWAN, WiFi, BLE, Ethernet,
/// RS485 / UART, 4G / 5G — see scope doc §2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectExecSummary {
    /// Project the summary belongs to (also the PK).
    pub project_id: Uuid,

    // Summary section -----------------------------------------------
    /// Product name (Summary tab).
    pub product_name: Option<String>,
    /// Part / SKU number.
    pub part_number: Option<String>,
    /// Target release date.
    pub target_release_date: Option<NaiveDate>,
    /// Primary objective (markdown).
    pub objective: Option<String>,
    /// Problem statement (markdown).
    pub problem: Option<String>,
    /// Value delivered (markdown).
    pub value: Option<String>,
    /// Key differentiators (markdown).
    pub differentiators: Option<String>,
    /// Success criteria (markdown).
    pub success_criteria: Option<String>,

    // Scope section -------------------------------------------------
    /// What's in scope (markdown).
    pub in_scope: Option<String>,
    /// What's out of scope (markdown).
    pub out_of_scope: Option<String>,
    /// Assumptions (markdown).
    pub assumptions: Option<String>,
    /// Dependencies (markdown).
    pub dependencies: Option<String>,
    /// Constraints (markdown).
    pub constraints: Option<String>,

    // Requirements section -----------------------------------------
    /// Must-have requirements (markdown).
    pub must_have: Option<String>,
    /// Optional / future requirements (markdown).
    pub optional: Option<String>,
    /// User-interaction notes (markdown).
    pub user_interaction: Option<String>,
    /// System architecture notes (markdown).
    pub architecture: Option<String>,
    /// Selected protocols (multi-select).
    pub protocols: Vec<String>,
    /// Power requirements (short text).
    pub power: Option<String>,
    /// Mounting / enclosure notes (short text).
    pub mounting: Option<String>,
    /// Certification notes (short text).
    pub certification: Option<String>,
    /// LoRa details — free text, e.g. "AU915, SF7–SF12" (feedback #3).
    pub lora: Option<String>,
    /// WiFi details — free text, e.g. "2.4 GHz b/g/n, WPA2" (feedback #3).
    pub wifi: Option<String>,
    /// General free-text notes on the Requirements section (feedback #3).
    pub req_notes: Option<String>,

    // Hardware section ---------------------------------------------
    /// Hardware features (markdown).
    pub hardware_features: Option<String>,
    /// Physical notes (markdown).
    pub physical_notes: Option<String>,
    /// Enclosure / housing (short text).
    pub enclosure: Option<String>,
    /// Mounting type (short text).
    pub mounting_type: Option<String>,
    /// Operating environment (short text).
    pub operating_env: Option<String>,

    // Commercial section -------------------------------------------
    /// Retail price in cents (no floats for money).
    pub rrp_cents: Option<i64>,
    /// OEM price in cents.
    pub oem_price_cents: Option<i64>,
    /// Target gross-profit, in basis points (1 bp = 0.01%). Range
    /// `0..=99_999` (0%–999.99%, mirroring the original NUMERIC(5,2)
    /// scope decision). Stored as `BIGINT` to keep the no-floats-
    /// for-money rule and avoid pulling in a decimal crate workspace-
    /// wide. REST DTOs expose `f64` percent for ease of display and
    /// multiply/divide by 100 at the wire seam.
    pub target_gp_bp: Option<i64>,
    /// Revenue model.
    pub revenue_model: Option<String>,
    /// Channel strategy.
    pub channel_strategy: Option<String>,
    /// Target market (markdown).
    pub target_market: Option<String>,
    /// Volume assumptions (markdown).
    pub volume_assumptions: Option<String>,

    // Approval section ---------------------------------------------
    /// Current status.
    pub status: ExecSummaryStatus,
    /// Reviewer (name/email).
    pub reviewer: Option<String>,
    /// Approver (name/email).
    pub approver: Option<String>,
    /// Review notes (markdown).
    pub review_notes: Option<String>,
    /// Approval notes (markdown).
    pub approval_notes: Option<String>,
    /// When the summary was last submitted for review.
    pub submitted_at: Option<DateTime<Utc>>,
    /// When the summary was approved (most recent transition).
    pub approved_at: Option<DateTime<Utc>>,

    /// Section ids the user has explicitly marked "N/A". Counts as
    /// complete for the §3.5 completion calc without needing dummy
    /// content. Section ids match
    /// `EXEC_SUMMARY_SECTIONS` on the frontend (`summary`, `scope`,
    /// `requirements`, `hardware`, `commercial`, `documents`,
    /// `approval`, `changelog`). Unknown ids are ignored.
    pub skipped_sections: Vec<String>,

    /// Row creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp; touched on every PATCH.
    pub updated_at: DateTime<Utc>,
}

/// One reference image on the Hardware section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecSummaryImage {
    /// Stable id.
    pub id: Uuid,
    /// Parent project.
    pub project_id: Uuid,
    /// Opaque `BlobRef` payload.
    pub blob_ref: BlobRefJson,
    /// Original client-supplied filename.
    pub filename: String,
    /// MIME type as captured at upload.
    pub content_type: String,
    /// Optional caption.
    pub caption: Option<String>,
    /// Display order within the project's image set.
    pub ord: i32,
    /// Upload timestamp.
    pub created_at: DateTime<Utc>,
}

/// One uploaded document on the Documents section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecSummaryDocument {
    /// Stable id.
    pub id: Uuid,
    /// Parent project.
    pub project_id: Uuid,
    /// Opaque `BlobRef` payload.
    pub blob_ref: BlobRefJson,
    /// User-facing title.
    pub title: String,
    /// Free-form category — UI offers a suggestions dropdown but the
    /// column is unconstrained per §3.1 of the scope doc.
    pub doc_type: Option<String>,
    /// Notes (markdown).
    pub notes: Option<String>,
    /// Required action the document implies (markdown).
    pub required_action: Option<String>,
    /// Uploader (free-text contact string).
    pub uploaded_by: Option<String>,
    /// Upload timestamp.
    pub created_at: DateTime<Utc>,
}

/// One row of the per-project change log. Append-only from the UI
/// (E5 hard rule); edits go through a separate confirm flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecSummaryChangelogEntry {
    /// Stable id.
    pub id: Uuid,
    /// Parent project.
    pub project_id: Uuid,
    /// Free-form version label (semver, calver, internal codes).
    pub version: String,
    /// Date of the change (calendar, not timestamp).
    pub changed_at: NaiveDate,
    /// Author free-text contact.
    pub changed_by: String,
    /// Change summary (markdown).
    pub summary: String,
    /// Content snapshot captured when the entry was cut — a
    /// fully-populated [`ProjectExecSummaryPatch`] serialised to JSON.
    /// `None` for entries written before snapshots existed (or cut
    /// without a summary row). Drives the "Restore this version" UI;
    /// the bytes are opaque to consumers other than the restore path.
    pub snapshot: Option<serde_json::Value>,
    /// Row creation timestamp.
    pub created_at: DateTime<Utc>,
}

/// Sparse patch payload for the exec-summary scalar columns.
///
/// Every field is `Option<Option<T>>`-shaped so that the REST layer
/// can distinguish "field absent from PATCH body" (outer `None`) from
/// "field set to NULL" (outer `Some(None)`). The store applies only
/// the present fields and bumps `updated_at`.
///
/// Protocols and approval transitions are not in this struct — those
/// have dedicated store methods so the state-machine rules in §3.4
/// of the scope doc stay in one place.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectExecSummaryPatch {
    // Summary
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_name: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_number: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_release_date: Option<Option<NaiveDate>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub differentiators: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success_criteria: Option<Option<String>>,

    // Scope
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_scope: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_of_scope: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assumptions: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constraints: Option<Option<String>>,

    // Requirements
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub must_have: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optional: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_interaction: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub architecture: Option<Option<String>>,
    /// Replace the protocols set wholesale when present. Not
    /// double-wrapped: protocols can be `[]` but never `null`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocols: Option<Vec<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mounting: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certification: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lora: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wifi: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub req_notes: Option<Option<String>>,

    // Hardware
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware_features: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical_notes: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enclosure: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mounting_type: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operating_env: Option<Option<String>>,

    // Commercial
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rrp_cents: Option<Option<i64>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oem_price_cents: Option<Option<i64>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_gp_bp: Option<Option<i64>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revenue_model: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_strategy: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_market: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_assumptions: Option<Option<String>>,

    // Approval (free-text contacts only — status transitions live on
    // dedicated submit / approve / revert methods).
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approver: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_notes: Option<Option<String>>,
    #[allow(missing_docs)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_notes: Option<Option<String>>,

    /// Replace the skipped-sections set wholesale when present.
    /// `Some(vec![])` clears every skip; `None` leaves the column
    /// untouched. Matches the `protocols` shape for the same reason
    /// (can be empty but never null).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped_sections: Option<Vec<String>>,
}

impl ProjectExecSummaryPatch {
    /// Build a fully-populated patch capturing the editable content of
    /// `s` — every section field present (`Some`), nulls preserved as
    /// `Some(None)`. Serialised, this is the change-log *snapshot*;
    /// deserialised and fed back through
    /// [`crate::store::Store::patch_project_exec_summary`] it restores
    /// the summary to this exact content.
    ///
    /// Deliberately omits the approval state machine (`status`,
    /// `submitted_at`, `approved_at`) — those aren't patch fields, so a
    /// restore rolls content back without touching sign-off.
    pub fn snapshot_of(s: &ProjectExecSummary) -> Self {
        Self {
            product_name: Some(s.product_name.clone()),
            part_number: Some(s.part_number.clone()),
            target_release_date: Some(s.target_release_date),
            objective: Some(s.objective.clone()),
            problem: Some(s.problem.clone()),
            value: Some(s.value.clone()),
            differentiators: Some(s.differentiators.clone()),
            success_criteria: Some(s.success_criteria.clone()),

            in_scope: Some(s.in_scope.clone()),
            out_of_scope: Some(s.out_of_scope.clone()),
            assumptions: Some(s.assumptions.clone()),
            dependencies: Some(s.dependencies.clone()),
            constraints: Some(s.constraints.clone()),

            must_have: Some(s.must_have.clone()),
            optional: Some(s.optional.clone()),
            user_interaction: Some(s.user_interaction.clone()),
            architecture: Some(s.architecture.clone()),
            protocols: Some(s.protocols.clone()),
            power: Some(s.power.clone()),
            mounting: Some(s.mounting.clone()),
            certification: Some(s.certification.clone()),
            lora: Some(s.lora.clone()),
            wifi: Some(s.wifi.clone()),
            req_notes: Some(s.req_notes.clone()),

            hardware_features: Some(s.hardware_features.clone()),
            physical_notes: Some(s.physical_notes.clone()),
            enclosure: Some(s.enclosure.clone()),
            mounting_type: Some(s.mounting_type.clone()),
            operating_env: Some(s.operating_env.clone()),

            rrp_cents: Some(s.rrp_cents),
            oem_price_cents: Some(s.oem_price_cents),
            target_gp_bp: Some(s.target_gp_bp),
            revenue_model: Some(s.revenue_model.clone()),
            channel_strategy: Some(s.channel_strategy.clone()),
            target_market: Some(s.target_market.clone()),
            volume_assumptions: Some(s.volume_assumptions.clone()),

            reviewer: Some(s.reviewer.clone()),
            approver: Some(s.approver.clone()),
            review_notes: Some(s.review_notes.clone()),
            approval_notes: Some(s.approval_notes.clone()),

            skipped_sections: Some(s.skipped_sections.clone()),
        }
    }
}

/// Per-section completion booleans computed server-side from the
/// row + child-table counts. See §6 of the scope doc for the rules.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ExecSummaryCompletion {
    /// Summary section complete.
    pub summary: bool,
    /// Scope section complete.
    pub scope: bool,
    /// Requirements section complete.
    pub requirements: bool,
    /// Hardware section complete.
    pub hardware: bool,
    /// Commercial section complete.
    pub commercial: bool,
    /// Documents section complete.
    pub documents: bool,
    /// Approval section complete.
    pub approval: bool,
    /// Change-log section complete.
    pub changelog: bool,
}

impl ExecSummaryCompletion {
    /// Apply the user-marked "N/A" set: any section listed in
    /// `skipped` flips to `true` for completion purposes. Lets the
    /// store layer compute the strict rules from the row + child
    /// counts and then merge the user's skip choices in one place.
    pub fn with_skips<I, S>(mut self, skipped: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for s in skipped {
            match s.as_ref() {
                "summary" => self.summary = true,
                "scope" => self.scope = true,
                "requirements" => self.requirements = true,
                "hardware" => self.hardware = true,
                "commercial" => self.commercial = true,
                "documents" => self.documents = true,
                "approval" => self.approval = true,
                "changelog" => self.changelog = true,
                _ => {}
            }
        }
        self
    }

    /// Number of completed sections (0..=8).
    pub fn completed(&self) -> u8 {
        [
            self.summary,
            self.scope,
            self.requirements,
            self.hardware,
            self.commercial,
            self.documents,
            self.approval,
            self.changelog,
        ]
        .into_iter()
        .filter(|b| *b)
        .count() as u8
    }

    /// Round-half-up percentage 0..=100.
    pub fn percent(&self) -> u8 {
        ((self.completed() as u16 * 100 + 4) / 8) as u8
    }
}

/// Submission threshold — server rejects `submit` below this percent.
/// See §3.4 of the scope doc.
pub const EXEC_SUMMARY_SUBMIT_THRESHOLD_PERCENT: u8 = 80;

/// New-row inputs for the changelog append endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecSummaryChangelogInsert {
    /// Project the entry belongs to.
    pub project_id: Uuid,
    /// Free-form version label.
    pub version: String,
    /// Date of the change.
    pub changed_at: NaiveDate,
    /// Author free-text contact.
    pub changed_by: String,
    /// Change summary (markdown).
    pub summary: String,
    /// Content snapshot to persist alongside the entry. Built by the
    /// REST layer from the live summary via
    /// [`ProjectExecSummaryPatch::snapshot_of`]; `None` leaves the
    /// column NULL (e.g. no summary row exists yet).
    pub snapshot: Option<serde_json::Value>,
}
