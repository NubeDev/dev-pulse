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
        // Tags (SCOPE-PROJECTS §7).
        crate::tags::list_tags,
        crate::tags::list_my_tags,
        crate::tags::create_tag,
        crate::tags::get_tag,
        crate::tags::update_tag,
        crate::tags::link_targets,
        crate::tags::unlink_targets,
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
    )),
    tags(
        (name = "reports",   description = "Per-user / team / org activity reports + freshness probe."),
        (name = "directory", description = "Operator-facing user / org / team listings + home-org flip."),
        (name = "admin",     description = "Operator-only surface: refresh, run-log, GDPR cascade + export."),
        (name = "pins",      description = "Per-user pinned repos / tags (SCOPE-PROJECTS §6)."),
        (name = "tags",      description = "Cross-org home-grown tags (SCOPE-PROJECTS §7)."),
        (name = "webhooks",  description = "GitHub webhook receiver. HMAC-authenticated, not principal-wrapped."),
    ),
)]
pub struct DevPulseApi;
