//! Projects ↔ issues membership REST surface
//! (`linear-projects-v2.md` §7.2).
//!
//! Four routes ship here:
//!
//! | route                                                  | what it does                                                  |
//! |--------------------------------------------------------|---------------------------------------------------------------|
//! | `GET    /projects/{id}/issues`                         | paginated issue list scoped to a project (`IssueListResponse`) |
//! | `POST   /projects/{id}/issues`                         | bulk add (`{ expected_version, issue_ids: [..] }`) ⇒ `BulkAddResult` |
//! | `DELETE /projects/{id}/issues/{issue_id}?expected_version=` | single detach, CAS-gated; 204 on success                  |
//! | `GET    /issues/{id}/project`                          | resolve the (single, per v1 `UNIQUE (issue_id)`) project for an issue, or `null` |
//!
//! `BulkAddResult` mirrors the per-row outcome shape pinned in
//! `linear-projects-v2.md` §7.2 / `SCOPE-PROJECTS.md` §7 — every
//! input id ends up either in `added` (the store accepted it) or in
//! `skipped` with a closed-vocabulary `reason` (`"already_in_project"`
//! also carries `existing_project_id` so the UI can render the
//! `Move here?` follow-up without a second round-trip).
//!
//! The bulk-add request is CAS-gated on the **project's** `version`
//! (matches `PATCH /projects/{id}` from §7.1). The detach takes the
//! same `expected_version` as a query param so the URL stays a clean
//! REST shape. The list and "what project owns this issue" GETs are
//! pure reads — no CAS.
//!
//! Authorisation: `(projects, read)` for the two GETs and `(projects,
//! write)` for POST / DELETE — same lanes as the §7.1 CRUD spine.
//! Audit verbs are pinned in [`crate::audit`]: one
//! [`PROJECT_ISSUE_ADD`] per accepted row in a bulk add, and one
//! [`PROJECT_ISSUE_REMOVE`] per detach. Skipped rows never audit
//! (they did not mutate state).
//!
//! [`PROJECT_ISSUE_ADD`]: crate::audit::PROJECT_ISSUE_ADD
//! [`PROJECT_ISSUE_REMOVE`]: crate::audit::PROJECT_ISSUE_REMOVE

use std::sync::Arc;

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{delete, get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::project::{ProjectIssueAddOutcome, ProjectIssueAddSkip};
use dp_domain::store::StoreError;

use crate::audit::{self, Principal};
use crate::error::ApiError;
use crate::issues_read::{
    attach_repo_slugs, IssueBucket, IssueDto, IssueListResponse,
};
use crate::projects::ProjectDto;
use crate::repos::{clamp_limit, clamp_offset};
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Wire DTOs
// ---------------------------------------------------------------------------

/// Hard cap on `issue_ids` per bulk-add request, pinned in
/// `linear-projects-v2.md` §7.2 / §9.3. Larger selections from the
/// §6.6 triage bulk affordance are chunked client-side.
pub const BULK_ADD_ISSUE_CAP: usize = 100;

/// Body for `POST /projects/{id}/issues`. CAS-gated on the project's
/// current `version` (§7.2); `issue_ids` is capped at
/// [`BULK_ADD_ISSUE_CAP`].
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct BulkAddIssuesRequest {
    /// The `version` the caller observed on the project row. A
    /// mismatch returns `409 stale_project_version` just like the
    /// §7.1 PATCH / archive routes. Optional only when `view_id`
    /// is set — view-scoped adds don't mutate the project row and
    /// therefore don't need CAS. Required for project-level adds;
    /// the handler returns `400 missing_expected_version` if it's
    /// missing in that case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_version: Option<i64>,
    /// Issue ids to attach. Capped at [`BULK_ADD_ISSUE_CAP`]; over
    /// the cap returns `400 bulk_add_too_large`. An empty array is
    /// accepted as a no-op (returns `BulkAddResult { added: [],
    /// skipped: [] }` and does not bump the project version).
    pub issue_ids: Vec<Uuid>,
    /// Optional saved-view id (PROJECT-VIEW.md §5.4 amendment).
    /// When set, the accepted issues are *also* attached to the
    /// named view's membership table after the project add
    /// succeeds, so the tab the user added them on retains them.
    /// Skipped (already-in-project) ids are still added to the
    /// view — the user expects the issues to appear on the tab
    /// regardless of whether they were brand new to the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_id: Option<Uuid>,
}

/// One row in [`BulkAddResult::skipped`]. Mirrors
/// [`ProjectIssueAddSkip`] but kept as a separate wire type so the
/// OpenAPI schema is decoupled from the domain crate and so the
/// `reason` vocabulary is documented at the REST boundary.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BulkAddSkipDto {
    /// The issue id that was rejected.
    pub issue_id: Uuid,
    /// Closed-vocabulary reason; one of:
    ///
    /// * `"already_in_project"` — the v1 `UNIQUE (issue_id)`
    ///   constraint fired. `existing_project_id` is set so the UI
    ///   can render a `Move here?` affordance.
    /// * `"unknown_issue"` — the issue id did not resolve in
    ///   `dp_issues`.
    /// * `"cross_org"` — the issue's `org_id` differs from the
    ///   project's `org_id` (v1: one org per project, §4).
    pub reason: String,
    /// Set when `reason == "already_in_project"`. Lets the UI link
    /// directly to the existing project's detail page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_project_id: Option<Uuid>,
}

impl From<ProjectIssueAddSkip> for BulkAddSkipDto {
    fn from(s: ProjectIssueAddSkip) -> Self {
        Self {
            issue_id: s.issue_id,
            reason: s.reason,
            existing_project_id: s.existing_project_id,
        }
    }
}

/// `BulkAddResult` — the per-row outcome shape `linear-projects-v2.md`
/// §7.2 / `SCOPE-PROJECTS.md` §7 wire through the REST layer so the
/// UI can render add-by-add status from one round-trip.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BulkAddResult {
    /// Issue ids the store accepted into the project.
    pub added: Vec<Uuid>,
    /// Issue ids the store refused, each with a closed-vocabulary
    /// `reason`. See [`BulkAddSkipDto`].
    pub skipped: Vec<BulkAddSkipDto>,
}

impl From<ProjectIssueAddOutcome> for BulkAddResult {
    fn from(o: ProjectIssueAddOutcome) -> Self {
        Self {
            added: o.added,
            skipped: o.skipped.into_iter().map(BulkAddSkipDto::from).collect(),
        }
    }
}

/// Query params for `DELETE /projects/{id}/issues/{issue_id}`. The
/// `expected_version` rides as a query param so the URL stays a
/// clean REST shape — matches the §7.1 PATCH convention.
#[derive(Debug, Clone, Deserialize)]
pub struct RemoveIssueQuery {
    /// The `version` the caller observed on the project row.
    /// Required when `view` is absent (project-level detach);
    /// ignored when `view` is set (view-membership detach does not
    /// mutate the project row).
    #[serde(default)]
    pub expected_version: Option<i64>,
    /// Optional saved-view id (PROJECT-VIEW.md §5.4 amendment).
    /// When set, the detach is scoped to the view's membership
    /// table only — the issue stays on the project and on every
    /// other view that includes it. When absent, the detach is
    /// project-level (and cascades into every view via the FK).
    #[serde(default)]
    pub view: Option<Uuid>,
}

/// Query params for `GET /projects/{id}/issues`. Slice A keeps the
/// filter narrow: pagination + state + a title substring. The full
/// `ListIssuesQuery` lane is reserved for slice B once project-aware
/// SQL filtering lands; v1 pulls the membership list, hydrates each
/// row, and filters in-memory — which is correct for the
/// O(≤100) project sizes the slice-A surfaces target.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListProjectIssuesQuery {
    /// State filter (`open` / `closed` / `all`); defaults to `all`
    /// here (a project detail surface wants to see both open and
    /// closed work by default — different from `GET /issues` which
    /// defaults to `open`). Pass `?state=open` for an active-only
    /// view.
    #[serde(default)]
    pub state: Option<String>,
    /// Case-insensitive substring on issue title.
    #[serde(default)]
    pub q: Option<String>,
    /// Page size; clamped 1..=200, default 50.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Page offset, 0-based.
    #[serde(default)]
    pub offset: Option<i64>,
    /// Group-by dimension (PROJECT-VIEW.md §5.1 / §7.2). Accepted:
    /// `status`, `tag:<key>`. Unknown values return
    /// `400 invalid_group_by`. When absent, the response is the
    /// flat list (no `buckets` sidecar).
    #[serde(default)]
    pub group_by: Option<String>,
    /// AND-combined filter chips (PROJECT-VIEW.md §5.2 / §5.4).
    /// Wire form: `<dim>:<value>;<dim>:<value>;…` with `;` as
    /// the chip separator and `:` as the dim/value separator
    /// (§5.4 — `,` is unsafe inside tag values, `;` is not legal
    /// in tag values nor UUIDs). Tag values themselves may
    /// contain `:` (e.g. `team:backend:v2`); the parser splits on
    /// the **first** `:` after the dim. Accepted dims this slice:
    ///
    /// * `status:open` / `status:closed`
    /// * `assignee:<login>`
    /// * `label:<text>`
    /// * `tag:<key>:<value>`
    ///
    /// Unknown dims return `400 invalid_filter`. Filters apply
    /// **before** bucket counts, so the `buckets` sidecar always
    /// reflects post-filter totals (§5.2).
    #[serde(default)]
    pub filter: Option<String>,
    /// Sort order (PROJECT-VIEW.md §5.3). Accepted:
    /// `updated_desc` (default), `updated_asc`, `title_asc`.
    /// Unknown values return `400 invalid_sort`.
    #[serde(default)]
    pub sort: Option<String>,
    /// Optional saved-view id (PROJECT-VIEW.md §5.4 amendment).
    /// When set, the response intersects project membership with
    /// the view's `dp_project_view_issues` rows, then applies the
    /// caller-supplied `filter` / `group_by` / `sort` on top. When
    /// absent, the request behaves as the "All" tab and returns
    /// every project-level issue (the historical default).
    #[serde(default)]
    pub view: Option<Uuid>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub(crate) fn map_cas_error(project_id: Uuid, err: StoreError) -> ApiError {
    match err {
        StoreError::NotFound { entity: "project", .. } => ApiError::NotFound {
            code: "project_not_found",
            message: format!("no project with id {project_id}"),
        },
        StoreError::NotFound { entity: "project_issue", id } => ApiError::NotFound {
            code: "project_issue_not_found",
            message: format!("issue {id} is not attached to project {project_id}"),
        },
        StoreError::Conflict(msg) => ApiError::Conflict {
            code: "stale_project_version",
            message: msg,
        },
        StoreError::Invalid(msg) => ApiError::BadRequest {
            code: "project_invalid",
            message: msg,
        },
        e => e.into(),
    }
}

/// `GET /projects/{id}/issues` — paginated issue list scoped to a
/// project (§7.2). Same envelope as `GET /issues`.
///
/// Implementation note: v1 resolves membership via
/// [`Store::list_issue_ids_for_project`], fetches each issue row
/// with [`Store::get_issue`], applies optional `state` / `q` filters
/// in-memory, then paginates. Correct for the slice-A target of
/// projects with ≤ 100 issues. The natural follow-up — a SQL-level
/// "filter `dp_issues` by `project_id`" — is deferred until
/// `IssueListFilter` grows a `project_id` field in slice B.
#[utoipa::path(
    get,
    path = "/projects/{id}/issues",
    params(
        ("id"       = Uuid,           Path,  description = "Project id"),
        ("state"    = Option<String>, Query, description = "open|closed|all (default all)"),
        ("q"        = Option<String>, Query, description = "Substring search on title"),
        ("limit"    = Option<i64>,    Query, description = "Page size (1..=200, default 50)"),
        ("offset"   = Option<i64>,    Query, description = "Page offset (default 0)"),
        ("group_by" = Option<String>, Query, description = "Bucket dimension: status | tag:<key> (PROJECT-VIEW.md §5.1)"),
        ("filter"   = Option<String>, Query, description = "AND-combined chips: <dim>:<value>;… — status|assignee|label|tag:<key> (PROJECT-VIEW.md §5.2/§5.4)"),
        ("sort"     = Option<String>, Query, description = "updated_desc (default) | updated_asc | title_asc"),
    ),
    responses(
        (status = 200, description = "Paginated issue list scoped to the project", body = IssueListResponse),
        (status = 404, description = "No such project"),
    ),
    tag = "projects",
)]
pub async fn list_project_issues(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Query(q): Query<ListProjectIssuesQuery>,
) -> Result<Json<IssueListResponse>, ApiError> {
    // 404 fast when the project itself is missing so the caller does
    // not get an empty-rows list and assume an empty project.
    let _project = state
        .store
        .get_project(project_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "project_not_found",
            message: format!("no project with id {project_id}"),
        })?;

    let state_filter = match q.state.as_deref() {
        None | Some("") | Some("all") => None,
        Some("open") => Some(dp_domain::issue::IssueState::Open),
        Some("closed") => Some(dp_domain::issue::IssueState::Closed),
        Some(other) => {
            return Err(ApiError::BadRequest {
                code: "invalid_state",
                message: format!("invalid state filter: {other}"),
            });
        }
    };
    let q_str = q.q.as_deref().map(|s| s.trim().to_lowercase());

    // Parse group_by / filter / sort upfront so a malformed param
    // doesn't waste a DB round-trip.
    let group_by = parse_group_by(q.group_by.as_deref())?;
    let filter_clauses = parse_filter(q.filter.as_deref())?;
    let sort_order = parse_sort(q.sort.as_deref())?;

    // PROJECT-VIEW.md §5.4 amendment — saved-view tabs are
    // independent containers. When `?view=` is set the membership
    // list comes *only* from `dp_project_view_issues`; we do not
    // intersect with project-level membership. This is what makes
    // an issue added on a saved-view tab appear *only* on that tab
    // and not bleed into the "All" tab.
    let ids: Vec<Uuid> = match q.view {
        Some(view_id) => state.store.list_issue_ids_for_view(view_id).await?,
        None => state.store.list_issue_ids_for_project(project_id).await?,
    };
    // Resolve each issue row. Missing rows (target FK was hard-deleted
    // out from under us — unlikely given `ON DELETE CASCADE` but
    // belt-and-braces) are silently dropped; the membership row
    // would normally have been cascaded along with it, so this
    // branch should never fire in practice.
    let mut issues: Vec<dp_domain::issue::Issue> = Vec::with_capacity(ids.len());
    for id in &ids {
        if let Some(i) = state.store.get_issue(*id).await? {
            issues.push(i);
        }
    }
    // Apply in-memory filters in the same conjunctive style the SQL
    // layer would.
    if let Some(s) = state_filter {
        issues.retain(|i| i.state == s);
    }
    if let Some(needle) = q_str.as_deref().filter(|s| !s.is_empty()) {
        issues.retain(|i| i.title.to_lowercase().contains(needle));
    }
    // §5.2 — filter chips apply **before** group-by bucket counts,
    // so the `buckets` sidecar always reflects post-filter totals.
    apply_filter_clauses(&*state.store, project_id, &filter_clauses, &mut issues).await?;

    // Build the per-issue bucket assignments + counts. Done **after**
    // filtering so the counts the client renders next to each
    // collapsed section match what's inside (§5.2 — post-filter
    // counts are non-negotiable for triage surfaces).
    let bucketing = match &group_by {
        Some(g) => Some(build_buckets(&*state.store, project_id, g, &issues).await?),
        None => None,
    };

    // Sort post-filter, pre-pagination (§5.3). Stable sort so equal
    // keys retain the existing `added_at ASC, issue_id ASC` order
    // that `list_issue_ids_for_project` already gives us.
    apply_sort(&mut issues, sort_order);

    let total = issues.len() as i64;
    let limit = clamp_limit(q.limit);
    let offset = clamp_offset(q.offset);
    let start = offset.max(0) as usize;
    let end = (start + limit.max(0) as usize).min(issues.len());
    let page = if start >= issues.len() {
        Vec::new()
    } else {
        issues[start..end].to_vec()
    };
    let mut dtos: Vec<IssueDto> = page.into_iter().map(IssueDto::from).collect();
    attach_repo_slugs(&*state.store, &mut dtos).await?;

    // Attach `bucket_keys` per row from the precomputed assignment
    // map. Issues that fell into the "No <key>" bucket carry a
    // single-element `[None]` so the client always knows the grouping
    // is active.
    let buckets_out = if let Some(b) = bucketing {
        for d in dtos.iter_mut() {
            let keys = b
                .assignments
                .get(&d.id)
                .cloned()
                .unwrap_or_else(|| vec![None]);
            d.bucket_keys = Some(keys);
        }
        Some(b.buckets)
    } else {
        None
    };

    Ok(Json(IssueListResponse {
        rows: dtos,
        total,
        limit,
        offset,
        buckets: buckets_out,
    }))
}

/// Parsed group-by dimension (PROJECT-VIEW.md §5.1).
#[derive(Debug, Clone)]
enum GroupBy {
    /// Bucket by `dp_issues.state` — two buckets, `open` and
    /// `closed`. The "No <key>" bucket never fires (state is
    /// non-null).
    Status,
    /// Bucket by `dp_tags.value` joined through `dp_tag_links`
    /// where `dp_tags.key = <key>` and `kind='kv'`. Issues with no
    /// matching link surface under the synthetic "No <key>" bucket.
    Tag { key: String },
}

fn parse_group_by(raw: Option<&str>) -> Result<Option<GroupBy>, ApiError> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if raw == "status" {
        return Ok(Some(GroupBy::Status));
    }
    if let Some(key) = raw.strip_prefix("tag:") {
        if !is_valid_tag_key(key) {
            return Err(ApiError::BadRequest {
                code: "invalid_group_by",
                message: format!("invalid tag key in group_by: {key:?}"),
            });
        }
        return Ok(Some(GroupBy::Tag { key: key.to_owned() }));
    }
    Err(ApiError::BadRequest {
        code: "invalid_group_by",
        message: format!("unsupported group_by dimension: {raw:?}"),
    })
}

/// Mirrors `tagging.md` §3 — kv keys are `[a-z0-9][a-z0-9-]*` up to
/// 50 chars. Keeps the parser identical to what the tag-write path
/// will enforce so views and tags can't drift.
fn is_valid_tag_key(s: &str) -> bool {
    if s.is_empty() || s.len() > 50 {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// One parsed filter clause (PROJECT-VIEW.md §5.2). AND-combined
/// across the wire `filter=` param.
#[derive(Debug, Clone)]
pub(crate) enum FilterClause {
    Status(dp_domain::issue::IssueState),
    Assignee(String),
    Label(String),
    Tag { key: String, value: String },
    /// `milestone:<dp_milestones.id>` — narrows the issue set to
    /// those pointing at the given milestone (PROJECT-VIEW.md
    /// §5.4, Slice 3↔1 bridge). The value is the dev-pulse
    /// surrogate UUID, not the GitHub number or title, so the
    /// filter survives milestone renames and disambiguates
    /// same-title milestones in different repos.
    Milestone(Uuid),
}

/// Lower a stored [`dp_domain::project_view::ProjectViewFilterClause`]
/// into the in-memory [`FilterClause`] the issue handler already
/// knows how to apply. The stored clauses were validated at write
/// time so any unknown / malformed entry here is a corruption-class
/// bug, not user input — we silently drop it (returning `None`) so
/// the rest of the count still reflects the well-formed clauses.
pub(crate) fn view_clause_to_filter(
    c: &dp_domain::project_view::ProjectViewFilterClause,
) -> Option<FilterClause> {
    use dp_domain::project_view::ProjectViewFilterClause as V;
    match c {
        V::Status { value } => match value.as_str() {
            "open" => Some(FilterClause::Status(dp_domain::issue::IssueState::Open)),
            "closed" => Some(FilterClause::Status(dp_domain::issue::IssueState::Closed)),
            _ => None,
        },
        V::Assignee { value } => Some(FilterClause::Assignee(value.clone())),
        V::Label { value } => Some(FilterClause::Label(value.clone())),
        V::Tag { key, value } => Some(FilterClause::Tag {
            key: key.clone(),
            value: value.clone(),
        }),
        V::Milestone { value } => Uuid::parse_str(value).ok().map(FilterClause::Milestone),
    }
}

/// Parse the wire `filter=` param (PROJECT-VIEW.md §5.4). `;`
/// separates clauses; the first `:` in each clause separates the
/// dim from the value (so tag values like `team:backend:v2` round-
/// trip). Empty clauses (`a;;b`) are ignored. An empty / absent
/// param parses to an empty vec.
fn parse_filter(raw: Option<&str>) -> Result<Vec<FilterClause>, ApiError> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for chunk in raw.split(';') {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }
        let (dim, value) = chunk.split_once(':').ok_or_else(|| ApiError::BadRequest {
            code: "invalid_filter",
            message: format!("filter clause missing ':' separator: {chunk:?}"),
        })?;
        let dim = dim.trim();
        let value = value.trim();
        if value.is_empty() {
            return Err(ApiError::BadRequest {
                code: "invalid_filter",
                message: format!("filter clause has empty value: {chunk:?}"),
            });
        }
        match dim {
            "status" => match value {
                "open" => out.push(FilterClause::Status(dp_domain::issue::IssueState::Open)),
                "closed" => out.push(FilterClause::Status(dp_domain::issue::IssueState::Closed)),
                other => {
                    return Err(ApiError::BadRequest {
                        code: "invalid_filter",
                        message: format!("status filter must be 'open' or 'closed', got {other:?}"),
                    });
                }
            },
            "assignee" => out.push(FilterClause::Assignee(value.to_owned())),
            "label" => out.push(FilterClause::Label(value.to_owned())),
            "milestone" => {
                let id = Uuid::parse_str(value).map_err(|_| ApiError::BadRequest {
                    code: "invalid_filter",
                    message: format!(
                        "milestone filter value must be a milestone UUID, got {value:?}"
                    ),
                })?;
                out.push(FilterClause::Milestone(id));
            }
            "tag" => {
                let (key, tag_value) = value.split_once(':').ok_or_else(|| {
                    ApiError::BadRequest {
                        code: "invalid_filter",
                        message: format!("tag filter must be 'tag:<key>:<value>', got {chunk:?}"),
                    }
                })?;
                if !is_valid_tag_key(key) {
                    return Err(ApiError::BadRequest {
                        code: "invalid_filter",
                        message: format!("invalid tag key in filter: {key:?}"),
                    });
                }
                if tag_value.is_empty() {
                    return Err(ApiError::BadRequest {
                        code: "invalid_filter",
                        message: format!("tag filter has empty value: {chunk:?}"),
                    });
                }
                out.push(FilterClause::Tag {
                    key: key.to_owned(),
                    value: tag_value.to_owned(),
                });
            }
            other => {
                return Err(ApiError::BadRequest {
                    code: "invalid_filter",
                    message: format!("unknown filter dim: {other:?}"),
                });
            }
        }
    }
    Ok(out)
}

/// Apply parsed [`FilterClause`]s in-memory against the project's
/// issue set. Tag filters resolve through the store's
/// `list_project_issue_tag_values` so the SQL is the same one the
/// group-by path uses — keeps "filter by category:firmware ⇒ group
/// by gate" totals aligned with the bucket counts (§5.2).
pub(crate) async fn apply_filter_clauses(
    store: &dyn dp_domain::store::Store,
    project_id: Uuid,
    clauses: &[FilterClause],
    issues: &mut Vec<dp_domain::issue::Issue>,
) -> Result<(), ApiError> {
    use std::collections::HashSet;
    for clause in clauses {
        match clause {
            FilterClause::Status(s) => {
                issues.retain(|i| i.state == *s);
            }
            FilterClause::Assignee(login) => {
                let needle = login.to_ascii_lowercase();
                issues.retain(|i| {
                    i.assignees
                        .iter()
                        .any(|a| a.eq_ignore_ascii_case(&needle))
                });
            }
            FilterClause::Label(label) => {
                let needle = label.to_ascii_lowercase();
                issues.retain(|i| {
                    i.labels.iter().any(|l| l.eq_ignore_ascii_case(&needle))
                });
            }
            FilterClause::Tag { key, value } => {
                let pairs = store
                    .list_project_issue_tag_values(project_id, key)
                    .await?;
                let matching: HashSet<Uuid> = pairs
                    .into_iter()
                    .filter(|(_, v)| v == value)
                    .map(|(id, _)| id)
                    .collect();
                issues.retain(|i| matching.contains(&i.id));
            }
            FilterClause::Milestone(mid) => {
                // Resolve via `list_project_milestones` so the
                // filter only matches milestones already adopted
                // by this project — a stale URL pointing at a
                // milestone from another project (or a deleted
                // one) collapses to an empty result, not an
                // accidental cross-project leak. Until
                // `dp_issues.milestone_id` ships, match by
                // (repo_id, title) — milestone titles are
                // unique per repo on the GitHub side.
                let milestones = store
                    .list_project_milestones(project_id, /* include_closed */ true)
                    .await?;
                let Some(m) = milestones.into_iter().find(|m| m.id == *mid) else {
                    issues.clear();
                    continue;
                };
                issues.retain(|i| {
                    i.repo_id == m.repo_id
                        && i.milestone.as_deref() == Some(m.title.as_str())
                });
            }
        }
    }
    Ok(())
}

/// Sort order (PROJECT-VIEW.md §5.3).
#[derive(Debug, Clone, Copy, Default)]
enum SortOrder {
    #[default]
    UpdatedDesc,
    UpdatedAsc,
    TitleAsc,
}

fn parse_sort(raw: Option<&str>) -> Result<SortOrder, ApiError> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None | Some("updated_desc") => Ok(SortOrder::UpdatedDesc),
        Some("updated_asc") => Ok(SortOrder::UpdatedAsc),
        Some("title_asc") => Ok(SortOrder::TitleAsc),
        Some(other) => Err(ApiError::BadRequest {
            code: "invalid_sort",
            message: format!("unknown sort: {other:?}"),
        }),
    }
}

fn apply_sort(issues: &mut [dp_domain::issue::Issue], sort: SortOrder) {
    match sort {
        SortOrder::UpdatedDesc => {
            issues.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        }
        SortOrder::UpdatedAsc => {
            issues.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
        }
        SortOrder::TitleAsc => {
            issues.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
        }
    }
}

/// Output of [`build_buckets`]: the ordered bucket list returned in
/// the response plus a per-issue-id assignment map used to stamp
/// `IssueDto::bucket_keys` after pagination.
struct Bucketing {
    buckets: Vec<IssueBucket>,
    /// `issue_id → bucket_keys`. An issue can map to multiple keys
    /// when grouping by a multi-valued tag (e.g. an issue tagged
    /// `category:firmware` and `category:hardware`). `None` in the
    /// vector means the synthetic "No <key>" bucket.
    assignments: std::collections::HashMap<Uuid, Vec<Option<String>>>,
}

async fn build_buckets(
    store: &dyn dp_domain::store::Store,
    project_id: Uuid,
    group_by: &GroupBy,
    issues: &[dp_domain::issue::Issue],
) -> Result<Bucketing, ApiError> {
    use std::collections::{HashMap, HashSet};

    let issue_ids: HashSet<Uuid> = issues.iter().map(|i| i.id).collect();
    let mut assignments: HashMap<Uuid, Vec<Option<String>>> =
        HashMap::with_capacity(issues.len());

    match group_by {
        GroupBy::Status => {
            let mut open = 0i64;
            let mut closed = 0i64;
            for i in issues {
                match i.state {
                    dp_domain::issue::IssueState::Open => {
                        open += 1;
                        assignments.insert(i.id, vec![Some("open".to_owned())]);
                    }
                    dp_domain::issue::IssueState::Closed => {
                        closed += 1;
                        assignments.insert(i.id, vec![Some("closed".to_owned())]);
                    }
                }
            }
            // Hide empty status buckets — post-filter (§5.2). When
            // the user filtered to `state=closed` there's no point
            // rendering an empty `Open` section.
            let mut buckets = Vec::new();
            if open > 0 {
                buckets.push(IssueBucket {
                    key: Some("open".into()),
                    label: "Open".into(),
                    open,
                    closed: 0,
                });
            }
            if closed > 0 {
                buckets.push(IssueBucket {
                    key: Some("closed".into()),
                    label: "Closed".into(),
                    open: 0,
                    closed,
                });
            }
            Ok(Bucketing { buckets, assignments })
        }
        GroupBy::Tag { key } => {
            // Pull every (issue_id, value) link for this key across
            // the project's issues. The store fn already restricts
            // to `kind='kv'` and non-archived tags.
            let rows = store
                .list_project_issue_tag_values(project_id, key)
                .await?;

            // Per-bucket open/closed counters. The issues vector is
            // already post-filter — use it as the source of truth
            // for issue states so a stale tag link on a filtered-out
            // issue doesn't get counted.
            let state_by_id: HashMap<Uuid, dp_domain::issue::IssueState> =
                issues.iter().map(|i| (i.id, i.state)).collect();

            let mut counts: HashMap<String, (i64, i64)> = HashMap::new();
            for (issue_id, value) in &rows {
                if !issue_ids.contains(issue_id) {
                    continue; // filtered out by state/q
                }
                let entry = assignments.entry(*issue_id).or_default();
                if !entry.iter().any(|v| v.as_deref() == Some(value.as_str())) {
                    entry.push(Some(value.clone()));
                }
                let bucket = counts.entry(value.clone()).or_insert((0, 0));
                match state_by_id.get(issue_id).copied() {
                    Some(dp_domain::issue::IssueState::Open) => bucket.0 += 1,
                    Some(dp_domain::issue::IssueState::Closed) => bucket.1 += 1,
                    None => {}
                }
            }

            // Synthetic "No <key>" bucket: every project issue that
            // didn't receive any assignment above.
            let mut no_key_open = 0i64;
            let mut no_key_closed = 0i64;
            for i in issues {
                if !assignments.contains_key(&i.id) {
                    assignments.insert(i.id, vec![None]);
                    match i.state {
                        dp_domain::issue::IssueState::Open => no_key_open += 1,
                        dp_domain::issue::IssueState::Closed => no_key_closed += 1,
                    }
                }
            }

            // Order: count desc, then key asc as a deterministic
            // tie-breaker. The ordinal-taxonomy override
            // (PROJECT-VIEW.md §5.1 — gate/priority) lands with the
            // config table; for now even `gate` falls under count
            // desc. The synthetic "No <key>" bucket is pinned last
            // and only emitted when non-empty (§5.2).
            let mut bucket_entries: Vec<(String, i64, i64)> = counts
                .into_iter()
                .map(|(k, (o, c))| (k, o, c))
                .collect();
            bucket_entries.sort_by(|a, b| {
                (b.1 + b.2)
                    .cmp(&(a.1 + a.2))
                    .then_with(|| a.0.cmp(&b.0))
            });
            let mut buckets: Vec<IssueBucket> = bucket_entries
                .into_iter()
                .map(|(k, o, c)| IssueBucket {
                    label: format!("{key}:{k}"),
                    key: Some(k),
                    open: o,
                    closed: c,
                })
                .collect();
            if no_key_open + no_key_closed > 0 {
                buckets.push(IssueBucket {
                    key: None,
                    label: format!("No {key}"),
                    open: no_key_open,
                    closed: no_key_closed,
                });
            }

            Ok(Bucketing { buckets, assignments })
        }
    }
}

/// `POST /projects/{id}/issues` — bulk add (§7.2). Returns
/// `BulkAddResult` so per-row outcomes flow back in one round-trip.
///
/// * `issue_ids` capped at [`BULK_ADD_ISSUE_CAP`]; over the cap
///   returns `400 bulk_add_too_large`.
/// * CAS-gated on the project's `version`; mismatch returns
///   `409 stale_project_version`.
/// * One audit row per accepted issue
///   ([`audit::PROJECT_ISSUE_ADD`]); skipped rows never audit.
#[utoipa::path(
    post,
    path = "/projects/{id}/issues",
    params(("id" = Uuid, Path, description = "Project id")),
    request_body = BulkAddIssuesRequest,
    responses(
        (status = 200, description = "Per-row outcome of the bulk add", body = BulkAddResult),
        (status = 400, description = "Validation failure (cap, etc.)"),
        (status = 404, description = "No such project"),
        (status = 409, description = "Stale `expected_version`"),
    ),
    tag = "projects",
)]
pub async fn bulk_add_issues(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(project_id): Path<Uuid>,
    Json(body): Json<BulkAddIssuesRequest>,
) -> Result<Json<BulkAddResult>, ApiError> {
    if body.issue_ids.len() > BULK_ADD_ISSUE_CAP {
        return Err(ApiError::BadRequest {
            code: "bulk_add_too_large",
            message: format!(
                "issue_ids is capped at {BULK_ADD_ISSUE_CAP}; got {}",
                body.issue_ids.len()
            ),
        });
    }

    // PROJECT-VIEW.md §5.4 amendment — saved-view tabs are
    // independent containers. A POST with `view_id` attaches the
    // issues *only* to the view membership and never touches
    // `dp_project_issues`. No CAS, no version bump, no "All" tab
    // side effect. We still validate cross-org + unknown-issue so
    // the UI gets the same closed-vocabulary `skipped` surface.
    if let Some(view_id) = body.view_id {
        let project = state
            .store
            .get_project(project_id)
            .await?
            .ok_or_else(|| ApiError::NotFound {
                code: "project_not_found",
                message: format!("no project with id {project_id}"),
            })?;
        let mut added: Vec<Uuid> = Vec::new();
        let mut skipped: Vec<BulkAddSkipDto> = Vec::new();
        for &issue_id in &body.issue_ids {
            match state.store.get_issue(issue_id).await? {
                None => skipped.push(BulkAddSkipDto {
                    issue_id,
                    reason: "unknown_issue".into(),
                    existing_project_id: None,
                }),
                Some(i) if i.org_id != project.org_id => skipped.push(BulkAddSkipDto {
                    issue_id,
                    reason: "cross_org".into(),
                    existing_project_id: None,
                }),
                Some(_) => added.push(issue_id),
            }
        }
        if !added.is_empty() {
            state.store.add_issues_to_view(view_id, &added).await?;
        }
        for issue_id in &added {
            audit::record(
                state.store.as_ref(),
                principal.actor_user_id,
                audit::PROJECT_ISSUE_ADD,
                format!("{project_id}:{issue_id}:view={view_id}"),
            )
            .await?;
        }
        return Ok(Json(BulkAddResult { added, skipped }));
    }

    // Project-level add (the "All" tab). CAS is mandatory here
    // because the project row's `version`/`issue_count` mutates.
    let expected_version = body.expected_version.ok_or(ApiError::BadRequest {
        code: "missing_expected_version",
        message: "expected_version is required for project-level bulk add".into(),
    })?;
    let outcome = state
        .store
        .add_issues_to_project(
            project_id,
            expected_version,
            &body.issue_ids,
            Some(principal.actor_user_id),
        )
        .await
        .map_err(|e| map_cas_error(project_id, e))?;
    for issue_id in &outcome.added {
        audit::record(
            state.store.as_ref(),
            principal.actor_user_id,
            audit::PROJECT_ISSUE_ADD,
            format!("{project_id}:{issue_id}"),
        )
        .await?;
    }
    Ok(Json(outcome.into()))
}

/// `DELETE /projects/{id}/issues/{issue_id}?expected_version=` —
/// single detach (§7.2). 204 on success; CAS-gated on the project's
/// `version`. A no-op detach (the issue is not currently in this
/// project) is `404 project_issue_not_found` — same idempotence-at-
/// the-application-boundary contract as the store layer.
#[utoipa::path(
    delete,
    path = "/projects/{id}/issues/{issue_id}",
    params(
        ("id"       = Uuid, Path,  description = "Project id"),
        ("issue_id" = Uuid, Path,  description = "Issue id to detach"),
        ("expected_version" = i64, Query, description = "Caller-observed project version (CAS)"),
    ),
    responses(
        (status = 204, description = "Detached"),
        (status = 404, description = "No such project, or issue is not in this project"),
        (status = 409, description = "Stale `expected_version`"),
    ),
    tag = "projects",
)]
pub async fn remove_project_issue(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((project_id, issue_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<RemoveIssueQuery>,
) -> Result<Response, ApiError> {
    // PROJECT-VIEW.md §5.4 amendment — a `?view=` scopes the detach
    // to the saved view's membership only; the issue stays on the
    // project. No `expected_version` is required (the project row
    // doesn't mutate). Audit verb stays `project_issue_remove`
    // with the view id appended so an operator can still trace
    // "why did this issue disappear from a tab".
    if let Some(view_id) = q.view {
        // Confirm the project exists so we 404 rather than silently
        // succeed on a stale URL.
        if state.store.get_project(project_id).await?.is_none() {
            return Err(ApiError::NotFound {
                code: "project_not_found",
                message: format!("no project with id {project_id}"),
            });
        }
        state.store.remove_issue_from_view(view_id, issue_id).await?;
        audit::record(
            state.store.as_ref(),
            principal.actor_user_id,
            audit::PROJECT_ISSUE_REMOVE,
            format!("{project_id}:{issue_id}:view={view_id}"),
        )
        .await?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    let expected_version = q.expected_version.ok_or(ApiError::BadRequest {
        code: "missing_expected_version",
        message: "expected_version query param is required for project-level detach"
            .to_owned(),
    })?;
    state
        .store
        .remove_issue_from_project(project_id, issue_id, expected_version)
        .await
        .map_err(|e| map_cas_error(project_id, e))?;
    audit::record(
        state.store.as_ref(),
        principal.actor_user_id,
        audit::PROJECT_ISSUE_REMOVE,
        format!("{project_id}:{issue_id}"),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `GET /issues/{id}/project` — resolve the (single, per v1 `UNIQUE
/// (issue_id)`) project for an issue, or `null` when the issue is
/// not in any project. Backs the §6.5 detail-pane chip.
#[utoipa::path(
    get,
    path = "/issues/{id}/project",
    params(("id" = Uuid, Path, description = "Issue id")),
    responses(
        (status = 200, description = "ProjectDto or null", body = Option<ProjectDto>),
    ),
    tag = "projects",
)]
pub async fn get_project_for_issue(
    State(state): State<AppState>,
    Path(issue_id): Path<Uuid>,
) -> Result<Json<Option<ProjectDto>>, ApiError> {
    let project = state.store.get_project_for_issue(issue_id).await?;
    Ok(Json(project.map(ProjectDto::from)))
}

/// One entry in [`GroupByOptionsResponse::dims`].
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct GroupByOptionDto {
    /// Wire-form dim id passed back as `?group_by=<id>` — e.g.
    /// `status`, `tag:gate`.
    pub id: String,
    /// Display label. For `tag:<key>` this is the key title-cased
    /// in v1; richer labels (gate prefix, milestone titles) ride in
    /// when the ordinal-taxonomy config lands (PROJECT-VIEW.md §10.1).
    pub label: String,
}

/// Body for `GET /projects/{id}/group-by-options` — dynamic
/// dimension catalogue powering the Group-by dropdown
/// (PROJECT-VIEW.md §5.1 / §7.3).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct GroupByOptionsResponse {
    /// Ordered list of dimensions the toolbar can show. `status`
    /// is always present; one `tag:<key>` entry per distinct kv
    /// key observable on the project's issues. Sticky keys from
    /// saved views (§5.1) will be merged in when slice 4 lands.
    pub dims: Vec<GroupByOptionDto>,
}

/// `GET /projects/{id}/group-by-options` — dynamic dim list for
/// the Group-by dropdown (PROJECT-VIEW.md §7.3).
///
/// Slice 2: `status` is the only fixed dim; every distinct
/// `dp_tags.key` linked to one of the project's issues (non-
/// archived, `kind='kv'`) becomes a `tag:<key>` entry. Cache /
/// invalidation per §7.3 is deferred — the query is cheap enough
/// on the slice-A target (≤100 issues per project) to run inline.
#[utoipa::path(
    get,
    path = "/projects/{id}/group-by-options",
    params(("id" = Uuid, Path, description = "Project id")),
    responses(
        (status = 200, description = "Available group-by dimensions", body = GroupByOptionsResponse),
        (status = 404, description = "No such project"),
    ),
    tag = "projects",
)]
pub async fn list_group_by_options(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<GroupByOptionsResponse>, ApiError> {
    let _ = state
        .store
        .get_project(project_id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            code: "project_not_found",
            message: format!("no project with id {project_id}"),
        })?;
    let mut dims = vec![GroupByOptionDto {
        id: "status".into(),
        label: "Status".into(),
    }];
    let keys = state
        .store
        .list_project_issue_tag_keys(project_id)
        .await?;
    for k in keys {
        dims.push(GroupByOptionDto {
            label: title_case_dim(&k),
            id: format!("tag:{k}"),
        });
    }
    Ok(Json(GroupByOptionsResponse { dims }))
}

/// `gate` → `Gate`, `priority` → `Priority`. Multi-word kv keys
/// aren't legal under the §3 grammar (no `_` / no spaces), so a
/// straight first-letter uppercase is enough.
fn title_case_dim(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut chars = key.chars();
    if let Some(first) = chars.next() {
        for c in first.to_uppercase() {
            out.push(c);
        }
    }
    for c in chars {
        out.push(c);
    }
    out
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the project ↔ issue membership router fragment.
/// `dp-server::build` merges this into the protected stack alongside
/// the §7.1 projects spine. Reads are gated on `(projects, read)`;
/// writes on `(projects, write)` — same lanes as the §7.1 routes.
pub fn project_issues_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new()
                .route("/projects/{id}/issues", get(list_project_issues))
                .route(
                    "/projects/{id}/group-by-options",
                    get(list_group_by_options),
                )
                .route("/issues/{id}/project", get(get_project_for_issue)),
            "projects",
            "read",
        ))
        .merge(with_permission(
            Router::new()
                .route("/projects/{id}/issues", post(bulk_add_issues))
                .route(
                    "/projects/{id}/issues/{issue_id}",
                    delete(remove_project_issue),
                ),
            "projects",
            "write",
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
    use chrono::Utc;
    use std::sync::Mutex;
    use tower::ServiceExt;

    use dp_domain::audit::AuditEntry;
    use dp_domain::issue::{Issue, IssueState};
    use dp_domain::project::{Project, ProjectStatus};
    use dp_domain::store::{EventActorRow, Store, StoreError};
    use dp_domain::{
        ActivityEvent, ActorRole, EventActor, FetchCursor, FetchRun, FetchRunKind, Membership,
        Org, Repo, ResourceKind, Team, User, WebhookDelivery, Window,
    };

    // -----------------------------------------------------------------
    // Minimal in-memory store: just the surface the membership routes
    // exercise. The §7.1 tests use a similar pattern in `projects.rs`.
    // -----------------------------------------------------------------

    #[derive(Default)]
    struct MemStore {
        projects: Mutex<Vec<Project>>,
        issues: Mutex<Vec<Issue>>,
        memberships: Mutex<Vec<(Uuid, Uuid)>>, // (project, issue)
        view_memberships: Mutex<Vec<(Uuid, Uuid)>>, // (view, issue)
        audit: Mutex<Vec<AuditEntry>>,
    }

    impl MemStore {
        fn audit_rows(&self) -> Vec<AuditEntry> {
            self.audit.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl Store for MemStore {
        async fn get_project(&self, id: Uuid) -> Result<Option<Project>, StoreError> {
            Ok(self
                .projects
                .lock()
                .unwrap()
                .iter()
                .find(|p| p.id == id)
                .cloned())
        }

        async fn list_issue_ids_for_project(
            &self,
            project_id: Uuid,
        ) -> Result<Vec<Uuid>, StoreError> {
            Ok(self
                .memberships
                .lock()
                .unwrap()
                .iter()
                .filter(|(p, _)| *p == project_id)
                .map(|(_, i)| *i)
                .collect())
        }

        async fn get_issue(&self, id: Uuid) -> Result<Option<Issue>, StoreError> {
            Ok(self
                .issues
                .lock()
                .unwrap()
                .iter()
                .find(|i| i.id == id)
                .cloned())
        }

        async fn add_issues_to_project(
            &self,
            project_id: Uuid,
            expected_version: i64,
            issue_ids: &[Uuid],
            _actor: Option<Uuid>,
        ) -> Result<ProjectIssueAddOutcome, StoreError> {
            let mut projects = self.projects.lock().unwrap();
            let project = projects
                .iter_mut()
                .find(|p| p.id == project_id)
                .ok_or_else(|| StoreError::NotFound {
                    entity: "project",
                    id: project_id.to_string(),
                })?;
            if project.version != expected_version {
                return Err(StoreError::Conflict(format!(
                    "project version mismatch: expected {expected_version}, found {}",
                    project.version
                )));
            }
            let project_org = project.org_id;
            drop(projects);

            let mut added = Vec::new();
            let mut skipped = Vec::new();
            for &issue_id in issue_ids {
                let issue = self
                    .issues
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|i| i.id == issue_id)
                    .cloned();
                let Some(issue) = issue else {
                    skipped.push(ProjectIssueAddSkip {
                        issue_id,
                        reason: "unknown_issue".into(),
                        existing_project_id: None,
                    });
                    continue;
                };
                if issue.org_id != project_org {
                    skipped.push(ProjectIssueAddSkip {
                        issue_id,
                        reason: "cross_org".into(),
                        existing_project_id: None,
                    });
                    continue;
                }
                let existing = self
                    .memberships
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|(_, i)| *i == issue_id)
                    .map(|(p, _)| *p);
                if let Some(existing) = existing {
                    skipped.push(ProjectIssueAddSkip {
                        issue_id,
                        reason: "already_in_project".into(),
                        existing_project_id: Some(existing),
                    });
                    continue;
                }
                self.memberships
                    .lock()
                    .unwrap()
                    .push((project_id, issue_id));
                added.push(issue_id);
            }
            if !added.is_empty() {
                let mut projects = self.projects.lock().unwrap();
                let project = projects.iter_mut().find(|p| p.id == project_id).unwrap();
                project.version += 1;
                project.issue_count = self
                    .memberships
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(p, _)| *p == project_id)
                    .count() as i32;
                project.updated_at = Utc::now();
            }
            Ok(ProjectIssueAddOutcome { added, skipped })
        }

        async fn remove_issue_from_project(
            &self,
            project_id: Uuid,
            issue_id: Uuid,
            expected_version: i64,
        ) -> Result<Project, StoreError> {
            let mut projects = self.projects.lock().unwrap();
            let project = projects
                .iter_mut()
                .find(|p| p.id == project_id)
                .ok_or_else(|| StoreError::NotFound {
                    entity: "project",
                    id: project_id.to_string(),
                })?;
            if project.version != expected_version {
                return Err(StoreError::Conflict(format!(
                    "project version mismatch: expected {expected_version}, found {}",
                    project.version
                )));
            }
            let mut links = self.memberships.lock().unwrap();
            let before = links.len();
            links.retain(|(p, i)| !(*p == project_id && *i == issue_id));
            if links.len() == before {
                return Err(StoreError::NotFound {
                    entity: "project_issue",
                    id: issue_id.to_string(),
                });
            }
            project.version += 1;
            project.issue_count = links.iter().filter(|(p, _)| *p == project_id).count() as i32;
            project.updated_at = Utc::now();
            // Cascade matches the PG `ON DELETE CASCADE` from the
            // 0036 migration — a project-level detach must drop
            // every view-membership row that references this issue.
            self.view_memberships
                .lock()
                .unwrap()
                .retain(|(_, i)| *i != issue_id);
            Ok(project.clone())
        }

        async fn list_issue_ids_for_view(
            &self,
            view_id: Uuid,
        ) -> Result<Vec<Uuid>, StoreError> {
            Ok(self
                .view_memberships
                .lock()
                .unwrap()
                .iter()
                .filter(|(v, _)| *v == view_id)
                .map(|(_, i)| *i)
                .collect())
        }

        async fn add_issues_to_view(
            &self,
            view_id: Uuid,
            issue_ids: &[Uuid],
        ) -> Result<(), StoreError> {
            let mut rows = self.view_memberships.lock().unwrap();
            for &iid in issue_ids {
                if !rows.iter().any(|(v, i)| *v == view_id && *i == iid) {
                    rows.push((view_id, iid));
                }
            }
            Ok(())
        }

        async fn remove_issue_from_view(
            &self,
            view_id: Uuid,
            issue_id: Uuid,
        ) -> Result<(), StoreError> {
            self.view_memberships
                .lock()
                .unwrap()
                .retain(|(v, i)| !(*v == view_id && *i == issue_id));
            Ok(())
        }

        async fn get_project_for_issue(
            &self,
            issue_id: Uuid,
        ) -> Result<Option<Project>, StoreError> {
            let owner = self
                .memberships
                .lock()
                .unwrap()
                .iter()
                .find(|(_, i)| *i == issue_id)
                .map(|(p, _)| *p);
            let Some(pid) = owner else { return Ok(None) };
            self.get_project(pid).await
        }

        async fn record_audit_log(&self, entry: &AuditEntry) -> Result<(), StoreError> {
            self.audit.lock().unwrap().push(entry.clone());
            Ok(())
        }

        // --- minimal stubs for the rest of the Store surface --------
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
            Ok(vec![])
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
        async fn data_as_of(&self) -> Result<dp_domain::freshness::DataAsOf, StoreError> {
            Ok(dp_domain::freshness::DataAsOf::default())
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

    // -----------------------------------------------------------------
    // Harness
    // -----------------------------------------------------------------

    fn build_app(store: Arc<MemStore>, actor: Uuid) -> Router {
        use starter_spi::auth::{Principal as SpiPrincipal, Role};
        use starter_spi::authz::{NoopPolicyEngine, PolicyEngine};
        use std::sync::Arc as StdArc;
        let app_state = Arc::new(AppState::new(store));
        let engine: StdArc<dyn PolicyEngine> = StdArc::new(NoopPolicyEngine);
        let spi_principal = SpiPrincipal {
            subject: actor.to_string(),
            role: Role::Admin,
            scopes: Vec::new(),
            extra: serde_json::Value::Null,
        };
        project_issues_router(app_state)
            .layer(Extension(Principal { actor_user_id: actor }))
            .layer(Extension(spi_principal))
            .layer(Extension(engine))
    }

    async fn json_of(resp: axum::response::Response) -> serde_json::Value {
        let body = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    fn seed_project(store: &MemStore, org: Uuid) -> Project {
        let p = Project {
            id: Uuid::new_v4(),
            org_id: org,
            name: "p".into(),
            description: None,
            lead_user_id: None,
            status: ProjectStatus::Active,
            start_at: None,
            due_at: None,
            issue_count: 0,
            closed_issue_count: 0,
            created_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            version: 1,
            primary_milestone_id: None,
        };
        store.projects.lock().unwrap().push(p.clone());
        p
    }

    fn seed_issue(store: &MemStore, org: Uuid, state: IssueState, title: &str) -> Issue {
        let i = Issue {
            id: Uuid::new_v4(),
            org_id: org,
            repo_id: Uuid::new_v4(),
            github_id: 0,
            number: 1,
            title: title.into(),
            body: None,
            state,
            labels: Vec::new(),
            assignees: Vec::new(),
            milestone: None,
            version: 1,
            github_node_id: None,
            updated_at: Utc::now(),
            is_local: false,
        };
        store.issues.lock().unwrap().push(i.clone());
        i
    }

    // -----------------------------------------------------------------
    // POST /projects/{id}/issues
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn bulk_add_returns_added_and_skipped_with_audit() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let i_ok = seed_issue(&store, org, IssueState::Open, "ok");
        let i_cross = seed_issue(&store, Uuid::new_v4(), IssueState::Open, "wrong org");
        let i_unknown = Uuid::new_v4();
        let app = build_app(store.clone(), actor);
        let body = serde_json::json!({
            "expected_version": 1,
            "issue_ids": [i_ok.id, i_cross.id, i_unknown],
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/projects/{}/issues", project.id))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert_eq!(v["added"], serde_json::json!([i_ok.id]));
        let skipped = v["skipped"].as_array().unwrap();
        assert_eq!(skipped.len(), 2);
        let reasons: Vec<&str> = skipped
            .iter()
            .map(|s| s["reason"].as_str().unwrap())
            .collect();
        assert!(reasons.contains(&"cross_org"));
        assert!(reasons.contains(&"unknown_issue"));
        let audit_rows = store.audit_rows();
        assert_eq!(audit_rows.len(), 1, "only the accepted row audits");
        assert_eq!(audit_rows[0].action, audit::PROJECT_ISSUE_ADD);
    }

    #[tokio::test]
    async fn bulk_add_returns_already_in_project_with_existing_id() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let other_project = seed_project(&store, org);
        let issue = seed_issue(&store, org, IssueState::Open, "shared");
        // Pre-attach to the other project.
        store
            .memberships
            .lock()
            .unwrap()
            .push((other_project.id, issue.id));
        let app = build_app(store.clone(), Uuid::new_v4());
        let body = serde_json::json!({
            "expected_version": 1,
            "issue_ids": [issue.id],
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/projects/{}/issues", project.id))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert!(v["added"].as_array().unwrap().is_empty());
        let skip0 = &v["skipped"][0];
        assert_eq!(skip0["reason"], "already_in_project");
        assert_eq!(skip0["existing_project_id"], serde_json::json!(other_project.id));
    }

    #[tokio::test]
    async fn bulk_add_rejects_stale_version_with_409() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let issue = seed_issue(&store, org, IssueState::Open, "x");
        let app = build_app(store.clone(), Uuid::new_v4());
        let body = serde_json::json!({
            "expected_version": 99,
            "issue_ids": [issue.id],
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/projects/{}/issues", project.id))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "stale_project_version");
        assert!(store.audit_rows().is_empty());
    }

    #[tokio::test]
    async fn bulk_add_rejects_over_cap_with_400() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let app = build_app(store.clone(), Uuid::new_v4());
        let ids: Vec<Uuid> = (0..(BULK_ADD_ISSUE_CAP + 1)).map(|_| Uuid::new_v4()).collect();
        let body = serde_json::json!({
            "expected_version": 1,
            "issue_ids": ids,
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/projects/{}/issues", project.id))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "bulk_add_too_large");
    }

    #[tokio::test]
    async fn bulk_add_empty_list_is_noop() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let app = build_app(store.clone(), Uuid::new_v4());
        let body = serde_json::json!({
            "expected_version": 1,
            "issue_ids": [],
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/projects/{}/issues", project.id))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert!(v["added"].as_array().unwrap().is_empty());
        assert!(v["skipped"].as_array().unwrap().is_empty());
        assert!(store.audit_rows().is_empty());
        // Version unchanged when nothing landed.
        let row = &store.projects.lock().unwrap()[0];
        assert_eq!(row.version, 1);
    }

    // -----------------------------------------------------------------
    // DELETE /projects/{id}/issues/{issue_id}
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn remove_detaches_and_returns_204() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let issue = seed_issue(&store, org, IssueState::Open, "x");
        store.memberships.lock().unwrap().push((project.id, issue.id));
        let app = build_app(store.clone(), actor);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/projects/{}/issues/{}?expected_version=1",
                        project.id, issue.id
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(store.memberships.lock().unwrap().is_empty());
        let rows = store.audit_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].action, audit::PROJECT_ISSUE_REMOVE);
    }

    #[tokio::test]
    async fn remove_returns_404_when_membership_missing() {
        let store = Arc::new(MemStore::default());
        let project = seed_project(&store, Uuid::new_v4());
        let app = build_app(store.clone(), Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/projects/{}/issues/{}?expected_version=1",
                        project.id,
                        Uuid::new_v4(),
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "project_issue_not_found");
        assert!(store.audit_rows().is_empty());
    }

    #[tokio::test]
    async fn remove_rejects_stale_version_with_409() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let issue = seed_issue(&store, org, IssueState::Open, "x");
        store.memberships.lock().unwrap().push((project.id, issue.id));
        let app = build_app(store.clone(), Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/projects/{}/issues/{}?expected_version=42",
                        project.id, issue.id
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    // -----------------------------------------------------------------
    // GET /issues/{id}/project
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn get_project_for_issue_returns_null_when_unattached() {
        let store = Arc::new(MemStore::default());
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/issues/{}/project", Uuid::new_v4()))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert!(v.is_null());
    }

    #[tokio::test]
    async fn get_project_for_issue_returns_owning_project() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let issue = seed_issue(&store, org, IssueState::Open, "x");
        store.memberships.lock().unwrap().push((project.id, issue.id));
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/issues/{}/project", issue.id))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert_eq!(v["id"], serde_json::json!(project.id));
    }

    // -----------------------------------------------------------------
    // GET /projects/{id}/issues
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn list_project_issues_returns_only_attached_rows() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let i_in = seed_issue(&store, org, IssueState::Open, "in-project");
        let _i_out = seed_issue(&store, org, IssueState::Open, "out-of-project");
        store
            .memberships
            .lock()
            .unwrap()
            .push((project.id, i_in.id));
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}/issues", project.id))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert_eq!(v["total"], 1);
        assert_eq!(v["rows"][0]["title"], "in-project");
    }

    #[tokio::test]
    async fn list_project_issues_filters_by_state_and_query() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let open_match = seed_issue(&store, org, IssueState::Open, "Alpha rollout");
        let closed_match = seed_issue(&store, org, IssueState::Closed, "Alpha cleanup");
        let open_other = seed_issue(&store, org, IssueState::Open, "Beta scout");
        for id in [open_match.id, closed_match.id, open_other.id] {
            store.memberships.lock().unwrap().push((project.id, id));
        }
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/projects/{}/issues?state=open&q=alpha",
                        project.id
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let v = json_of(resp).await;
        assert_eq!(v["total"], 1);
        assert_eq!(v["rows"][0]["title"], "Alpha rollout");
    }

    #[tokio::test]
    async fn list_project_issues_404_when_project_missing() {
        let store = Arc::new(MemStore::default());
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}/issues", Uuid::new_v4()))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ---- PROJECT-VIEW.md §7.2 — group-by + buckets sidecar -----

    #[tokio::test]
    async fn group_by_status_returns_open_and_closed_buckets() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let open_a = seed_issue(&store, org, IssueState::Open, "open A");
        let open_b = seed_issue(&store, org, IssueState::Open, "open B");
        let closed_a = seed_issue(&store, org, IssueState::Closed, "closed A");
        for id in [open_a.id, open_b.id, closed_a.id] {
            store.memberships.lock().unwrap().push((project.id, id));
        }
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/projects/{}/issues?group_by=status",
                        project.id
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        let buckets = v["buckets"].as_array().expect("buckets sidecar");
        assert_eq!(buckets.len(), 2);
        // Order is count desc → open (2) before closed (1).
        assert_eq!(buckets[0]["key"], "open");
        assert_eq!(buckets[0]["open"], 2);
        assert_eq!(buckets[0]["closed"], 0);
        assert_eq!(buckets[1]["key"], "closed");
        assert_eq!(buckets[1]["closed"], 1);
        // Every row carries a single-element `bucket_keys`.
        for row in v["rows"].as_array().unwrap() {
            let keys = row["bucket_keys"].as_array().expect("bucket_keys");
            assert_eq!(keys.len(), 1);
            let expected = if row["state"] == "open" { "open" } else { "closed" };
            assert_eq!(keys[0], expected);
        }
    }

    #[tokio::test]
    async fn group_by_tag_with_no_links_yields_no_key_bucket() {
        // MemStore's default `list_project_issue_tag_values` returns
        // empty, so every project issue falls into the synthetic
        // `No <key>` bucket (PROJECT-VIEW.md §5.1).
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let i = seed_issue(&store, org, IssueState::Open, "untagged");
        store.memberships.lock().unwrap().push((project.id, i.id));
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/projects/{}/issues?group_by=tag:gate",
                        project.id
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        let buckets = v["buckets"].as_array().expect("buckets sidecar");
        assert_eq!(buckets.len(), 1);
        assert!(buckets[0]["key"].is_null());
        assert_eq!(buckets[0]["label"], "No gate");
        assert_eq!(buckets[0]["open"], 1);
        let row_keys = v["rows"][0]["bucket_keys"].as_array().unwrap();
        assert_eq!(row_keys.len(), 1);
        assert!(row_keys[0].is_null());
    }

    #[tokio::test]
    async fn group_by_rejects_unknown_dim_with_400() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/projects/{}/issues?group_by=assignee",
                        project.id
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "invalid_group_by");
    }

    #[tokio::test]
    async fn group_by_rejects_invalid_tag_key_with_400() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/projects/{}/issues?group_by=tag:Bad_Key",
                        project.id
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "invalid_group_by");
    }

    #[tokio::test]
    async fn no_group_by_omits_buckets_field() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let i = seed_issue(&store, org, IssueState::Open, "x");
        store.memberships.lock().unwrap().push((project.id, i.id));
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}/issues", project.id))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let v = json_of(resp).await;
        // `skip_serializing_if = "Option::is_none"` ⇒ field absent.
        assert!(v.get("buckets").is_none());
        assert!(v["rows"][0].get("bucket_keys").is_none());
    }

    #[tokio::test]
    async fn group_by_options_includes_status_by_default() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}/group-by-options", project.id))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        let dims = v["dims"].as_array().expect("dims");
        assert!(dims.iter().any(|d| d["id"] == "status"));
    }

    // ---- PROJECT-VIEW.md §5.2/§5.3/§5.4 — filter chips + sort -

    fn seed_issue_with(
        store: &MemStore,
        org: Uuid,
        state: IssueState,
        title: &str,
        assignees: Vec<String>,
        labels: Vec<String>,
        updated_at: chrono::DateTime<chrono::Utc>,
    ) -> Issue {
        let mut i = seed_issue(store, org, state, title);
        // seed_issue inserts the row already; mutate in place so the
        // store reflects the assignees/labels/updated_at.
        let mut issues = store.issues.lock().unwrap();
        let idx = issues.iter().position(|x| x.id == i.id).unwrap();
        issues[idx].assignees = assignees.clone();
        issues[idx].labels = labels.clone();
        issues[idx].updated_at = updated_at;
        i.assignees = assignees;
        i.labels = labels;
        i.updated_at = updated_at;
        i
    }

    #[tokio::test]
    async fn filter_by_status_and_assignee_anded() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let now = chrono::Utc::now();
        let want = seed_issue_with(
            &store,
            org,
            IssueState::Open,
            "match",
            vec!["alice".into()],
            vec![],
            now,
        );
        let wrong_state = seed_issue_with(
            &store,
            org,
            IssueState::Closed,
            "closed",
            vec!["alice".into()],
            vec![],
            now,
        );
        let wrong_assignee = seed_issue_with(
            &store,
            org,
            IssueState::Open,
            "wrong assignee",
            vec!["bob".into()],
            vec![],
            now,
        );
        for id in [want.id, wrong_state.id, wrong_assignee.id] {
            store.memberships.lock().unwrap().push((project.id, id));
        }
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/projects/{}/issues?filter=status:open;assignee:alice",
                        project.id
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert_eq!(v["total"], 1);
        assert_eq!(v["rows"][0]["title"], "match");
    }

    #[tokio::test]
    async fn filter_by_label_case_insensitive() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let now = chrono::Utc::now();
        let bug = seed_issue_with(
            &store,
            org,
            IssueState::Open,
            "with bug label",
            vec![],
            vec!["Bug".into()],
            now,
        );
        let other = seed_issue_with(
            &store,
            org,
            IssueState::Open,
            "feature",
            vec![],
            vec!["feature".into()],
            now,
        );
        for id in [bug.id, other.id] {
            store.memberships.lock().unwrap().push((project.id, id));
        }
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/projects/{}/issues?filter=label:bug",
                        project.id
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let v = json_of(resp).await;
        assert_eq!(v["total"], 1);
        assert_eq!(v["rows"][0]["title"], "with bug label");
    }

    #[tokio::test]
    async fn filter_rejects_unknown_dim_with_400() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/projects/{}/issues?filter=mystery:42",
                        project.id
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "invalid_filter");
    }

    #[tokio::test]
    async fn filter_milestone_rejects_non_uuid_value() {
        // `milestone:<uuid>` value must parse as a UUID
        // (PROJECT-VIEW.md §5.4 — milestone filter wire grammar).
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/projects/{}/issues?filter=milestone:not-a-uuid",
                        project.id
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "invalid_filter");
    }

    #[tokio::test]
    async fn filter_milestone_unknown_id_collapses_to_empty() {
        // A UUID that isn't in the project's adopted-milestone set
        // must collapse to zero rows, not leak issues from another
        // project (PROJECT-VIEW.md §5.4 / Slice 3↔1 bridge).
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let now = chrono::Utc::now();
        let issue = seed_issue_with(
            &store,
            org,
            IssueState::Open,
            "needs-milestone",
            vec![],
            vec![],
            now,
        );
        store
            .memberships
            .lock()
            .unwrap()
            .push((project.id, issue.id));
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/projects/{}/issues?filter=milestone:{}",
                        project.id,
                        Uuid::new_v4()
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert_eq!(v["total"], 0);
    }

    #[tokio::test]
    async fn filter_tag_clause_preserves_colon_inside_value() {
        // `tag:team:backend:v2` must parse as key=`team`,
        // value=`backend:v2` (PROJECT-VIEW.md §5.4 — first-colon
        // split inside the tag clause).
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let app = build_app(store, Uuid::new_v4());
        // No tag links — but parser must accept the form so we just
        // expect 200 with zero rows, not 400.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/projects/{}/issues?filter=tag:team:backend:v2",
                        project.id
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert_eq!(v["total"], 0);
    }

    #[tokio::test]
    async fn sort_title_asc_orders_by_lowercase_title() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let now = chrono::Utc::now();
        let banana = seed_issue_with(&store, org, IssueState::Open, "banana", vec![], vec![], now);
        let apple = seed_issue_with(&store, org, IssueState::Open, "Apple", vec![], vec![], now);
        for id in [banana.id, apple.id] {
            store.memberships.lock().unwrap().push((project.id, id));
        }
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}/issues?sort=title_asc", project.id))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let v = json_of(resp).await;
        assert_eq!(v["rows"][0]["title"], "Apple");
        assert_eq!(v["rows"][1]["title"], "banana");
    }

    #[tokio::test]
    async fn sort_rejects_unknown_with_400() {
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}/issues?sort=oldest", project.id))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "invalid_sort");
    }

    // -----------------------------------------------------------------
    // View (tab) membership — PROJECT-VIEW.md §5.4 amendment.
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn list_with_view_uses_view_membership_only() {
        // `?view=<id>` returns the view's manual membership
        // independently of project-level membership. Project rows
        // that aren't on the view must not appear; conversely,
        // view rows that aren't on the project SHOULD appear.
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let on_tab = seed_issue(&store, org, IssueState::Open, "on tab");
        let off_tab = seed_issue(&store, org, IssueState::Open, "off tab");
        // `off_tab` is on the project but not the view; `on_tab`
        // is only on the view (proving the view-tab no longer
        // depends on project membership).
        store
            .memberships
            .lock()
            .unwrap()
            .push((project.id, off_tab.id));
        let view_id = Uuid::new_v4();
        store
            .view_memberships
            .lock()
            .unwrap()
            .push((view_id, on_tab.id));
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}/issues?view={}", project.id, view_id))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert_eq!(v["total"], 1);
        assert_eq!(v["rows"][0]["title"], "on tab");
    }

    #[tokio::test]
    async fn bulk_add_with_view_id_attaches_to_view_only() {
        // Per the §5.4 amendment, saved-view tabs are independent
        // containers: a POST with `view_id` attaches the issues
        // *only* to the view membership and never touches the
        // project membership. `expected_version` is therefore
        // optional in this mode.
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let issue = seed_issue(&store, org, IssueState::Open, "newly added");
        let view_id = Uuid::new_v4();
        let app = build_app(store.clone(), Uuid::new_v4());
        let body = serde_json::json!({
            "issue_ids": [issue.id],
            "view_id": view_id,
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/projects/{}/issues", project.id))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // View membership grew; project membership did not.
        assert!(store
            .view_memberships
            .lock()
            .unwrap()
            .iter()
            .any(|(v, i)| *v == view_id && *i == issue.id));
        assert!(!store
            .memberships
            .lock()
            .unwrap()
            .iter()
            .any(|(p, i)| *p == project.id && *i == issue.id));
    }

    #[tokio::test]
    async fn bulk_add_with_view_id_rejects_cross_org_issue() {
        // Validation (org match) still runs in view mode so the UI
        // gets the same `skipped` surface it gets on project-level
        // adds.
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let other_org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let issue = seed_issue(&store, other_org, IssueState::Open, "wrong org");
        let view_id = Uuid::new_v4();
        let app = build_app(store.clone(), Uuid::new_v4());
        let body = serde_json::json!({
            "issue_ids": [issue.id],
            "view_id": view_id,
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/projects/{}/issues", project.id))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_of(resp).await;
        assert_eq!(v["added"].as_array().unwrap().len(), 0);
        assert_eq!(v["skipped"][0]["reason"], "cross_org");
        assert!(store.view_memberships.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn delete_with_view_scope_removes_only_from_view() {
        // `?view=<id>` scopes the detach to the view's membership
        // table; the project-level link survives, and no
        // `expected_version` is required.
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let issue = seed_issue(&store, org, IssueState::Open, "stays in project");
        store
            .memberships
            .lock()
            .unwrap()
            .push((project.id, issue.id));
        let view_id = Uuid::new_v4();
        store
            .view_memberships
            .lock()
            .unwrap()
            .push((view_id, issue.id));
        let app = build_app(store.clone(), Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/projects/{}/issues/{}?view={}",
                        project.id, issue.id, view_id
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        // View row gone.
        assert!(!store
            .view_memberships
            .lock()
            .unwrap()
            .iter()
            .any(|(v, i)| *v == view_id && *i == issue.id));
        // Project row preserved.
        assert!(store
            .memberships
            .lock()
            .unwrap()
            .iter()
            .any(|(p, i)| *p == project.id && *i == issue.id));
    }

    #[tokio::test]
    async fn delete_without_view_still_requires_expected_version() {
        // The project-level detach path is unchanged: missing
        // expected_version returns 400.
        let store = Arc::new(MemStore::default());
        let org = Uuid::new_v4();
        let project = seed_project(&store, org);
        let issue = seed_issue(&store, org, IssueState::Open, "x");
        store
            .memberships
            .lock()
            .unwrap()
            .push((project.id, issue.id));
        let app = build_app(store, Uuid::new_v4());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/projects/{}/issues/{}",
                        project.id, issue.id
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "missing_expected_version");
    }
}
