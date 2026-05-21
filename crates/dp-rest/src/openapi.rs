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
        // Directory (stage 4).
        crate::directory::list_users,
        crate::directory::list_orgs,
        crate::directory::list_my_orgs,
        crate::directory::list_teams,
        crate::directory::set_home_org,
        // Admin (stage 5).
        crate::admin::refresh,
        crate::admin::list_runs,
        crate::admin::anonymise_user,
        crate::admin::export_user,
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
        // Tags (SCOPE-PROJECTS §7).
        crate::tags::list_tags,
        crate::tags::list_my_tags,
        crate::tags::create_tag,
        crate::tags::get_tag,
        crate::tags::update_tag,
        crate::tags::link_targets,
        crate::tags::unlink_targets,
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
        crate::admin::FetchRunDto,
        crate::admin::UserExport,
        crate::admin::ExportEvent,
        crate::admin::MembershipDto,
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
        (name = "tags",      description = "Cross-org home-grown tags (SCOPE-PROJECTS §7)."),
        (name = "github_app", description = "GitHub App install permission surface (SCOPE-PROJECTS §8.4, §13.6)."),
        (name = "identities", description = "Linked OAuth identities for the caller (linear-projects-idea.md §3.0 / §10)."),
        (name = "settings", description = "Per-user K/V settings (Account → Settings page). Pinned key catalogue in `dp_rest::settings::KEYS`."),
        (name = "webhooks",  description = "GitHub webhook receiver. HMAC-authenticated, not principal-wrapped."),
    ),
)]
pub struct DevPulseApi;
