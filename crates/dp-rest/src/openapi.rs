//! OpenAPI aggregation for dev-pulse (Phase 4 stage 6).
//!
//! Every utoipa-annotated handler in `dp-rest` is collected here
//! into a single [`DevPulseApi`] document, plus a docs-only stub
//! for the Phase 2 webhook receiver (the live handler lives in
//! `dp-fetcher::webhook::router::receive`; the stub below exists
//! purely so the path shows up in the spec). `dp-server` calls
//! [`DevPulseApi::openapi`] and hands the result to
//! `ServerBuilder::with_openapi` — consumer-rules §6.7, dp-rest
//! owns the document.
//!
//! A snapshot test in `tests/openapi_snapshot.rs` pins the
//! generated JSON to `tests/openapi.snapshot.json` so accidental
//! schema drift surfaces as a failing test, not a silent breaking
//! change for the Phase 5 MCP / Phase 7 frontend clients. The
//! snapshot is regenerated via:
//!
//! ```text
//! cargo test -p dp-rest -- --update-openapi-snapshot
//! ```

use utoipa::OpenApi;

// ---------------------------------------------------------------------------
// Webhook docs-only stub
// ---------------------------------------------------------------------------

/// Documentation-only stub for `POST /webhooks/github`.
///
/// The live receiver is `dp_fetcher::webhook::router::receive`;
/// authentication is the GitHub HMAC (`X-Hub-Signature-256`),
/// **not** a principal cookie. Stage 6 mirrors the route's
/// behaviour into the OpenAPI document so MCP / frontend clients
/// know it exists, without coupling `dp-fetcher` to utoipa.
///
/// This function is never invoked — `dp-server` mounts the real
/// router from `dp-fetcher` directly.
#[utoipa::path(
    post,
    path = "/webhooks/github",
    request_body(
        description = "Raw GitHub webhook JSON payload.",
        content_type = "application/json",
    ),
    params(
        ("X-GitHub-Delivery"   = String, Header, description = "GitHub-issued delivery id; the inbox dedup key."),
        ("X-GitHub-Event"      = String, Header, description = "Event kind (e.g. `pull_request`, `push`)."),
        ("X-Hub-Signature-256" = String, Header, description = "Hex-encoded `sha256=<digest>` HMAC over the raw body."),
    ),
    responses(
        (status = 200, description = "Delivery enqueued (or replayed — GitHub retries on any non-2xx)"),
        (status = 400, description = "Missing required header or unparseable JSON body"),
        (status = 401, description = "Missing / malformed / mismatched signature"),
        (status = 500, description = "Persistence failure other than a replay conflict"),
    ),
    tag = "webhooks",
)]
#[allow(dead_code)]
async fn webhook_github_stub() {}

// ---------------------------------------------------------------------------
// The aggregated document
// ---------------------------------------------------------------------------

/// `#[derive(OpenApi)]` aggregator — one entry per utoipa-annotated
/// handler dev-pulse exposes, plus the docs-only webhook stub.
///
/// Re-exported by the crate root so `dp-server::build()` can pass
/// `DevPulseApi::openapi()` to `ServerBuilder::with_openapi`.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "dev-pulse",
        version = "0.1.0",
        description = "GitHub activity reports + operator surface (Phase 4).",
    ),
    paths(
        // Reports (stage 3).
        crate::reports::user_report,
        crate::reports::team_report,
        crate::reports::org_report,
        crate::reports::home_org_split_report,
        crate::reports::freshness_report,
        crate::reports::project_portfolio_report,
        // Directory (stage 4).
        crate::directory::list_users,
        crate::directory::list_orgs,
        crate::directory::list_my_orgs,
        crate::directory::list_teams,
        crate::directory::set_home_org,
        // Admin (stage 5).
        crate::admin::refresh,
        crate::admin::import_repo,
        crate::admin::list_runs,
        crate::admin::anonymise_user,
        crate::admin::export_user,
        crate::admin::set_user_role,
        crate::me_identities::list_user_identities_admin,
        // Pins (SCOPE-PROJECTS §6).
        crate::pins::list_pins,
        crate::pins::add_pin,
        crate::pins::remove_pin,
        crate::pins::reorder_pins,
        // Projects v2 CRUD (linear-projects-v2.md §7.1).
        crate::projects::list_projects,
        crate::projects::get_project,
        crate::projects::create_project,
        crate::projects::patch_project,
        crate::projects::archive_project,
        // Product & Manufacturing — P1 (DOCS/ideas/product-manufacturing.md).
        crate::parties::list_manufacturers,
        crate::parties::get_manufacturer,
        crate::parties::create_manufacturer,
        crate::parties::patch_manufacturer,
        crate::parties::archive_manufacturer,
        crate::parties::list_suppliers,
        crate::parties::get_supplier,
        crate::parties::create_supplier,
        crate::parties::patch_supplier,
        crate::parties::archive_supplier,
        crate::parties::list_customers,
        crate::parties::get_customer,
        crate::parties::create_customer,
        crate::parties::patch_customer,
        crate::parties::archive_customer,
        crate::products::list_products,
        crate::products::get_product,
        crate::products::create_product,
        crate::products::patch_product,
        crate::products::archive_product,
        crate::products::list_product_projects,
        crate::products::list_project_products,
        crate::products::link_product_project,
        crate::products::unlink_product_project,
        crate::products::list_product_documents,
        crate::products::upload_product_document,
        crate::products::delete_product_document,
        crate::products::proxy_product_blob,
        crate::product_manuals::list_manuals,
        crate::product_manuals::create_manual,
        crate::product_manuals::list_revisions,
        crate::product_manuals::create_revision,
        crate::product_manuals::publish_revision,
        crate::product_releases::list_releases,
        crate::product_releases::create_release,
        crate::product_releases::patch_release,
        crate::product_releases::archive_release,
        // P2 — runs / units / EOL / QR.
        crate::manufacturing::list_runs,
        crate::manufacturing::create_run,
        crate::manufacturing::get_run,
        crate::manufacturing::patch_run,
        crate::manufacturing::list_run_units,
        crate::manufacturing::allocate_units,
        crate::manufacturing::get_unit,
        crate::manufacturing::patch_unit,
        crate::manufacturing::unit_qr_svg,
        crate::manufacturing::list_unit_eol,
        crate::manufacturing::record_eol,
        crate::manufacturing::get_run_eol_summary,
        crate::manufacturing::upsert_run_eol_summary,
        // P3 — returns / RMA.
        crate::rma::list_rma,
        crate::rma::get_rma,
        crate::rma::create_rma,
        crate::rma::patch_rma,
        // Project ↔ issue membership (linear-projects-v2.md §7.2).
        crate::project_issues::list_project_issues,
        crate::project_issues::list_group_by_options,
        crate::project_issues::bulk_add_issues,
        crate::project_issues::remove_project_issue,
        crate::project_issues::get_project_for_issue,
        // Project ↔ repo soft scoping.
        crate::project_repos::list_project_repos,
        crate::project_repos::add_project_repo,
        crate::project_repos::remove_project_repo,
        // Project ↔ GitHub board mirror picker + link CRUD
        // (linear-projects-v2.md §7.3).
        crate::board_links::list_org_projects_v2,
        crate::board_links::list_board_links,
        crate::board_links::create_board_link,
        crate::board_links::delete_board_link,
        crate::project_views::list_project_views,
        crate::project_views::get_project_view,
        crate::project_views::create_project_view,
        crate::project_views::update_project_view,
        crate::project_views::delete_project_view,
        crate::project_views::reorder_project_views,
        // Project milestones strip (PROJECT-VIEW.md §5.5, Slice 1).
        crate::project_milestones::list_project_milestones,
        // Adopt milestone as project primary (Slice 5).
        crate::project_milestones::adopt_milestone,
        // Create a milestone on a linked repo (two-way sync).
        crate::project_milestones::create_project_milestone,
        // Edit / close / reopen a mirrored milestone.
        crate::project_milestones::patch_project_milestone,
        // Delete a mirrored milestone from GitHub + locally.
        crate::project_milestones::delete_project_milestone,
        // Project Executive Summary (DOCS/SCOPE-PROJECT-EXECUTIVE-SUMMARY.md).
        crate::project_exec_summary::get_project_exec_summary,
        crate::project_exec_summary::patch_project_exec_summary,
        crate::project_exec_summary::submit_project_exec_summary,
        crate::project_exec_summary::approve_project_exec_summary,
        crate::project_exec_summary::revert_project_exec_summary,
        crate::project_exec_summary::restore_exec_summary_changelog,
        // Tags (SCOPE-PROJECTS §7).
        crate::tags::list_tags,
        crate::tags::list_my_tags,
        crate::tags::create_tag,
        crate::tags::get_tag,
        crate::tags::update_tag,
        crate::tags::link_targets,
        crate::tags::unlink_targets,
        crate::tags::list_project_tags,
        // GitHub App permission banner (SCOPE-PROJECTS §13.6).
        crate::app_permissions::list_app_install_banner,
        // Issue write surface (SCOPE §18 / SCOPE-PROJECTS §8).
        crate::issues_write::create_issue,
        crate::issues_write::patch_issue,
        crate::issues_write::create_comment,
        crate::issues_write::refresh_issue,
        // Issue dates surface (§3.10).
        crate::issue_dates::patch_issue_dates,
        crate::issue_dates::get_issue_dates,
        // Issue read surface — present since slice 1, registered
        // here in slice 2 so the OpenAPI document covers every
        // mounted handler.
        crate::issues_read::list_issues,
        crate::issues_read::me_queue,
        crate::issues_read::get_issue_by_id,
        crate::issues_read::get_issue_by_number,
        crate::issues_read::get_issue_timeline,
        // Repo read surface + slice-2 sync endpoints.
        crate::repos::list_repos,
        crate::repos::get_repo_metadata,
        crate::repos::get_repo_pr_size_stats,
        crate::repos::get_repo_ci_stats,
        crate::repos::get_repo_activity_heatmap,
        crate::repos::get_repo_review_velocity,
        crate::repos::get_repo_contributor_diversity,
        crate::repos::get_repo_sync_status,
        crate::repos::request_repo_sync,
        crate::repos::request_repo_sync_by_name,
        // Reports — slice 2 issue-metric surface.
        crate::reports::issues_report,
        // Inbox endpoints (slice 1) — were missing from the spec.
        crate::inbox::mark_seen,
        crate::inbox::set_inbox_state,
        crate::inbox::bulk_inbox,
        // Multi-identity surface (§3.0 / §10).
        crate::me_identities::list_me_identities,
        // Per-user settings (§Account → Settings).
        crate::settings::list_settings,
        crate::settings::get_setting,
        crate::settings::put_setting,
        crate::settings::delete_setting,
        crate::settings::test_github_pat,
        // Webhooks (Phase 2 — docs-only stub, real handler in dp-fetcher).
        webhook_github_stub,
    ),
    components(schemas(
        // Reports envelope + row shapes.
        crate::reports::ReportResponse,
        crate::reports::DataAsOfDto,
        crate::reports::CountRow,
        crate::reports::HomeOrgSplitRow,
        // Directory.
        crate::directory::UserDto,
        crate::directory::OrgDto,
        crate::directory::TeamDto,
        crate::directory::Ack,
        crate::directory::SetHomeOrgRequest,
        // Admin.
        crate::admin::RefreshResponse,
        crate::admin::ImportRepoRequest,
        crate::admin::ImportRepoResponse,
        crate::admin::FetchRunDto,
        crate::admin::FetchRunErrorSampleDto,
        crate::admin::UserExport,
        crate::admin::ExportEvent,
        crate::admin::MembershipDto,
        crate::admin::SetUserRoleRequest,
        // Pins.
        crate::pins::PinDto,
        crate::pins::PinKindDto,
        crate::pins::AddPinRequest,
        crate::pins::PinKeyDto,
        crate::pins::ReorderRequest,
        // Projects v2 CRUD (linear-projects-v2.md §7.1).
        crate::projects::ProjectDto,
        crate::projects::ProjectStatusDto,
        crate::projects::ProjectListResponse,
        crate::projects::CreateProjectRequest,
        crate::projects::PatchProjectRequest,
        crate::projects::ArchiveProjectRequest,
        // Product & Manufacturing — P1 DTOs.
        crate::parties::PartyDto,
        crate::parties::PartyListResponse,
        crate::parties::CustomerDto,
        crate::parties::CustomerListResponse,
        crate::parties::CreatePartyRequest,
        crate::parties::PatchPartyRequest,
        crate::parties::CreateCustomerRequest,
        crate::parties::PatchCustomerRequest,
        crate::parties::ArchivePartyRequest,
        crate::products::ProductDto,
        crate::products::ProductStatusDto,
        crate::products::ProductListResponse,
        crate::products::ProductDocumentDto,
        crate::products::CreateProductRequest,
        crate::products::PatchProductRequest,
        crate::products::ArchiveProductRequest,
        crate::products::LinkProjectRequest,
        crate::product_manuals::ManualDto,
        crate::product_manuals::ManualRevisionDto,
        crate::product_manuals::RevisionStatusDto,
        crate::product_manuals::CreateManualRequest,
        crate::product_manuals::CreateRevisionRequest,
        crate::product_releases::ProductReleaseDto,
        crate::product_releases::ReleaseKindDto,
        crate::product_releases::ReleaseLinkDto,
        crate::product_releases::CreateReleaseRequest,
        crate::product_releases::PatchReleaseRequest,
        crate::product_releases::ArchiveReleaseRequest,
        // P2 — runs / units / EOL DTOs.
        crate::manufacturing::RunDto,
        crate::manufacturing::RunStatusDto,
        crate::manufacturing::UnitDto,
        crate::manufacturing::UnitStatusDto,
        crate::manufacturing::UnitAllocationDto,
        crate::manufacturing::EolReportDto,
        crate::manufacturing::EolResultDto,
        crate::manufacturing::RunEolSummaryDto,
        crate::manufacturing::PublicUnitDto,
        crate::manufacturing::PublicManualRef,
        crate::manufacturing::CreateRunRequest,
        crate::manufacturing::PatchRunRequest,
        crate::manufacturing::AllocateUnitsRequest,
        crate::manufacturing::PatchUnitRequest,
        crate::manufacturing::RecordEolRequest,
        crate::manufacturing::RunEolSummaryRequest,
        crate::rma::RmaDto,
        crate::rma::RmaStatusDto,
        crate::rma::ListRmaQuery,
        crate::rma::CreateRmaRequest,
        crate::rma::PatchRmaRequest,
        // Project ↔ issue membership (linear-projects-v2.md §7.2).
        crate::project_issues::BulkAddIssuesRequest,
        crate::project_issues::BulkAddResult,
        crate::project_issues::BulkAddSkipDto,
        crate::project_issues::GroupByOptionDto,
        crate::project_issues::GroupByOptionsResponse,
        // Project ↔ repo soft scoping.
        crate::project_repos::ProjectRepoDto,
        // Project ↔ board mirror picker + link CRUD
        // (linear-projects-v2.md §7.3).
        crate::board_links::OrgProjectPickerDto,
        crate::board_links::BoardPickerDto,
        crate::board_links::DateFieldDto,
        crate::board_links::BoardLinkDto,
        crate::board_links::CreateBoardLinkRequest,
        crate::project_views::ProjectViewDto,
        crate::project_views::ProjectViewCreateBody,
        crate::project_views::ProjectViewReorderBody,
        // Project milestones (PROJECT-VIEW.md §5.5).
        crate::project_milestones::MilestoneDto,
        crate::project_milestones::AdoptMilestoneBody,
        crate::project_milestones::CreateMilestoneRequest,
        crate::project_milestones::PatchMilestoneRequest,
        // Project Executive Summary DTOs.
        crate::project_exec_summary::ExecSummaryDto,
        crate::project_exec_summary::ExecSummarySummaryDto,
        crate::project_exec_summary::ExecSummaryScopeDto,
        crate::project_exec_summary::ExecSummaryRequirementsDto,
        crate::project_exec_summary::ExecSummaryHardwareDto,
        crate::project_exec_summary::ExecSummaryCommercialDto,
        crate::project_exec_summary::ExecSummaryApprovalDto,
        crate::project_exec_summary::ExecSummaryImageDto,
        crate::project_exec_summary::ExecSummaryDocumentDto,
        crate::project_exec_summary::ExecSummaryChangelogEntryDto,
        crate::project_exec_summary::ExecSummaryCompletionDto,
        crate::project_exec_summary::ExecSummaryStatusDto,
        crate::project_exec_summary::ExecSummaryPatchBody,
        crate::project_exec_summary::ExecSummarySummaryPatch,
        crate::project_exec_summary::ExecSummaryScopePatch,
        crate::project_exec_summary::ExecSummaryRequirementsPatch,
        crate::project_exec_summary::ExecSummaryHardwarePatch,
        crate::project_exec_summary::ExecSummaryCommercialPatch,
        crate::project_exec_summary::ExecSummaryApprovalPatch,
        crate::project_exec_summary::ExecSummaryDocumentPatchBody,
        crate::project_exec_summary::ExecSummaryChangelogInsertBody,
        crate::project_exec_summary::ExecSummaryChangelogRestoreBody,
        crate::project_exec_summary::ExecSummaryApproveBody,
        crate::project_exec_summary::SubmitIncompleteBody,
        // Tags.
        crate::tags::TagDto,
        crate::tags::TagScopeKindDto,
        crate::tags::TagLinkKindDto,
        crate::tags::CreateTagRequest,
        crate::tags::UpdateTagRequest,
        crate::tags::LinkRequestItem,
        crate::tags::LinkBatchRequest,
        crate::tags::LinkBatchResponse,
        crate::tags::UnlinkBatchRequest,
        crate::tags::TagDetailResponse,
        crate::tags::TagLinkDto,
        // GitHub App permission banner (SCOPE-PROJECTS §13.6).
        crate::app_permissions::AppInstallBannerOrgDto,
        crate::app_permissions::AppInstallBannerResponse,
        crate::app_permissions::AppManifest,
        // Issue write surface (SCOPE §18 / SCOPE-PROJECTS §8).
        crate::issues_write::CreateIssueRequest,
        crate::issues_write::CreateIssueResponse,
        crate::issues_write::PatchIssueRequest,
        crate::issues_write::CreateCommentRequest,
        crate::issues_write::IssuePatch,
        // Issue dates DTOs (§3.10).
        crate::issue_dates::PatchIssueDatesRequest,
        crate::issue_dates::IssueDatesDto,
        crate::issues_read::IssueDto,
        crate::issues_read::IssueStateDto,
        crate::issues_read::IssueListResponse,
        crate::issues_read::IssueBucket,
        crate::issues_read::TimelineEntryDto,
        crate::issues_read::TimelineResponse,
        // Repo wire DTOs.
        crate::repos::RepoSummaryDto,
        crate::repos::RepoListResponse,
        crate::repos::RepoMetadataDto,
        crate::repos::PercentileTripleDto,
        crate::repos::RepoPrSizeStatsDto,
        crate::repos::RepoCiStatsDto,
        crate::repos::HeatmapBucketDto,
        crate::repos::RepoActivityHeatmapDto,
        crate::repos::RepoReviewVelocityDto,
        crate::repos::RepoContributorDiversityDto,
        crate::repos::RepoSyncStatusDto,
        crate::repos::RepoSyncQueuedDto,
        crate::repos::RepoSyncByNameRequest,
        crate::repos::RepoSyncQueuedByNameDto,
        // Issue-metrics reports (slice 2).
        crate::reports::IssuesReportResponse,
        crate::reports::IssuesReportRow,
        // Inbox DTOs (slice 1, registered now).
        crate::inbox::UserIssueStateDto,
        crate::inbox::InboxStatusDto,
        crate::inbox::MarkSeenRequest,
        crate::inbox::SetInboxStateRequest,
        crate::inbox::BulkInboxOp,
        crate::inbox::BulkInboxRequest,
        crate::inbox::BulkInboxResponse,
        // Multi-identity surface (§3.0 / §10).
        crate::me_identities::MeIdentityDto,
        crate::me_identities::MeIdentitiesResponse,
        // Per-user settings.
        crate::settings::SettingDto,
        crate::settings::PutSettingRequest,
        crate::settings::TestGithubPatResponse,
    )),
    tags(
        (name = "reports",   description = "Per-user / team / org activity reports + freshness probe."),
        (name = "directory", description = "Operator-facing user / org / team listings + home-org flip."),
        (name = "admin",     description = "Operator-only surface: refresh, run-log, GDPR cascade + export."),
        (name = "pins",      description = "Per-user pinned repos / tags (SCOPE-PROJECTS §6)."),
        (name = "projects",  description = "First-class projects surface (linear-projects-v2.md §7)."),
        (name = "manufacturing", description = "Product & Manufacturing surface — products, parties, manuals, runs, units, EOL, returns (DOCS/ideas/product-manufacturing.md)."),
        (name = "tags",      description = "Cross-org home-grown tags (SCOPE-PROJECTS §7)."),
        (name = "github_app", description = "GitHub App install permission surface (SCOPE-PROJECTS §8.4, §13.6)."),
        (name = "identities", description = "Linked OAuth identities for the caller (linear-projects-idea.md §3.0 / §10)."),
        (name = "settings", description = "Per-user K/V settings (Account → Settings page). Pinned key catalogue in `dp_rest::settings::KEYS`."),
        (name = "webhooks",  description = "GitHub webhook receiver. HMAC-authenticated, not principal-wrapped."),
    ),
)]
pub struct DevPulseApi;
