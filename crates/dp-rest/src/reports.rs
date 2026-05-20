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
    extract::{Path, Query, State},
    response::Json,
    routing::get,
    Router,
};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::event::{ActorRole, EventKind};
use dp_domain::store::EventActorRow;
use dp_domain::window::WindowAnchor;
use dp_reports::lenses::{all_orgs_combined, per_org_split, single_org};
use dp_reports::{
    count_by_bucket, count_by_org, count_by_repo, count_by_user, pick_freshness_headline,
    resolve_window, DataAsOf, GroupBy, ReportRequest, ScopeMode, TrendBucket, Window,
    WindowLabel, WindowSpec,
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
            &[],
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
    let window = resolve_window(&request.window)?;
    let rows = fetch_rows(&state, &request, &window).await?;
    let tz = parse_tz_or_utc(&request.window.tz);
    let counts = count_rows(&rows, request.group_by, &tz)?;
    let data_as_of = state.store.data_as_of().await?;
    Ok(Json(ReportResponse {
        resolved_window: window,
        rows: json!(counts),
        data_as_of: DataAsOfDto::from_domain(data_as_of, request.scope_mode, &request.orgs),
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
    let window = resolve_window(&request.window)?;
    let rows = fetch_rows(&state, &request, &window).await?;
    let tz = parse_tz_or_utc(&request.window.tz);
    let counts = count_rows(&rows, request.group_by, &tz)?;
    let data_as_of = state.store.data_as_of().await?;
    Ok(Json(ReportResponse {
        resolved_window: window,
        rows: json!(counts),
        data_as_of: DataAsOfDto::from_domain(data_as_of, request.scope_mode, &request.orgs),
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
    let window = resolve_window(&request.window)?;
    let rows = fetch_rows(&state, &request, &window).await?;
    let tz = parse_tz_or_utc(&request.window.tz);
    let counts = count_rows(&rows, request.group_by, &tz)?;
    let data_as_of = state.store.data_as_of().await?;
    Ok(Json(ReportResponse {
        resolved_window: window,
        rows: json!(counts),
        data_as_of: DataAsOfDto::from_domain(data_as_of, request.scope_mode, &request.orgs),
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
    let window = resolve_window(&request.window)?;
    let raw = state
        .store
        .list_event_actor_rows_in_window(
            &window,
            &request.orgs,
            &[],
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
    let window = resolve_window(&request.window)?;
    let data_as_of = state.store.data_as_of().await?;
    Ok(Json(ReportResponse {
        resolved_window: window,
        rows: serde_json::Value::Null,
        data_as_of: DataAsOfDto::from_domain(data_as_of, request.scope_mode, &request.orgs),
    }))
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

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
    }

    impl FakeStore {
        fn with_freshness(d: DataAsOf) -> Self {
            Self {
                rows: Mutex::new(vec![]),
                freshness: Mutex::new(d),
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
