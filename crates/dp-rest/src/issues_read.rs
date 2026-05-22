//! Issue read surface — list + detail handlers.
//!
//! The §8 write path (acquire / commit / rollback / sweeper) lives
//! in [`crate::issues`]; this module only handles **reads** off
//! `dp_issues` so the workflow UI can render its paginated drill-
//! down from repo → issues → one-issue detail without making the
//! frontend re-hydrate from GitHub.
//!
//! | route                                  | shape                              |
//! |----------------------------------------|------------------------------------|
//! | `GET /issues`                          | paginated `IssueListResponse`      |
//! | `GET /issues/{id}`                     | one `IssueDto`                     |
//! | `GET /repos/{repo_id}/issues/{number}` | one `IssueDto` (deep-link form)    |
//!
//! All three routes wear the `issues.read` authz pair so they can
//! be gated independently of the §8 write surface (`issues.write`).
//! Reads are not audited (low-sensitivity directory traversal, same
//! rationale as `GET /repos` / `GET /users`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::{
    extract::{Extension, Path, Query, State},
    response::Json,
    routing::get,
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::issue::{Issue, IssueState};
use dp_domain::store::IssueListFilter;
use dp_domain::tag_link::TagLinkKind;

use crate::audit::Principal;
use crate::error::ApiError;
use crate::repos::{clamp_limit, clamp_offset};
use crate::state::AppState;
use crate::tags::ViewerVisibility;

// ---------------------------------------------------------------------------
// Wire DTOs
// ---------------------------------------------------------------------------

/// Wire form of [`IssueState`]. Lower-case to match GitHub's wire
/// form (`"open"` / `"closed"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum IssueStateDto {
    /// Issue is open.
    Open,
    /// Issue is closed.
    Closed,
}

impl From<IssueState> for IssueStateDto {
    fn from(s: IssueState) -> Self {
        match s {
            IssueState::Open => Self::Open,
            IssueState::Closed => Self::Closed,
        }
    }
}

impl From<IssueStateDto> for IssueState {
    fn from(s: IssueStateDto) -> Self {
        match s {
            IssueStateDto::Open => Self::Open,
            IssueStateDto::Closed => Self::Closed,
        }
    }
}

/// Full issue projection. Matches the shape the frontend's
/// `IssueDto` already declared (`api/client.ts`) so the existing
/// §8.3 detail pane wires up without DTO shape changes.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct IssueDto {
    /// Internal id.
    pub id: Uuid,
    /// Parent repo id.
    pub repo_id: Uuid,
    /// Parent org id.
    pub org_id: Uuid,
    /// Repo-relative issue number.
    pub number: i64,
    /// Title.
    pub title: String,
    /// Body, when present.
    pub body: Option<String>,
    /// State.
    pub state: IssueStateDto,
    /// Labels as strings.
    pub labels: Vec<String>,
    /// Assignee logins.
    pub assignees: Vec<String>,
    /// Milestone title, when set.
    pub milestone: Option<String>,
    /// §8 CAS token.
    pub version: i64,
    /// Last update.
    pub updated_at: DateTime<Utc>,
    /// Short `owner/repo` label rendered in list rows. Populated
    /// by a per-page join through `repo_id -> (org_login, name)`.
    /// Omitted when the join is unavailable (point-lookup detail
    /// handlers backfill this too so the peek panel and the row
    /// it was selected from agree).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_slug: Option<String>,
    /// Per-caller unread flag — `true` when the issue's `version`
    /// is newer than what the caller has marked seen. Only the
    /// `/me/queue` endpoint populates this with a meaningful value;
    /// every other endpoint emits `false`. Defaults to `false` so
    /// older frontend clients that ignore the field render rows
    /// the same as before.
    #[serde(default, skip_serializing_if = "is_false")]
    pub unread: bool,
    /// Bucket keys assigned by an active `group_by` (PROJECT-VIEW.md
    /// §7.2). Populated **only** by `GET /projects/{id}/issues` when
    /// `?group_by=` is set. Each entry is the bucket the issue
    /// belongs to — `None` (serialised as `null`) for the synthetic
    /// "No <key>" bucket. An issue can appear in multiple buckets
    /// when `group_by=tag:<key>` and the issue carries multiple kv
    /// values for the same key (e.g. `category:firmware` +
    /// `category:hardware`). Omitted entirely when no grouping is
    /// active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket_keys: Option<Vec<Option<String>>>,
    /// DP tags currently attached to this issue, filtered to the
    /// viewer-visible subset per tagging.md §7.4. Populated by the
    /// list and detail handlers; absent (treated as empty by
    /// clients) on responses built before that join lands. Each
    /// entry is the slim chip-render projection (`id`, `name`,
    /// `color`, `scope_kind`) — enough for the issue-detail
    /// picker and the row chips, no per-link metadata.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<IssueTagDto>,
}

/// Slim per-issue tag chip projection embedded in [`IssueDto::tags`].
/// Tracks just what the workflow UI needs to render a chip and let
/// the user remove it (`tag_id` keys the `DELETE /tags/{id}/links`
/// call). Visibility is the *tag*'s — issue visibility is enforced
/// by the parent handler.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct IssueTagDto {
    /// Tag id (the picker uses this for link/unlink).
    pub id: Uuid,
    /// Display name (single token or `key:value`).
    pub name: String,
    /// Hex colour token used by the chip renderer.
    pub color: String,
    /// `user` / `team` / `org`. Lets the UI surface "your tag" vs
    /// "team tag" without a second round-trip.
    pub scope_kind: crate::tags::TagScopeKindDto,
}

/// Helper for the `skip_serializing_if` on [`IssueDto::unread`].
/// We omit the field when it is `false` so the on-the-wire shape
/// stays identical to pre-triage callers (no schema break).
fn is_false(b: &bool) -> bool {
    !*b
}

impl From<Issue> for IssueDto {
    fn from(i: Issue) -> Self {
        Self {
            id: i.id,
            repo_id: i.repo_id,
            org_id: i.org_id,
            number: i.number,
            title: i.title,
            body: i.body,
            state: i.state.into(),
            labels: i.labels,
            assignees: i.assignees,
            milestone: i.milestone,
            version: i.version,
            updated_at: i.updated_at,
            repo_slug: None,
            unread: false,
            bucket_keys: None,
            tags: Vec::new(),
        }
    }
}

/// Resolve `repo_id -> "owner/repo"` for every distinct repo
/// referenced by `rows` and stamp it onto `dto.repo_slug`.
///
/// Implemented with point lookups (`get_repo` + `get_org`) keyed
/// by the distinct ids only — a 50-row page usually covers a
/// handful of repos, so the round-trip count stays bounded. Repos
/// that fail to resolve leave `repo_slug = None` rather than
/// failing the whole request, matching the §14.3 contract that
/// the slug is decorative.
pub(crate) async fn attach_repo_slugs(
    store: &dyn dp_domain::store::Store,
    dtos: &mut [IssueDto],
) -> Result<(), ApiError> {
    let repo_ids: HashSet<Uuid> = dtos.iter().map(|d| d.repo_id).collect();
    let mut slugs: HashMap<Uuid, String> = HashMap::with_capacity(repo_ids.len());
    let mut org_cache: HashMap<Uuid, String> = HashMap::new();
    for repo_id in repo_ids {
        let Some(repo) = store.get_repo(repo_id).await? else { continue };
        let login = if let Some(l) = org_cache.get(&repo.org_id) {
            l.clone()
        } else {
            let Some(org) = store.get_org(repo.org_id).await? else { continue };
            org_cache.insert(org.id, org.login.clone());
            org.login
        };
        slugs.insert(repo.id, format!("{login}/{}", repo.name));
    }
    for d in dtos.iter_mut() {
        if let Some(s) = slugs.get(&d.repo_id) {
            d.repo_slug = Some(s.clone());
        }
    }
    Ok(())
}

/// Single-issue variant of [`attach_repo_slugs`] for the detail
/// handlers. Same fallback behaviour — a missing repo or org leaves
/// the slug unset.
pub(crate) async fn attach_repo_slug_one(
    store: &dyn dp_domain::store::Store,
    dto: &mut IssueDto,
) -> Result<(), ApiError> {
    let Some(repo) = store.get_repo(dto.repo_id).await? else { return Ok(()) };
    let Some(org) = store.get_org(repo.org_id).await? else { return Ok(()) };
    dto.repo_slug = Some(format!("{}/{}", org.login, repo.name));
    Ok(())
}

/// Embed `tags: Vec<IssueTagDto>` on every dto.
///
/// Single store round-trip for the link list, single round-trip
/// for the visible-tag catalogue, then an in-memory join. Tags
/// outside the viewer's visibility (per tagging.md §7.4) are
/// dropped so the chip set matches what the picker would offer.
///
/// Failures inside this helper are *non-fatal* to the parent
/// response: we surface them as empty `tags` rather than 500'ing
/// the whole issue list, matching the §14.3 "decorative join"
/// pattern already used by `attach_repo_slugs`.
pub(crate) async fn attach_issue_tags(
    store: &dyn dp_domain::store::Store,
    viewer_user_id: Uuid,
    dtos: &mut [IssueDto],
) -> Result<(), ApiError> {
    if dtos.is_empty() {
        return Ok(());
    }
    let issue_ids: Vec<Uuid> = dtos.iter().map(|d| d.id).collect();
    let links = store
        .list_tag_links_for_targets(TagLinkKind::Issue, &issue_ids)
        .await?;
    if links.is_empty() {
        return Ok(());
    }

    let visibility = ViewerVisibility::load(store, viewer_user_id).await?;
    let team_ids = visibility.visible_team_ids(store).await?;
    let orgs: Vec<Uuid> = visibility.visible_org_ids.iter().copied().collect();
    let teams: Vec<Uuid> = team_ids.iter().copied().collect();
    let tags = store
        .list_tags_visible_to(viewer_user_id, &teams, &orgs, false)
        .await?;
    let mut by_id: HashMap<Uuid, IssueTagDto> = HashMap::with_capacity(tags.len());
    for tag in tags {
        if !visibility.can_see(&tag, &team_ids) {
            continue;
        }
        by_id.insert(
            tag.id,
            IssueTagDto {
                id: tag.id,
                name: tag.name.clone(),
                color: tag.color.clone(),
                scope_kind: tag.scope_kind.into(),
            },
        );
    }

    let mut per_issue: HashMap<Uuid, Vec<IssueTagDto>> = HashMap::new();
    for link in links {
        let Some(issue_id) = link.target_issue_id else { continue };
        let Some(chip) = by_id.get(&link.tag_id) else { continue };
        per_issue.entry(issue_id).or_default().push(chip.clone());
    }
    for d in dtos.iter_mut() {
        if let Some(v) = per_issue.remove(&d.id) {
            d.tags = v;
        }
    }
    Ok(())
}

impl From<dp_domain::inbox::InboxIssueRow> for IssueDto {
    fn from(r: dp_domain::inbox::InboxIssueRow) -> Self {
        let mut dto = IssueDto::from(r.issue);
        dto.unread = r.unread;
        dto
    }
}

/// Paginated envelope mirroring `RepoListResponse`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct IssueListResponse {
    /// Issues on this page.
    pub rows: Vec<IssueDto>,
    /// Total matching the filter, ignoring pagination.
    pub total: i64,
    /// Echoed limit.
    pub limit: i64,
    /// Echoed offset.
    pub offset: i64,
    /// Server-side bucket sidecar — populated **only** by
    /// `GET /projects/{id}/issues` when `?group_by=` is set
    /// (PROJECT-VIEW.md §7.2). Counts are **post-filter** (§5.2)
    /// and authoritative; the client never re-buckets. `key` is
    /// `None` for the synthetic "No <key>" bucket. Bucket ordering
    /// is server-decided (§5.1 — ordinal taxonomies first, then
    /// count desc).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buckets: Option<Vec<IssueBucket>>,
}

/// One bucket in [`IssueListResponse::buckets`]. The label is the
/// human-readable form the client renders verbatim; `key` is the
/// stable identifier used for collapse state and as the
/// `bucket_keys` value on `IssueDto`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct IssueBucket {
    /// Stable bucket key (`g3-mvp-build`, `open`, `<uuid>` for
    /// milestone/type buckets). `None` for the synthetic "No <key>"
    /// bucket.
    pub key: Option<String>,
    /// Human-readable label (`G3 · MVP build`, `Open`). For now
    /// equals `key` for tag buckets; richer labels (gate prefix,
    /// milestone title) land with the ordinal-taxonomy config
    /// (PROJECT-VIEW.md §5.1 / §10.1) and milestone joins.
    pub label: String,
    /// Open issue count in this bucket, post-filter.
    pub open: i64,
    /// Closed issue count in this bucket, post-filter.
    pub closed: i64,
}

/// State filter accepted on the wire. `Open` is the v1 default; the
/// store layer treats `None` as "open + closed" so `All` maps
/// straight through.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StateFilter {
    /// Only `state = 'open'`.
    #[default]
    Open,
    /// Only `state = 'closed'`.
    Closed,
    /// Both states.
    All,
}

impl StateFilter {
    fn to_store(self) -> Option<IssueState> {
        match self {
            Self::Open => Some(IssueState::Open),
            Self::Closed => Some(IssueState::Closed),
            Self::All => None,
        }
    }
}

/// Query params for `GET /issues`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListIssuesQuery {
    /// Restrict to one repo (back-compat shorthand).
    #[serde(default)]
    pub repo_id: Option<Uuid>,
    /// Restrict to one org (back-compat shorthand).
    #[serde(default)]
    pub org_id: Option<Uuid>,
    /// State filter. Defaults to `open`.
    #[serde(default)]
    pub state: StateFilter,
    /// Assignee login (back-compat shorthand for `assignees`).
    #[serde(default)]
    pub assignee: Option<String>,
    /// Case-insensitive substring on title.
    #[serde(default)]
    pub q: Option<String>,
    /// Page size; clamped server-side.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Page offset (`0`-based).
    #[serde(default)]
    pub offset: Option<i64>,

    // ---- triage-spine extensions (slice 1) --------------------
    //
    // All array fields are accepted as comma-separated lists in
    // the query string (`?repo_ids=u1,u2`). Matches the convention
    // the reports module already uses; see `client.ts` for the
    // wire-side construction.

    /// Restrict to these repo ids (comma-separated).
    #[serde(default, deserialize_with = "csv_uuids")]
    pub repo_ids: Vec<Uuid>,
    /// Restrict to these org ids (comma-separated).
    #[serde(default, deserialize_with = "csv_uuids")]
    pub org_ids: Vec<Uuid>,
    /// AND-containment over assignee logins (comma-separated).
    #[serde(default, deserialize_with = "csv_strings")]
    pub assignees: Vec<String>,
    /// AND-containment over labels (comma-separated).
    #[serde(default, deserialize_with = "csv_strings")]
    pub labels: Vec<String>,
    /// Author login (exact match on `dp_issues.author`).
    #[serde(default)]
    pub author: Option<String>,
    /// `state_reason` exact match (`completed` / `not_planned` / `reopened`).
    #[serde(default)]
    pub state_reason: Option<String>,
    /// `updated_at >= updated_since` (RFC3339).
    #[serde(default)]
    pub updated_since: Option<DateTime<Utc>>,
    /// Shortcut for the `Untriaged` smart view — restrict to rows
    /// with no assignees and no labels.
    #[serde(default)]
    pub untriaged: bool,

    /// Keyset cursor for `/me/queue` — wire form
    /// `"<rfc3339_updated_at>,<uuid>"`. The server emits the next
    /// page strictly older than this `(updated_at, id)` pair. The
    /// `GET /issues` handler ignores this field. Backed by the
    /// covering index `dp_issues_updated_at_idx` introduced in
    /// migration `0013_triage_timeline_and_sync.sql`.
    #[serde(default)]
    pub after: Option<String>,
}

/// Deserialize a query-string field of the shape `a,b,c` into
/// `Vec<Uuid>`. An absent or empty field yields an empty vector.
/// Whitespace around commas is tolerated. Invalid UUIDs surface as
/// a `serde::Error` so the request fails with `400`.
fn csv_uuids<'de, D>(de: D) -> Result<Vec<Uuid>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;
    let s: Option<String> = Option::deserialize(de)?;
    let s = match s {
        Some(s) if !s.is_empty() => s,
        _ => return Ok(Vec::new()),
    };
    s.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| Uuid::parse_str(p).map_err(D::Error::custom))
        .collect()
}

/// Same shape as [`csv_uuids`] but for opaque strings (logins,
/// label names, milestone names). Empty pieces are dropped so a
/// trailing comma is harmless.
fn csv_strings<'de, D>(de: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(de)?;
    let s = match s {
        Some(s) if !s.is_empty() => s,
        _ => return Ok(Vec::new()),
    };
    Ok(s.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /issues` — paginated, filterable issue list. Ordered by
/// `updated_at DESC` (matches GitHub's default "recently updated"
/// view).
#[utoipa::path(
    get,
    path = "/issues",
    params(
        ("repo_id"  = Option<Uuid>, Query, description = "Restrict to one repo"),
        ("org_id"   = Option<Uuid>, Query, description = "Restrict to one org"),
        ("state"    = Option<String>, Query, description = "open|closed|all (default open)"),
        ("assignee" = Option<String>, Query, description = "Assignee login (exact)"),
        ("q"        = Option<String>, Query, description = "Substring search on title"),
        ("limit"    = Option<i64>, Query, description = "Page size (1..=200, default 50)"),
        ("offset"   = Option<i64>, Query, description = "Page offset (default 0)"),
    ),
    responses(
        (status = 200, description = "Paginated issue list", body = IssueListResponse),
    ),
    tag = "issues",
)]
pub async fn list_issues(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(q): Query<ListIssuesQuery>,
) -> Result<Json<IssueListResponse>, ApiError> {
    let filter = filter_from_query(&q);
    let rows = state.store.list_issues(&filter).await?;
    let total = state.store.count_issues(&filter).await?;
    let mut dtos: Vec<IssueDto> = rows.into_iter().map(IssueDto::from).collect();
    attach_repo_slugs(&*state.store, &mut dtos).await?;
    attach_issue_tags(&*state.store, principal.actor_user_id, &mut dtos).await?;
    Ok(Json(IssueListResponse {
        rows: dtos,
        total,
        limit: filter.limit,
        offset: filter.offset,
        buckets: None,
    }))
}

/// Translate the wire-side [`ListIssuesQuery`] to the store
/// filter, preserving the back-compat scalar / array dual.
fn filter_from_query(q: &ListIssuesQuery) -> IssueListFilter {
    IssueListFilter {
        repo_id: q.repo_id,
        org_id: q.org_id,
        state: q.state.to_store(),
        assignee: q.assignee.clone().filter(|s| !s.is_empty()),
        q: q.q.clone(),
        limit: clamp_limit(q.limit),
        offset: clamp_offset(q.offset),
        repo_ids: q.repo_ids.clone(),
        org_ids: q.org_ids.clone(),
        assignees: q.assignees.clone(),
        labels: q.labels.clone(),
        author: q.author.clone().filter(|s| !s.is_empty()),
        state_reason: q.state_reason.clone().filter(|s| !s.is_empty()),
        updated_since: q.updated_since,
        untriaged_only: q.untriaged,
        keyset_after: q
            .after
            .as_deref()
            .and_then(parse_keyset_cursor),
    }
}

/// Parse a keyset cursor — wire form
/// `"<rfc3339_updated_at>,<uuid>"`. Invalid cursors fall through
/// to `None` so a malformed `?after=` parameter just resets to
/// the first page rather than 400-ing.
fn parse_keyset_cursor(s: &str) -> Option<(DateTime<Utc>, Uuid)> {
    let (ts_str, id_str) = s.split_once(',')?;
    let ts = DateTime::parse_from_rfc3339(ts_str.trim())
        .ok()?
        .with_timezone(&Utc);
    let id = Uuid::parse_str(id_str.trim()).ok()?;
    Some((ts, id))
}

/// `GET /me/queue` — the caller's inbox. The default landing view
/// for the triage page (`linear-projects-idea.md` §3.8 / §5.4).
///
/// Returns issues that the caller has not dismissed (`status <>
/// 'done'`) and that are not currently snoozed (`status <>
/// 'snoozed' OR snoozed_until < now()`). Rows that have never
/// been touched surface as default-state ("inbox", unread until
/// the user opens the peek panel).
///
/// Accepts the same filter set as `GET /issues` so the smart-view
/// rail can pre-narrow ("My queue · phoenix only", etc.).
///
/// Each row carries an `unread` boolean — `true` when
/// `dp_issues.version > last_seen_version`. The frontend renders
/// an indicator and uses `POST /me/inbox/seen` to clear it when
/// the user opens the row in the peek panel.
#[utoipa::path(
    get,
    path = "/me/queue",
    params(
        ("repo_id"   = Option<Uuid>,   Query, description = "Restrict to one repo"),
        ("repo_ids"  = Option<String>, Query, description = "Comma-separated repo ids"),
        ("org_id"    = Option<Uuid>,   Query, description = "Restrict to one org"),
        ("org_ids"   = Option<String>, Query, description = "Comma-separated org ids"),
        ("state"     = Option<String>, Query, description = "open|closed|all (default open)"),
        ("assignees" = Option<String>, Query, description = "Comma-separated assignee logins (AND)"),
        ("labels"    = Option<String>, Query, description = "Comma-separated labels (AND)"),
        ("author"    = Option<String>, Query, description = "Author login"),
        ("state_reason" = Option<String>, Query, description = "completed|not_planned|reopened"),
        ("updated_since" = Option<String>, Query, description = "RFC3339 lower bound"),
        ("untriaged" = Option<bool>,   Query, description = "Restrict to rows with no assignee and no label"),
        ("q"         = Option<String>, Query, description = "Substring search on title"),
        ("limit"     = Option<i64>,    Query, description = "Page size (1..=200, default 50)"),
        ("offset"    = Option<i64>,    Query, description = "Page offset (default 0)"),
    ),
    responses(
        (status = 200, description = "Caller's inbox queue", body = IssueListResponse),
    ),
    tag = "issues",
)]
pub async fn me_queue(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(q): Query<ListIssuesQuery>,
) -> Result<Json<IssueListResponse>, ApiError> {
    let filter = filter_from_query(&q);
    let rows = state
        .store
        .list_inbox_issues(principal.actor_user_id, &filter)
        .await?;
    let total = state
        .store
        .count_inbox_issues(principal.actor_user_id, &filter)
        .await?;
    let mut dtos: Vec<IssueDto> = rows.into_iter().map(IssueDto::from).collect();
    attach_repo_slugs(&*state.store, &mut dtos).await?;
    attach_issue_tags(&*state.store, principal.actor_user_id, &mut dtos).await?;
    Ok(Json(IssueListResponse {
        rows: dtos,
        total,
        limit: filter.limit,
        offset: filter.offset,
        buckets: None,
    }))
}

/// `GET /issues/{id}` — single issue by id.
#[utoipa::path(
    get,
    path = "/issues/{id}",
    params(("id" = Uuid, Path, description = "Issue id")),
    responses(
        (status = 200, description = "Issue detail", body = IssueDto),
        (status = 404, description = "No such issue"),
    ),
    tag = "issues",
)]
pub async fn get_issue_by_id(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> Result<Json<IssueDto>, ApiError> {
    let issue = state.store.get_issue(id).await?;
    match issue {
        Some(i) => {
            let mut dto = IssueDto::from(i);
            attach_repo_slug_one(&*state.store, &mut dto).await?;
            let mut slice = [dto];
            attach_issue_tags(&*state.store, principal.actor_user_id, &mut slice).await?;
            let [dto] = slice;
            Ok(Json(dto))
        }
        None => Err(ApiError::NotFound {
            code: "issue_not_found",
            message: format!("no issue with id {id}"),
        }),
    }
}

/// `GET /repos/{repo_id}/issues/{number}` — single issue via the
/// canonical deep-link shape audit log entries already record.
#[utoipa::path(
    get,
    path = "/repos/{repo_id}/issues/{number}",
    params(
        ("repo_id" = Uuid, Path, description = "Repo id"),
        ("number"  = i64,  Path, description = "Repo-relative issue number"),
    ),
    responses(
        (status = 200, description = "Issue detail", body = IssueDto),
        (status = 404, description = "No such issue"),
    ),
    tag = "issues",
)]
pub async fn get_issue_by_number(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((repo_id, number)): Path<(Uuid, i64)>,
) -> Result<Json<IssueDto>, ApiError> {
    let issue = state
        .store
        .get_issue_by_repo_and_number(repo_id, number)
        .await?;
    match issue {
        Some(i) => {
            let mut dto = IssueDto::from(i);
            attach_repo_slug_one(&*state.store, &mut dto).await?;
            let mut slice = [dto];
            attach_issue_tags(&*state.store, principal.actor_user_id, &mut slice).await?;
            let [dto] = slice;
            Ok(Json(dto))
        }
        None => Err(ApiError::NotFound {
            code: "issue_not_found",
            message: format!("no issue #{number} in repo {repo_id}"),
        }),
    }
}

// ---------------------------------------------------------------------------
// Timeline (§5.6)
// ---------------------------------------------------------------------------

/// Wire form of an issue-timeline row. The shape mirrors §5.6 —
/// the peek panel renders this as a vertical activity strip.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TimelineEntryDto {
    /// `dp_activity_events.id`.
    pub id: Uuid,
    /// Event kind (`issue_opened` / `issue_closed` / `issue_comment`).
    pub kind: String,
    /// Source timestamp.
    pub ts: DateTime<Utc>,
    /// One-line summary, e.g. `"commented: looks good"`.
    pub payload_summary: String,
}

/// Paginated envelope for `GET /issues/{id}/timeline`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TimelineResponse {
    /// Newest-first rows.
    pub rows: Vec<TimelineEntryDto>,
    /// Total matching the filter, ignoring pagination.
    pub total: i64,
    /// Echoed limit.
    pub limit: i64,
    /// Echoed offset.
    pub offset: i64,
}

/// Query string for `GET /issues/{id}/timeline`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TimelineQuery {
    /// Page size; clamped 1..=200, default 50.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Page offset, 0-based.
    #[serde(default)]
    pub offset: Option<i64>,
}

/// `GET /issues/{id}/timeline` — newest-first activity strip for
/// one issue. The lookup uses the §6 guarded expression index on
/// `dp_activity_events` so the predicate is index-only even when
/// the parent repo has decades of accumulated history.
#[utoipa::path(
    get,
    path = "/issues/{id}/timeline",
    params(
        ("id"     = Uuid,        Path,  description = "Issue id"),
        ("limit"  = Option<i64>, Query, description = "Page size (1..=200, default 50)"),
        ("offset" = Option<i64>, Query, description = "Page offset (default 0)"),
    ),
    responses(
        (status = 200, description = "Newest-first timeline", body = TimelineResponse),
        (status = 404, description = "No such issue"),
    ),
    tag = "issues",
)]
pub async fn get_issue_timeline(
    State(state): State<AppState>,
    Extension(_principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Query(q): Query<TimelineQuery>,
) -> Result<Json<TimelineResponse>, ApiError> {
    let issue = state.store.get_issue(id).await?.ok_or(ApiError::NotFound {
        code: "issue_not_found",
        message: format!("no issue with id {id}"),
    })?;
    let limit = clamp_limit(q.limit);
    let offset = clamp_offset(q.offset);
    let rows = state
        .store
        .list_events_for_issue(issue.repo_id, issue.number, limit, offset)
        .await?;
    let total = state
        .store
        .count_events_for_issue(issue.repo_id, issue.number)
        .await?;
    Ok(Json(TimelineResponse {
        rows: rows
            .into_iter()
            .map(|r| TimelineEntryDto {
                id: r.id,
                kind: format!("{:?}", r.kind)
                    .chars()
                    .flat_map(|c| {
                        if c.is_uppercase() {
                            // Convert CamelCase → snake_case
                            let mut v = vec!['_'];
                            v.extend(c.to_lowercase());
                            v
                        } else {
                            vec![c]
                        }
                    })
                    .collect::<String>()
                    .trim_start_matches('_')
                    .to_string(),
                ts: r.ts,
                payload_summary: r.payload_summary,
            })
            .collect(),
        total,
        limit,
        offset,
    }))
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the issue-read router. Gated on `issues.read` so the §8
/// write surface (`issues.write`) can be toggled separately.
pub fn issues_read_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new()
                .route("/issues", get(list_issues))
                .route("/issues/{id}", get(get_issue_by_id))
                .route("/issues/{id}/timeline", get(get_issue_timeline))
                .route("/repos/{repo_id}/issues/{number}", get(get_issue_by_number))
                .route("/me/queue", get(me_queue)),
            "issues",
            "read",
        ))
        .with_state(inner)
}
