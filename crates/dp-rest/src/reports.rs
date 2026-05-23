//! Report handlers (TODO §Phase 4 stage 3).
//!
//! Five `GET` routes — one per report shape — all sharing the same
//! [`ReportResponse`] envelope per TODO §0.4 (resolved [`Window`]
//! echoed back) and §11.7 (`data_as_of` carried on every response).
//!
//! | route                                | rows shape                            |
//! |--------------------------------------|---------------------------------------|
//! | `GET /reports/user/:user_id`         | per-bucket counts (default: by user)  |
//! | `GET /reports/team/:team_id`         | per-bucket counts                     |
//! | `GET /reports/org/:org_id`           | per-bucket counts                     |
//! | `GET /reports/home-org-split`        | per `(user, org)` bucket counts       |
//! | `GET /reports/freshness`             | `null` — freshness envelope only      |
//!
//! Each handler:
//!
//! 1. Deserialises a flat [`ReportQuery`] from the query string
//!    (axum's `Query` runs `serde_urlencoded`, which doesn't nest;
//!    [`ReportQuery::to_request`] expands the flat form into a
//!    full [`ReportRequest`] by parsing comma-separated lists).
//! 2. Calls [`resolve_window`] to turn `(label, tz, anchor)` into a
//!    concrete UTC `[start, end)` — server-side, never the
//!    frontend.
//! 3. Reads `event_actor` rows through
//!    [`Store::list_event_actor_rows_in_window`][slist].
//! 4. Applies the [`ScopeMode`] lens.
//! 5. Snapshots [`Store::data_as_of`][sda].
//! 6. Wraps the result in [`ReportResponse`] and returns `200`.
//!
//! Each handler is `#[utoipa::path(...)]`-annotated so the
//! `DevPulseApi` aggregator (added in a later stage) picks them up
//! per `ServerBuilder::with_openapi` (consumer-rules §6.7).
//!
//! [slist]: dp_domain::store::Store::list_event_actor_rows_in_window
//! [sda]: dp_domain::store::Store::data_as_of

use std::collections::HashSet;
use std::sync::Arc;

use axum::{
    extract::{Extension, Path, Query, State},
    response::Json,
    routing::{get, post},
    Router,
};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::event::{ActorRole, EventKind};
use dp_domain::project::{PortfolioQueryFilter, ProjectStatus};
use dp_domain::store::EventActorRow;
use dp_domain::window::WindowAnchor;
use dp_reports::lenses::{all_orgs_combined, per_org_split, single_org};
use dp_reports::{
    count_by_bucket, count_by_org, count_by_repo, count_by_user, empty_reason_for_tag_filter,
    pick_freshness_headline, resolve_window, rollup_kpis, DataAsOf, GroupBy, PortfolioKpis,
    ProjectPortfolioRequest, ProjectPortfolioResponse, ProjectPortfolioRow, ReportRequest,
    ScopeMode, TrendBucket, Window, WindowLabel, WindowSpec, EMPTY_REASON_TAG_KIND_MISMATCH,
    MAX_TAGS_FOR_GROUP_BY_TAG, PORTFOLIO_LIMIT_MAX,
};

use crate::error::ApiError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Wire envelopes (response side)
// ---------------------------------------------------------------------------

/// `data_as_of` wire shape. Mirrors `dp_domain::freshness::DataAsOf`
/// but with the serde derives + utoipa schema the dp-domain type
/// intentionally lacks (dp-domain stays free of HTTP-shape concerns
/// per §0.6).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DataAsOfDto {
    /// Most recent finished webhook-worker tick.
    pub webhook_latest: Option<DateTime<Utc>>,
    /// Most recent finished reconciler tick.
    pub reconciler_latest: Option<DateTime<Utc>>,
    /// Per-org reconciler freshness. Absent orgs have never been
    /// touched by the reconciler (treat as "pending", not "stale").
    pub per_org: std::collections::HashMap<Uuid, DateTime<Utc>>,
    /// Convenience headline picked by lens (SCOPE §11.7). `None`
    /// for `per_org_split` (the UI renders per row) and for
    /// `single_org` / `all_orgs_combined` when the requested orgs
    /// have no freshness entry yet.
    pub headline: Option<DateTime<Utc>>,
}

impl DataAsOfDto {
    fn from_domain(d: DataAsOf, scope_mode: ScopeMode, orgs: &[Uuid]) -> Self {
        let headline = pick_freshness_headline(&d, scope_mode, orgs);
        Self {
            webhook_latest: d.webhook_latest,
            reconciler_latest: d.reconciler_latest,
            per_org: d.per_org,
            headline,
        }
    }
}

/// The envelope every report handler returns. `rows` is left as
/// [`serde_json::Value`] so the same struct serves the five report
/// shapes without forcing a per-handler enum — the OpenAPI doc
/// describes each handler's `rows` schema via per-route `body =
/// <Concrete>` overrides (added when the aggregator lands).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ReportResponse {
    /// The UTC window the report ran against. Echoed verbatim from
    /// [`resolve_window`] so the UI can label the report without
    /// re-resolving (§0.4).
    #[schema(value_type = Object)]
    pub resolved_window: Window,
    /// Aggregated rows. `null` for `/reports/freshness`.
    pub rows: serde_json::Value,
    /// Per-response freshness envelope (§11.7).
    pub data_as_of: DataAsOfDto,
    /// Why the report has no rows, when the emptiness is the
    /// *intentional* SCOPE-PROJECTS §7.7 outcome of the tag filter
    /// not matching the requested metric's attribution column
    /// (e.g. an `issue`-only tag queried against a commit metric).
    ///
    /// `None` for every other empty result (window too tight,
    /// no matching events, etc.) — those stay as `rows: []` so the
    /// UI's existing empty-state copy still renders. Locked literal
    /// is [`EMPTY_REASON_TAG_KIND_MISMATCH`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty_reason: Option<String>,
}

/// One aggregated count row. The `key` is stringified so the same
/// shape works for UUID group-bys (`User`, `Team`, `Repo`, `Org`) and
/// time-bucket group-bys (`Day`, `Week`, `Month` — RFC3339 instants).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CountRow {
    /// The bucket key, formatted as a string.
    pub key: String,
    /// Event count attributed to this bucket.
    pub count: i64,
}

/// One row of the `/reports/home-org-split` shape — per (user, org)
/// bucket, count of distinct events the user is credited on within
/// that org.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct HomeOrgSplitRow {
    /// User the bucket is for.
    pub user_id: Uuid,
    /// Org the bucket is for.
    pub org_id: Uuid,
    /// Event count in this bucket.
    pub count: i64,
}

// ---------------------------------------------------------------------------
// Wire envelope (request side)
// ---------------------------------------------------------------------------

/// Flat query shape accepted by every report handler.
/// `serde_urlencoded` cannot decode nested objects, so [`WindowSpec`]
/// is flattened into top-level keys (`window_label`, `tz`, `anchor`,
/// `custom_start`, `custom_end`) and the vector filters are
/// comma-separated strings.
///
/// Example:
///
/// ```text
/// /reports/user/<uid>?window_label=last_week&tz=UTC&anchor=utc&scope_mode=single_org&orgs=<o1>
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct ReportQuery {
    /// Window label (`"last_week"`, `"custom"`, …). Defaults to
    /// `last_7_days` if absent.
    #[serde(default = "default_window_label")]
    pub window_label: WindowLabel,
    /// IANA TZ. Defaults to `UTC` if absent.
    #[serde(default = "default_tz")]
    pub tz: String,
    /// Anchor (`viewer`, `org`, `utc`). Defaults to `utc`.
    #[serde(default = "default_anchor")]
    pub anchor: WindowAnchor,
    /// Required iff `window_label == custom`.
    #[serde(default)]
    pub custom_start: Option<DateTime<Utc>>,
    /// Required iff `window_label == custom`.
    #[serde(default)]
    pub custom_end: Option<DateTime<Utc>>,
    /// Org-scope lens. Defaults to `single_org`.
    #[serde(default = "default_scope_mode")]
    pub scope_mode: ScopeMode,
    /// Optional group-by. `None` means "headline only".
    #[serde(default)]
    pub group_by: Option<GroupBy>,
    /// Comma-separated UUIDs.
    #[serde(default)]
    pub orgs: Option<String>,
    /// Comma-separated UUIDs.
    #[serde(default)]
    pub users: Option<String>,
    /// Comma-separated UUIDs.
    #[serde(default)]
    pub teams: Option<String>,
    /// Comma-separated UUIDs. SCOPE-PROJECTS §7.7 — required to
    /// express tag links of kind `repo`.
    #[serde(default)]
    pub repos: Option<String>,
    /// Comma-separated UUIDs. SCOPE-PROJECTS §7.7 — capped at
    /// [`MAX_TAGS_FOR_GROUP_BY_TAG`] when paired with
    /// `group_by=tag`.
    #[serde(default)]
    pub tags: Option<String>,
    /// Comma-separated snake_case enum names — see [`EventKind`].
    #[serde(default)]
    pub activity_types: Option<String>,
    /// Comma-separated snake_case enum names — see [`ActorRole`].
    #[serde(default)]
    pub actor_roles: Option<String>,
}

fn default_window_label() -> WindowLabel {
    WindowLabel::Last7Days
}
fn default_tz() -> String {
    "UTC".into()
}
fn default_anchor() -> WindowAnchor {
    WindowAnchor::Utc
}
fn default_scope_mode() -> ScopeMode {
    ScopeMode::SingleOrg
}

impl ReportQuery {
    /// Expand to a [`ReportRequest`]. Returns [`ApiError::BadRequest`]
    /// on parse failures so the caller sees `400` not `500`.
    pub fn to_request(&self) -> Result<ReportRequest, ApiError> {
        Ok(ReportRequest {
            orgs: parse_uuid_list(self.orgs.as_deref(), "orgs")?,
            users: parse_uuid_list(self.users.as_deref(), "users")?,
            teams: parse_uuid_list(self.teams.as_deref(), "teams")?,
            repos: parse_uuid_list(self.repos.as_deref(), "repos")?,
            tags: parse_uuid_list(self.tags.as_deref(), "tags")?,
            window: WindowSpec {
                label: self.window_label,
                tz: self.tz.clone(),
                anchor: self.anchor,
                custom_start: self.custom_start,
                custom_end: self.custom_end,
            },
            scope_mode: self.scope_mode,
            group_by: self.group_by,
            activity_types: parse_enum_list::<EventKind>(
                self.activity_types.as_deref(),
                "activity_types",
            )?,
            actor_roles: parse_enum_list::<ActorRole>(
                self.actor_roles.as_deref(),
                "actor_roles",
            )?,
        })
    }
}

fn parse_uuid_list(s: Option<&str>, field: &'static str) -> Result<Vec<Uuid>, ApiError> {
    let Some(s) = s else { return Ok(vec![]) };
    s.split(',')
        .filter(|p| !p.is_empty())
        .map(|p| {
            Uuid::parse_str(p.trim()).map_err(|_| ApiError::BadRequest {
                code: "invalid_uuid",
                message: format!("{field} contains invalid uuid: {p}"),
            })
        })
        .collect()
}

fn parse_enum_list<T>(s: Option<&str>, field: &'static str) -> Result<Vec<T>, ApiError>
where
    T: for<'de> Deserialize<'de>,
{
    let Some(s) = s else { return Ok(vec![]) };
    s.split(',')
        .filter(|p| !p.is_empty())
        .map(|p| {
            // Round-trip through JSON so the snake_case wire form
            // applied by the existing serde derives is reused
            // verbatim — no second source of truth.
            let quoted = format!("\"{}\"", p.trim());
            serde_json::from_str::<T>(&quoted).map_err(|_| ApiError::BadRequest {
                code: "invalid_enum",
                message: format!("{field} contains unknown value: {p}"),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Common helpers
// ---------------------------------------------------------------------------

/// Validate the SCOPE-PROJECTS §7.7 tag-filter rules:
///
/// * `group_by = Tag` requires a non-empty `tags` filter
///   ("all visible tags" is rejected as a UI footgun).
/// * `tags` is capped at [`MAX_TAGS_FOR_GROUP_BY_TAG`] whenever
///   `group_by = Tag` is present — over-cap is a hard `400`, not
///   a silent truncation.
///
/// Both are pre-store checks so a misconfigured frontend gets a
/// fast, predictable error without touching `dp-store-pg`. Codes
/// are stable so the frontend can map them to user-visible copy.
fn validate_tag_filter(request: &ReportRequest) -> Result<(), ApiError> {
    if request.group_by == Some(GroupBy::Tag) {
        if request.tags.is_empty() {
            return Err(ApiError::BadRequest {
                code: "group_by_tag_requires_tags",
                message:
                    "group_by=tag requires a non-empty tags filter (SCOPE-PROJECTS §7.7)"
                        .into(),
            });
        }
        if request.tags.len() > MAX_TAGS_FOR_GROUP_BY_TAG {
            return Err(ApiError::BadRequest {
                code: "tags_filter_over_cap",
                message: format!(
                    "tags filter has {} entries, max is {} when group_by=tag (SCOPE-PROJECTS §7.7)",
                    request.tags.len(),
                    MAX_TAGS_FOR_GROUP_BY_TAG
                ),
            });
        }
    }
    Ok(())
}

/// Resolve the SCOPE-PROJECTS §7.7 metric × link-kind compatibility
/// for a request, returning the locked `empty_reason` literal when
/// the tag filter cannot contribute to the requested
/// `activity_types`.
///
/// This is the gate that turns the empty-result case described in
/// §7.7 ("an issue-linked tag with no other link kinds, queried
/// against a commit metric") into an explicit reason on the wire
/// rather than a silent zero.
///
/// Currently uses [`Store::resolve_tag_targets`] with empty
/// visibility allow-lists — the resulting kind set is good enough
/// for the kind-vs-metric check (we only need *which kinds* the
/// tag carries, not which targets the viewer can see). When the
/// repo/team visibility primitives mature this caller can pass
/// the real allow-lists and the same helper still works.
async fn empty_reason_for_request(
    state: &AppState,
    request: &ReportRequest,
) -> Result<Option<&'static str>, ApiError> {
    if request.tags.is_empty() {
        return Ok(None);
    }
    let links = state
        .store
        .resolve_tag_targets(&request.tags, &[], &[], &[])
        .await?;
    // De-duplicate kinds — three links with `kind = repo` is the
    // same signal as one for this check.
    let mut seen: std::collections::HashSet<dp_domain::TagLinkKind> =
        std::collections::HashSet::new();
    for l in &links {
        seen.insert(l.kind);
    }
    Ok(empty_reason_for_tag_filter(
        seen.into_iter(),
        request.activity_types.iter().copied(),
    ))
}

/// Build a [`ReportResponse`] with empty `rows` and a pinned
/// `empty_reason`, used for the SCOPE-PROJECTS §7.7 metric ×
/// link-kind mismatch short-circuit (the only branch that
/// populates `empty_reason` today).
///
/// Still snapshots `data_as_of` so the UI's "data as of …" widget
/// renders the same way for an empty-with-reason response as for a
/// regular zero-row response.
async fn empty_response(
    state: &AppState,
    request: &ReportRequest,
    window: Window,
    reason: &'static str,
) -> Result<ReportResponse, ApiError> {
    let data_as_of = state.store.data_as_of().await?;
    // Sanity: the only reason literal this stage emits is the
    // §7.7 mismatch. Future reasons must be added explicitly so
    // accidental new literals don't slip past code review.
    debug_assert_eq!(reason, EMPTY_REASON_TAG_KIND_MISMATCH);
    Ok(ReportResponse {
        resolved_window: window,
        rows: json!([] as [serde_json::Value; 0]),
        data_as_of: DataAsOfDto::from_domain(data_as_of, request.scope_mode, &request.orgs),
        empty_reason: Some(reason.to_string()),
    })
}

/// Parse `tz` into [`chrono_tz::Tz`] for trend-bucket truncation.
/// Falls back to UTC on failure — the upstream `resolve_window`
/// already validated the TZ for viewer / org anchors; if we get
/// here with a UTC anchor and a junk TZ string, treating the
/// bucket-axis as UTC is the least-surprising fallback.
fn parse_tz_or_utc(tz: &str) -> Tz {
    tz.parse::<Tz>().unwrap_or(Tz::UTC)
}

/// Apply the [`ScopeMode`] lens to the rows the store returned.
fn apply_lens(
    rows: Vec<EventActorRow>,
    scope_mode: ScopeMode,
    orgs: &[Uuid],
) -> Vec<EventActorRow> {
    match scope_mode {
        ScopeMode::SingleOrg => match orgs.first() {
            Some(o) => single_org(&rows, *o),
            None => rows,
        },
        ScopeMode::AllOrgsCombined => all_orgs_combined(&rows),
        // `per_org_split` lives at the row-shaping layer (the
        // `home-org-split` handler reaches for it directly). For
        // user/team/org handlers we keep the raw rows so the
        // group-by reducer sees every (user, org) pair.
        ScopeMode::PerOrgSplit => rows,
    }
}

/// Run the common store-read + lens pipeline. Centralised so the
/// four per-entity handlers don't drift.
async fn fetch_rows(
    state: &AppState,
    request: &ReportRequest,
    window: &Window,
) -> Result<Vec<EventActorRow>, ApiError> {
    let rows = state
        .store
        .list_event_actor_rows_in_window(
            window,
            &request.orgs,
            &request.repos,
            &request.users,
            &request.actor_roles,
        )
        .await?;
    // The store API does not (yet) take a `kinds` filter, so the
    // `activity_types` query-string filter has to be applied here.
    // Empty slice means "no filter on this dimension" per the
    // `list_event_actor_rows_in_window` contract.
    let filtered: Vec<EventActorRow> = if request.activity_types.is_empty() {
        rows
    } else {
        rows.into_iter()
            .filter(|r| request.activity_types.contains(&r.kind))
            .collect()
    };
    Ok(apply_lens(filtered, request.scope_mode, &request.orgs))
}

/// Pick the count-by reducer matching the request's `group_by`.
/// Defaults to `count_by_user` (matches the SCOPE §8 "headline per
/// actor" default). The `team` group-by needs a `user → team`
/// resolver that this stage's read path does not have; we surface
/// it as a `BadRequest` so the caller knows to omit it for now
/// rather than getting silent empty results.
fn count_rows(
    rows: &[EventActorRow],
    group_by: Option<GroupBy>,
    tz: &Tz,
) -> Result<Vec<CountRow>, ApiError> {
    let out = match group_by {
        Some(GroupBy::Team) => {
            return Err(ApiError::BadRequest {
                code: "group_by_team_unsupported",
                message:
                    "group_by=team requires a user→team resolver not wired in Phase 4 stage 3"
                        .into(),
            })
        }
        Some(GroupBy::Tag) => {
            // The per-tag SQL UNION + row-attribution lives on the
            // store side (SCOPE-PROJECTS §7.7) and lands with the
            // store impl of `resolve_tag_targets` in a later stage.
            // For now the handler returns an explicit 400 rather
            // than zero rows so callers can't read "no data" when
            // they mean "this surface isn't wired yet".
            return Err(ApiError::BadRequest {
                code: "group_by_tag_unsupported",
                message:
                    "group_by=tag requires a tag-aware row builder not wired in this stage \
                     (SCOPE-PROJECTS §7.7)"
                        .into(),
            });
        }
        Some(GroupBy::Repo) => count_by_repo(rows)
            .into_iter()
            .map(|(k, v)| CountRow {
                key: k.to_string(),
                count: v as i64,
            })
            .collect(),
        Some(GroupBy::Org) => count_by_org(rows)
            .into_iter()
            .map(|(k, v)| CountRow {
                key: k.to_string(),
                count: v as i64,
            })
            .collect(),
        Some(bucket_kind @ (GroupBy::Day | GroupBy::Week | GroupBy::Month)) => {
            let bucket = match bucket_kind {
                GroupBy::Day => TrendBucket::Day,
                GroupBy::Week => TrendBucket::Week,
                GroupBy::Month => TrendBucket::Month,
                _ => unreachable!(),
            };
            count_by_bucket(rows, bucket, tz)
                .into_iter()
                .map(|(k, v)| CountRow {
                    key: k.to_rfc3339(),
                    count: v as i64,
                })
                .collect()
        }
        // Default + explicit `User`.
        Some(GroupBy::User) | None => count_by_user(rows)
            .into_iter()
            .map(|(k, v)| CountRow {
                key: k.to_string(),
                count: v as i64,
            })
            .collect(),
    };
    Ok(out)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /reports/user/:user_id` — counts for one user across the
/// requested orgs / window / roles. The path `:user_id` is pushed
/// onto `request.users` so the query string can omit it (the path
/// is authoritative).
#[utoipa::path(
    get,
    path = "/reports/user/{user_id}",
    params(
        ("user_id" = Uuid, Path, description = "User the report is about"),
    ),
    responses(
        (status = 200, description = "Counts for the user in the requested lens", body = ReportResponse),
        (status = 400, description = "Validation failed"),
    ),
    tag = "reports",
)]
pub async fn user_report(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Query(q): Query<ReportQuery>,
) -> Result<Json<ReportResponse>, ApiError> {
    let mut request = q.to_request()?;
    if !request.users.contains(&user_id) {
        request.users.push(user_id);
    }
    validate_tag_filter(&request)?;
    let window = resolve_window(&request.window)?;
    if let Some(reason) = empty_reason_for_request(&state, &request).await? {
        return Ok(Json(empty_response(&state, &request, window, reason).await?));
    }
    let rows = fetch_rows(&state, &request, &window).await?;
    let tz = parse_tz_or_utc(&request.window.tz);
    let counts = count_rows(&rows, request.group_by, &tz)?;
    let data_as_of = state.store.data_as_of().await?;
    Ok(Json(ReportResponse {
        resolved_window: window,
        rows: json!(counts),
        data_as_of: DataAsOfDto::from_domain(data_as_of, request.scope_mode, &request.orgs),
        empty_reason: None,
    }))
}

/// `GET /reports/team/:team_id` — counts narrowed to one team
/// (passed through to the store via `request.teams`). Per-team
/// `group_by=team` is rejected in this stage; the route still
/// supports the other group-bys.
#[utoipa::path(
    get,
    path = "/reports/team/{team_id}",
    params(
        ("team_id" = Uuid, Path, description = "Team the report is about"),
    ),
    responses(
        (status = 200, description = "Counts for the team in the requested lens", body = ReportResponse),
        (status = 400, description = "Validation failed"),
    ),
    tag = "reports",
)]
pub async fn team_report(
    State(state): State<AppState>,
    Path(team_id): Path<Uuid>,
    Query(q): Query<ReportQuery>,
) -> Result<Json<ReportResponse>, ApiError> {
    let mut request = q.to_request()?;
    if !request.teams.contains(&team_id) {
        request.teams.push(team_id);
    }
    validate_tag_filter(&request)?;
    let window = resolve_window(&request.window)?;
    if let Some(reason) = empty_reason_for_request(&state, &request).await? {
        return Ok(Json(empty_response(&state, &request, window, reason).await?));
    }
    let rows = fetch_rows(&state, &request, &window).await?;
    let tz = parse_tz_or_utc(&request.window.tz);
    let counts = count_rows(&rows, request.group_by, &tz)?;
    let data_as_of = state.store.data_as_of().await?;
    Ok(Json(ReportResponse {
        resolved_window: window,
        rows: json!(counts),
        data_as_of: DataAsOfDto::from_domain(data_as_of, request.scope_mode, &request.orgs),
        empty_reason: None,
    }))
}

/// `GET /reports/org/:org_id` — counts for one org. The path
/// `:org_id` is forced into `request.orgs` (single-element) so the
/// `single_org` lens has the org to filter on regardless of what
/// the query string carried.
#[utoipa::path(
    get,
    path = "/reports/org/{org_id}",
    params(
        ("org_id" = Uuid, Path, description = "Org the report is about"),
    ),
    responses(
        (status = 200, description = "Counts for the org in the requested lens", body = ReportResponse),
        (status = 400, description = "Validation failed"),
    ),
    tag = "reports",
)]
pub async fn org_report(
    State(state): State<AppState>,
    Path(org_id): Path<Uuid>,
    Query(q): Query<ReportQuery>,
) -> Result<Json<ReportResponse>, ApiError> {
    let mut request = q.to_request()?;
    // For an org-scoped report the path id is authoritative; replace
    // any orgs the caller passed.
    request.orgs = vec![org_id];
    validate_tag_filter(&request)?;
    let window = resolve_window(&request.window)?;
    if let Some(reason) = empty_reason_for_request(&state, &request).await? {
        return Ok(Json(empty_response(&state, &request, window, reason).await?));
    }
    let rows = fetch_rows(&state, &request, &window).await?;
    let tz = parse_tz_or_utc(&request.window.tz);
    let counts = count_rows(&rows, request.group_by, &tz)?;
    let data_as_of = state.store.data_as_of().await?;
    Ok(Json(ReportResponse {
        resolved_window: window,
        rows: json!(counts),
        data_as_of: DataAsOfDto::from_domain(data_as_of, request.scope_mode, &request.orgs),
        empty_reason: None,
    }))
}

/// `GET /reports/home-org-split` — the SCOPE §7 cross-org executive
/// view. Always uses the [`ScopeMode::PerOrgSplit`] lens regardless
/// of what the request says (the route only makes sense in that
/// mode); each output row is one `(user, org)` bucket with the
/// count of distinct events the user is credited on in that org.
#[utoipa::path(
    get,
    path = "/reports/home-org-split",
    responses(
        (status = 200, description = "Per (user, org) bucket counts", body = ReportResponse),
        (status = 400, description = "Validation failed"),
    ),
    tag = "reports",
)]
pub async fn home_org_split_report(
    State(state): State<AppState>,
    Query(q): Query<ReportQuery>,
) -> Result<Json<ReportResponse>, ApiError> {
    let mut request = q.to_request()?;
    request.scope_mode = ScopeMode::PerOrgSplit;
    validate_tag_filter(&request)?;
    let window = resolve_window(&request.window)?;
    if let Some(reason) = empty_reason_for_request(&state, &request).await? {
        return Ok(Json(empty_response(&state, &request, window, reason).await?));
    }
    let raw = state
        .store
        .list_event_actor_rows_in_window(
            &window,
            &request.orgs,
            &request.repos,
            &request.users,
            &request.actor_roles,
        )
        .await?;
    let buckets = per_org_split(&raw);
    let mut rows: Vec<HomeOrgSplitRow> = buckets
        .into_iter()
        .map(|(key, bucket_rows): ((Uuid, Uuid), Vec<EventActorRow>)| {
            // Distinct events per (user, org) bucket — matches the
            // all-orgs-combined dedup semantics within each bucket
            // so a multi-role row doesn't double-count.
            let mut seen: HashSet<Uuid> = HashSet::new();
            let count = bucket_rows
                .iter()
                .filter(|r| seen.insert(r.event_id))
                .count() as i64;
            HomeOrgSplitRow {
                user_id: key.0,
                org_id: key.1,
                count,
            }
        })
        .collect();
    rows.sort_by(|a, b| a.user_id.cmp(&b.user_id).then(a.org_id.cmp(&b.org_id)));
    let data_as_of = state.store.data_as_of().await?;
    Ok(Json(ReportResponse {
        resolved_window: window,
        rows: json!(rows),
        data_as_of: DataAsOfDto::from_domain(data_as_of, request.scope_mode, &request.orgs),
        empty_reason: None,
    }))
}

/// `GET /reports/freshness` — the data-freshness probe. Returns the
/// same envelope as the report routes but with `rows: null` so the
/// UI's "data as of …" widget can poll a single endpoint without
/// paying for an event scan.
///
/// The resolved window is still echoed back (defaulting to
/// `last_7_days` UTC) so the response shape is uniform.
#[utoipa::path(
    get,
    path = "/reports/freshness",
    responses(
        (status = 200, description = "Data-freshness envelope only", body = ReportResponse),
        (status = 400, description = "Validation failed"),
    ),
    tag = "reports",
)]
pub async fn freshness_report(
    State(state): State<AppState>,
    Query(q): Query<ReportQuery>,
) -> Result<Json<ReportResponse>, ApiError> {
    let request = q.to_request()?;
    validate_tag_filter(&request)?;
    let window = resolve_window(&request.window)?;
    let data_as_of = state.store.data_as_of().await?;
    Ok(Json(ReportResponse {
        resolved_window: window,
        rows: serde_json::Value::Null,
        data_as_of: DataAsOfDto::from_domain(data_as_of, request.scope_mode, &request.orgs),
        empty_reason: None,
    }))
}

// ---------------------------------------------------------------------------
// /reports/project-portfolio — SCOPE-PROJECT-REPORTS.md
// ---------------------------------------------------------------------------

/// `POST /reports/project-portfolio`.
///
/// Returns one row per visible project plus portfolio-level KPIs. The
/// envelope is a structured JSON body (not query string) because the
/// request carries an optional `window` object — same reason
/// SCOPE.md §15.6 chose POST for the activity-report envelope.
///
/// Visibility: trusts the caller-supplied `orgs` list, gated by the
/// outer `with_permission("reports", "read")` layer. A stricter
/// "orgs the caller can see" filter is an authz follow-up tracked
/// in PORTFOLIO-REPORT-PROGRESS.md.
#[utoipa::path(
    post,
    path = "/reports/project-portfolio",
    request_body(
        content_type = "application/json",
        description = "ProjectPortfolioRequest — see dp-reports::project_portfolio.",
    ),
    responses(
        (status = 200, description = "One row per visible project + portfolio KPIs (ProjectPortfolioResponse)"),
        (status = 400, description = "Validation failed"),
    ),
    tag = "reports",
)]
pub async fn project_portfolio_report(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let req: ProjectPortfolioRequest = serde_json::from_value(req).map_err(|e| {
        ApiError::BadRequest {
            code: "invalid_body",
            message: format!("invalid request body: {e}"),
        }
    })?;
    if req.limit == 0 {
        return Err(ApiError::BadRequest {
            code: "invalid_limit",
            message: "limit must be >= 1".into(),
        });
    }
    if req.limit > PORTFOLIO_LIMIT_MAX {
        return Err(ApiError::BadRequest {
            code: "invalid_limit",
            message: format!("limit {} exceeds maximum {PORTFOLIO_LIMIT_MAX}", req.limit),
        });
    }

    let resolved_window = req
        .window
        .as_ref()
        .map(resolve_window)
        .transpose()?;
    let window_pair = resolved_window
        .as_ref()
        .map(|w| (w.start, w.end));

    // Spec §6: empty `statuses` ⇒ default to `[Active, Backlog]`.
    // The SQL builder treats `cardinality(statuses) = 0` as
    // "no filter", so the default must be applied here.
    let statuses: Vec<ProjectStatus> = if req.statuses.is_empty() {
        vec![ProjectStatus::Active, ProjectStatus::Backlog]
    } else {
        req.statuses.clone()
    };

    let now = Utc::now();
    let filter = PortfolioQueryFilter {
        orgs: req.orgs.clone(),
        statuses,
        window: window_pair,
        hide_overdue: req.hide_overdue,
        sort: req.sort,
        now,
        limit: i64::from(req.limit),
        offset: i64::from(req.offset),
    };

    let raw_rows = state.store.list_project_portfolio(&filter).await?;
    let total: u32 = raw_rows
        .first()
        .map(|r| u32::try_from(r.total).unwrap_or(u32::MAX))
        .unwrap_or(0);
    let rows: Vec<ProjectPortfolioRow> = raw_rows.into_iter().map(Into::into).collect();
    let kpis: PortfolioKpis = rollup_kpis(&rows, now);

    let resp = ProjectPortfolioResponse {
        rows,
        resolved_window,
        now,
        total,
        limit: req.limit,
        offset: req.offset,
        kpis,
    };
    Ok(Json(serde_json::to_value(resp).expect("serialise response")))
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// /reports/issues — slice 2, §5.10
// ---------------------------------------------------------------------------

/// Query parameters for `GET /reports/issues`. Mirrors §5.10.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct IssuesReportQuery {
    /// `throughput | lead_time | wip | stale | untriaged`.
    #[serde(default)]
    pub metric: Option<String>,
    /// `repo | org | assignee | week | day`.
    #[serde(default)]
    pub group_by: Option<String>,
    /// Inclusive lower bound (RFC3339).
    #[serde(default)]
    pub since: Option<chrono::DateTime<chrono::Utc>>,
    /// Exclusive upper bound (RFC3339).
    #[serde(default)]
    pub until: Option<chrono::DateTime<chrono::Utc>>,
    /// Comma-separated org ids.
    #[serde(default)]
    pub org_id: Option<String>,
    /// Comma-separated repo ids.
    #[serde(default)]
    pub repo_id: Option<String>,
}

fn csv_uuid_field(s: &Option<String>) -> Vec<uuid::Uuid> {
    s.as_deref()
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .filter_map(|p| uuid::Uuid::parse_str(p).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Row in the issues-report envelope.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct IssuesReportRow {
    /// Bucket label (repo id, org id, assignee login, or ISO date).
    pub bucket: String,
    /// Metric value — count for throughput/wip/stale/untriaged,
    /// median seconds for lead_time.
    pub value: f64,
    /// Row count contributing to the value (useful for medians).
    pub count: i64,
}

/// Envelope for `GET /reports/issues`.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct IssuesReportResponse {
    /// Bucketed rows, sorted by bucket asc.
    pub rows: Vec<IssuesReportRow>,
    /// Total number of buckets.
    pub total: i64,
}

/// `GET /reports/issues` — §5.10 aggregate metrics over
/// `dp_issues`. Authorisation: `("issues", "read")`. The filter
/// scope is intersected with the caller's `org_ids` at the
/// handler boundary.
#[utoipa::path(
    get,
    path = "/reports/issues",
    params(
        ("metric"   = Option<String>, Query, description = "throughput | lead_time | wip | stale | untriaged"),
        ("group_by" = Option<String>, Query, description = "repo | org | assignee | week | day"),
        ("since"    = Option<String>, Query, description = "RFC3339 inclusive lower bound"),
        ("until"    = Option<String>, Query, description = "RFC3339 exclusive upper bound"),
        ("org_id"   = Option<String>, Query, description = "Comma-separated org ids"),
        ("repo_id"  = Option<String>, Query, description = "Comma-separated repo ids"),
    ),
    responses(
        (status = 200, description = "Bucketed issue metrics", body = IssuesReportResponse),
        (status = 400, description = "Validation failed"),
    ),
    tag = "reports",
)]
pub async fn issues_report(
    State(state): State<AppState>,
    Extension(_principal): Extension<crate::audit::Principal>,
    Query(q): Query<IssuesReportQuery>,
) -> Result<Json<IssuesReportResponse>, ApiError> {
    use dp_domain::store::{IssueMetric, IssueMetricGroupBy, IssueMetricsFilter};
    let metric = match q.metric.as_deref().unwrap_or("throughput") {
        "throughput" => IssueMetric::Throughput,
        "lead_time" => IssueMetric::LeadTime,
        "wip" => IssueMetric::Wip,
        "stale" => IssueMetric::Stale,
        "untriaged" => IssueMetric::Untriaged,
        other => {
            return Err(ApiError::BadRequest {
                code: "invalid_metric",
                message: format!("unknown metric: {other}"),
            })
        }
    };
    let group_by = match q.group_by.as_deref().unwrap_or("repo") {
        "repo" => IssueMetricGroupBy::Repo,
        "org" => IssueMetricGroupBy::Org,
        "assignee" => IssueMetricGroupBy::Assignee,
        "week" => IssueMetricGroupBy::Week,
        "day" => IssueMetricGroupBy::Day,
        other => {
            return Err(ApiError::BadRequest {
                code: "invalid_group_by",
                message: format!("unknown group_by: {other}"),
            })
        }
    };
    let filter = IssueMetricsFilter {
        metric,
        group_by,
        since: q.since,
        until: q.until,
        org_ids: csv_uuid_field(&q.org_id),
        repo_ids: csv_uuid_field(&q.repo_id),
    };
    let rows = state.store.issue_metrics(&filter).await?;
    let total = rows.len() as i64;
    Ok(Json(IssuesReportResponse {
        rows: rows
            .into_iter()
            .map(|r| IssuesReportRow {
                bucket: r.bucket,
                value: r.value,
                count: r.count,
            })
            .collect(),
        total,
    }))
}

/// Build the report-router fragment. Mount with `Router::merge` from
/// the composition root (`dp-server::build()`); auth + audit
/// wrappers are added by the composition layer per Phase 4 stage 6.
pub fn reports_router(state: Arc<AppState>) -> Router {
    // Per Phase 4 stage 9 / SCOPE D4.2 every protected route
    // wears a `require_permission(<resource>, <action>)` layer.
    // The kind/action pair here is the same pair the report
    // handlers audit under (`audit::REPORT_READ`); a forgotten
    // decoration trips the `require_permission-covers-every-
    // protected-route` smoke. The engine is inserted as an
    // axum Extension by `dp_server::build`; for unit tests in
    // this file the test helper inserts a NoopPolicyEngine.
    //
    // We use `starter_authz::with_permission` (applied to a
    // freshly-built `Router`) rather than per-route
    // `route(...).layer(require_permission(...))` because the
    // return-type annotation on `require_permission` is fixed at
    // `FromFnLayer<_, (), ()>` and axum 0.8's `MethodRouter::layer`
    // wants the marker tuple to match the closure args
    // (`(Request,)`); `with_permission` sidesteps the annotation
    // by applying the layer directly to a `Router`.
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new().route("/reports/user/{user_id}", get(user_report)),
            "reports",
            "read",
        ))
        .merge(with_permission(
            Router::new().route("/reports/team/{team_id}", get(team_report)),
            "reports",
            "read",
        ))
        .merge(with_permission(
            Router::new().route("/reports/org/{org_id}", get(org_report)),
            "reports",
            "read",
        ))
        .merge(with_permission(
            Router::new().route("/reports/home-org-split", get(home_org_split_report)),
            "reports",
            "read",
        ))
        .merge(with_permission(
            Router::new().route("/reports/freshness", get(freshness_report)),
            "reports",
            "read",
        ))
        .merge(with_permission(
            Router::new().route(
                "/reports/project-portfolio",
                post(project_portfolio_report),
            ),
            "reports",
            "read",
        ))
        .merge(with_permission(
            Router::new().route("/reports/issues", get(issues_report)),
            "issues",
            "read",
        ))
        .with_state(inner)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::body::to_bytes;
    use axum::http::Request;
    use chrono::TimeZone;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tower::ServiceExt;

    use dp_domain::store::{EventActorRow, Store, StoreError};
    use dp_domain::tag_link::{TagLink, TagLinkKind};
    use dp_domain::{
        ActivityEvent, ActorRole, EventActor, FetchCursor, FetchRun, FetchRunKind, Membership,
        Org, Repo, ResourceKind, Team, User, WebhookDelivery,
    };

    /// Tiny in-memory store. `list_event_actor_rows_in_window` returns
    /// a pre-seeded vec; `data_as_of` returns a pre-seeded snapshot.
    /// Every other method is a no-op so we don't drag the rest of
    /// the store surface into this test module.
    #[derive(Default)]
    struct FakeStore {
        rows: Mutex<Vec<EventActorRow>>,
        freshness: Mutex<DataAsOf>,
        /// Returned verbatim by [`Store::resolve_tag_targets`].
        /// Lets §7.7 tests stage "issue-only tag" vs "repo-link
        /// tag" without dragging the full Postgres backend in.
        tag_links: Mutex<Vec<TagLink>>,
    }

    impl FakeStore {
        fn with_freshness(d: DataAsOf) -> Self {
            Self {
                rows: Mutex::new(vec![]),
                freshness: Mutex::new(d),
                tag_links: Mutex::new(vec![]),
            }
        }

        fn with_tag_links(links: Vec<TagLink>) -> Self {
            Self {
                rows: Mutex::new(vec![]),
                freshness: Mutex::new(DataAsOf::default()),
                tag_links: Mutex::new(links),
            }
        }
    }

    #[async_trait]
    impl Store for FakeStore {
        async fn upsert_user(&self, u: &User) -> Result<User, StoreError> {
            Ok(u.clone())
        }
        async fn get_user(&self, _: Uuid) -> Result<User, StoreError> {
            unimplemented!()
        }
        async fn get_user_by_github_id(&self, _: i64) -> Result<User, StoreError> {
            unimplemented!()
        }
        async fn list_users(&self) -> Result<Vec<User>, StoreError> {
            Ok(vec![])
        }
        async fn pseudonymise_user(&self, _: Uuid) -> Result<(), StoreError> {
            Ok(())
        }
        async fn upsert_org(&self, o: &Org) -> Result<Org, StoreError> {
            Ok(o.clone())
        }
        async fn upsert_team(&self, t: &Team) -> Result<Team, StoreError> {
            Ok(t.clone())
        }
        async fn upsert_repo(&self, r: &Repo) -> Result<Repo, StoreError> {
            Ok(r.clone())
        }
        async fn upsert_membership(&self, m: &Membership) -> Result<Membership, StoreError> {
            Ok(m.clone())
        }
        async fn list_memberships_for_user(
            &self,
            _: Uuid,
        ) -> Result<Vec<Membership>, StoreError> {
            Ok(vec![])
        }
        async fn set_home_org(
            &self,
            _: Uuid,
            _: Uuid,
            _: Option<Uuid>,
        ) -> Result<(), StoreError> {
            Ok(())
        }
        async fn record_event(&self, e: &ActivityEvent) -> Result<ActivityEvent, StoreError> {
            Ok(e.clone())
        }
        async fn add_event_actors(&self, _: &[EventActor]) -> Result<(), StoreError> {
            Ok(())
        }
        async fn list_event_actor_rows_in_window(
            &self,
            _: &Window,
            _: &[Uuid],
            _: &[Uuid],
            _: &[Uuid],
            _: &[ActorRole],
        ) -> Result<Vec<EventActorRow>, StoreError> {
            Ok(self.rows.lock().unwrap().clone())
        }
        async fn get_cursor(
            &self,
            _: Uuid,
            _: Option<Uuid>,
            _: ResourceKind,
        ) -> Result<FetchCursor, StoreError> {
            Err(StoreError::NotFound {
                entity: "fetch_cursor",
                id: String::new(),
            })
        }
        async fn put_cursor(&self, _: &FetchCursor) -> Result<(), StoreError> {
            Ok(())
        }
        async fn start_fetch_run(&self, _: FetchRunKind) -> Result<Uuid, StoreError> {
            Ok(Uuid::new_v4())
        }
        async fn finish_fetch_run(
            &self,
            _: Uuid,
            _: i64,
            _: i64,
            _: bool,
        ) -> Result<(), StoreError> {
            Ok(())
        }
        async fn list_recent_fetch_runs(&self, _: i64) -> Result<Vec<FetchRun>, StoreError> {
            Ok(vec![])
        }
        async fn data_as_of(&self) -> Result<DataAsOf, StoreError> {
            Ok(self.freshness.lock().unwrap().clone())
        }
        async fn enqueue_webhook(&self, _: &WebhookDelivery) -> Result<(), StoreError> {
            Ok(())
        }
        async fn claim_webhooks(&self, _: i64) -> Result<Vec<WebhookDelivery>, StoreError> {
            Ok(vec![])
        }
        async fn mark_webhook_processed(&self, _: Uuid) -> Result<(), StoreError> {
            Ok(())
        }
        async fn mark_webhook_failed(&self, _: Uuid, _: &str) -> Result<(), StoreError> {
            Ok(())
        }
        async fn resolve_tag_targets(
            &self,
            _: &[Uuid],
            _: &[Uuid],
            _: &[Uuid],
            _: &[Uuid],
        ) -> Result<Vec<TagLink>, StoreError> {
            Ok(self.tag_links.lock().unwrap().clone())
        }
    }

    fn utc(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, 0, 0).single().unwrap()
    }

    fn sample_freshness(org: Uuid) -> DataAsOf {
        DataAsOf {
            webhook_latest: Some(utc(2025, 5, 19, 10)),
            reconciler_latest: Some(utc(2025, 5, 19, 8)),
            per_org: HashMap::from([(org, utc(2025, 5, 19, 9))]),
        }
    }

    fn build_router(store: Arc<dyn Store>) -> Router {
        // Inject a SPI Principal + a NoopPolicyEngine so the
        // `require_permission` layer attached to every report
        // route in `reports_router` evaluates to Allow in unit
        // tests. Production wiring (in `dp_server::build`)
        // replaces the no-op engine with `StaticRbacEngine`.
        use axum::Extension;
        use starter_spi::auth::{Principal as SpiPrincipal, Role};
        use starter_spi::authz::{NoopPolicyEngine, PolicyEngine};
        let state = Arc::new(AppState::new(store));
        let engine: Arc<dyn PolicyEngine> = Arc::new(NoopPolicyEngine);
        let spi_principal = SpiPrincipal {
            subject: "test-user".to_string(),
            role: Role::Admin,
            scopes: Vec::new(),
            tenant_id: None,
            teams: Vec::new(),
            extra: serde_json::Value::Null,
        };
        reports_router(state)
            .layer(Extension(spi_principal))
            .layer(Extension(engine))
    }

    async fn get_json(app: Router, uri: &str) -> serde_json::Value {
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            200,
            "non-200 from GET {uri}: {:?}",
            resp.status()
        );
        let body = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    // -- resolved-window echo across every shape -----------------------

    #[tokio::test]
    async fn every_handler_echoes_resolved_window_verbatim() {
        let org = Uuid::new_v4();
        let store: Arc<dyn Store> =
            Arc::new(FakeStore::with_freshness(sample_freshness(org)));

        // Pick a fully-pinned custom window so the assertion is
        // independent of the system clock. The handler MUST echo
        // these exact start/end/label/tz/anchor values.
        let start = "2025-05-12T00:00:00Z";
        let end = "2025-05-19T00:00:00Z";
        let qs = format!(
            "window_label=custom&tz=Australia/Sydney&anchor=viewer\
             &custom_start={start}&custom_end={end}&scope_mode=single_org&orgs={org}"
        );

        let uris = vec![
            format!("/reports/user/{}?{}", Uuid::new_v4(), qs),
            format!("/reports/team/{}?{}", Uuid::new_v4(), qs),
            format!("/reports/org/{}?{}", org, qs),
            format!("/reports/home-org-split?{}", qs),
            format!("/reports/freshness?{}", qs),
        ];

        for uri in uris {
            let v = get_json(build_router(store.clone()), &uri).await;
            let w = &v["resolved_window"];
            assert_eq!(w["start"], start, "start echo at {uri}");
            assert_eq!(w["end"], end, "end echo at {uri}");
            assert_eq!(w["label"], "custom", "label echo at {uri}");
            assert_eq!(w["tz"], "Australia/Sydney", "tz echo at {uri}");
            assert_eq!(w["anchor"], "viewer", "anchor echo at {uri}");
        }
    }

    // -- data_as_of present on every shape -----------------------------

    #[tokio::test]
    async fn every_handler_returns_data_as_of_object() {
        let org = Uuid::new_v4();
        let store: Arc<dyn Store> =
            Arc::new(FakeStore::with_freshness(sample_freshness(org)));
        let qs = format!(
            "window_label=last_7_days&tz=UTC&anchor=utc&scope_mode=single_org&orgs={org}"
        );

        let uris = vec![
            format!("/reports/user/{}?{}", Uuid::new_v4(), qs),
            format!("/reports/team/{}?{}", Uuid::new_v4(), qs),
            format!("/reports/org/{}?{}", org, qs),
            format!("/reports/home-org-split?{}", qs),
            format!("/reports/freshness?{}", qs),
        ];

        for uri in uris {
            let v = get_json(build_router(store.clone()), &uri).await;
            let d = &v["data_as_of"];
            assert!(d.is_object(), "data_as_of must be an object at {uri}");
            assert!(d.get("webhook_latest").is_some(), "webhook_latest at {uri}");
            assert!(
                d.get("reconciler_latest").is_some(),
                "reconciler_latest at {uri}"
            );
            assert!(d.get("per_org").is_some(), "per_org at {uri}");
            // Headline picked from the single requested org (single-
            // org / all-orgs-combined). per_org_split returns None.
            let want = if uri.contains("home-org-split") {
                serde_json::Value::Null
            } else {
                serde_json::Value::String("2025-05-19T09:00:00Z".into())
            };
            assert_eq!(d["headline"], want, "headline at {uri}");
        }
    }

    // -- error paths --------------------------------------------------

    #[tokio::test]
    async fn invalid_tz_returns_400_with_stable_code() {
        let store: Arc<dyn Store> = Arc::new(FakeStore::default());
        let app = build_router(store);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/reports/user/{}?window_label=last_week&tz=Not/Real&anchor=viewer\
                         &scope_mode=single_org",
                        Uuid::new_v4()
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
        let body = to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["code"], "invalid_tz");
    }

    #[tokio::test]
    async fn custom_window_missing_bounds_returns_400() {
        let store: Arc<dyn Store> = Arc::new(FakeStore::default());
        let app = build_router(store);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/reports/org/{}?window_label=custom&tz=UTC&anchor=utc&scope_mode=single_org",
                        Uuid::new_v4()
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
        let body = to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["code"], "missing_custom_range");
    }

    #[tokio::test]
    async fn invalid_uuid_in_orgs_query_returns_400() {
        let store: Arc<dyn Store> = Arc::new(FakeStore::default());
        let app = build_router(store);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/reports/user/{}?window_label=today&tz=UTC&anchor=utc&scope_mode=single_org\
                         &orgs=not-a-uuid",
                        Uuid::new_v4()
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
        let body = to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["code"], "invalid_uuid");
    }

    // -- freshness shape ----------------------------------------------

    // -- SCOPE-PROJECTS §7.7 tag filter --------------------------------

    fn issue_link(tag_id: Uuid) -> TagLink {
        TagLink {
            id: Uuid::new_v4(),
            tag_id,
            kind: TagLinkKind::Issue,
            target_repo_id: None,
            target_issue_id: Some(Uuid::new_v4()),
            target_user_id: None,
            target_team_id: None,
            added_by: Uuid::new_v4(),
            added_at: Utc::now(),
        }
    }

    fn repo_link(tag_id: Uuid) -> TagLink {
        TagLink {
            id: Uuid::new_v4(),
            tag_id,
            kind: TagLinkKind::Repo,
            target_repo_id: Some(Uuid::new_v4()),
            target_issue_id: None,
            target_user_id: None,
            target_team_id: None,
            added_by: Uuid::new_v4(),
            added_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn group_by_tag_without_tags_filter_returns_400() {
        let store: Arc<dyn Store> = Arc::new(FakeStore::default());
        let app = build_router(store);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/reports/user/{}?window_label=today&tz=UTC&anchor=utc\
                         &scope_mode=single_org&group_by=tag",
                        Uuid::new_v4()
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
        let body = to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["code"], "group_by_tag_requires_tags");
    }

    #[tokio::test]
    async fn group_by_tag_with_too_many_tags_returns_400_with_cap_code() {
        let store: Arc<dyn Store> = Arc::new(FakeStore::default());
        let app = build_router(store);
        // 51 distinct UUIDs → over the §7.7 cap of 50.
        let tags: Vec<String> = (0..51).map(|_| Uuid::new_v4().to_string()).collect();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/reports/user/{}?window_label=today&tz=UTC&anchor=utc\
                         &scope_mode=single_org&group_by=tag&tags={}",
                        Uuid::new_v4(),
                        tags.join(",")
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
        let body = to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["code"], "tags_filter_over_cap");
    }

    #[tokio::test]
    async fn issue_only_tag_on_commit_metric_returns_empty_reason_literal() {
        // The locked §7.7 case: a tag with only `issue`-kind links
        // queried against `activity_types=commit` returns 200 with
        // empty rows + the exact `empty_reason` literal.
        let tag = Uuid::new_v4();
        let store: Arc<dyn Store> =
            Arc::new(FakeStore::with_tag_links(vec![issue_link(tag)]));
        let app = build_router(store);
        let v = get_json(
            app,
            &format!(
                "/reports/user/{}?window_label=today&tz=UTC&anchor=utc\
                 &scope_mode=single_org&tags={}&activity_types=commit",
                Uuid::new_v4(),
                tag
            ),
        )
        .await;
        assert_eq!(
            v["empty_reason"], "tag links do not match metric attribution",
            "§7.7 literal must be returned verbatim"
        );
        assert!(v["rows"].is_array(), "rows must still be an array");
        assert_eq!(v["rows"].as_array().unwrap().len(), 0, "rows must be empty");
    }

    #[tokio::test]
    async fn issue_only_tag_on_issue_metric_does_not_set_empty_reason() {
        // Same tag, but the requested metric IS issue-centric →
        // empty_reason must be absent (the field is `Option` and
        // `skip_serializing_if = "Option::is_none"`).
        let tag = Uuid::new_v4();
        let store: Arc<dyn Store> =
            Arc::new(FakeStore::with_tag_links(vec![issue_link(tag)]));
        let app = build_router(store);
        let v = get_json(
            app,
            &format!(
                "/reports/user/{}?window_label=today&tz=UTC&anchor=utc\
                 &scope_mode=single_org&tags={}&activity_types=issue_opened",
                Uuid::new_v4(),
                tag
            ),
        )
        .await;
        assert!(
            v.get("empty_reason").is_none(),
            "empty_reason must be absent for satisfiable tag/metric pair, got {v}"
        );
    }

    #[tokio::test]
    async fn repo_link_tag_satisfies_commit_metric() {
        // A tag with a `repo` link satisfies every metric per §7.7,
        // so a commit metric must NOT trip the empty_reason path.
        let tag = Uuid::new_v4();
        let store: Arc<dyn Store> =
            Arc::new(FakeStore::with_tag_links(vec![repo_link(tag)]));
        let app = build_router(store);
        let v = get_json(
            app,
            &format!(
                "/reports/user/{}?window_label=today&tz=UTC&anchor=utc\
                 &scope_mode=single_org&tags={}&activity_types=commit",
                Uuid::new_v4(),
                tag
            ),
        )
        .await;
        assert!(
            v.get("empty_reason").is_none(),
            "empty_reason must be absent when a repo-link tag is paired with a commit metric"
        );
    }

    #[tokio::test]
    async fn empty_reason_field_is_absent_when_no_tag_filter() {
        // Existing report request shape — no tags filter → no
        // empty_reason field on the wire.
        let store: Arc<dyn Store> = Arc::new(FakeStore::default());
        let app = build_router(store);
        let v = get_json(
            app,
            &format!(
                "/reports/user/{}?window_label=today&tz=UTC&anchor=utc&scope_mode=single_org",
                Uuid::new_v4()
            ),
        )
        .await;
        assert!(v.get("empty_reason").is_none());
    }

    #[tokio::test]
    async fn group_by_tag_with_valid_tags_falls_through_to_unsupported_400() {
        // Even with a satisfiable tag, GroupBy::Tag itself is not
        // wired in this stage — the count_rows path returns the
        // explicit `group_by_tag_unsupported` 400 so callers can't
        // confuse "no data" with "feature not wired yet".
        let tag = Uuid::new_v4();
        let store: Arc<dyn Store> =
            Arc::new(FakeStore::with_tag_links(vec![repo_link(tag)]));
        let app = build_router(store);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/reports/user/{}?window_label=today&tz=UTC&anchor=utc\
                         &scope_mode=single_org&group_by=tag&tags={}",
                        Uuid::new_v4(),
                        tag
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
        let body = to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["code"], "group_by_tag_unsupported");
    }

    async fn post_json(app: Router, uri: &str, body: serde_json::Value) -> (u16, serde_json::Value) {
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status().as_u16();
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let v: serde_json::Value = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
        };
        (status, v)
    }

    #[tokio::test]
    async fn project_portfolio_empty_store_returns_zeroed_envelope() {
        let store: Arc<dyn Store> = Arc::new(FakeStore::default());
        let app = build_router(store);
        let (status, v) = post_json(
            app,
            "/reports/project-portfolio",
            serde_json::json!({ "limit": 50 }),
        )
        .await;
        assert_eq!(status, 200, "got {status}: {v}");
        assert!(v["rows"].is_array() && v["rows"].as_array().unwrap().is_empty());
        assert_eq!(v["total"], 0);
        assert_eq!(v["limit"], 50);
        assert_eq!(v["offset"], 0);
        assert_eq!(v["kpis"]["total_projects"], 0);
        assert_eq!(v["kpis"]["on_track"], 0);
        assert_eq!(v["kpis"]["overdue"], 0);
        assert!(v["now"].is_string(), "now must be an RFC3339 string");
    }

    #[tokio::test]
    async fn project_portfolio_rejects_limit_over_max() {
        let store: Arc<dyn Store> = Arc::new(FakeStore::default());
        let app = build_router(store);
        let (status, v) = post_json(
            app,
            "/reports/project-portfolio",
            serde_json::json!({ "limit": 10_000 }),
        )
        .await;
        assert_eq!(status, 400, "expected 400, got {status}: {v}");
        assert_eq!(v["code"], "invalid_limit");
    }

    #[tokio::test]
    async fn project_portfolio_rejects_zero_limit() {
        let store: Arc<dyn Store> = Arc::new(FakeStore::default());
        let app = build_router(store);
        let (status, _v) = post_json(
            app,
            "/reports/project-portfolio",
            serde_json::json!({ "limit": 0 }),
        )
        .await;
        assert_eq!(status, 400);
    }

    #[tokio::test]
    async fn freshness_handler_returns_null_rows() {
        let store: Arc<dyn Store> = Arc::new(FakeStore::default());
        let app = build_router(store);
        let v = get_json(
            app,
            "/reports/freshness?window_label=today&tz=UTC&anchor=utc&scope_mode=single_org",
        )
        .await;
        assert!(v["rows"].is_null());
        assert!(v["data_as_of"].is_object());
    }
}
