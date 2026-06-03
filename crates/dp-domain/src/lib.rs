//! `dp-domain` — entity types and the `Store` trait for dev-pulse.
//!
//! This crate is **storage-agnostic**. It defines the value types that
//! flow through every dev-pulse surface (HTTP, MCP, CLI, reports,
//! fetcher) and the [`Store`] trait that the postgres implementation
//! (`dp-store-pg`) satisfies.
//!
//! Hard boundary rule (TODO §0.6): **zero `starter_*` imports** in this
//! crate. `scripts/check-boundaries.sh` enforces this in CI from
//! stage 2 onward.
//!
//! Entities mirror the schema decisions in TODO §0.2–§0.5:
//!
//! * Events are split from their actors — [`ActivityEvent`] carries no
//!   `user_id`; attribution lives in [`EventActor`] rows keyed by
//!   `(event_id, user_id, role)`.
//! * Resumable cursors are per-`(org_id, repo_id, resource_kind)` —
//!   see [`FetchCursor`]. [`FetchRun`] is a run log only.
//! * Soft-delete + pseudonymisation lives on [`User::deleted_at`].
//! * All timestamps are `chrono::DateTime<Utc>` (no naive times).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod app_install;
pub mod audit;
pub mod board_link;
pub mod event;
pub mod fetch;
pub mod freshness;
pub mod identity;
pub mod inbox;
pub mod issue;
pub mod issue_dates;
pub mod issue_mutation;
pub mod membership;
pub mod milestone;
pub mod eol;
pub mod manufacturing;
pub mod org;
pub mod party;
pub mod pin;
pub mod product;
pub mod product_doc;
pub mod product_manual;
pub mod product_release;
pub mod project;
pub mod project_exec_summary;
pub mod project_view;
pub mod repo;
pub mod rma;
pub mod setting;
pub mod store;
pub mod tag;
pub mod tag_link;
pub mod team;
pub mod user;
pub mod webhook;
pub mod window;

pub use app_install::{AppInstallPermissions, OrgAppInstall};
pub use audit::AuditEntry;
pub use board_link::{BoardItem, BoardItemMirrorOutcome, BoardLink, BoardLinkUpsert};
pub use event::{ActivityEvent, ActorRole, EventActor, EventKind};
pub use fetch::{FetchCursor, FetchRun, FetchRunKind, ResourceKind};
pub use freshness::DataAsOf;
pub use identity::{
    IdentityLinkPending, IdentityLinkRejection, MembershipIdentity, UserIdentity, VerifiedVia,
};
pub use inbox::{InboxIssueRow, InboxStatus, UserIssueState};
pub use issue::{Issue, IssueState, IssueUpsert, IssueUpsertOutcome, RepoSummary};
pub use issue_dates::{
    IssueDates, ProjectV2MirrorTask, ProjectV2MirrorTaskKind, RepoProjectLink,
};
pub use issue_mutation::{IssueMutation, IssueMutationOp, IssueMutationResult};
pub use membership::{Membership, MembershipRole};
pub use milestone::{Milestone, MilestoneState, MilestoneUpsert};
pub use eol::{EolResult, EolTestReport, EolTestUpsert, RunEolSummary, RunEolSummaryUpsert};
pub use manufacturing::{
    ManufacturingRun, ProductUnit, RunStatus, RunUpsert, UnitAllocation, UnitStatus, UnitUpsert,
    MAX_UNIT_ALLOC,
};
pub use org::Org;
pub use party::{
    Customer, CustomerUpsert, Manufacturer, ManufacturerUpsert, PartyListFilter, Supplier,
    SupplierUpsert,
};
pub use pin::{Pin, PinKind, PIN_CAP};
pub use product::{
    Product, ProductListFilter, ProductProjectLink, ProductStatus, ProductUpsert,
};
pub use product_doc::ProductDocument;
pub use product_manual::{
    ManualRevision, ManualUpsert, ProductManual, RevisionStatus, RevisionUpsert,
};
pub use product_release::{
    ProductRelease, ProductReleaseCreate, ProductReleaseUpdate, ReleaseKind, ReleaseLink,
};
pub use project::{
    Project, ProjectIssueAddOutcome, ProjectIssueAddSkip, ProjectListFilter, ProjectRepo,
    ProjectStatus, ProjectUpsert,
};
pub use project_exec_summary::{
    BlobRefJson, ExecSummaryChangelogEntry, ExecSummaryChangelogInsert, ExecSummaryCompletion,
    ExecSummaryDocument, ExecSummaryImage, ExecSummaryStatus, ProjectExecSummary,
    ProjectExecSummaryPatch, EXEC_SUMMARY_SUBMIT_THRESHOLD_PERCENT,
};
pub use repo::{Repo, RepoMetadata};
pub use rma::{Rma, RmaCreate, RmaFilter, RmaStatus, RmaUpdate};
pub use setting::UserSetting;
pub use store::{PendingRemoteIssue, Store, StoreError};
pub use store::{IssueListFilter, RepoListFilter, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT};
pub use store::{
    HeatmapBucket, PercentileTriple, RepoActivityHeatmap, RepoCiStats,
    RepoContributorDiversity, RepoPrSizeStats, RepoReviewVelocity,
};
pub use store::{
    IssueMetric, IssueMetricGroupBy, IssueMetricRow, IssueMetricsFilter, IssueTimelineRow,
    RepoSyncStatus,
};
pub use tag::{Tag, TagScopeKind, TAG_LINK_WARN_THRESHOLD};
pub use tag_link::{TagLink, TagLinkKind};
pub use team::Team;
pub use user::{Role, User};
pub use webhook::WebhookDelivery;
pub use window::{Window, WindowAnchor};
