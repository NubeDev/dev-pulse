//! `dp-rest` — axum `Router` fragments dev-pulse mounts onto the
//! starter-server app.
//!
//! Modules:
//!
//! * [`admin`] — `POST /admin/refresh` (stage 8 of Phase 2, the
//!   operator-triggered reconciler tick). Auth is added by the
//!   composition layer; the router fragment here doesn't enforce it.
//! * [`audit`] — pinned `audit_log` action vocabulary + the single
//!   `record()` helper every protected handler routes through
//!   (Phase 4 D4.4). Also re-exports the tiny [`Principal`]
//!   carried via `axum::Extension` until stage 9 swaps in the
//!   starter-server one.
//! * [`directory`] — Phase 4 stage 4 directory surface: `GET /users`,
//!   `GET /orgs`, `GET /teams`, `POST /home-org`. The home-org
//!   mutation is atomic per the [`Store::set_home_org_for_user`]
//!   contract.
//! * [`reports`] — Phase 4 stage 3 report surface: `GET /reports/user/:id`,
//!   `/team/:id`, `/org/:id`, `/home-org-split`, `/freshness`. Every
//!   handler echoes the resolved [`Window`][dp_reports::Window] back
//!   per TODO §0.4 and carries [`DataAsOfDto`] per §11.7.
//! * [`state`] — shared [`AppState`] (currently just a [`Store`]
//!   handle; later Phase 4 stages widen it).
//! * [`error`] — one [`ApiError`] type every handler returns.
//!
//! Boundary note (§0.6): `dp-rest` is an edge crate, so starter-*
//! imports are allowed here. Stages 3 and 4 don't need any; the
//! `with_principal` / `require_permission` wrappers land in later
//! stages.
//!
//! [`Scheduler::try_trigger_now`]: dp_fetcher::reconciler::Scheduler::try_trigger_now
//! [`Store`]: dp_domain::store::Store
//! [`Store::set_home_org_for_user`]: dp_domain::store::Store::set_home_org_for_user

pub mod admin;
pub mod app_permissions;
pub mod audit;
pub mod board_links;
pub mod directory;
pub mod error;
pub mod inbox;
pub mod issue_dates;
pub mod issues;
pub mod issues_read;
pub mod issues_write;
pub mod me_identities;
pub mod openapi;
pub mod pins;
pub mod project_issues;
pub mod projects;
pub mod reports;
pub mod repo_project_link;
pub mod repos;
pub mod state;
pub mod tags;

pub use admin::{
    admin_router, anonymise_user, export_user, list_runs, AdminState, ExportEvent, FetchRunDto,
    MembershipDto, RefreshQuery, RefreshResponse, RunsQuery, UserExport, EXPORT_PAGE_SIZE,
    RUNS_DEFAULT_LIMIT, RUNS_MAX_LIMIT,
};
pub use app_permissions::{
    app_manifest_permissions, app_permissions_router, list_app_install_banner,
    require_issues_write, AppInstallBannerOrgDto, AppInstallBannerResponse, AppManifest,
    GitHubAppConfig,
};
pub use audit::Principal;
pub use board_links::{
    board_links_router, create_board_link, delete_board_link, list_board_links,
    list_org_projects_v2, normalize_picker_envelope, BoardLinkDto, BoardPickerDto,
    CreateBoardLinkRequest, DateFieldDto, OctocrabOrgProjectsPicker,
    OrgProjectPickerDto, OrgProjectsPickerBackend, OrgProjectsPickerError,
    UnconfiguredOrgProjectsPicker,
};
pub use directory::{
    directory_router, list_orgs, list_teams, list_users, set_home_org, Ack, OrgDto, OrgFilter,
    OrgRequired, SetHomeOrgRequest, TeamDto, UserDto,
};
pub use error::ApiError;
pub use inbox::{
    bulk_inbox, inbox_router, mark_seen, set_inbox_state, BulkInboxOp, BulkInboxRequest,
    BulkInboxResponse, InboxStatusDto, MarkSeenRequest, SetInboxStateRequest, UserIssueStateDto,
    SEEN_BATCH_CAP,
};
pub use issues::{
    acquire_issue_mutation_slot, commit_issue_mutation, rollback_issue_mutation,
    sweep_pending_remote_timeouts, AcquireOutcome, AcquiredSlot, SweepReport,
};
pub use issue_dates::{
    get_issue_dates, issue_dates_router, patch_issue_dates, IssueDatesDto,
    IssueNodeIdRef, MirrorDatesOk, MirrorError, OctocrabProjectV2Mirror,
    PatchIssueDatesRequest, ProjectV2MirrorBackend, UnconfiguredProjectV2Mirror,
};
pub use issues_write::{
    create_comment, create_issue, issues_write_router, patch_issue, CreateCommentRequest,
    CreateIssueRequest, CreateIssueResponse, FetcherIssueWriter, IssuePatch, IssueWriteBackend,
    IssueWriteError, PatchIssueRequest, UnconfiguredIssueWriter,
};
pub use issues_read::{
    get_issue_by_id, get_issue_by_number, get_issue_timeline, issues_read_router, list_issues,
    me_queue, IssueDto, IssueListResponse, IssueStateDto, ListIssuesQuery, StateFilter,
    TimelineEntryDto, TimelineQuery, TimelineResponse,
};
pub use repos::{
    get_repo_sync_status, list_repos, repos_router, request_repo_sync, ListReposQuery,
    RepoListResponse, RepoSummaryDto, RepoSyncQueuedDto, RepoSyncStatusDto,
};
pub use repo_project_link::{
    delete_repo_project_link, get_repo_project_link, list_repo_projects,
    put_repo_project_link, repo_project_link_router, OctocrabProjectsPicker,
    ProjectsPickerBackend, ProjectsPickerError, PutRepoProjectLinkRequest,
    RepoProjectLinkDto, UnconfiguredProjectsPicker,
};
pub use openapi::DevPulseApi;
pub use pins::{
    add_pin, list_pins, pins_router, remove_pin, reorder_pins, AddPinRequest, PinDto, PinKeyDto,
    PinKindDto, ReorderRequest, PIN_CAP,
};
pub use projects::{
    archive_project, create_project, get_project, list_projects, patch_project, projects_router,
    ArchiveProjectRequest, CreateProjectRequest, ListProjectsQuery, PatchProjectRequest,
    ProjectDto, ProjectListResponse, ProjectStatusDto,
};
pub use project_issues::{
    bulk_add_issues, get_project_for_issue, list_project_issues, project_issues_router,
    remove_project_issue, BulkAddIssuesRequest, BulkAddResult, BulkAddSkipDto,
    ListProjectIssuesQuery, RemoveIssueQuery, BULK_ADD_ISSUE_CAP,
};
pub use reports::{
    freshness_report, home_org_split_report, issues_report, org_report, reports_router,
    team_report, user_report, CountRow, DataAsOfDto, HomeOrgSplitRow, IssuesReportQuery,
    IssuesReportResponse, IssuesReportRow, ReportQuery, ReportResponse,
};
pub use state::AppState;
pub use me_identities::{
    list_me_identities, me_identities_router, MeIdentitiesResponse, MeIdentityDto,
};
pub use tags::{
    create_tag, get_tag, link_targets, list_my_tags, list_tags, tags_router, unlink_targets,
    update_tag, CreateTagRequest, LinkBatchRequest, LinkBatchResponse, LinkRequestItem,
    ListTagsQuery, TagDetailQuery, TagDetailResponse, TagDto, TagLinkKindDto, TagScopeKindDto,
    UnlinkBatchRequest, UpdateTagRequest, LINKS_PAGE_SIZE,
};
